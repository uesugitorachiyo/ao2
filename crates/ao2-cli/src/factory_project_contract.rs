use crate::cli_util::json_string;

pub(crate) fn factory_project_start_bundle_verify_trust_boundary(
    value: &serde_json::Value,
) -> bool {
    let trust = if value.get("trust_boundary").is_some() {
        &value["trust_boundary"]
    } else if value.get("factory_replacement_boundary").is_some() {
        &value["factory_replacement_boundary"]
    } else {
        value
    };
    json_string(trust, "release_acceptance_owner") == "factory-v3 evaluator-closer"
        && json_string(trust, "factory_v3_role") == "parity_oracle_only"
        && json_string(trust, "control_plane_role") == "read_only_observer_after_signed_evidence"
        && trust
            .get("control_plane_approves_release")
            .and_then(serde_json::Value::as_bool)
            == Some(false)
        && trust
            .get("mutates_ao_artifacts")
            .and_then(serde_json::Value::as_bool)
            == Some(false)
}
