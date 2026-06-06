#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_ROOT="${AO2_OPERATOR_EVIDENCE_INDEX_ROOT:-$ROOT/target/operator-evidence-index/latest}"
# Default JSON output: target/operator-evidence-index/latest/index.json
SUMMARY="$OUT_ROOT/summary.json"
INDEX_JSON="$OUT_ROOT/index.json"
INDEX_HTML="$OUT_ROOT/index.html"

rm -rf "$OUT_ROOT"
mkdir -p "$OUT_ROOT"

python3 - "$ROOT" "$OUT_ROOT" "$SUMMARY" "$INDEX_JSON" "$INDEX_HTML" <<'PY'
import html
import json
import sys
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path

root = Path(sys.argv[1]).resolve()
out_root = Path(sys.argv[2]).resolve()
summary_path = Path(sys.argv[3]).resolve()
index_json = Path(sys.argv[4]).resolve()
index_html = Path(sys.argv[5]).resolve()
summaries = []
for path in sorted((root / "target").glob("*/latest/summary.json")):
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        continue
    schema = data.get("schema_version")
    if not schema:
        continue
    summaries.append({"path": str(path), "schema_version": schema, "status": data.get("status"), "artifact_root": data.get("artifact_root")})
schema_counts = Counter(item["schema_version"] for item in summaries)
latest_pulse_packet = root / "target" / "pulse-next-recommended-tasks" / "packet.md"
payload = {
    "schema_version": "ao2.operator-evidence-index.v1",
    "generated_at_utc": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
    "status": "passed",
    "artifact_root": str(out_root),
    "summary_count": len(summaries),
    "schema_counts": dict(sorted(schema_counts.items())),
    "latest_pulse_packet": str(latest_pulse_packet) if latest_pulse_packet.is_file() else None,
    "summaries": summaries,
    "trust_boundary": {"local_only": True, "stores_credentials": False},
}
index_json.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
rows = "\n".join(
    f"<tr><td>{html.escape(item['schema_version'])}</td><td>{html.escape(str(item.get('status')))}</td><td>{html.escape(item['path'])}</td></tr>"
    for item in summaries
)
index_html.write_text(f"""<!doctype html>
<html lang=\"en\">
<head><meta charset=\"utf-8\"><title>AO2 Operator Evidence Index</title></head>
<body>
<h1>AO2 Operator Evidence Index</h1>
<p>ao2.operator-evidence-index.v1</p>
<table><thead><tr><th>Schema</th><th>Status</th><th>Path</th></tr></thead><tbody>{rows}</tbody></table>
</body>
</html>
""", encoding="utf-8")
summary_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"summary={summary_path}")
print("status=passed")
PY
