use zeroize::Zeroizing;
use zhuangsheng_core::application::secret::SecretStoreSessionView;

use super::SecretStoreError;

pub const SESSION_IDLE_TIMEOUT_MS: i64 = 15 * 60 * 1000;
const UNLOCK_WINDOW_MS: i64 = 60 * 1000;
const UNLOCK_BACKOFF_MS: i64 = 30 * 1000;
const MAX_UNLOCK_FAILURES: u32 = 5;

pub(crate) struct SecretSessionRegistry {
    process_generation: String,
    generation: u64,
    active: Option<ActiveSession>,
    unlock_window_started_at: i64,
    unlock_failures: u32,
    unlock_blocked_until: i64,
}

struct ActiveSession {
    session_id: String,
    store_id: String,
    data_key: Zeroizing<[u8; 32]>,
    expires_at: i64,
}

pub(crate) struct ActiveSessionKey {
    pub session: SecretStoreSessionView,
    pub data_key: Zeroizing<[u8; 32]>,
}

impl SecretSessionRegistry {
    pub fn new(process_generation: String) -> Self {
        Self {
            process_generation,
            generation: 0,
            active: None,
            unlock_window_started_at: 0,
            unlock_failures: 0,
            unlock_blocked_until: 0,
        }
    }

    pub fn process_generation(&self) -> &str {
        &self.process_generation
    }

    pub fn install(
        &mut self,
        store_id: String,
        session_id: String,
        data_key: Zeroizing<[u8; 32]>,
        now: i64,
    ) -> SecretStoreSessionView {
        self.generation = self.generation.saturating_add(1);
        let expires_at = now.saturating_add(SESSION_IDLE_TIMEOUT_MS);
        self.active = Some(ActiveSession {
            session_id: session_id.clone(),
            store_id: store_id.clone(),
            data_key,
            expires_at,
        });
        SecretStoreSessionView {
            store_id,
            format_version: 1,
            session_id,
            expires_at,
        }
    }

    pub fn active(
        &mut self,
        expected_session_id: Option<&str>,
        now: i64,
    ) -> Result<ActiveSessionKey, SecretStoreError> {
        self.expire_if_needed(now);
        let active = self.active.as_mut().ok_or(SecretStoreError::Locked)?;
        if expected_session_id.is_some_and(|expected| expected != active.session_id) {
            return Err(SecretStoreError::SessionExpired);
        }
        active.expires_at = now.saturating_add(SESSION_IDLE_TIMEOUT_MS);
        Ok(ActiveSessionKey {
            session: SecretStoreSessionView {
                store_id: active.store_id.clone(),
                format_version: 1,
                session_id: active.session_id.clone(),
                expires_at: active.expires_at,
            },
            data_key: active.data_key.clone(),
        })
    }

    pub fn is_locked(&mut self, now: i64) -> bool {
        self.expire_if_needed(now);
        self.active.is_none()
    }

    pub fn current_session_id(&mut self, now: i64) -> Option<String> {
        self.expire_if_needed(now);
        self.active.as_ref().map(|active| active.session_id.clone())
    }

    pub fn check_unlock_rate(&mut self, now: i64) -> Result<(), SecretStoreError> {
        if self.unlock_blocked_until > now {
            return Err(SecretStoreError::RateLimited);
        }
        if now.saturating_sub(self.unlock_window_started_at) >= UNLOCK_WINDOW_MS {
            self.unlock_window_started_at = now;
            self.unlock_failures = 0;
            self.unlock_blocked_until = 0;
        }
        Ok(())
    }

    pub fn record_unlock_failure(&mut self, now: i64) {
        if now.saturating_sub(self.unlock_window_started_at) >= UNLOCK_WINDOW_MS {
            self.unlock_window_started_at = now;
            self.unlock_failures = 0;
        }
        self.unlock_failures = self.unlock_failures.saturating_add(1);
        if self.unlock_failures >= MAX_UNLOCK_FAILURES {
            self.unlock_blocked_until = now.saturating_add(UNLOCK_BACKOFF_MS);
        }
    }

    pub fn record_unlock_success(&mut self) {
        self.unlock_failures = 0;
        self.unlock_blocked_until = 0;
    }

    pub fn lock(&mut self, expected_session_id: Option<&str>, now: i64) -> bool {
        self.expire_if_needed(now);
        if expected_session_id.is_some_and(|expected| {
            self.active
                .as_ref()
                .is_none_or(|active| active.session_id != expected)
        }) {
            return false;
        }
        self.generation = self.generation.saturating_add(1);
        self.active.take();
        true
    }

    fn expire_if_needed(&mut self, now: i64) {
        if self
            .active
            .as_ref()
            .is_some_and(|active| active.expires_at <= now)
        {
            self.generation = self.generation.saturating_add(1);
            self.active.take();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_expiry_and_unlock_rate_limit_are_process_local() {
        let mut registry = SecretSessionRegistry::new("process-1".into());
        let session = registry.install(
            "store-1".into(),
            "session-1".into(),
            Zeroizing::new([7_u8; 32]),
            1_000,
        );
        assert!(registry.active(Some(&session.session_id), 1_001).is_ok());
        assert!(registry.is_locked(1_001 + SESSION_IDLE_TIMEOUT_MS));
        for _ in 0..MAX_UNLOCK_FAILURES {
            registry.record_unlock_failure(2_000);
        }
        assert!(matches!(
            registry.check_unlock_rate(2_001),
            Err(SecretStoreError::RateLimited)
        ));
        assert!(
            registry
                .check_unlock_rate(2_000 + UNLOCK_BACKOFF_MS)
                .is_ok()
        );
    }
}
