import json
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
WRAPPER = REPO_ROOT / "scripts" / "phase1-prepare-preflight-publish.sh"


def test_package_json_exposes_one_command_phase1_promotion():
    package_json = json.loads((REPO_ROOT / "package.json").read_text(encoding="utf-8"))

    assert (
        package_json["scripts"]["phase1:promote"]
        == "node scripts/run-sh-script.js scripts/phase1-prepare-preflight-publish.sh"
    )
    assert (
        package_json["scripts"]["phase1:dashboard-snapshot"]
        == "node scripts/run-sh-script.js scripts/phase1-control-plane-dashboard-snapshot.sh"
    )


def test_phase1_promote_wrapper_prepares_preflights_and_publishes_without_token_literal():
    script = WRAPPER.read_text(encoding="utf-8")

    assert "scripts/prepare_phase1_promotion_prerequisites.py" in script
    assert "AO2_PHASE1_PROMOTION_PREFLIGHT=1" in script
    assert "AO2_PHASE1_PROMOTION_PUBLISH=1" in script
    assert "AO2_PHASE1_DASHBOARD_SNAPSHOT" in script
    assert "scripts/phase1-control-plane-dashboard-snapshot.sh" in script
    assert ". \"$env_file\"" in script
    assert "--api-token-env" not in script
    assert "cat target/long-lived-control-plane/api-token" not in script
    assert "AO2_CP_API_TOKEN" in script


def test_phase1_dashboard_snapshot_wrapper_delegates_to_control_plane_helper_without_token_literal():
    script = (REPO_ROOT / "scripts/phase1-control-plane-dashboard-snapshot.sh").read_text(
        encoding="utf-8"
    )

    assert "../ao2-control-plane/scripts/cp_dashboard_snapshot.py" in script
    assert "--api-token-env" in script
    assert "AO2_PHASE1_API_TOKEN_ENV" in script
    assert "AO2_PHASE1_CONTROL_PLANE_URL" in script
    assert "AO2_PHASE1_DASHBOARD_SNAPSHOT_ROOT" in script
    assert "cat target/long-lived-control-plane/api-token" not in script
    assert "Authorization: Bearer" not in script


def test_operator_docs_explain_one_command_phase1_promotion_and_token_boundary():
    readme = (REPO_ROOT / "README.md").read_text(encoding="utf-8")
    status = (
        REPO_ROOT
        / "docs/status/20260530T020000Z-phase1-one-command-promotion.md"
    ).read_text(encoding="utf-8")

    for doc in (readme, status):
        assert "npm run phase1:promote" in doc
        assert "phase1:prepare-prerequisites" in doc
        assert "--api-token-env AO2_CP_API_TOKEN" in doc
        assert "bearer token" in doc.lower()
        assert "AO2_PHASE1_DASHBOARD_SNAPSHOT=1" in doc
        assert "phase1:dashboard-snapshot" in doc
