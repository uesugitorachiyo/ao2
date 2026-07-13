# Manifest-Bound AO2 Publication Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Require an operator-approved manifest for live AO2 publication and regenerate the exact-head authorization packet without publishing.

**Architecture:** A standalone standard-library Python verifier validates one descriptor-bound manifest against the staged publication directory and list. The shell publisher handles the four approval-variable modes, invokes the verifier after staging, and reports binding state before any mutation path.

**Tech Stack:** POSIX shell, Bash, Python 3 standard library, Rust integration tests, GitHub Actions.

## Global Constraints

- `PUBLIC_RELEASE_AUTHORIZED=NO`; create no tag, release, upload, deployment, or provider call.
- Use one AO2 branch and no new worktree.
- Keep the staged AO2 asset set at exactly 23 basenames.
- Keep existing channel, notes, signing, pilot, dirty-head, moving-head, tag, and overwrite guards.
- Do not inspect production private-key material.
- Do not rebuild Control Plane while commit `f1702b387607566cac457458af9adb5871a5c412` and its staged assets remain unchanged.

---

### Task 1: Strict approved-asset verifier

**Files:**
- Create: `scripts/release-verify-approved-assets.py`
- Modify: `crates/ao2-cli/tests/release_packaging.rs`

**Interfaces:**
- Consumes: `--manifest`, `--manifest-sha256`, `--publication-dir`, and `--publication-list`.
- Produces: verified digest, asset count, and `release_approved_assets=passed` on stdout; nonzero exit with one diagnostic on stderr.

- [ ] **Step 1: Write failing process tests** for exact success, byte drift, missing, extra, digest drift, duplicate and unsafe paths, and symlinks.
- [ ] **Step 2: Run the filtered release-packaging tests** and confirm failure because the verifier is absent.
- [ ] **Step 3: Implement descriptor-bound parsing and hashing** with `os.open`, `os.fstat`, `stat.S_ISREG`, strict regex validation, exact set equality, and per-asset SHA-256 checks.
- [ ] **Step 4: Run the filtered tests** and confirm every verifier case passes.
- [ ] **Step 5: Run `python3 -m py_compile scripts/release-verify-approved-assets.py`** and confirm success.

### Task 2: Four-mode publisher binding

**Files:**
- Modify: `scripts/release-ship.sh`
- Modify: `crates/ao2-cli/tests/release_packaging.rs`

**Interfaces:**
- Consumes: `AO2_RELEASE_EXPECTED_ASSET_MANIFEST` and `AO2_RELEASE_EXPECTED_ASSET_MANIFEST_SHA256`.
- Produces: `release_approval_bound` and `release_approved_asset_manifest_sha256` in dry-run and successful publication output.

- [ ] **Step 1: Write failing tests** for live missing values, dry-run neither, dry-run one, dry-run both, and verifier-before-mutation ordering.
- [ ] **Step 2: Run the filtered tests** and confirm the four-mode assertions fail against the existing publisher.
- [ ] **Step 3: Add the immediate variable-mode guard** before moving-head, build, GitHub, or mutation paths.
- [ ] **Step 4: Invoke the verifier after staging and the publication contract** and before tag/release checks; print binding state and digest in final output.
- [ ] **Step 5: Run `sh -n scripts/release-ship.sh` and the focused tests** and confirm success.

### Task 3: Review, PR, and merge

**Files:**
- Modify only files from Tasks 1 and 2 plus this design and plan.

**Interfaces:**
- Produces: one reviewed merge commit on AO2 `main` and no surviving feature branch.

- [ ] **Step 1: Run the complete focused publication suite and shell/Python syntax checks.**
- [ ] **Step 2: Inspect `git diff --check`, the diff, and repository status.**
- [ ] **Step 3: Commit, push the one branch, and open one narrow PR.**
- [ ] **Step 4: Wait for every required CI check and merge only after all are green.**
- [ ] **Step 5: Synchronize local `main`, delete local and remote feature branches, and verify a clean AO2 worktree.**

### Task 4: Exact-head AO2 requalification

**Files:**
- Regenerate ignored artifacts under `dist*` and `target/release-*`.

**Interfaces:**
- Consumes: the new exact AO2 `main` commit and unchanged Control Plane commit.
- Produces: four archives, four SBOM assets, sidecars, signatures, provenance, public key, closure/readiness summaries, and a 23-entry staged list.

- [ ] **Step 1: Reconfirm both repository heads and intended tag/release absence.**
- [ ] **Step 2: Build and package all four AO2 targets from the exact head, using native hosted evidence where local architecture cannot execute a target.**
- [ ] **Step 3: Sign and verify provenance, stage exactly 23 assets, and verify internal and outer checksums, SBOMs, versions, commits, LICENSE, and NOTICE.**
- [ ] **Step 4: Run scoped native install, upgrade, rollback, uninstall, offline verification, and exact-pair compatibility checks.**
- [ ] **Step 5: Run an unbound candidate dry run and a manifest-bound dry run with no mutations.**

### Task 5: New authorization packet and final audit

**Files:**
- Create: `target/release-qualification/<new-head>/authorization-packet/*`

**Interfaces:**
- Produces: a `READY_FOR_PUBLICATION_APPROVAL` packet that cannot authorize artifacts from `cec59de…`.

- [ ] **Step 1: Generate exact AO2 and unchanged Control Plane asset manifests and hash each manifest file.**
- [ ] **Step 2: Record exact notes, flags, commits, commands, manifest variables, and Control Plane's `shasum -c ... && gh release create ...` chain.**
- [ ] **Step 3: Change one staged byte in an isolated copy and prove the verifier rejects it before mutation sentinel creation.**
- [ ] **Step 4: Verify packet manifests against staged files and validate the packet summary.**
- [ ] **Step 5: Audit branches, worktrees, dirty repositories, public release state, boundaries, and every requested deliverable before reporting readiness.**
