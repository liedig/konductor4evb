//! Jittered backoff for the publish path's throttled-delivery recovery.
//! Randomness comes from a small [`JitterSource`] trait, uuid-v4-backed in
//! production and deterministic in tests, so no `rand` dependency is needed.

use std::time::Duration;

const RETRY_BASE: Duration = Duration::from_millis(100);
const RETRY_CAP: Duration = Duration::from_secs(1);
const MAX_VISIBILITY_BACKOFF_SECS: u64 = 60;

/// Randomness for jittered delays. Implementations return a uniformly random
/// duration within the given bound.
pub trait JitterSource: Send + Sync {
    /// A uniformly random duration in `[0, max]`.
    fn up_to(&self, max: Duration) -> Duration;
}

/// Production jitter source backed by uuid-v4 random bytes.
pub struct UuidJitter;

impl JitterSource for UuidJitter {
    fn up_to(&self, max: Duration) -> Duration {
        let bytes = uuid::Uuid::new_v4();
        let b = bytes.as_bytes();
        let raw = u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]);
        max.mul_f64(raw as f64 / u64::MAX as f64)
    }
}

/// Full-jitter delay before in-invocation retry `attempt` (1-based):
/// uniform within `[0, min(1s, 100ms * 2^(attempt-1))]`.
pub fn retry_delay(jitter: &dyn JitterSource, attempt: u32) -> Duration {
    let shift = attempt.saturating_sub(1).min(13);
    let budget = RETRY_BASE.saturating_mul(1 << shift).min(RETRY_CAP);
    jitter.up_to(budget)
}

/// Redelivery delay for a throttled message seen `receive_count` times
/// (1-based): `d/2 + jitter(0, d/2)` with `d = min(60, 2^(receive_count-1))`
/// seconds, rounded up to whole seconds and never below one second.
pub fn visibility_backoff(jitter: &dyn JitterSource, receive_count: u32) -> Duration {
    let n = receive_count.max(1);
    let d_secs = 1u64
        .checked_shl(n - 1)
        .unwrap_or(u64::MAX)
        .min(MAX_VISIBILITY_BACKOFF_SECS);
    let half = Duration::from_millis(d_secs * 500);
    let raw = half + jitter.up_to(half);
    let rounded = Duration::from_secs(raw.as_secs() + u64::from(raw.subsec_nanos() > 0));
    rounded.max(Duration::from_secs(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ZeroJitter;

    impl JitterSource for ZeroJitter {
        fn up_to(&self, _max: Duration) -> Duration {
            Duration::ZERO
        }
    }

    struct MaxJitter;

    impl JitterSource for MaxJitter {
        fn up_to(&self, max: Duration) -> Duration {
            max
        }
    }

    struct HalfJitter;

    impl JitterSource for HalfJitter {
        fn up_to(&self, max: Duration) -> Duration {
            max / 2
        }
    }

    #[test]
    fn in_invocation_retry_delay_uses_full_jitter_from_a_100ms_base_with_a_1s_cap() {
        assert_eq!(retry_delay(&ZeroJitter, 1), Duration::ZERO);
        assert_eq!(retry_delay(&MaxJitter, 1), Duration::from_millis(100));
        assert_eq!(retry_delay(&MaxJitter, 2), Duration::from_millis(200));
        assert_eq!(retry_delay(&MaxJitter, 3), Duration::from_millis(400));
        assert_eq!(retry_delay(&MaxJitter, 4), Duration::from_millis(800));
        assert_eq!(
            retry_delay(&MaxJitter, 5),
            Duration::from_secs(1),
            "the cap binds"
        );
        assert_eq!(
            retry_delay(&MaxJitter, 30),
            Duration::from_secs(1),
            "large attempts stay capped"
        );
    }

    #[test]
    fn visibility_backoff_lies_within_half_d_and_d() {
        for receive_count in 1..=25u32 {
            let d_secs = 1u64
                .checked_shl(receive_count - 1)
                .unwrap_or(u64::MAX)
                .min(60);
            let low = visibility_backoff(&ZeroJitter, receive_count);
            let high = visibility_backoff(&MaxJitter, receive_count);
            assert!(
                low >= Duration::from_secs(d_secs / 2),
                "receive {receive_count}: minimum backoff {low:?} below half of {d_secs}s"
            );
            assert!(
                high <= Duration::from_secs(d_secs),
                "receive {receive_count}: maximum backoff {high:?} above {d_secs}s"
            );
            assert!(
                low >= Duration::from_secs(1),
                "receive {receive_count}: backoff below one second"
            );
        }
    }

    #[test]
    fn visibility_backoff_rounds_up_to_whole_seconds() {
        for receive_count in 1..=25u32 {
            for jitter in [&ZeroJitter as &dyn JitterSource, &HalfJitter, &MaxJitter] {
                let delay = visibility_backoff(jitter, receive_count);
                assert_eq!(
                    delay.subsec_nanos(),
                    0,
                    "receive {receive_count}: {delay:?} is not a whole number of seconds"
                );
            }
        }
    }

    #[test]
    fn minimum_visibility_backoffs_cover_the_ten_minute_budget() {
        let total: Duration = (1..=25u32)
            .map(|n| visibility_backoff(&ZeroJitter, n))
            .sum();
        assert!(
            total >= Duration::from_secs(600),
            "25 receives cover {total:?}, under the 10-minute bar"
        );
    }

    #[test]
    fn a_deterministic_jitter_source_yields_reproducible_delays() {
        assert_eq!(
            visibility_backoff(&HalfJitter, 3),
            visibility_backoff(&HalfJitter, 3)
        );
        assert_eq!(retry_delay(&HalfJitter, 2), retry_delay(&HalfJitter, 2));
        assert_eq!(visibility_backoff(&HalfJitter, 3), Duration::from_secs(3));
    }

    #[test]
    fn uuid_jitter_stays_within_bounds() {
        for _ in 0..100 {
            let delay = visibility_backoff(&UuidJitter, 5);
            assert!(delay >= Duration::from_secs(8) && delay <= Duration::from_secs(16));
        }
    }
}
