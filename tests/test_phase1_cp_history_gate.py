import json
import subprocess
import sys
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT = REPO_ROOT / "scripts" / "check_phase1_cp_history.py"
WRAPPER = REPO_ROOT / "scripts" / "phase1-replacement-promotion.sh"


def run_gate(history_path, out_path=None):
    cmd = [sys.executable, str(SCRIPT), "--history", str(history_path)]
    if out_path is not None:
        cmd.extend(["--out", str(out_path)])
    return subprocess.run(
        cmd,
        cwd=REPO_ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def raw_history(*, input_count=1, decision_count=1, signature_verified=True):
    signed_decisions = []
    if decision_count:
        signed_decisions.append(
            {
                "sha256": "d" * 64,
                "signature": {
                    "present": True,
                    "signature_verified": signature_verified,
                },
            }
        )

    return {
        "schema_version": "ao2.cp-phase1-promotion-history.v1",
        "counts": {
            "promotion_input_verifications": input_count,
            "signed_decisions": decision_count,
        },
        "latest": {
            "promotion_inputs_verification_sha256": "a" * 64 if input_count else None,
            "decision_sha256": "d" * 64 if decision_count else None,
        },
        "history": {
            "promotion_input_verifications": [
                {"sha256": "a" * 64}
            ]
            if input_count
            else [],
            "signed_decisions": signed_decisions,
        },
        "trust_boundary": {
            "role": "read_only_observer",
            "mutates_ao_artifacts": False,
            "release_acceptance_owner": "factory-v3 evaluator-closer",
        },
    }


def write_json(path, payload):
    path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")


def test_cp_history_gate_accepts_raw_history(tmp_path):
    history = tmp_path / "history.json"
    out = tmp_path / "gate.json"
    write_json(history, raw_history())

    result = run_gate(history, out)

    assert result.returncode == 0, result.stderr
    report = json.loads(result.stdout)
    assert report["schema_version"] == "ao2.phase1-control-plane-history-gate.v1"
    assert report["status"] == "passed"
    assert report["promotion_input_verifications"] == 1
    assert report["signed_decisions"] == 1
    assert report["decision_signature_verified"] is True
    assert report["trust_boundary"]["control_plane_role"] == "read_only_observer"
    assert json.loads(out.read_text(encoding="utf-8")) == report


def test_cp_history_gate_accepts_ao2_fetch_wrapper_shape(tmp_path):
    history = tmp_path / "wrapped-history.json"
    write_json(
        history,
        {
            "schema_version": "ao2.phase1-promotion-history-control-plane-fetch.v1",
            "history": raw_history(),
        },
    )

    result = run_gate(history)

    assert result.returncode == 0, result.stderr
    report = json.loads(result.stdout)
    assert report["status"] == "passed"
    assert report["latest_promotion_inputs_verification_sha256"] == "a" * 64
    assert report["latest_decision_sha256"] == "d" * 64


def test_cp_history_gate_fails_when_input_verification_is_missing(tmp_path):
    history = tmp_path / "history.json"
    write_json(history, raw_history(input_count=0))

    result = run_gate(history)

    assert result.returncode == 1
    report = json.loads(result.stdout)
    assert report["status"] == "failed"
    assert "promotion_input_verifications" in report["failures"]


def test_cp_history_gate_fails_when_signed_decision_is_missing(tmp_path):
    history = tmp_path / "history.json"
    write_json(history, raw_history(decision_count=0))

    result = run_gate(history)

    assert result.returncode == 1
    report = json.loads(result.stdout)
    assert report["status"] == "failed"
    assert "signed_decisions" in report["failures"]


def test_cp_history_gate_fails_when_latest_decision_signature_is_unverified(tmp_path):
    history = tmp_path / "history.json"
    write_json(history, raw_history(signature_verified=False))

    result = run_gate(history)

    assert result.returncode == 1
    report = json.loads(result.stdout)
    assert report["status"] == "failed"
    assert "latest_decision_signature_verified" in report["failures"]


def test_phase1_wrapper_invokes_cp_history_gate_after_publish():
    wrapper = WRAPPER.read_text(encoding="utf-8")

    assert "scripts/check_phase1_cp_history.py" in wrapper
    assert '--api-token-env "$AO2_PHASE1_API_TOKEN_ENV"' in wrapper
    assert "phase1_cp_history_gate=" in wrapper
