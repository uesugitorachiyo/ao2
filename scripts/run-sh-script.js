#!/usr/bin/env node
const { spawnSync } = require("child_process");
const fs = require("fs");
const path = require("path");

const root = path.resolve(__dirname, "..");
const [scriptArg, ...scriptArgs] = process.argv.slice(2);

if (!scriptArg) {
  console.error("usage: node scripts/run-sh-script.js <script> [args...]");
  process.exit(2);
}

function commandExists(command) {
  const result = spawnSync(command, ["-c", "exit 0"], {
    stdio: "ignore",
    shell: false,
  });
  return result.status === 0;
}

function windowsShCandidates() {
  return [
    "C:\\Program Files\\Git\\bin\\sh.exe",
    "C:\\Program Files\\Git\\usr\\bin\\sh.exe",
    "C:\\Program Files (x86)\\Git\\bin\\sh.exe",
    "C:\\Program Files (x86)\\Git\\usr\\bin\\sh.exe",
  ];
}

function findShell() {
  if (commandExists("sh")) {
    return "sh";
  }
  if (process.platform === "win32") {
    for (const candidate of windowsShCandidates()) {
      if (fs.existsSync(candidate)) {
        return candidate;
      }
    }
  }
  return "sh";
}

function pathEntries() {
  return (process.env.PATH || "")
    .split(path.delimiter)
    .filter(Boolean);
}

const shell = findShell();
const env = { ...process.env };

if (process.platform === "win32" && path.isAbsolute(shell)) {
  const shellDir = path.dirname(shell);
  const gitRoot = path.dirname(shellDir);
  const extra = [
    shellDir,
    path.join(gitRoot, "usr", "bin"),
    path.join(gitRoot, "bin"),
  ].filter((entry) => fs.existsSync(entry));
  env.PATH = [...extra, ...pathEntries()].join(path.delimiter);
}

const script = path.isAbsolute(scriptArg) ? scriptArg : path.join(root, scriptArg);
const result = spawnSync(shell, [script, ...scriptArgs], {
  cwd: root,
  env,
  stdio: "inherit",
  shell: false,
});

if (result.error) {
  console.error(result.error.message);
  process.exit(1);
}

process.exit(result.status == null ? 1 : result.status);
