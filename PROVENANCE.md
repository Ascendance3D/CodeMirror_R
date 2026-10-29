# Provenance

CodeMirror_R was extracted from `Ascendance3D/X_ITE_R` on 2026-10-25, under the owner decision `CODEMIRROR_EXTRACTION_OPTION_A_AUTHORIZED`. X_ITE_R keeps its full Git history unchanged.

| item | value |
| --- | --- |
| source repository | `Ascendance3D/X_ITE_R` |
| source commit | `ffe080b193f031605095f87ab4cafe443e7b50c5` (master; no CodeMirror path changed after it at extraction time) |
| method | `git archive ffe080b -- <paths>`, then `sha256sum` and mode check of every file |
| CodeMirror files | 676, `docs/provenance/cm-files-ffe080b.sha256` (sha256, mode, blob id, size, path) |
| shared test corpus | 859, `docs/provenance/corpus-items-ffe080b.sha256` |
| CM5 reference | 5.65.21, commit `4ca3d0551cf9125114e0802877480599472dc186`, vendored digest `440 d2daa3a3a863163603b645e453e2ffc6c1a4277d228108a4d437c7627f0f911c` |
| extraction plan | the owner's extraction plan of 2026-10-25 (not published) |

## Commits of the extraction PR

1. **Verbatim copy.** The 676 manifest files, byte-identical and mode-identical to `ffe080b`.
2. **Shared test corpus.** `oracle/corpus-items/` (859 files, 69,552,646 bytes), byte-identical to `ffe080b`. It is the owner's frozen Mall item snapshot (`oracle/corpus-items/PROVENANCE.json`). Three tests read it: `javascript_tokens_match_cm5_runmode`, `vrml_tokens_match_js_vrml_mode` and `cmd_fuzz_oracle`. The owner approved the copy on 2026-10-25; X_ITE_R keeps its own copy.
3. **Standalone workspace.** The intentional differences below.

## Intentional differences from the source

| file | change | reason |
| --- | --- | --- |
| `crates/cm-view/Cargo.toml` | dev-dependency `x3d-evidence` → `cm-evidence` | independence from X_ITE_R |
| `crates/cm-view/tests/app_script.rs:267` | `x3d_evidence::tool_absent` → `cm_evidence::tool_absent` | the same call in the new crate; no behavior change |
| `oracle/cm5-run.mjs:18`, `oracle/cm5-cmd-ops.mjs:42`, `oracle/cm5-doc-ops.mjs:30` | `createRequire (process .env .CM5_PLAYWRIGHT_FROM \|\| <old path>)` | the old path `../../../x_ite-tests/` exists only beside X_ITE_R; unset, the behavior is unchanged |
| `crates/cm-evidence/` (new) | the strict/ci policy, `skip`, `no_gpu`, `tool_absent` from `x3d-evidence/src/lib.rs` at `ffe080b`; variables renamed `X3D_EVIDENCE*` → `CM_EVIDENCE*` | the policy without the X_ITE answer-key resolver |
| `Cargo.toml` (new) | the `[workspace]` and `[workspace.package]` of X_ITE_R | same edition, license, `publish = false` |
| `Cargo.lock` | X_ITE_R's lock, pruned by Cargo to this workspace (489 → 321 packages) | no retained package changed version |
| `tools/reference.lock` (new) | the `CM5_*` lines of X_ITE_R `tools/candidate.lock` (38–44), byte-identical values; `CM5_LIVE_DIR` is now repo-relative; upstream URL, tree and digest added | CM5 half of the reference gate |
| `tools/verify-reference.sh` (new) | the CM5 half of X_ITE_R `tools/verify-candidate.sh`; also checks the upstream tree, cleanliness and content digest | fail-closed reference check |
| `tools/fetch-upstream.sh` (new) | fetches `4ca3d05` into the ignored `upstream/codemirror5`, then verifies it | the four runmode tests need it |
| `tools/reference-digest.py` | byte-identical copy | used by the gate |
| `tools/evidence_summary.py` | X_ITE_R copy with `X3D_EVIDENCE_ALLOW` → `CM_EVIDENCE_ALLOW` | CI skip summary |
| `.github/workflows/rust-tests.yml`, `.gitignore`, `.gitattributes`, `README.md`, `CLAUDE.md`, `AGENTS.md`, `NOTICE`, `PROVENANCE.md`, `docs/provenance/*` (new) | repository files | standalone project |

`upstream/codemirror5/` fetched from `https://github.com/codemirror/codemirror5.git` at `4ca3d05` matched the owner's clone exactly: tree `f6d6ff4bd14d85cf65e44f47659ac3eaabcaa5a0`, content digest `582 4918a946…7410`.

## Removal of the private inputs (2026-10-29)

The repository is public. `oracle/corpus-items/` (859 files, 69,552,646 bytes) and `oracle/cm5-cmd-ops-baseline.json` (8,666,084 bytes; it embeds about 4.2 MB of `corpus-items` text) were removed from the tracked tree at `32f3cca`. They have no redistribution permission (Ascendance3D/X_ITE_R#168). `oracle/private-inputs.tsv` records their kind, size and sha256, and `cm-evidence` (`private_input`) finds and verifies them. `docs/provenance/corpus-items-ffe080b.sha256` stays: it lists hashes and paths only, and it verifies the private copy. The mode baselines keep their `oracle/corpus-items/...` file rows, which hold a path, counts and sha256 digests, not file text. Removal from `master` does not erase older commits.
