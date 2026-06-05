from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def test_license_provenance_gate_has_rg_free_fallback():
    script = (ROOT / "scripts" / "license-provenance-gate.sh").read_text()

    assert "grep_file()" in script
    assert "command -v rg" in script
    assert "\nrg -q " not in script


def test_factory_v3_parity_oracle_converts_paths_for_native_python():
    script = (ROOT / "scripts" / "factory-v3-parity-oracle.sh").read_text()

    assert "python_path()" in script
    assert "cygpath -w" in script
    assert "AO2_TABLE_PY=$(python_path \"$AO2_TABLE\")" in script
    assert "PARITY_OUT_PY=$(python_path \"$PARITY_OUT\")" in script
    assert "open('$AO2_TABLE')" not in script
