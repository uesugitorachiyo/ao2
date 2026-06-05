#!/usr/bin/env sh
set -eu

echo "AO2 local CI"
echo "node: $(node --version 2>/dev/null || echo unavailable)"
echo "npm: $(npm --version 2>/dev/null || echo unavailable)"
echo "rustc: $(rustc --version)"
echo "cargo: $(cargo --version)"

npm run verify
npm run build:release

tmpdir=$(mktemp -d "${TMPDIR:-/tmp}/ao2-local-ci.XXXXXX")
cp -R fixtures/discount-service "$tmpdir/discount-service"

env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  target/release/ao2 run examples/risky-pr-run/risky-pr.yaml \
  --target "$tmpdir/discount-service" \
  --run-id local-ci \
  --pause-for-approval > "$tmpdir/run.txt"

ticket=$(awk -F= '/approval_ticket_id=/{print $2}' "$tmpdir/run.txt")

env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  target/release/ao2 approve "$ticket" \
  --target "$tmpdir/discount-service" \
  --approver human:local-ci > "$tmpdir/approve.txt"

env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  target/release/ao2 run --resume local-ci \
  --target "$tmpdir/discount-service" > "$tmpdir/resume.txt"

env -u OPENAI_API_KEY -u ANTHROPIC_API_KEY \
  target/release/ao2 replay local-ci \
  --target "$tmpdir/discount-service" > "$tmpdir/replay.json"

node -e 'const fs=require("fs"); const r=JSON.parse(fs.readFileSync(process.argv[1],"utf8")); if (r.status !== "accepted" || JSON.stringify(r.digest_failures) !== "[]") { throw new Error("bad replay"); } console.log(`local_ci_replay_status=${r.status}`); console.log(`local_ci_event_count=${r.event_count}`); console.log(`local_ci_artifact_count=${r.artifact_count}`);' "$tmpdir/replay.json"
