//! Capacity model for the metadata composition hot paths. These are the
//! local, closed-form checks that run in `cargo test` without touching AWS;
//! they quantify the design's load claims until an authorized live run
//! replaces the stated assumptions with measured telemetry.
//!
//! Every constant below is either an AWS-documented throughput limit or an
//! explicitly named modelling assumption. The assumptions are conservative
//! placeholders: replace them with the deployment's real telemetry when the
//! separately-authorized live benchmark runs.

/// AWS-documented sustained read throughput per DynamoDB partition:
/// 3,000 read-capacity units per second (strongly consistent).
pub const DDB_PARTITION_RCU_PER_SEC: f64 = 3000.0;

/// One admission performs exactly one strongly consistent point read of the
/// environment pointer item. Pointer items are a few hundred bytes, so each
/// read consumes one read-capacity unit (the ≤4KB granularity floor).
pub const POINTER_READ_RCU_PER_EVENT: f64 = 1.0;

/// Modelling assumption: sustained publish rate per environment. Replace
/// with measured telemetry at the authorized live run.
pub const ASSUMED_SUSTAINED_EVENTS_PER_SEC: f64 = 50.0;

/// Modelling assumption: short peak publish rate per environment (10× the
/// sustained rate, which is the burst pattern publish queues typically see).
pub const ASSUMED_PEAK_EVENTS_PER_SEC: f64 = 500.0;

/// Modelling assumption: schemas in one environment's registry (the catalog
/// size whose activation cold start the burst probe models).
pub const ASSUMED_REGISTRY_SCHEMAS: usize = 40;

/// All of an environment's admissions read the same pointer item, which
/// lives on a single partition: the read ceiling for one environment is the
/// partition's sustained read throughput.
pub fn pointer_read_ceiling_events_per_sec() -> f64 {
    DDB_PARTITION_RCU_PER_SEC / POINTER_READ_RCU_PER_EVENT
}

/// How many times over the assumed event rate fits under the pointer-read
/// ceiling. Below 1.0 means the environment's admissions would saturate the
/// pointer item's partition and admission would start failing closed (503).
pub fn pointer_read_headroom(events_per_sec: f64) -> f64 {
    pointer_read_ceiling_events_per_sec() / events_per_sec
}

/// After an activation advances the pointer's generation, the first publish
/// of each schema refreshes its registry entry once. The activation cold
/// start is therefore one `DescribeSchema` per schema in the registry —
/// bounded by the schema count, never by the event rate.
pub fn registry_burst_calls(schema_count: usize) -> usize {
    schema_count
}

/// The model envelope reported alongside every benchmark run, so the raw
/// probe numbers always travel with the assumptions and limits they are
/// compared against.
pub fn model_summary() -> serde_json::Value {
    serde_json::json!({
        "pointerReadRcuPerEvent": POINTER_READ_RCU_PER_EVENT,
        "dynamoPartitionRcuPerSec": DDB_PARTITION_RCU_PER_SEC,
        "pointerReadCeilingEventsPerSec": pointer_read_ceiling_events_per_sec(),
        "assumedSustainedEventsPerSec": ASSUMED_SUSTAINED_EVENTS_PER_SEC,
        "sustainedHeadroom": pointer_read_headroom(ASSUMED_SUSTAINED_EVENTS_PER_SEC),
        "assumedPeakEventsPerSec": ASSUMED_PEAK_EVENTS_PER_SEC,
        "peakHeadroom": pointer_read_headroom(ASSUMED_PEAK_EVENTS_PER_SEC),
        "assumedRegistrySchemas": ASSUMED_REGISTRY_SCHEMAS,
        "activationBurstCalls": registry_burst_calls(ASSUMED_REGISTRY_SCHEMAS),
        "liveRun": "live load tests require separate authorization; these are modelled limits",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_reads_are_one_rcu_per_admission() {
        assert_eq!(POINTER_READ_RCU_PER_EVENT, 1.0);
        assert_eq!(pointer_read_ceiling_events_per_sec(), 3000.0);
    }

    #[test]
    fn assumed_sustained_rate_leaves_partition_headroom() {
        let headroom = pointer_read_headroom(ASSUMED_SUSTAINED_EVENTS_PER_SEC);
        assert_eq!(headroom, 60.0);
        assert!(
            headroom >= 20.0,
            "sustained pointer reads should keep ≥20× partition headroom"
        );
    }

    #[test]
    fn assumed_peak_rate_leaves_partition_headroom() {
        let headroom = pointer_read_headroom(ASSUMED_PEAK_EVENTS_PER_SEC);
        assert_eq!(headroom, 6.0);
        assert!(
            headroom >= 2.0,
            "peak pointer reads should keep ≥2× partition headroom"
        );
    }

    #[test]
    fn headroom_below_one_is_the_fail_closed_boundary() {
        let ceiling = pointer_read_ceiling_events_per_sec();
        assert!(pointer_read_headroom(ceiling) < 1.01);
        // Beyond the ceiling the model says admissions saturate the pointer
        // partition; the recorded operational answer is 503 fail-closed, not
        // a pointer cache.
        assert!(pointer_read_headroom(ceiling * 2.0) < 1.0);
    }

    #[test]
    fn activation_burst_is_bounded_by_schema_count() {
        assert_eq!(registry_burst_calls(ASSUMED_REGISTRY_SCHEMAS), 40);
        assert_eq!(registry_burst_calls(0), 0);
        // The burst scales with the catalog, not with the event rate: double
        // the schemas, double the calls — nothing multiplicative.
        assert_eq!(
            registry_burst_calls(ASSUMED_REGISTRY_SCHEMAS * 2),
            ASSUMED_REGISTRY_SCHEMAS * 2
        );
    }
}
