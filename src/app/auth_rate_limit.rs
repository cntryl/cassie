use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::{normalize_role_name, CassieError};

const IDLE_EVICTION_MILLIS: u64 = 15 * 60 * 1_000;
const CREDIT_UNITS_PER_TOKEN: u128 = 60_000;

#[derive(Debug)]
pub(crate) struct AuthRateLimiter {
    user_capacity: u64,
    ip_capacity: u64,
    max_entries: usize,
    state: Mutex<LimiterState>,
}

#[derive(Debug, Default)]
struct LimiterState {
    users: HashMap<String, TokenBucket>,
    ips: HashMap<IpAddr, TokenBucket>,
    overflow_user: Option<TokenBucket>,
    overflow_ip: Option<TokenBucket>,
}

#[derive(Debug, Clone)]
struct TokenBucket {
    credits: u128,
    last_refill_millis: u64,
    last_seen_millis: u64,
}

#[derive(Debug)]
pub(crate) struct AuthAttempt {
    user: BucketTarget<String>,
    ip: BucketTarget<IpAddr>,
}

#[derive(Debug)]
enum BucketTarget<T> {
    Entry(T),
    Overflow,
}

impl AuthRateLimiter {
    pub(crate) fn new(
        user_attempts_per_minute: usize,
        ip_attempts_per_minute: usize,
        max_entries: usize,
    ) -> Self {
        Self {
            user_capacity: u64::try_from(user_attempts_per_minute.max(1)).unwrap_or(u64::MAX),
            ip_capacity: u64::try_from(ip_attempts_per_minute.max(1)).unwrap_or(u64::MAX),
            max_entries: max_entries.max(1),
            state: Mutex::new(LimiterState::default()),
        }
    }

    pub(crate) fn consume(&self, user: &str, ip: IpAddr) -> Result<AuthAttempt, CassieError> {
        self.consume_at(user, ip, unix_millis())
    }

    fn consume_at(
        &self,
        user: &str,
        ip: IpAddr,
        now_millis: u64,
    ) -> Result<AuthAttempt, CassieError> {
        let user = normalize_role_name(user);
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.evict_recovered_entries(&mut state, now_millis);

        let user_target = self.user_target(&mut state, user, now_millis);
        let ip_target = self.ip_target(&mut state, ip, now_millis);
        let user_allowed = {
            let LimiterState {
                users,
                overflow_user,
                ..
            } = &mut *state;
            consume_target(
                users,
                overflow_user,
                &user_target,
                self.user_capacity,
                now_millis,
            )
        };
        let ip_allowed = {
            let LimiterState {
                ips, overflow_ip, ..
            } = &mut *state;
            consume_target(ips, overflow_ip, &ip_target, self.ip_capacity, now_millis)
        };
        if !user_allowed || !ip_allowed {
            return Err(CassieError::AuthenticationRateLimited);
        }

        Ok(AuthAttempt {
            user: user_target,
            ip: ip_target,
        })
    }

    pub(crate) fn refund(&self, attempt: &AuthAttempt) {
        let now_millis = unix_millis();
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        {
            let LimiterState {
                users,
                overflow_user,
                ..
            } = &mut *state;
            refund_target(
                users,
                overflow_user,
                &attempt.user,
                self.user_capacity,
                now_millis,
            );
        }
        {
            let LimiterState {
                ips, overflow_ip, ..
            } = &mut *state;
            refund_target(ips, overflow_ip, &attempt.ip, self.ip_capacity, now_millis);
        }
    }

    fn user_target(
        &self,
        state: &mut LimiterState,
        user: String,
        now_millis: u64,
    ) -> BucketTarget<String> {
        if state.users.contains_key(&user) {
            return BucketTarget::Entry(user);
        }
        if state.users.len().saturating_add(state.ips.len()) < self.max_entries {
            state.users.insert(
                user.clone(),
                TokenBucket::full(self.user_capacity, now_millis),
            );
            BucketTarget::Entry(user)
        } else {
            BucketTarget::Overflow
        }
    }

    fn ip_target(
        &self,
        state: &mut LimiterState,
        ip: IpAddr,
        now_millis: u64,
    ) -> BucketTarget<IpAddr> {
        if state.ips.contains_key(&ip) {
            return BucketTarget::Entry(ip);
        }
        if state.users.len().saturating_add(state.ips.len()) < self.max_entries {
            state
                .ips
                .insert(ip, TokenBucket::full(self.ip_capacity, now_millis));
            BucketTarget::Entry(ip)
        } else {
            BucketTarget::Overflow
        }
    }

    fn evict_recovered_entries(&self, state: &mut LimiterState, now_millis: u64) {
        state.users.retain(|_, bucket| {
            bucket.refill(self.user_capacity, now_millis);
            !bucket.evictable(self.user_capacity, now_millis)
        });
        state.ips.retain(|_, bucket| {
            bucket.refill(self.ip_capacity, now_millis);
            !bucket.evictable(self.ip_capacity, now_millis)
        });
    }
}

impl TokenBucket {
    fn full(capacity: u64, now_millis: u64) -> Self {
        Self {
            credits: full_credits(capacity),
            last_refill_millis: now_millis,
            last_seen_millis: now_millis,
        }
    }

    fn refill(&mut self, capacity: u64, now_millis: u64) {
        let elapsed = now_millis.saturating_sub(self.last_refill_millis);
        let replenished = u128::from(elapsed).saturating_mul(u128::from(capacity));
        self.credits = self
            .credits
            .saturating_add(replenished)
            .min(full_credits(capacity));
        self.last_refill_millis = now_millis;
    }

    fn consume(&mut self, capacity: u64, now_millis: u64) -> bool {
        self.refill(capacity, now_millis);
        self.last_seen_millis = now_millis;
        if self.credits < CREDIT_UNITS_PER_TOKEN {
            return false;
        }
        self.credits -= CREDIT_UNITS_PER_TOKEN;
        true
    }

    fn refund(&mut self, capacity: u64, now_millis: u64) {
        self.refill(capacity, now_millis);
        self.credits = self
            .credits
            .saturating_add(CREDIT_UNITS_PER_TOKEN)
            .min(full_credits(capacity));
        self.last_seen_millis = now_millis;
    }

    fn evictable(&self, capacity: u64, now_millis: u64) -> bool {
        self.credits >= full_credits(capacity)
            || now_millis.saturating_sub(self.last_seen_millis) >= IDLE_EVICTION_MILLIS
    }
}

fn full_credits(capacity: u64) -> u128 {
    u128::from(capacity).saturating_mul(CREDIT_UNITS_PER_TOKEN)
}

fn consume_target<T: std::hash::Hash + Eq>(
    entries: &mut HashMap<T, TokenBucket>,
    overflow: &mut Option<TokenBucket>,
    target: &BucketTarget<T>,
    capacity: u64,
    now_millis: u64,
) -> bool {
    match target {
        BucketTarget::Entry(key) => entries
            .get_mut(key)
            .expect("selected authentication bucket should exist")
            .consume(capacity, now_millis),
        BucketTarget::Overflow => overflow
            .get_or_insert_with(|| TokenBucket::full(capacity, now_millis))
            .consume(capacity, now_millis),
    }
}

fn refund_target<T: std::hash::Hash + Eq>(
    entries: &mut HashMap<T, TokenBucket>,
    overflow: &mut Option<TokenBucket>,
    target: &BucketTarget<T>,
    capacity: u64,
    now_millis: u64,
) {
    match target {
        BucketTarget::Entry(key) => {
            if let Some(bucket) = entries.get_mut(key) {
                bucket.refund(capacity, now_millis);
            }
        }
        BucketTarget::Overflow => {
            if let Some(bucket) = overflow {
                bucket.refund(capacity, now_millis);
            }
        }
    }
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    const IP: IpAddr = IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);

    #[test]
    fn should_refill_authentication_buckets_with_injected_timestamps() {
        // Arrange
        let limiter = AuthRateLimiter::new(1, 1, 8);
        assert!(limiter.consume_at("reader", IP, 0).is_ok());

        // Act
        let rejected = limiter.consume_at("reader", IP, 1);
        let refilled = limiter.consume_at("reader", IP, 60_000);

        // Assert
        assert!(matches!(
            rejected,
            Err(CassieError::AuthenticationRateLimited)
        ));
        assert!(refilled.is_ok());
    }

    #[test]
    fn should_refund_successful_authentication_attempts() {
        // Arrange
        let limiter = AuthRateLimiter::new(1, 1, 8);
        let attempt = limiter.consume_at("reader", IP, 0).expect("first attempt");

        // Act
        limiter.refund(&attempt);
        let next = limiter.consume_at("reader", IP, 0);

        // Assert
        assert!(next.is_ok());
    }

    #[test]
    fn should_route_flooded_usernames_through_bounded_overflow_bucket() {
        // Arrange
        let limiter = AuthRateLimiter::new(1, 100, 1);

        // Act
        let first = limiter.consume_at("one", IP, 0);
        let second = limiter.consume_at("two", IP, 0);
        let third = limiter.consume_at("three", IP, 0);

        // Assert
        assert!(first.is_ok());
        assert!(second.is_ok());
        assert!(matches!(third, Err(CassieError::AuthenticationRateLimited)));
        let state = limiter.state.lock().expect("limiter state");
        assert!(state.users.len() + state.ips.len() <= 1);
    }
}
