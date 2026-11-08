//! Batched `PutEvents` delivery with throttled-delivery recovery: prepared
//! entries bound for the same bus share one request, requests for different
//! buses go out concurrently, and each failed entry maps back to its SQS
//! message by index. Each request records its own EventBridge acceptance
//! time, so a slow request never inflates another's latency metrics. Every
//! send round is bounded by the invocation's remaining time minus a response
//! reserve, and a round with no budget left sends nothing, so a slow round
//! reports its messages for retry instead of letting the invocation time out
//! with its accepted entries unreported. Each chunk re-confirms its domains
//! immediately before its request is dispatched, inside the round's time
//! bound, so neither the first send nor a resend can deliver past a disable
//! that landed while the batch was being prepared or another domain was
//! being checked. Throttled entries are retried inside the invocation with
//! jittered backoff; entries still throttled are rescheduled with a short
//! per-message visibility backoff that grows with the receive count, then
//! reported for retry. The visibility change is bounded by the remaining
//! time minus the same response reserve and skipped when none remains, so it
//! can never cost the invocation its response or the acknowledgements of
//! accepted entries.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use aws_sdk_eventbridge::config::retry::RetryConfig;
use aws_sdk_eventbridge::error::ProvideErrorMetadata;
use aws_sdk_eventbridge::types::PutEventsRequestEntry;

use konductor_common::backoff::{retry_delay, visibility_backoff, JitterSource};
use konductor_common::domain_liveness::{Clock, DomainLivenessCache};
use konductor_common::envelope::chunk_put_events_entries;

use crate::handler::{require_active_domain, Clients, PreparedEntry};

/// How many times throttled entries are resent inside the invocation after
/// the first send.
const MAX_IN_INVOCATION_RETRIES: u32 = 3;

/// No retry starts when the invocation has less than this much time left.
const RETRY_TIME_MARGIN: Duration = Duration::from_secs(5);

/// Time every send round leaves free for recording outcomes and returning
/// the response, so a round can never spend the invocation's last moment on
/// a `PutEvents` request whose answer would arrive too late to matter.
pub(crate) const RESPONSE_TIME_MARGIN: Duration = Duration::from_secs(2);

const THROTTLING_CODE: &str = "ThrottlingException";

/// The per-entry result of one `PutEvents` request.
pub(crate) enum EntrySendResult {
    Accepted,
    Throttled,
    Failed(String),
}

/// A prepared entry paired with its delivery result. `accepted_ms` is the
/// EventBridge acceptance time captured inside the request's own task, so a
/// concurrently slow request cannot inflate this entry's latency metrics.
pub(crate) struct ChunkOutcome {
    entry: PreparedEntry,
    result: EntrySendResult,
    accepted_ms: i64,
}

/// The delivery result of one queue message.
#[derive(Debug, PartialEq)]
pub(crate) enum DeliveryOutcome {
    Delivered {
        sent_timestamp_ms: i64,
        accepted_ms: i64,
    },
    Throttled {
        message_id: String,
    },
    Failed {
        message_id: String,
        error: String,
    },
}

/// Groups prepared entries by bus, chunks each group within the `PutEvents`
/// entry-count and size limits, and sends every chunk concurrently. Each
/// chunk re-checks the liveness of its entries' domains immediately before
/// the request is dispatched, inside the round's time budget. With a zero
/// liveness window the first round skips the re-check, because preparation
/// has just read the store for every record.
pub(crate) async fn deliver<C: Clock>(
    clients: &Clients,
    liveness: &DomainLivenessCache<C>,
    jitter: &dyn JitterSource,
    prepared: Vec<PreparedEntry>,
    remaining: &dyn Fn() -> Duration,
) -> Vec<DeliveryOutcome> {
    let mut pending = prepared;
    let mut outcomes = Vec::new();
    let mut retries = 0;
    loop {
        let round_ids: Vec<String> = pending
            .iter()
            .map(|entry| entry.message_id.clone())
            .collect();
        let round_budget = remaining().saturating_sub(RESPONSE_TIME_MARGIN);
        if round_budget.is_zero() {
            tracing::warn!(
                metric = "PublishDeliveryRetry",
                "no invocation time left for a send round; reporting the round for retry"
            );
            outcomes.extend(
                round_ids
                    .into_iter()
                    .map(|message_id| DeliveryOutcome::Failed {
                        message_id,
                        error: "the invocation ran out of time before the send round".to_string(),
                    }),
            );
            return outcomes;
        }
        let recheck = retries > 0 || liveness.reuses_confirmations();
        let round = async { send_chunks(clients, liveness, pending, recheck).await };
        let mut throttled = Vec::new();
        match tokio::time::timeout(round_budget, round).await {
            Ok((chunk_outcomes, lost)) => {
                for (message_id, error) in lost {
                    outcomes.push(DeliveryOutcome::Failed { message_id, error });
                }
                for outcome in chunk_outcomes {
                    match outcome.result {
                        EntrySendResult::Accepted => outcomes.push(DeliveryOutcome::Delivered {
                            sent_timestamp_ms: outcome.entry.sent_timestamp_ms,
                            accepted_ms: outcome.accepted_ms,
                        }),
                        EntrySendResult::Throttled => throttled.push(outcome.entry),
                        EntrySendResult::Failed(error) => outcomes.push(DeliveryOutcome::Failed {
                            message_id: outcome.entry.message_id,
                            error,
                        }),
                    }
                }
            }
            Err(_elapsed) => {
                tracing::warn!(
                    metric = "PublishDeliveryRetry",
                    "the invocation ran out of time during the send; reporting the round for retry"
                );
                outcomes.extend(
                    round_ids
                        .into_iter()
                        .map(|message_id| DeliveryOutcome::Failed {
                            message_id,
                            error: "the invocation ran out of time during the send".to_string(),
                        }),
                );
                return outcomes;
            }
        }
        if throttled.is_empty() {
            return outcomes;
        }
        if retries >= MAX_IN_INVOCATION_RETRIES || remaining() < RETRY_TIME_MARGIN {
            shorten_visibility(clients, jitter, &throttled, remaining).await;
            outcomes.extend(
                throttled
                    .into_iter()
                    .map(|entry| DeliveryOutcome::Throttled {
                        message_id: entry.message_id,
                    }),
            );
            return outcomes;
        }
        retries += 1;
        tokio::time::sleep(retry_delay(jitter, retries)).await;
        pending = throttled;
    }
}

/// Confirms every entry's domain immediately before its chunk's request is
/// dispatched. `confirm_active` re-reads exactly the expired confirmations,
/// so this costs nothing while confirmations are fresh. Entries whose domain
/// is no longer active — or whose check failed — fail instead of sending.
async fn keep_entries_with_live_domains<C: Clock>(
    clients: &Clients,
    liveness: &DomainLivenessCache<C>,
    entries: Vec<PreparedEntry>,
) -> (Vec<PreparedEntry>, Vec<ChunkOutcome>) {
    let reject = |entry: PreparedEntry, error: String| ChunkOutcome {
        entry,
        result: EntrySendResult::Failed(error),
        accepted_ms: chrono::Utc::now().timestamp_millis(),
    };
    let Ok(table) = std::env::var("DOMAINS_TABLE") else {
        tracing::error!(
            metric = "PublishDeliveryRetry",
            "DOMAINS_TABLE not set; reporting the round for retry"
        );
        return (
            Vec::new(),
            entries
                .into_iter()
                .map(|entry| reject(entry, "DOMAINS_TABLE not set before the send".to_string()))
                .collect(),
        );
    };
    let mut live = Vec::with_capacity(entries.len());
    let mut rejected = Vec::new();
    let mut denied: HashSet<String> = HashSet::new();
    let mut confirmed: HashSet<String> = HashSet::new();
    for entry in entries {
        if confirmed.contains(&entry.domain) {
            live.push(entry);
            continue;
        }
        if denied.contains(&entry.domain) {
            let error = format!("domain '{}' is not ACTIVE or does not exist", entry.domain);
            rejected.push(reject(entry, error));
            continue;
        }
        match require_active_domain(clients, liveness, &table, &entry.domain).await {
            Ok(()) => {
                confirmed.insert(entry.domain.clone());
                live.push(entry);
            }
            Err(e) => {
                denied.insert(entry.domain.clone());
                rejected.push(reject(entry, format!("{e:#}")));
            }
        }
    }
    (live, rejected)
}

fn chunk_by_bus(entries: Vec<PreparedEntry>) -> Vec<Vec<PreparedEntry>> {
    // Sorted by bus name, so the order in which requests are spawned — and
    // therefore which one is polled first — is deterministic across runs.
    let mut by_bus: std::collections::BTreeMap<String, Vec<(usize, PreparedEntry)>> =
        std::collections::BTreeMap::new();
    for entry in entries {
        by_bus
            .entry(
                entry
                    .request_entry
                    .event_bus_name()
                    .unwrap_or_default()
                    .to_string(),
            )
            .or_default()
            .push((entry.size_bytes, entry));
    }
    by_bus
        .into_values()
        .flat_map(chunk_put_events_entries)
        .collect()
}

/// Sends every chunk concurrently, confirming each chunk's domains
/// immediately before its request is dispatched, so a confirmation never
/// outlives its window across other domains' awaited reads. A task that
/// fails to complete (panic or cancellation) takes its chunk's entries with
/// it, so the message ids are recorded before spawning and returned alongside
/// the outcomes; the caller reports them for retry rather than letting the
/// event source mapping delete them unreported.
async fn send_chunks<C: Clock>(
    clients: &Clients,
    liveness: &DomainLivenessCache<C>,
    prepared: Vec<PreparedEntry>,
    recheck: bool,
) -> (Vec<ChunkOutcome>, Vec<(String, String)>) {
    let mut set = tokio::task::JoinSet::new();
    let mut chunk_ids: HashMap<tokio::task::Id, Vec<String>> = HashMap::new();
    let mut outcomes = Vec::new();
    for chunk in chunk_by_bus(prepared) {
        let chunk = if recheck {
            let (live, rejected) = keep_entries_with_live_domains(clients, liveness, chunk).await;
            outcomes.extend(rejected);
            live
        } else {
            chunk
        };
        if chunk.is_empty() {
            continue;
        }
        let eb = clients.eb.clone();
        let ids = chunk
            .iter()
            .map(|entry| entry.message_id.clone())
            .collect::<Vec<_>>();
        let task = set.spawn(async move { send_chunk(&eb, chunk).await });
        chunk_ids.insert(task.id(), ids);
    }
    let mut lost = Vec::new();
    while let Some(joined) = set.join_next_with_id().await {
        match joined {
            Ok((id, chunk_outcomes)) => {
                chunk_ids.remove(&id);
                outcomes.extend(chunk_outcomes);
            }
            Err(e) => {
                let ids = chunk_ids.remove(&e.id()).unwrap_or_default();
                tracing::error!(
                    error = %e,
                    message_ids = ?ids,
                    "a send task failed to complete; reporting its messages for retry"
                );
                lost.extend(ids.into_iter().map(|id| (id, e.to_string())));
            }
        }
    }
    (outcomes, lost)
}

async fn send_chunk(
    eb: &aws_sdk_eventbridge::Client,
    chunk: Vec<PreparedEntry>,
) -> Vec<ChunkOutcome> {
    let entries: Vec<PutEventsRequestEntry> = chunk
        .iter()
        .map(|entry| entry.request_entry.clone())
        .collect();
    // Opaque SDK retries are disabled: a delayed automatic retry could
    // publish past a domain's disable window. Retries belong to the
    // invocation's round loop, which re-checks liveness between rounds.
    let response = eb
        .put_events()
        .set_entries(Some(entries))
        .customize()
        .config_override(
            aws_sdk_eventbridge::config::Builder::default()
                .retry_config(RetryConfig::standard().with_max_attempts(1)),
        )
        .send()
        .await;
    let accepted_ms = chrono::Utc::now().timestamp_millis();
    match response {
        Ok(output) => {
            let results = output.entries();
            chunk
                .into_iter()
                .enumerate()
                .map(|(index, entry)| {
                    let result = match results.get(index) {
                        Some(result_entry) => match result_entry.error_code() {
                            None => EntrySendResult::Accepted,
                            Some(THROTTLING_CODE) => EntrySendResult::Throttled,
                            Some(code) => {
                                let message = result_entry.error_message().unwrap_or_default();
                                tracing::warn!(
                                    metric = "PublishPutEventsRejected",
                                    error_code = code,
                                    "EventBridge rejected the entry"
                                );
                                EntrySendResult::Failed(format!("{code}: {message}"))
                            }
                        },
                        None => EntrySendResult::Failed(
                            "PutEvents returned no result entry".to_string(),
                        ),
                    };
                    ChunkOutcome {
                        entry,
                        result,
                        accepted_ms,
                    }
                })
                .collect()
        }
        Err(e) => {
            let service_error = e.into_service_error();
            let throttled = service_error.code() == Some(THROTTLING_CODE);
            let message = service_error
                .message()
                .unwrap_or("unknown error")
                .to_string();
            chunk
                .into_iter()
                .map(|entry| ChunkOutcome {
                    entry,
                    result: if throttled {
                        EntrySendResult::Throttled
                    } else {
                        EntrySendResult::Failed(message.clone())
                    },
                    accepted_ms,
                })
                .collect()
        }
    }
}

/// Builds one visibility-change entry, dropping entries whose receipt handle
/// is missing or rejected by the SDK (the standard visibility timeout then
/// applies, which only delays the redrive).
fn visibility_entry(
    jitter: &dyn JitterSource,
    entry: &PreparedEntry,
) -> Option<aws_sdk_sqs::types::ChangeMessageVisibilityBatchRequestEntry> {
    let Some(receipt) = entry.receipt_handle.as_ref() else {
        tracing::warn!(
            message_id = %entry.message_id,
            "missing receipt handle; the standard visibility timeout applies"
        );
        return None;
    };
    match aws_sdk_sqs::types::ChangeMessageVisibilityBatchRequestEntry::builder()
        .id(&entry.message_id)
        .receipt_handle(receipt)
        .visibility_timeout(visibility_backoff(jitter, entry.receive_count).as_secs() as i32)
        .build()
    {
        Ok(batch_entry) => Some(batch_entry),
        Err(e) => {
            tracing::warn!(
                error = %e,
                "could not build a visibility change; the standard visibility timeout applies"
            );
            None
        }
    }
}

/// Reschedules still-throttled messages with a short per-message visibility
/// backoff instead of the queue's full visibility timeout. Each batch call
/// is bounded by the invocation's remaining time minus the response reserve
/// and skipped when none remains, so the best-effort change can never cost
/// the invocation its response or the acknowledgements of already accepted
/// entries. The
/// messages are still reported for retry; the event source mapping leaves
/// the visibility of failed messages alone, so a shorter timeout holds. Any
/// failure here only falls back to the standard timing — no message is lost.
async fn shorten_visibility(
    clients: &Clients,
    jitter: &dyn JitterSource,
    throttled: &[PreparedEntry],
    remaining: &dyn Fn() -> Duration,
) {
    let Ok(queue_url) = std::env::var("PUBLISH_QUEUE_URL") else {
        tracing::warn!(
            "PUBLISH_QUEUE_URL not set; throttled messages keep the standard visibility timeout"
        );
        return;
    };
    for group in throttled.chunks(10) {
        let entries: Vec<_> = group
            .iter()
            .filter_map(|entry| visibility_entry(jitter, entry))
            .collect();
        if entries.is_empty() {
            continue;
        }
        if !send_within_remaining_time(clients, &queue_url, entries, remaining).await {
            return;
        }
    }
}

fn visibility_budget(remaining: Duration) -> Duration {
    remaining.saturating_sub(RESPONSE_TIME_MARGIN)
}

/// Sends one batch of visibility changes inside the invocation's remaining
/// time minus the response reserve. `false` means no time outside the
/// reserve is left — not for this batch nor any later one — so the caller
/// stops and the standard visibility timeout applies.
async fn send_within_remaining_time(
    clients: &Clients,
    queue_url: &str,
    entries: Vec<aws_sdk_sqs::types::ChangeMessageVisibilityBatchRequestEntry>,
    remaining: &dyn Fn() -> Duration,
) -> bool {
    let budget = visibility_budget(remaining());
    if budget.is_zero() {
        tracing::warn!(
            "no invocation time left for a visibility change; the standard visibility timeout applies"
        );
        return false;
    }
    match tokio::time::timeout(budget, send_visibility_batch(clients, queue_url, entries)).await {
        Ok(()) => true,
        Err(_elapsed) => {
            tracing::warn!(
                "the invocation ran out of time during the visibility change; the standard visibility timeout applies"
            );
            false
        }
    }
}

/// Sends one batch of visibility changes and logs any entry-level or
/// request-level rejection; every failure just keeps the standard timeout.
async fn send_visibility_batch(
    clients: &Clients,
    queue_url: &str,
    entries: Vec<aws_sdk_sqs::types::ChangeMessageVisibilityBatchRequestEntry>,
) {
    match clients
        .sqs
        .change_message_visibility_batch()
        .queue_url(queue_url)
        .set_entries(Some(entries))
        .send()
        .await
    {
        Ok(output) => {
            for failed in output.failed() {
                tracing::warn!(
                    message_id = failed.id(),
                    error = failed.message().unwrap_or(""),
                    "visibility change rejected; the standard visibility timeout applies"
                );
            }
        }
        Err(e) => tracing::warn!(
            error = %e,
            "ChangeMessageVisibilityBatch failed; the standard visibility timeout applies"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_visibility_budget_reserves_the_response_margin() {
        assert_eq!(
            visibility_budget(Duration::from_secs(5)),
            Duration::from_secs(3)
        );
        assert!(visibility_budget(Duration::from_secs(1)).is_zero());
    }
}
