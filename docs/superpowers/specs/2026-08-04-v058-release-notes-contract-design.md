# v0.5.8 Release Notes Contract Repair

## Problem

The public release workflow accepts `docs/release/READY-TO-SHIP.md` during its
initial input binding but the protected publisher requires a versioned stable
release note at `docs/release/v<version>-stable.md`. AO2 v0.5.8 has no
`docs/release/v0.5.8-stable.md`, so the live publisher fails before creating a
tag, release, or upload.

## Design

Add the missing v0.5.8 stable release note using the existing versioned-note
format. Add a static regression check that derives the next stable AO2 version
from `docs/release/release-train.json` and requires the corresponding
`docs/release/v<version>-stable.md` regular file.

The check belongs with the release workflow contract tests. It must fail on the
current source before the note exists and pass only when the release-train
version and versioned note agree. It does not weaken either publisher check or
change release authority.

## Qualification

Merge one bounded AO2 pull request. Because its source head changes, discard
the failed live publication attempt as historical evidence and mint fresh
exact-head qualification: physical Windows import, immutable AO2 promotion
plan, and Control Plane promotion plan. Publish only from the new frozen plans
under the existing explicit release authority.

## Boundaries

No release, tag, upload, credential change, provider call, or Control Plane
mutation occurs in this repair pull request. The release-note change is source
documentation plus its regression contract only.
