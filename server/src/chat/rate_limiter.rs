use std::num::NonZeroU32;

use common::consts::{MAX_MESSAGES_PER_SECOND, MESSAGE_BURST_CAPACITY};
use governor::{
    Quota, RateLimiter as GovRateLimiter,
    clock::DefaultClock,
    state::{InMemoryState, NotKeyed},
};

type DirectRateLimiter = GovRateLimiter<NotKeyed, InMemoryState, DefaultClock>;

#[derive(Debug)]
pub struct RateLimiter {
    inner: DirectRateLimiter,
}

impl RateLimiter {
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(MAX_MESSAGES_PER_SECOND, MESSAGE_BURST_CAPACITY)
    }

    #[must_use]
    pub fn with_config(rate_per_second: u32, burst_capacity: u32) -> Self {
        let rate = NonZeroU32::new(rate_per_second).unwrap_or(NonZeroU32::MIN);
        let burst = NonZeroU32::new(burst_capacity).unwrap_or(NonZeroU32::MIN);

        let quota = Quota::per_second(rate).allow_burst(burst);
        let limiter = GovRateLimiter::direct(quota);

        Self { inner: limiter }
    }

    #[must_use]
    #[allow(dead_code)]
    pub fn try_acquire(&self) -> bool {
        self.inner.check().is_ok()
    }

    pub async fn acquire(&self) {
        self.inner.until_ready().await;
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::{thread, time::Duration};

    use super::*;

    #[test]
    fn test_new_rate_limiter_has_tokens() {
        let limiter = RateLimiter::new();
        assert!(limiter.try_acquire());
    }

    #[test]
    fn test_acquire_consumes_token() {
        let limiter = RateLimiter::with_config(10, 5);

        for _ in 0..5 {
            assert!(limiter.try_acquire());
        }

        assert!(!limiter.try_acquire());
    }

    #[test]
    fn test_exhausted_bucket_rejects() {
        let limiter = RateLimiter::with_config(1, 2);

        assert!(limiter.try_acquire());
        assert!(limiter.try_acquire());

        assert!(!limiter.try_acquire());
    }

    #[test]
    fn test_refill_over_time() {
        let limiter = RateLimiter::with_config(100, 10);

        for _ in 0..10 {
            let _ = limiter.try_acquire();
        }
        assert!(!limiter.try_acquire());

        thread::sleep(Duration::from_millis(150));

        assert!(limiter.try_acquire());
    }

    #[test]
    fn test_default_impl() {
        let limiter = RateLimiter::default();

        assert!(limiter.try_acquire());
    }

    #[test]
    fn test_zero_rate_uses_minimum() {
        let limiter = RateLimiter::with_config(0, 0);

        assert!(limiter.try_acquire());
    }

    #[tokio::test]
    async fn test_async_acquire_throttles() {
        let limiter = RateLimiter::with_config(10, 1);

        limiter.acquire().await;

        let start = std::time::Instant::now();

        limiter.acquire().await;
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_millis() >= 90,
            "Should handle throttling, waited {elapsed:?}"
        );
    }
}
