import json
import stat
from pathlib import Path


REPO = Path(__file__).resolve().parents[1]


def read(path: str) -> str:
    return (REPO / path).read_text(encoding="utf-8")


def test_rsi_cross_repo_e2e_contract():
    package = json.loads(read("package.json"))
    assert package["scripts"]["rsi:cross-repo-e2e"] == (
        "node scripts/run-sh-script.js scripts/rsi-cross-repo-e2e.sh"
    )

    readme = read("README.md")
    for needle in [
        "npm run rsi:cross-repo-e2e",
        "ao2.rsi-cross-repo-e2e.v1",
        "target/rsi-cross-repo-e2e/latest/summary.json",
        "covenant.rsi-claim-publish-gate.v1",
        "publish_authority=false",
    ]:
        assert needle in readme

    script = REPO / "scripts" / "rsi-cross-repo-e2e.sh"
    assert script.is_file()
    assert script.stat().st_mode & stat.S_IXUSR
    text = script.read_text(encoding="utf-8")
    for needle in [
        "ao2.rsi-cross-repo-e2e.v1",
        "AO2_CONTROL_PLANE_REPO",
        "AO_COVENANT_REPO",
        "AO2_RSI_LIVE_SELF_CHANGE_REHEARSAL=1",
        "rsi:live-self-change-rehearsal",
        "verify_ao2_rsi_live_self_change_rehearsal.py",
        "rsi:live-self-change-readback-index",
        "rsi:claim-readiness",
        "policy claim-publish-gate",
        "covenant.rsi-claim-publish-gate.v1",
        "publishes_claims",
        "approves_rsi_claims",
    ]:
        assert needle in text
    assert "OPENAI_API_KEY" not in text
    assert "ANTHROPIC_API_KEY" not in text
