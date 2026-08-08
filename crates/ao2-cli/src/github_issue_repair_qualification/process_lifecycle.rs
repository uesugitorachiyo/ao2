use super::{is_digest, validate_fresh_timestamp};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(super) struct ProcessLifecycle {
    pub(super) completed_at: String,
    pub(super) evidence_sha256: String,
    pub(super) process_death_observed: bool,
    pub(super) list_tools_failure_typed: bool,
    pub(super) tool_call_failure_typed: bool,
    pub(super) lifecycle_wakeup_observed: bool,
    pub(super) disconnected_state_truthful: bool,
    pub(super) explicit_close_passed: bool,
    pub(super) repeated_close_passed: bool,
    pub(super) initialization_failure_passed: bool,
    pub(super) reinitialization_passed: bool,
    pub(super) orphan_processes: u64,
    pub(super) timeout_seconds: u64,
}

pub(super) fn validate(value: &ProcessLifecycle) -> Result<()> {
    validate_fresh_timestamp(&value.completed_at)?;
    if !is_digest(&value.evidence_sha256) {
        bail!("process lifecycle evidence digest must use lowercase sha256:<64 hex>");
    }
    if value.timeout_seconds == 0 || value.timeout_seconds > 300 {
        bail!("process lifecycle timeout must be between 1 and 300 seconds");
    }
    if !value.process_death_observed
        || !value.list_tools_failure_typed
        || !value.tool_call_failure_typed
        || !value.lifecycle_wakeup_observed
        || !value.disconnected_state_truthful
        || !value.explicit_close_passed
        || !value.repeated_close_passed
        || !value.initialization_failure_passed
        || !value.reinitialization_passed
        || value.orphan_processes != 0
    {
        bail!("process lifecycle evidence is incomplete or failed");
    }
    Ok(())
}
