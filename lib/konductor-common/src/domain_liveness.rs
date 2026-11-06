//! Process-local cache of positive domain-liveness confirmations for the
//! publish path. Only `ACTIVE` confirmations are remembered, each for at most
//! the configured window counted from the start of the confirming read;
//! `INACTIVE` and missing domains always hit the domain store, so a
//! re-enabled or newly provisioned domain works on the very next publish.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Window applied when `DOMAIN_LIVENESS_CACHE_SECONDS` is unset or cannot be
/// parsed: five minutes.
pub const DEFAULT_DOMAIN_LIVENESS_WINDOW: Duration = Duration::from_secs(300);

/// Time source for the cache. Production uses [`SystemClock`]; tests advance
/// a manual clock instead of sleeping.
pub trait Clock: Send + Sync {
    /// The current instant.
    fn now(&self) -> Instant;
}

/// Wall-clock time source used in Lambda.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// Remembers when each domain was last confirmed `ACTIVE`. A confirmation
/// younger than the window answers the liveness check without a store read;
/// an older one is re-fetched. A window of zero disables reuse entirely, so
/// every check reads the store.
pub struct DomainLivenessCache<C: Clock = SystemClock> {
    confirmed_active_at: Mutex<HashMap<String, Instant>>,
    window: Duration,
    clock: C,
}

impl DomainLivenessCache<SystemClock> {
    /// Builds a cache with the given window on wall-clock time.
    pub fn new(window: Duration) -> Self {
        Self::with_clock(window, SystemClock)
    }

    /// Builds a cache from `DOMAIN_LIVENESS_CACHE_SECONDS`.
    pub fn from_env() -> Self {
        Self::new(window_from_env())
    }
}

impl<C: Clock> DomainLivenessCache<C> {
    /// Builds a cache with an explicit time source, for tests.
    pub fn with_clock(window: Duration, clock: C) -> Self {
        Self {
            confirmed_active_at: Mutex::new(HashMap::new()),
            window,
            clock,
        }
    }

    /// Reports whether the domain is confirmed `ACTIVE`, reusing a fresh
    /// confirmation when one exists and calling `fetch` otherwise. `fetch`
    /// reads the domain store and returns `Ok(true)` only for an `ACTIVE`
    /// domain; its error type passes through unchanged so callers keep their
    /// own mapping (for example a 404 or 503 response). Only positive
    /// results are stored, and the window starts when the read begins —
    /// not when its response arrives — so a slow read cannot extend the
    /// disable bound by its own latency.
    pub async fn confirm_active<E, F, Fut>(&self, domain: &str, fetch: F) -> Result<bool, E>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<bool, E>>,
    {
        if !self.window.is_zero() {
            if let Some(confirmed_at) = self.entries().get(domain) {
                if self.clock.now().saturating_duration_since(*confirmed_at) < self.window {
                    return Ok(true);
                }
            }
        }
        let observed_at = self.clock.now();
        let active = fetch().await?;
        if active && !self.window.is_zero() {
            self.entries().insert(domain.to_string(), observed_at);
        }
        Ok(active)
    }

    /// Whether confirmations are reused at all. With a zero window every
    /// check already reads the store, so a caller's extra "has my
    /// confirmation expired?" check before committing has nothing to add.
    pub fn reuses_confirmations(&self) -> bool {
        !self.window.is_zero()
    }

    fn entries(&self) -> MutexGuard<'_, HashMap<String, Instant>> {
        self.confirmed_active_at
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }
}

/// Reads `DOMAIN_LIVENESS_CACHE_SECONDS`: unset or unparseable values give
/// the default window, and `0` disables reuse.
pub fn window_from_env() -> Duration {
    std::env::var("DOMAIN_LIVENESS_CACHE_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_DOMAIN_LIVENESS_WINDOW)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[derive(Clone)]
    struct ManualClock(Arc<Mutex<Instant>>);

    impl ManualClock {
        fn new() -> Self {
            Self(Arc::new(Mutex::new(Instant::now())))
        }

        fn advance(&self, delta: Duration) {
            *self.0.lock().unwrap() += delta;
        }
    }

    impl Clock for ManualClock {
        fn now(&self) -> Instant {
            *self.0.lock().unwrap()
        }
    }

    fn counting_fetch(
        calls: &AtomicUsize,
        active: bool,
    ) -> impl FnOnce() -> std::future::Ready<Result<bool, String>> + '_ {
        move || {
            calls.fetch_add(1, Ordering::SeqCst);
            std::future::ready(Ok(active))
        }
    }

    #[tokio::test]
    async fn a_fresh_confirmation_is_served_without_fetching() {
        let cache = DomainLivenessCache::new(Duration::from_secs(300));
        let calls = AtomicUsize::new(0);

        assert!(cache
            .confirm_active("orders", counting_fetch(&calls, true))
            .await
            .unwrap());
        assert!(cache
            .confirm_active("orders", counting_fetch(&calls, true))
            .await
            .unwrap());

        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "the second check reuses the confirmation"
        );
    }

    #[tokio::test]
    async fn an_expired_confirmation_fetches_again() {
        let clock = ManualClock::new();
        let cache = DomainLivenessCache::with_clock(Duration::from_secs(300), clock.clone());
        let calls = AtomicUsize::new(0);

        assert!(cache
            .confirm_active("orders", counting_fetch(&calls, true))
            .await
            .unwrap());
        clock.advance(Duration::from_secs(299));
        assert!(cache
            .confirm_active("orders", counting_fetch(&calls, true))
            .await
            .unwrap());
        clock.advance(Duration::from_secs(2));
        assert!(cache
            .confirm_active("orders", counting_fetch(&calls, true))
            .await
            .unwrap());

        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "the confirmation expired one second past the window"
        );
    }

    #[tokio::test]
    async fn the_window_starts_before_the_fetch() {
        let clock = ManualClock::new();
        let cache = DomainLivenessCache::with_clock(Duration::from_secs(5), clock.clone());
        let calls = AtomicUsize::new(0);

        let slow_read = || async {
            calls.fetch_add(1, Ordering::SeqCst);
            clock.advance(Duration::from_secs(3));
            Ok::<_, String>(true)
        };
        assert!(cache.confirm_active("orders", slow_read).await.unwrap());
        clock.advance(Duration::from_millis(2_500));
        assert!(cache
            .confirm_active("orders", counting_fetch(&calls, true))
            .await
            .unwrap());

        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "the window runs from the read's start, so a slow read cannot extend the disable bound"
        );
    }

    #[tokio::test]
    async fn inactive_and_missing_results_are_never_stored() {
        let cache = DomainLivenessCache::new(Duration::from_secs(300));
        let calls = AtomicUsize::new(0);

        assert!(!cache
            .confirm_active("orders", counting_fetch(&calls, false))
            .await
            .unwrap());
        assert!(!cache
            .confirm_active("orders", counting_fetch(&calls, false))
            .await
            .unwrap());
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "negative results always hit the store"
        );

        assert!(cache
            .confirm_active("orders", counting_fetch(&calls, true))
            .await
            .unwrap());
        assert!(cache
            .confirm_active("orders", counting_fetch(&calls, true))
            .await
            .unwrap());
        assert_eq!(
            calls.load(Ordering::SeqCst),
            3,
            "a later ACTIVE confirmation is stored"
        );
    }

    #[tokio::test]
    async fn a_zero_window_always_fetches() {
        let cache = DomainLivenessCache::new(Duration::ZERO);
        let calls = AtomicUsize::new(0);

        assert!(cache
            .confirm_active("orders", counting_fetch(&calls, true))
            .await
            .unwrap());
        assert!(cache
            .confirm_active("orders", counting_fetch(&calls, true))
            .await
            .unwrap());

        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "a zero window disables reuse"
        );
    }

    #[tokio::test]
    async fn a_fetch_error_propagates_unchanged_and_stores_nothing() {
        let cache = DomainLivenessCache::new(Duration::from_secs(300));

        let err = cache
            .confirm_active("orders", || {
                std::future::ready(Err::<bool, _>("store down".to_string()))
            })
            .await
            .unwrap_err();
        assert_eq!(err, "store down");

        let calls = AtomicUsize::new(0);
        assert!(cache
            .confirm_active("orders", counting_fetch(&calls, true))
            .await
            .unwrap());
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "the failed check stored nothing"
        );
    }

    #[tokio::test]
    async fn confirmations_are_isolated_per_domain() {
        let cache = DomainLivenessCache::new(Duration::from_secs(300));
        let calls = AtomicUsize::new(0);

        assert!(cache
            .confirm_active("orders", counting_fetch(&calls, true))
            .await
            .unwrap());
        assert!(cache
            .confirm_active("payments", counting_fetch(&calls, true))
            .await
            .unwrap());
        assert!(cache
            .confirm_active("orders", counting_fetch(&calls, true))
            .await
            .unwrap());

        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "each domain is confirmed independently"
        );
    }

    #[test]
    fn the_window_comes_from_the_environment() {
        std::env::remove_var("DOMAIN_LIVENESS_CACHE_SECONDS");
        assert_eq!(window_from_env(), DEFAULT_DOMAIN_LIVENESS_WINDOW);

        std::env::set_var("DOMAIN_LIVENESS_CACHE_SECONDS", "not-a-number");
        assert_eq!(window_from_env(), DEFAULT_DOMAIN_LIVENESS_WINDOW);

        std::env::set_var("DOMAIN_LIVENESS_CACHE_SECONDS", "0");
        assert_eq!(window_from_env(), Duration::ZERO);

        std::env::set_var("DOMAIN_LIVENESS_CACHE_SECONDS", "60");
        assert_eq!(window_from_env(), Duration::from_secs(60));

        std::env::remove_var("DOMAIN_LIVENESS_CACHE_SECONDS");
    }
}
