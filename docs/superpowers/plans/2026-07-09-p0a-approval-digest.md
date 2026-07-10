# P0-A Content-Bound Approval Digest Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bind every AO2 sandbox-patch approval digest to the exact target repository, base commit, ordered operations, canonical paths, before/after content, symlink targets, and supported file modes, then reject drift before the first target write.

**Architecture:** Extract sandbox patch identity into a focused `ao2-adapters` module. Preview creates one typed canonical approval subject and hashes its serialized bytes; runtime continues to put that digest in existing tickets; apply reconstructs the subject from current target and sandbox state and fails closed before mutation when anything differs.

**Tech Stack:** Rust 2021 workspace (`rust-version = 1.83.0`), `serde`, `serde_json`, `sha2`, `walkdir`, Git CLI for repository identity and `HEAD`, `tempfile` fixtures.

## Global Constraints

- Blueprint authorization is planning-only; implementation requires a separate exact node clearance.
- `safe_to_execute` remains false until AO Mission records Foundry, Covenant, and Sentinel clearance.
- No live provider calls. Runtime tests named `provider_backed_run` must use the scripted local adapter only.
- No credential or token inspection and no secret-bearing environment reads.
- No release, deploy, publish, upload, tag, dependency update, policy widening, auth widening, or direct `main` mutation.
- P0-A strengthens patch identity only; do not remove automatic approval in this slice (P0-B).
- P0-A rejects drift before writes; do not claim transactional apply or rollback in this slice (P0-C).
- RSI remains denied.

## File Map

- Create `crates/ao2-adapters/src/sandbox_patch.rs`: canonical subject types, Git identity, snapshots, path validation, operation diff, digest, preview, and apply.
- Modify `crates/ao2-adapters/src/lib.rs`: declare and re-export the sandbox patch module; remove the old digest and apply implementation while retaining shared adapter sandbox helpers.
- Modify `crates/ao2-adapters/tests/sandbox_adapter.rs`: Git-backed fixtures and the P0-A adversarial matrix.
- Create `crates/ao2-runtime/tests/support/mod.rs`: shared Git initialization for runtime mutation fixtures.
- Modify `crates/ao2-runtime/tests/provider_backed_run.rs`: assert preview evidence exposes the bound subject and provider flow remains approval-gated.
- Modify `crates/ao2-runtime/tests/approval_replay.rs`: assert replay rejects approval evidence whose patch subject no longer matches current state.
- Modify `crates/ao2-runtime/tests/risky_pr_run.rs`: migrate provider-free mutation fixtures to committed Git targets.
- Modify `crates/ao2-cli/tests/cli_approval_replay.rs`: assert CLI preview output carries the canonical subject and apply fails before writes after drift.
- Modify `docs/SCHEMAS-AND-INTERFACES.md`: document `ao2.sandbox-patch-approval-subject.v1`.
- Modify `docs/SDD-risky-pr-run.md`: document preview-to-ticket-to-apply binding and P0-B/P0-C exclusions.

---

### Task 1: Establish Git-Backed Patch Test Fixtures

**Files:**
- Modify: `crates/ao2-adapters/tests/sandbox_adapter.rs`
- Create: `crates/ao2-runtime/tests/support/mod.rs`
- Modify: `crates/ao2-runtime/tests/provider_backed_run.rs`
- Modify: `crates/ao2-runtime/tests/approval_replay.rs`
- Modify: `crates/ao2-runtime/tests/risky_pr_run.rs`

**Interfaces:**
- Consumes: existing `preview_sandbox_patch`, `apply_sandbox_patch`, and `copy_dir_recursive` APIs.
- Produces: `init_git_target(root: &Path, files: &[(&str, &[u8])]) -> PathBuf`, `git(root: &Path, args: &[&str])`, `commit_all(root: &Path, message: &str)`, and `sandbox_copy(root: &Path, target: &Path) -> PathBuf` test helpers.

- [ ] **Step 1: Add a Git fixture helper and convert existing preview/apply tests**

```rust
fn git(root: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

fn init_git_target(root: &Path, files: &[(&str, &[u8])]) -> std::path::PathBuf {
    let target = root.join("target");
    fs::create_dir_all(&target).unwrap();
    git(&target, &["init", "--quiet"]);
    git(&target, &["config", "user.name", "AO2 Test"]);
    git(&target, &["config", "user.email", "ao2-test@example.invalid"]);
    for (path, bytes) in files {
        let path = target.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    git(&target, &["add", "."]);
    git(&target, &["commit", "--quiet", "-m", "fixture"]);
    target
}

fn commit_all(root: &Path, message: &str) {
    git(root, &["add", "-A"]);
    git(root, &["commit", "--quiet", "-m", message]);
}

fn sandbox_copy(root: &Path, target: &Path) -> std::path::PathBuf {
    let sandbox = root.join("sandbox");
    ao2_adapters::copy_dir_recursive(target, &sandbox).unwrap();
    sandbox
}
```

Replace plain target-directory setup in `sandbox_patch_apply_requires_exact_digest_and_then_promotes_changes` and `sandbox_patch_applies_file_deletions` with `init_git_target`.

- [ ] **Step 2: Run the existing adapter patch tests**

Run:

```sh
cargo test -p ao2-adapters --test sandbox_adapter sandbox_patch -- --nocapture
```

Expected: PASS against the current implementation. This commit changes fixture setup only.

- [ ] **Step 3: Add shared runtime Git fixture initialization**

Create `crates/ao2-runtime/tests/support/mod.rs`:

```rust
use std::path::Path;
use std::process::Command;

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git").args(args).current_dir(root).output().unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

pub fn commit_fixture(root: &Path) {
    git(root, &["init", "--quiet"]);
    git(root, &["config", "user.name", "AO2 Test"]);
    git(root, &["config", "user.email", "ao2-test@example.invalid"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "--quiet", "-m", "fixture"]);
}
```

Add `mod support;` to `provider_backed_run.rs`, `approval_replay.rs`, and
`risky_pr_run.rs`. At the end of each file's `copy_fixture`, call
`support::commit_fixture(dst)`. These three files use copied projects as AO2
mutation targets, so every preview has an exact repository identity and
committed base.

- [ ] **Step 4: Run runtime fixture baselines**

Run:

```sh
cargo test -p ao2-runtime --test provider_backed_run --no-fail-fast
cargo test -p ao2-runtime --test approval_replay --no-fail-fast
cargo test -p ao2-runtime --test risky_pr_run --no-fail-fast
```

Expected: PASS before the production digest changes.

- [ ] **Step 5: Commit the fixture migration**

```sh
git add crates/ao2-adapters/tests/sandbox_adapter.rs crates/ao2-runtime/tests/support/mod.rs crates/ao2-runtime/tests/provider_backed_run.rs crates/ao2-runtime/tests/approval_replay.rs crates/ao2-runtime/tests/risky_pr_run.rs
git commit -m "test: make sandbox patch fixtures git-backed"
```

### Task 2: Define the Canonical Approval Subject

**Files:**
- Create: `crates/ao2-adapters/src/sandbox_patch.rs`
- Modify: `crates/ao2-adapters/src/lib.rs`
- Modify: `crates/ao2-adapters/tests/sandbox_adapter.rs`

**Interfaces:**
- Produces: `SandboxPatchApprovalSubject`, `SandboxPatchOperation`, `SandboxPatchOperationKind`, `SandboxFileState`, `SandboxFileKind`, and `SandboxPatchApprovalSubject::action_digest(&self) -> Result<String>`.
- Consumed by: Tasks 3 through 7.

- [ ] **Step 1: Add a failing field-sensitivity test**

Add a test that constructs a subject, clones it, changes one field at a time,
and requires a different digest:

```rust
fn sample_file_state(byte: u8) -> SandboxFileState {
    SandboxFileState {
        kind: SandboxFileKind::RegularFile,
        content_sha256: Some(format!("sha256:{}", byte.to_string().repeat(64))),
        symlink_target_sha256: None,
        unix_mode: Some(0o644),
    }
}

fn sample_approval_subject() -> SandboxPatchApprovalSubject {
    SandboxPatchApprovalSubject {
        schema_version: "ao2.sandbox-patch-approval-subject.v1".to_string(),
        repository_identity: format!("sha256:{}", "1".repeat(64)),
        base_commit: "a".repeat(40),
        operation_type: "sandbox_patch_apply".to_string(),
        operations: vec![
            SandboxPatchOperation {
                order: 0,
                path: "a.txt".to_string(),
                kind: SandboxPatchOperationKind::Modified,
                before: Some(sample_file_state(3)),
                after: Some(sample_file_state(4)),
            },
            SandboxPatchOperation {
                order: 1,
                path: "b.txt".to_string(),
                kind: SandboxPatchOperationKind::Added,
                before: None,
                after: Some(sample_file_state(5)),
            },
        ],
    }
}

#[test]
fn approval_subject_digest_binds_every_contract_field() {
    let subject = sample_approval_subject();
    let original = subject.action_digest().unwrap();

    let mut different_repo = subject.clone();
    different_repo.repository_identity = format!("sha256:{}", "2".repeat(64));
    assert_ne!(different_repo.action_digest().unwrap(), original);

    let mut different_base = subject.clone();
    different_base.base_commit = "b".repeat(40);
    assert_ne!(different_base.action_digest().unwrap(), original);

    let mut reordered = subject.clone();
    reordered.operations.swap(0, 1);
    assert_ne!(reordered.action_digest().unwrap(), original);
}
```

- [ ] **Step 2: Run the test and verify the contract is missing**

Run:

```sh
cargo test -p ao2-adapters --test sandbox_adapter approval_subject_digest_binds_every_contract_field -- --exact
```

Expected: FAIL to compile because `SandboxPatchApprovalSubject` does not exist.

- [ ] **Step 3: Add the typed contract and digest method**

Create `sandbox_patch.rs` with these public types:

```rust
use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const SANDBOX_PATCH_APPROVAL_SUBJECT_SCHEMA: &str =
    "ao2.sandbox-patch-approval-subject.v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SandboxPatchApprovalSubject {
    pub schema_version: String,
    pub repository_identity: String,
    pub base_commit: String,
    pub operation_type: String,
    pub operations: Vec<SandboxPatchOperation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SandboxPatchOperation {
    pub order: u32,
    pub path: String,
    pub kind: SandboxPatchOperationKind,
    pub before: Option<SandboxFileState>,
    pub after: Option<SandboxFileState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SandboxPatchOperationKind { Added, Modified, Deleted }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SandboxFileState {
    pub kind: SandboxFileKind,
    pub content_sha256: Option<String>,
    pub symlink_target_sha256: Option<String>,
    pub unix_mode: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SandboxFileKind { RegularFile, Symlink }

impl SandboxPatchApprovalSubject {
    pub fn action_digest(&self) -> Result<String> {
        let bytes = serde_json::to_vec(self)?;
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        Ok(format!("{:x}", hasher.finalize()))
    }
}
```

Declare `mod sandbox_patch;` in `lib.rs` and re-export these types. Do not yet
move preview or apply.

- [ ] **Step 4: Run the focused test**

Run:

```sh
cargo test -p ao2-adapters --test sandbox_adapter approval_subject_digest_binds_every_contract_field -- --exact
```

Expected: PASS.

- [ ] **Step 5: Commit the contract**

```sh
git add crates/ao2-adapters/src/lib.rs crates/ao2-adapters/src/sandbox_patch.rs crates/ao2-adapters/tests/sandbox_adapter.rs
git commit -m "feat: define sandbox patch approval subject"
```

### Task 3: Capture Repository Identity, Base Commit, Paths, and File State

**Files:**
- Modify: `crates/ao2-adapters/src/sandbox_patch.rs`
- Modify: `crates/ao2-adapters/tests/sandbox_adapter.rs`

**Interfaces:**
- Produces: private `repository_state(target: &Path) -> Result<(String, String)>`, `canonical_relative_path(path: &Path) -> Result<String>`, and `snapshot_tree(root: &Path) -> Result<BTreeMap<String, SandboxFileState>>`.
- Consumed by: `build_approval_subject` in Task 4.

- [ ] **Step 1: Add failing repository/base/path tests**

Add an integration test proving that a non-Git target fails. Add unit tests in
`sandbox_patch.rs` for path components so no test-only public API is needed:

```rust
#[test]
fn patch_subject_rejects_non_git_target() {
    let temp = tempfile::tempdir().unwrap();
    let plain = temp.path().join("plain");
    fs::create_dir_all(&plain).unwrap();
    assert!(preview_sandbox_patch(&plain, &plain).unwrap_err().to_string().contains("Git"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_path_rejects_aliases_and_traversal() {
        for alias in ["../escape", "./value.txt", "/absolute", ""] {
            assert!(canonical_relative_path(Path::new(alias)).is_err(), "{alias}");
        }
        assert_eq!(
            canonical_relative_path(Path::new("src/value.txt")).unwrap(),
            "src/value.txt"
        );
    }
}
```

- [ ] **Step 2: Verify the tests fail**

Run:

```sh
cargo test -p ao2-adapters --test sandbox_adapter patch_subject_rejects_non_git_target -- --exact
cargo test -p ao2-adapters canonical_path_rejects_aliases_and_traversal -- --exact
```

Expected: FAIL because preview still accepts plain directories and no canonical
path validator exists.

- [ ] **Step 3: Implement Git identity and base resolution**

Use `git -C <target> rev-parse --git-common-dir` and `git -C <target> rev-parse
HEAD`. Resolve relative Git output against `target`, canonicalize the common
directory, require valid UTF-8, normalize `\` to `/`, hash those UTF-8 bytes,
emit only `sha256:<digest>`, and validate `HEAD` as 40 or 64 lowercase hex
characters. Return a contextual error on any nonzero Git exit.

- [ ] **Step 4: Implement non-following typed snapshots**

Use `WalkDir::follow_links(false)`. For each regular file, hash file bytes. For
each symlink, call `fs::read_link` and hash the platform target bytes without
following it. Reject special files. Reuse the existing ignored-component list.

On Unix, bind `metadata.permissions().mode() & 0o777`; on non-Unix platforms,
return `None`. Normalize paths from components, rejecting every component other
than `Component::Normal`.

Update `copy_dir_recursive` so `entry.file_type().is_symlink()` recreates the
link itself without following it. Use `std::os::unix::fs::symlink` on Unix. On
Windows, inspect the target only to select `symlink_file` or `symlink_dir`; if
the platform denies safe link recreation, return an error and remove the
partially created sandbox.

- [ ] **Step 5: Run focused and platform-neutral tests**

Run:

```sh
cargo test -p ao2-adapters --test sandbox_adapter patch_subject_rejects_non_git_target -- --exact
cargo test -p ao2-adapters canonical_path_rejects_aliases_and_traversal -- --exact
cargo test -p ao2-adapters --test sandbox_adapter sandbox_run_captures_diff_without_mutating_target_repo -- --exact
```

Expected: PASS.

- [ ] **Step 6: Commit repository and file-state capture**

```sh
git add crates/ao2-adapters/src/sandbox_patch.rs crates/ao2-adapters/tests/sandbox_adapter.rs
git commit -m "feat: bind patch snapshots to repository state"
```

### Task 4: Build Ordered Operations and Replace Preview Digest

**Files:**
- Modify: `crates/ao2-adapters/src/sandbox_patch.rs`
- Modify: `crates/ao2-adapters/src/lib.rs`
- Modify: `crates/ao2-adapters/tests/sandbox_adapter.rs`

**Interfaces:**
- Produces: `build_approval_subject(target_repo: &Path, sandbox_path: &Path) -> Result<SandboxPatchApprovalSubject>` and a `SandboxPatchPreview` containing `approval_subject`, `action_digest`, `changed_files`, and `diff_summary`.
- Consumed by: runtime tickets and apply revalidation.

- [ ] **Step 1: Add the P0-A digest matrix as failing tests**

Add separate tests for:

```rust
#[test]
fn patch_digest_changes_for_content_base_repository_and_operation_kind() {
    let temp = tempfile::tempdir().unwrap();
    let target = init_git_target(temp.path(), &[("value.txt", b"before\n")]);
    let sandbox = sandbox_copy(temp.path(), &target);
    fs::write(sandbox.join("value.txt"), "after-one\n").unwrap();
    let first = preview_sandbox_patch(&target, &sandbox).unwrap();
    assert_eq!(first.approval_subject.operations[0].kind, SandboxPatchOperationKind::Modified);

    fs::write(sandbox.join("value.txt"), "after-two\n").unwrap();
    let different_content = preview_sandbox_patch(&target, &sandbox).unwrap();
    assert_ne!(first.action_digest, different_content.action_digest);
    assert_ne!(
        first.approval_subject.operations[0].after,
        different_content.approval_subject.operations[0].after
    );

    fs::write(target.join("unrelated.txt"), "new base\n").unwrap();
    commit_all(&target, "advance base");
    fs::write(sandbox.join("unrelated.txt"), "new base\n").unwrap();
    let different_base = preview_sandbox_patch(&target, &sandbox).unwrap();
    assert_ne!(first.action_digest, different_base.action_digest);
    assert_ne!(first.approval_subject.base_commit, different_base.approval_subject.base_commit);

    let other_root = temp.path().join("other");
    fs::create_dir_all(&other_root).unwrap();
    let other_target = init_git_target(&other_root, &[("value.txt", b"before\n")]);
    let other_sandbox = sandbox_copy(&other_root, &other_target);
    fs::write(other_sandbox.join("value.txt"), "after-one\n").unwrap();
    let different_repo = preview_sandbox_patch(&other_target, &other_sandbox).unwrap();
    assert_ne!(first.approval_subject.repository_identity, different_repo.approval_subject.repository_identity);
    assert_ne!(first.action_digest, different_repo.action_digest);
}

#[cfg(unix)]
#[test]
fn patch_digest_changes_for_symlink_target_and_executable_mode() {
    use std::os::unix::fs::{symlink, PermissionsExt};

    let temp = tempfile::tempdir().unwrap();
    let target = init_git_target(temp.path(), &[("tool.sh", b"#!/bin/sh\n")]);
    symlink("tool.sh", target.join("tool-link")).unwrap();
    commit_all(&target, "add link");
    let sandbox = sandbox_copy(temp.path(), &target);

    fs::remove_file(sandbox.join("tool-link")).unwrap();
    symlink("other.sh", sandbox.join("tool-link")).unwrap();
    let link_change = preview_sandbox_patch(&target, &sandbox).unwrap();
    assert_eq!(link_change.approval_subject.operations[0].path, "tool-link");

    fs::remove_file(sandbox.join("tool-link")).unwrap();
    symlink("tool.sh", sandbox.join("tool-link")).unwrap();
    let mut permissions = fs::metadata(sandbox.join("tool.sh")).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(sandbox.join("tool.sh"), permissions).unwrap();
    let mode_change = preview_sandbox_patch(&target, &sandbox).unwrap();
    assert_ne!(link_change.action_digest, mode_change.action_digest);
    assert_eq!(mode_change.approval_subject.operations[0].after.as_ref().unwrap().unix_mode, Some(0o755));
}

#[test]
fn preview_emits_sorted_contiguous_operations() {
    let temp = tempfile::tempdir().unwrap();
    let target = init_git_target(temp.path(), &[("z.txt", b"z\n"), ("m.txt", b"m\n")]);
    let sandbox = sandbox_copy(temp.path(), &target);
    fs::write(sandbox.join("a.txt"), "a\n").unwrap();
    fs::write(sandbox.join("z.txt"), "changed\n").unwrap();
    fs::remove_file(sandbox.join("m.txt")).unwrap();

    let preview = preview_sandbox_patch(&target, &sandbox).unwrap();
    let paths = preview.approval_subject.operations.iter().map(|op| op.path.as_str()).collect::<Vec<_>>();
    let orders = preview.approval_subject.operations.iter().map(|op| op.order).collect::<Vec<_>>();
    let kinds = preview.approval_subject.operations.iter().map(|op| op.kind.clone()).collect::<Vec<_>>();
    assert_eq!(paths, vec!["a.txt", "m.txt", "z.txt"]);
    assert_eq!(orders, vec![0, 1, 2]);
    assert_eq!(
        kinds,
        vec![
            SandboxPatchOperationKind::Added,
            SandboxPatchOperationKind::Deleted,
            SandboxPatchOperationKind::Modified,
        ]
    );
}
```

Each test must assert both the changed digest and the expected typed field. Do
not rely only on `assert_ne!`.

- [ ] **Step 2: Run the matrix and verify it fails**

Run:

```sh
cargo test -p ao2-adapters --test sandbox_adapter patch_digest_changes -- --nocapture
cargo test -p ao2-adapters --test sandbox_adapter preview_emits_sorted_contiguous_operations -- --exact
```

Expected: FAIL because preview still hashes changed labels and summary text.

- [ ] **Step 3: Implement operation construction**

Union the before and after snapshot keys in a `BTreeSet`. Emit `Added`,
`Modified`, or `Deleted` only when states differ. Enumerate the sorted vector to
set `order`. Build the subject with fixed schema and operation type, then hash
the exact `serde_json::to_vec` bytes.

- [ ] **Step 4: Update preview without removing compatibility fields**

Move `SandboxPatchPreview` to `sandbox_patch.rs` and add:

```rust
pub approval_subject: SandboxPatchApprovalSubject,
```

Derive `changed_files` from `operations.path` and derive `diff_summary` from
typed operation kinds. Remove `sandbox_patch_digest(changed_files,
diff_summary)` from `lib.rs`.

- [ ] **Step 5: Run the complete adapter suite**

Run:

```sh
cargo test -p ao2-adapters --no-fail-fast
```

Expected: PASS, including all P0-A matrix tests.

- [ ] **Step 6: Commit canonical preview generation**

```sh
git add crates/ao2-adapters/src/lib.rs crates/ao2-adapters/src/sandbox_patch.rs crates/ao2-adapters/tests/sandbox_adapter.rs
git commit -m "feat: hash canonical sandbox patch operations"
```

### Task 5: Revalidate the Exact Subject Before Apply

**Files:**
- Modify: `crates/ao2-adapters/src/sandbox_patch.rs`
- Modify: `crates/ao2-adapters/src/lib.rs`
- Modify: `crates/ao2-adapters/tests/sandbox_adapter.rs`

**Interfaces:**
- Produces: updated `apply_sandbox_patch(SandboxPatchApplyRequest) -> Result<SandboxPatchApplyResult>` that performs no write before digest equality and reports `approval_subject`.

- [ ] **Step 1: Add failing drift-before-write tests**

Create one helper that previews a two-file patch, applies a drift callback, runs
apply, expects an error, and asserts every target byte and path is unchanged by
the failed apply. Use it for:

```rust
fn assert_drift_rejected(drift: impl FnOnce(&Path, &Path)) {
    let temp = tempfile::tempdir().unwrap();
    let target = init_git_target(temp.path(), &[("a.txt", b"before-a\n"), ("b.txt", b"before-b\n")]);
    let sandbox = sandbox_copy(temp.path(), &target);
    fs::write(sandbox.join("a.txt"), "approved-a\n").unwrap();
    fs::write(sandbox.join("b.txt"), "approved-b\n").unwrap();
    let preview = preview_sandbox_patch(&target, &sandbox).unwrap();

    drift(&target, &sandbox);
    let result = apply_sandbox_patch(SandboxPatchApplyRequest {
        target_repo: target.clone(),
        sandbox_path: sandbox,
        expected_digest: preview.action_digest,
        approver: "human:test".to_string(),
    });
    assert!(result.unwrap_err().to_string().contains("digest mismatch"));
    assert_ne!(fs::read(target.join("a.txt")).unwrap(), b"approved-a\n");
    assert_ne!(fs::read(target.join("b.txt")).unwrap(), b"approved-b\n");
}

#[test]
fn apply_rejects_target_content_changed_after_preview_before_any_write() {
    assert_drift_rejected(|target, _| fs::write(target.join("a.txt"), "target drift\n").unwrap());
}

#[test]
fn apply_rejects_sandbox_content_changed_after_preview_before_any_write() {
    assert_drift_rejected(|_, sandbox| fs::write(sandbox.join("a.txt"), "sandbox drift\n").unwrap());
}

#[test]
fn apply_rejects_target_head_changed_after_preview_before_any_write() {
    assert_drift_rejected(|target, _| {
        fs::write(target.join("unrelated.txt"), "new base\n").unwrap();
        commit_all(target, "advance target head");
    });
}
```

- [ ] **Step 2: Verify the stale-base test fails against the old digest**

Run:

```sh
cargo test -p ao2-adapters --test sandbox_adapter apply_rejects_target_head_changed_after_preview_before_any_write -- --exact
```

Expected: FAIL because the current digest does not include `HEAD`.

- [ ] **Step 3: Move apply into the canonical module**

At function entry, call `preview_sandbox_patch`, compare its digest to
`expected_digest`, and return before entering the operation loop on mismatch.
Apply from the validated operation vector rather than rescanning paths. Use
`symlink` creation APIs per platform for symlink operations and set Unix mode
after regular-file copies.

Do not add rollback or a transaction journal in this task. Record in the module
documentation that P0-C must close the remaining race between revalidation and
individual filesystem operations.

- [ ] **Step 4: Prove every mismatch is pre-write**

Run:

```sh
cargo test -p ao2-adapters --test sandbox_adapter apply_rejects_ -- --nocapture
cargo test -p ao2-adapters --test sandbox_adapter sandbox_patch_apply_requires_exact_digest_and_then_promotes_changes -- --exact
```

Expected: all rejection tests PASS and the unchanged approved subject still
applies.

- [ ] **Step 5: Commit apply revalidation**

```sh
git add crates/ao2-adapters/src/lib.rs crates/ao2-adapters/src/sandbox_patch.rs crates/ao2-adapters/tests/sandbox_adapter.rs
git commit -m "fix: reject sandbox patch drift before apply"
```

### Task 6: Bind Runtime Approval Evidence to the Canonical Subject

**Files:**
- Modify: `crates/ao2-runtime/tests/provider_backed_run.rs`
- Modify: `crates/ao2-runtime/tests/approval_replay.rs`
- Modify: `crates/ao2-runtime/tests/risky_pr_run.rs`

**Interfaces:**
- Consumes: `SandboxPatchPreview.approval_subject` and the existing
  `preview.action_digest` ticket binding.
- Produces: runtime regressions proving canonical subject evidence and replay
  rejection.

- [ ] **Step 1: Add a failing preview-evidence assertion**

In the provider-backed sandbox test, load the `sandbox_patch_preview` artifact
and assert:

```rust
assert_eq!(
    preview["approval_subject"]["schema_version"],
    "ao2.sandbox-patch-approval-subject.v1"
);
assert_eq!(
    ticket["action_digest"],
    preview["action_digest"]
);
assert_eq!(preview["approval_subject"]["base_commit"].as_str().unwrap().len(), 40);
```

- [ ] **Step 2: Add a replay drift test**

Pause a fixture run, approve its exact ticket, mutate or commit the fixture
target, resume, and assert the run rejects before writing sandbox output to the
target. The rejection text must identify approval-subject or digest mismatch.

- [ ] **Step 3: Run runtime tests**

Run:

```sh
cargo test -p ao2-runtime --test provider_backed_run --no-fail-fast
cargo test -p ao2-runtime --test approval_replay --no-fail-fast
cargo test -p ao2-runtime --test risky_pr_run --no-fail-fast
```

Expected: PASS with no provider calls; tests use the scripted adapter and local
temporary repositories only.

- [ ] **Step 4: Commit runtime evidence coverage**

```sh
git add crates/ao2-runtime/tests/provider_backed_run.rs crates/ao2-runtime/tests/approval_replay.rs crates/ao2-runtime/tests/risky_pr_run.rs
git commit -m "test: bind runtime approvals to canonical patch subjects"
```

### Task 7: Expose and Verify the Contract Through the CLI

**Files:**
- Modify: `crates/ao2-cli/tests/cli_approval_replay.rs`

**Interfaces:**
- Consumes: existing `ao2 adapter patch preview` and `ao2 adapter patch apply` commands.
- Produces: CLI-level contract and stale-state negative controls.

- [ ] **Step 1: Extend the CLI preview/apply test**

In `cli_adapter_patch_preview_and_apply_promotes_exact_digest`, initialize the
target as a committed Git repository and assert the JSON contains:

```rust
assert_eq!(
    preview_json["approval_subject"]["schema_version"],
    "ao2.sandbox-patch-approval-subject.v1"
);
assert_eq!(
    preview_json["approval_subject"]["operation_type"],
    "sandbox_patch_apply"
);
assert_eq!(
    preview_json["approval_subject"]["operations"][0]["order"],
    0
);
```

- [ ] **Step 2: Add CLI stale-target rejection**

Preview, change and commit a target file, invoke apply with the old digest,
require nonzero exit, and assert that the sandbox change was not copied to any
target file.

- [ ] **Step 3: Run exact CLI tests**

Run:

```sh
cargo test -p ao2-cli --test cli_approval_replay cli_adapter_patch_preview_and_apply_promotes_exact_digest -- --exact
cargo test -p ao2-cli --test cli_approval_replay cli_adapter_patch_apply_rejects_stale_target_before_write -- --exact
```

Expected: PASS.

- [ ] **Step 4: Commit CLI regressions**

```sh
git add crates/ao2-cli/tests/cli_approval_replay.rs
git commit -m "test: expose content-bound patch approval in CLI"
```

### Task 8: Document the Wire Contract and Run Closure Verification

**Files:**
- Modify: `docs/SCHEMAS-AND-INTERFACES.md`
- Modify: `docs/SDD-risky-pr-run.md`

**Interfaces:**
- Produces: public contract documentation for
  `ao2.sandbox-patch-approval-subject.v1` and an explicit P0-A boundary.

- [ ] **Step 1: Document every field and invariant**

Add the exact JSON shape from the design, define repository identity, base
commit, path normalization, file states, operation kinds, ordering, and the
pre-write recomputation rule. State that raw local paths are not emitted.

- [ ] **Step 2: Document exclusions**

State plainly:

```text
P0-A makes the approval action identifier content- and base-bound. It does not
validate who approved it, make a ticket single-use, or make application
transactional. Those remain blocked by P0-B, P0-C, and P0-D.
```

- [ ] **Step 3: Run formatting and focused verification**

Run:

```sh
cargo fmt --all --check
cargo test -p ao2-adapters --test sandbox_adapter --no-fail-fast
cargo test -p ao2-runtime --test provider_backed_run --no-fail-fast
cargo test -p ao2-runtime --test approval_replay --no-fail-fast
cargo test -p ao2-cli --test cli_approval_replay cli_adapter_patch -- --nocapture
git diff --check
```

Expected: PASS.

- [ ] **Step 4: Run full workspace verification**

Run:

```sh
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --no-fail-fast
```

Expected: PASS with no live-provider guard enabled and no release command run.

- [ ] **Step 5: Run a scoped public-safety scan**

Run:

```sh
rg -n -i 'auto.?approv|bypass|provider call|credential|release|deploy|publish|upload|tag|RSI' \
  crates/ao2-adapters/src/sandbox_patch.rs \
  crates/ao2-adapters/tests/sandbox_adapter.rs \
  crates/ao2-runtime/tests/provider_backed_run.rs \
  crates/ao2-runtime/tests/approval_replay.rs \
  crates/ao2-cli/tests/cli_approval_replay.rs \
  docs/SCHEMAS-AND-INTERFACES.md \
  docs/SDD-risky-pr-run.md
```

Expected: only explicit denials or test descriptions; no authority widening or
claim that P0-B through P0-D are complete.

- [ ] **Step 6: Commit documentation and verification closure**

```sh
git add docs/SCHEMAS-AND-INTERFACES.md docs/SDD-risky-pr-run.md
git commit -m "docs: specify content-bound patch approval contract"
```

## Mission Closure Requirements

After implementation, AO Mission must not mark the node complete from a local
test result alone. The node requires:

- the exact Blueprint authorization digest;
- Atlas candidate and Foundry import digests;
- Foundry run-link to the implementation branch and commits;
- Covenant exact-scope ticket readback without claiming approval authority;
- Sentinel public-safety result;
- Promoter `no_promotion_requested` verdict;
- Command readback agreeing that P0-B through P0-G and RSI remain denied;
- PR number, immutable pre-merge head, CI results, merge commit, and post-merge
  branch cleanup evidence; and
- Atlas workgraph readback advancing from P0-A to P0-B only after all evidence
  is bound.

## Execution Stop

Do not execute this plan from the current planning authorization. The next
allowed action is AO Mission/Foundry review of this plan and generation of an
exact node gate for `ao2-approval-digest-p0-candidate-node`. Implementation may
start only when that gate says `safe_to_execute=true` for the listed files and
commands.
