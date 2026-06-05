//! Coverage for `fail_on_forbidden_provider_api_keys`, the guard the runtime
//! calls at every entry point to ensure a provider-free run cannot silently
//! pick up a real API key from the ambient environment. Both the reject and
//! the allow branch are exercised.
//!
//! These tests mutate process-global environment variables, so they are
//! serialized through a mutex and each restores the pre-test value of every
//! key it touches — otherwise a developer who actually exports
//! `OPENAI_API_KEY` would see spurious failures, and parallel tests in this
//! binary would race on the shared environment.

use std::ffi::OsString;
use std::sync::Mutex;

use ao2_policy::fail_on_forbidden_provider_api_keys;

static ENV_LOCK: Mutex<()> = Mutex::new(());

const FORBIDDEN_KEYS: [&str; 2] = ["OPENAI_API_KEY", "ANTHROPIC_API_KEY"];

/// Snapshot + clear the forbidden keys, restoring them on drop so the test
/// cannot leak state into the developer's environment or sibling tests.
struct EnvSnapshot {
    saved: Vec<(&'static str, Option<OsString>)>,
}

impl EnvSnapshot {
    fn clear() -> Self {
        let saved = FORBIDDEN_KEYS
            .iter()
            .map(|key| {
                let prior = std::env::var_os(key);
                std::env::remove_var(key);
                (*key, prior)
            })
            .collect();
        Self { saved }
    }
}

impl Drop for EnvSnapshot {
    fn drop(&mut self) {
        for (key, value) in &self.saved {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

#[test]
fn rejects_when_a_forbidden_provider_key_is_present() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _env = EnvSnapshot::clear();

    std::env::set_var("OPENAI_API_KEY", "sk-should-not-be-used");
    let err = fail_on_forbidden_provider_api_keys()
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("forbidden provider API key present in environment"),
        "expected forbidden-key rejection, got: {err}"
    );
    assert!(
        err.contains("OPENAI_API_KEY"),
        "rejection should name the offending key, got: {err}"
    );
}

#[test]
fn allows_when_no_forbidden_provider_key_is_present() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _env = EnvSnapshot::clear();

    // With both forbidden keys removed, the guard must pass.
    assert!(fail_on_forbidden_provider_api_keys().is_ok());
}
