#!/usr/bin/env node
const { spawnSync } = require("child_process");
const fs = require("fs");
const path = require("path");

const root = path.resolve(__dirname, "..");
const binary = path.join(
  root,
  "target",
  "release",
  process.platform === "win32" ? "ao2.exe" : "ao2",
);

if (!fs.existsSync(binary)) {
  console.error(`missing release binary: ${binary}`);
  console.error("run npm run build:release first");
  process.exit(1);
}

const result = spawnSync(binary, process.argv.slice(2), {
  cwd: root,
  env: process.env,
  stdio: "inherit",
  shell: false,
});

if (result.error) {
  console.error(result.error.message);
  process.exit(1);
}

process.exit(result.status == null ? 1 : result.status);
