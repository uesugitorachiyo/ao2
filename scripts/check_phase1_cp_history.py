#!/usr/bin/env python3
"""Fail-closed readback gate for Phase 1 promotion history."""

import argparse
import json
import os
import sys
import urllib.error
import urllib.request
from pathlib import Path


RAW_HISTORY_SCHEMA = "ao2.cp-phase1-promotion-history.v1"
FETCH_WRAPPER_SCHEMA = "ao2.phase1-promotion-history-control-plane-fetch.v1"
REPORT_SCHEMA = "ao2.phase1-control-plane-history-gate.v1"


def load_history_from_path(path):
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def load_history_from_control_plane(control_plane_url, api_token_env):
    token = os.environ.get(api_token_env or "")
    if not token:
        raise ValueError(f"missing API token env var: {api_token_env}")

    endpoint = control_plane_url.rstrip("/") + "/api/v1/phase1/promotion/history.json"
    request = urllib.request.Request(endpoint)
    request.add_header("Authorization", f"Bearer {token}")
    request.add_header("Accept", "application/json")
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.loads(response.read().decode("utf-8"))


def normalize_history(payload):
    schema = payload.get("schema_version")
    if schema == RAW_HISTORY_SCHEMA:
        return payload
    if schema == FETCH_WRAPPER_SCHEMA and isinstance(payload.get("history"), dict):
        nested = payload["history"]
        if nested.get("schema_version") == RAW_HISTORY_SCHEMA:
            return nested
    raise ValueError(f"unsupported history schema: {schema}")


def latest_signed_decision(history):
    signed_decisions = history.get("history", {}).get("signed_decisions", [])
    if not signed_decisions:
        return {}
    latest_sha = history.get("latest", {}).get("decision_sha256")
    if latest_sha:
        for decision in signed_decisions:
            if decision.get("sha256") == latest_sha:
                return decision
    return signed_decisions[-1]


def build_report(history):
    counts = history.get("counts", {})
    latest = history.get("latest", {})
    input_count = int(counts.get("promotion_input_verifications") or 0)
    decision_count = int(counts.get("signed_decisions") or 0)
    decision = latest_signed_decision(history)
    signature = decision.get("signature", {}) if isinstance(decision, dict) else {}
    signature_verified = signature.get("signature_verified") is True

    failures = []
    if input_count < 1:
        failures.append("promotion_input_verifications")
    if decision_count < 1:
        failures.append("signed_decisions")
    if decision_count >= 1 and not signature_verified:
        failures.append("latest_decision_signature_verified")

    return {
        "schema_version": REPORT_SCHEMA,
        "status": "failed" if failures else "passed",
        "failures": failures,
        "promotion_input_verifications": input_count,
        "signed_decisions": decision_count,
        "latest_promotion_inputs_verification_sha256": latest.get(
            "promotion_inputs_verification_sha256"
        ),
        "latest_decision_sha256": latest.get("decision_sha256"),
        "decision_signature_verified": signature_verified,
        "trust_boundary": {
            "control_plane_role": "read_only_observer",
            "mutates_ao_artifacts": False,
            "release_acceptance_owner": "factory-v3 evaluator-closer",
            "control_plane_approves_release": False,
        },
    }


def write_report(report, out):
    text = json.dumps(report, indent=2, sort_keys=True) + "\n"
    if out:
        path = Path(out)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
    sys.stdout.write(text)


def parse_args():
    parser = argparse.ArgumentParser(
        description="Verify control-plane Phase 1 history contains the publish readback prerequisites."
    )
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--history", help="Path to raw CP history or ao2 fetch-wrapper JSON.")
    source.add_argument("--control-plane-url", help="Control-plane base URL.")
    parser.add_argument(
        "--api-token-env",
        help="Environment variable containing the bearer token. Required with --control-plane-url.",
    )
    parser.add_argument("--out", help="Optional path to write the gate report.")
    return parser.parse_args()


def main():
    args = parse_args()
    try:
        if args.history:
            payload = load_history_from_path(args.history)
        else:
            if not args.api_token_env:
                raise ValueError("--api-token-env is required with --control-plane-url")
            payload = load_history_from_control_plane(args.control_plane_url, args.api_token_env)
        history = normalize_history(payload)
        report = build_report(history)
    except (OSError, ValueError, json.JSONDecodeError, urllib.error.URLError) as exc:
        report = {
            "schema_version": REPORT_SCHEMA,
            "status": "failed",
            "failures": ["history_readback"],
            "error": str(exc),
            "trust_boundary": {
                "control_plane_role": "read_only_observer",
                "mutates_ao_artifacts": False,
                "release_acceptance_owner": "factory-v3 evaluator-closer",
                "control_plane_approves_release": False,
            },
        }

    write_report(report, args.out)
    return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
