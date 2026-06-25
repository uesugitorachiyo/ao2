import json
import os
import subprocess
from pathlib import Path


REPO = Path(__file__).resolve().parents[1]


def read_json(path: str) -> dict:
    return json.loads((REPO / path).read_text(encoding="utf-8"))


def read(path: str) -> str:
    return (REPO / path).read_text(encoding="utf-8")


def test_rsi_claim_readiness_audit_denies_full_self_mutating_claim(tmp_path):
    package = read_json("package.json")
    assert package["scripts"]["rsi:claim-readiness"] == (
        "node scripts/run-sh-script.js scripts/rsi-claim-readiness-audit.sh"
    )
    readme = read("README.md")
    assert "npm run rsi:claim-readiness" in readme
    assert "bounded_governed_rsi" in readme
    assert "full_autonomous_self_mutating_rsi" in readme

    out_root = tmp_path / "rsi-claim-readiness"
    result = subprocess.run(
        ["npm", "run", "rsi:claim-readiness"],
        cwd=REPO,
        env={
            **os.environ,
            "AO2_RSI_CLAIM_READINESS_ROOT": str(out_root),
        },
        capture_output=True,
        text=True,
    )

    assert result.returncode == 0, result.stderr + result.stdout
    summary_path = out_root / "summary.json"
    summary = json.loads(summary_path.read_text(encoding="utf-8"))

    assert summary["schema_version"] == "ao2.rsi-claim-readiness-audit.v1"
    assert summary["status"] == "claim_boundary_enforced"
    assert summary["claim_boundary"] == {
        "bounded_governed_rsi": "allowed",
        "full_autonomous_self_mutating_rsi": "denied",
    }

    bounded = summary["claims"]["bounded_governed_rsi"]
    assert bounded["decision"] == "allowed"
    assert bounded["evidence_state"] == "present"

    full = summary["claims"]["full_autonomous_self_mutating_rsi"]
    assert full["decision"] == "denied"
    assert full["evidence_state"] == "missing_required_evidence"
    assert {blocker["id"] for blocker in full["blockers"]} == {
        "mutation_authority",
        "rollback_evidence",
        "live_self_change_evidence",
        "observer_readback",
        "covenant_claim_publish_approval",
    }

    trust_boundary = summary["trust_boundary"]
    assert trust_boundary == {
        "local_only": True,
        "uses_network": False,
        "stores_credentials": False,
        "requires_provider_api_key": False,
        "mutates_repositories": False,
        "publishes_claims": False,
    }

    serialized = json.dumps(summary, sort_keys=True)
    assert str(REPO) not in serialized
    assert str(Path.home()) not in serialized
