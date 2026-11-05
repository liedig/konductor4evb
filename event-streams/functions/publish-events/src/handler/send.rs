//! Batched `PutEvents` delivery with throttled-delivery recovery: prepared
//! entries bound for the same bus share one request, requests for different
//! buses go out concurrently, and each failed entry maps back to its SQS
//! message by index. Each request records its own EventBridge acceptance
//! time, so a slow request never inflates another's latency metrics.
//! Throttled entries are retried inside the invocation with jittered backoff;
//! entries still throttled are rescheduled with a short per-message
//! visibility backoff that grows with the receive count, then reported for
//! retry. The visibility change is time-bounded by the invocation's
//! remaining time and skipped when none remains, so it can never cost the
//! invocation its response or the acknowledgements of accepted entries.

use std::collections::HashMap;
use std::time::Duration;

use aws_sdk_eventbridge::error::ProvideErrorMetadata;
use aws_sdk_eventbridge::types::PutEventsRequestEntry;

use konductor_common::backoff::{retry_delay, visibility_backoff, JitterSource};
use konductor_common::envelope::chunk_put_events_entries;

use crate::handler::{Clients, PreparedEntry};

/// How many times throttled entries are resent inside the invocation after
/// the first send.
const MAX_IN_INVOCATION_RETRIES: u32 = 3;

/// No retry starts when the invocation has less than this much time left.
const RETRY_TIME_MARGIN: Duration = Duration::from_secs(5);

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
/// entry-count and size limits, and sends every chunk concurrently.
/// Throttled entries are retried in the invocation while time remains;
/// entries still throttled are rescheduled with a visibility backoff and
/// reported for retry.
pub(crate) async fn deliver(
    clients: &Clients,
    jitter: &dyn JitterSource,
    prepared: Vec<PreparedEntry>,
    remaining: &dyn Fn() -> Duration,
) -> Vec<DeliveryOutcome> {
    let mut pending = prepared;
    let mut outcomes = Vec::new();
    let mut retries = 0;
    loop {
        let mut throttled = Vec::new();
        let (chunk_outcomes, lost) = send_chunks(clients, pending).await;
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

fn chunk_by_bus(entries: Vec<PreparedEntry>) -> Vec<Vec<PreparedEntry>> {
    let mut by_bus: HashMap<String, Vec<(usize, PreparedEntry)>> = HashMap::new();
    for entry in entries {
        by_bus
            .entry(entry.envelope.event_bus_name.clone())
            .or_default()
            .push((entry.size_bytes, entry));
    }
    by_bus
        .into_values()
        .flat_map(chunk_put_events_entries)
        .collect()
}

/// Sends every chunk concurrently. A task that fails to complete (panic or
/// cancellation) takes its chunk's entries with it, so the message ids are
/// recorded before spawning and returned alongside the outcomes; the caller
/// reports them for retry rather than letting the event source mapping
/// delete them unreported.
async fn send_chunks(
    clients: &Clients,
    prepared: Vec<PreparedEntry>,
) -> (Vec<ChunkOutcome>, Vec<(String, String)>) {
    let mut set = tokio::task::JoinSet::new();
    let mut chunk_ids: HashMap<tokio::task::Id, Vec<String>> = HashMap::new();
    for chunk in chunk_by_bus(prepared) {
        let eb = clients.eb.clone();
        let ids = chunk
            .iter()
            .map(|entry| entry.message_id.clone())
            .collect::<Vec<_>>();
        let task = set.spawn(async move { send_chunk(&eb, chunk).await });
        chunk_ids.insert(task.id(), ids);
    }
    let mut outcomes = Vec::new();
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
        .map(|entry| {
            PutEventsRequestEntry::builder()
                .event_bus_name(&entry.envelope.event_bus_name)
                .source(&entry.envelope.source)
                .detail_type(&entry.envelope.detail_type)
                .detail(&entry.detail_json)
                .build()
        })
        .collect();
    let response = eb.put_events().set_entries(Some(entries)).send().await;
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
/// is time-bounded by the invocation's remaining time and skipped when none
/// remains, so the best-effort change can never cost the invocation its
/// response or the acknowledgements of already accepted entries. The
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

/// Sends one batch of visibility changes inside the invocation's remaining
/// time. `false` means the invocation has no time left — not for this batch
/// nor any later one — so the caller stops and the standard visibility
/// timeout applies.
async fn send_within_remaining_time(
    clients: &Clients,
    queue_url: &str,
    entries: Vec<aws_sdk_sqs::types::ChangeMessageVisibilityBatchRequestEntry>,
    remaining: &dyn Fn() -> Duration,
) -> bool {
    let budget = remaining();
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
