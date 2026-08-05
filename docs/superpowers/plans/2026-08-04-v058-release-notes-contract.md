# v0.5.8 Release Notes Contract Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make AO2’s stable-release input binding and protected publisher require the same versioned release-note contract, then requalify and publish the paired release.

**Architecture:** The release-train manifest remains the source of the candidate version. A static Python regression test derives that version and verifies the corresponding stable note exists. The publisher consumes that same path rather than an independently hard-coded expression. The v0.5.8 note is the only release-content addition.

**Tech Stack:** Python `unittest`, GitHub Actions YAML, Markdown, existing AO2 release workflows.

## Global Constraints

- Keep the repair in AO2 only; do not change Control Plane source.
- Do not weaken a release gate or bypass exact-head qualification.
- No tag, release, upload, provider call, or credential change occurs before fresh immutable plans pass.
- Retain the failed run `30958935736` as historical evidence.

---

### Task 1: Bind both release stages to versioned stable notes

**Files:**

- Modify: `tests/test_public_stabilization.py`
- Modify: `.github/workflows/public-release-build.yml`
- Create: `docs/release/v0.5.8-stable.md`

**Interfaces:**

- Consumes: `docs/release/release-train.json` `next_patch.ao2.version`.
- Produces: a required regular file `docs/release/v<version>-stable.md` used by both input binding and the protected publisher.

- [ ] **Step 1: Write the failing regression test**

Add a test near the existing public-release workflow tests that loads the train manifest, derives `v<next_patch.ao2.version>-stable.md`, and asserts that the file exists and that the workflow contains the derived path expression in both the initial binding and protected publisher stages.

- [ ] **Step 2: Run the focused test and verify RED**

Run: `python3 -m unittest tests.test_public_stabilization.PublicStabilizationTests.test_next_patch_has_versioned_stable_release_notes`

Expected: FAIL because `docs/release/v0.5.8-stable.md` does not exist and input binding still references `READY-TO-SHIP.md`.

- [ ] **Step 3: Add the minimum release content and workflow alignment**

Create `docs/release/v0.5.8-stable.md` with v0.5.8 scope, supported archive targets, checksum verification, credential-free fixture guidance, rollback command, and the non-publishing Control Plane boundary. Change the input-binding workflow check and bound-input artifact to reference `docs/release/v${RELEASE_VERSION}-stable.md`, matching the protected publisher.

- [ ] **Step 4: Run focused verification and verify GREEN**

Run: `python3 -m unittest tests.test_public_stabilization.PublicStabilizationTests.test_next_patch_has_versioned_stable_release_notes`

Expected: PASS.

- [ ] **Step 5: Run release-contract regression coverage**

Run: `python3 -m unittest tests.test_public_stabilization tests.test_physical_windows_qualification`

Expected: PASS with no changed release authority behavior.

- [ ] **Step 6: Commit the repair**

Run: `git add tests/test_public_stabilization.py .github/workflows/public-release-build.yml docs/release/v0.5.8-stable.md && git commit -m "Align v0.5.8 release notes contract"`

### Task 2: Merge and publish from fresh evidence

**Files:**

- Evidence only: `/Users/torachiyouesugi/Documents/canary-test/ao-stack-public-release-v058-v0119-*`

**Interfaces:**

- Consumes: merged AO2 source SHA, fresh physical-Windows import artifact, immutable AO2 and Control Plane promotion plans.
- Produces: public AO2 `v0.5.8` and Control Plane `v0.1.19` releases plus post-publication verification evidence.

- [ ] **Step 1: Open one AO2 PR and wait for required CI**

Run focused checks, formatting checks, `git diff --check`, open one PR, and merge only after required hosted CI passes.

- [ ] **Step 2: Mint exact-head release evidence**

Run the existing physical-Windows outbound qualification/import flow for the merged SHA. Run AO2 public-release build with `dry_run=true`; run Control Plane release promotion with `dry_run=true`.

- [ ] **Step 3: Independently validate both plans**

Download the frozen artifacts under `canary-test`, verify plan SHA-256 values and archive digests, and confirm the release tags are absent.

- [ ] **Step 4: Publish AO2 then Control Plane**

Dispatch AO2 live publication with its exact confirmation and protected-environment approval. Dispatch Control Plane live publication with its exact dry-run plan ID, plan SHA-256, and confirmation.

- [ ] **Step 5: Verify public releases**

Verify tags, release pages, every asset checksum, and install/doctor behavior on macOS, native Ubuntu, and Windows. Seal a final manifest and retain the failed pre-repair run as historical evidence.
