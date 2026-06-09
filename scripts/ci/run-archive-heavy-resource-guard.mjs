#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const guard = resolve(here, "archive-heavy-resource-guard.py");
const candidates = [
  { command: "python3", args: [guard] },
  { command: "python", args: [guard] },
  { command: "py", args: ["-3", guard] },
];

const failures = [];
for (const candidate of candidates) {
  const result = spawnSync(candidate.command, candidate.args, {
    env: process.env,
    stdio: "inherit",
    windowsHide: true,
  });

  if (result.error) {
    if (result.error.code === "ENOENT") {
      failures.push(`${candidate.command}: not found`);
      continue;
    }
    console.error(`${candidate.command}: ${result.error.message}`);
    process.exit(1);
  }

  process.exit(result.status ?? 1);
}

console.error("No Python 3 executable found for archive-heavy resource guard.");
for (const failure of failures) {
  console.error(`- ${failure}`);
}
process.exit(1);
