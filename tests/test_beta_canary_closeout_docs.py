from pathlib import Path


REPO = Path(__file__).resolve().parents[1]


def read(path: str) -> str:
    return (REPO / path).read_text(encoding="utf-8")


def test_beta_canary_closeout_docs_point_to_current_rust_cargo_path():
    readme = read("README.md")
    install = read("docs/INSTALL.md")
    verification = read("docs/VERIFICATION.md")
    release_notes = read("docs/release/v0.5.0-beta.1.md")
    template_readme = read("examples/task-templates/README.md")
    closeout = read("docs/beta/v0.5.0-beta.1-canary-closeout.md")

    combined = "\n".join(
        [readme, install, verification, release_notes, template_readme, closeout]
    )

    assert "docs/beta/v0.5.0-beta.1-canary-closeout.md" in combined
    assert "ao2 version --json" in combined
    assert "examples/task-templates/rust-cargo-bug-fix.yaml" in combined
    assert "cargo test" in combined
    assert "87c4cbe9706ea7d1721eaadcdb50e816cc96e91f" in combined
    assert "d71a29c433cba67bbbf516e1d0be952a6a504833df32f5fc4c4dd48bdc69d312" in closeout
    assert "17121acfc037a3c032679480adfa54605222ad1d038474dc05681652f3e40e6d" in closeout
    assert "workflow-by-path" in closeout
    assert "ao2 --version` is not supported" in closeout
