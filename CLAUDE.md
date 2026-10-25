# CodeMirror_R — Agent Guide

Native Rust port of CodeMirror 5.65.21 (`Ascendance3D/CodeMirror_R`, PRIVATE). It was extracted from `Ascendance3D/X_ITE_R`; `PROVENANCE.md` records the source commit and manifest.

## Rules

- Keep the repository PRIVATE. Never change its visibility.
- The CodeMirror 5.65.21 pin is in `tools/reference.lock`. Change it only with owner review.
- Do not edit `oracle/cm5-build/`, `upstream/codemirror5/` or an `oracle/cm5-*-baseline.json` to make a test pass.
- Do not run a rollup build inside `oracle/cm5-build/`. Build the C0 suite in a disposable copy (`README.md`).
- No X_ITE code belongs here. X_ITE_R does not depend on this repository.

## Checks

- Run `cargo fmt --all` before every commit.
- `tools/fetch-upstream.sh` (it runs `tools/verify-reference.sh`), then `cargo test --workspace --no-fail-fast`, strict.
- A test that cannot run must call `cm_evidence::no_gpu` or `cm_evidence::tool_absent`. It never returns in silence.
