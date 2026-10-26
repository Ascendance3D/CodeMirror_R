# CodeMirror_R

A native Rust port of [CodeMirror 5](https://codemirror.net/5/), version **5.65.21**, checked line by line against the pinned upstream source.

This repository is private. It was extracted from `Ascendance3D/X_ITE_R` at `ffe080b193f031605095f87ab4cafe443e7b50c5`; see `PROVENANCE.md`.

## Crates

| crate | contents |
| --- | --- |
| `cm-core` | the CM5 document model and utilities, no UI |
| `cm-modes` | the mode API, the tokenization loop, and the js, xml, css, htmlmixed, toml and VRML modes |
| `cm-view` | commands, keymaps, measurement, highlighting, input, and a native wgpu editor (`cm-edit`, `cm-render`) |
| `cm-evidence` | the test evidence policy: a test that cannot run fails or skips by name |

## Reference

The CodeMirror 5.65.21 reference is pinned in `tools/reference.lock`:

- `oracle/cm5-build/` is the vendored upstream tree (MIT, `oracle/cm5-build/LICENSE`). Its content digest is `440 d2daa3a3…f911c`.
- `upstream/codemirror5/` is the pinned upstream clone at `4ca3d05`. It is gitignored, and `tools/fetch-upstream.sh` writes it.
- `tools/verify-reference.sh` checks both. Run it on an unbuilt tree; a rollup build writes into the digest scope.

Do not change a pin, a baseline or the vendored tree to make a test pass.

## Tests

```
tools/fetch-upstream.sh
cargo test --workspace --no-fail-fast
```

The default evidence mode is strict (`CM_EVIDENCE` unset). An absent tool or GPU adapter fails the test.

CI runs `CM_EVIDENCE=ci` with `CM_EVIDENCE_ALLOW=no-gpu-adapter,tool-absent`. Each permitted skip is one line in `CM_EVIDENCE_LOG`, which `tools/evidence_summary.py` checks.

Renderer goldens are exact only for the adapter of a reviewed golden set (`crates/cm-view/tests/visual/goldens/ADAPTER.txt`, `REVIEWED.txt`). On any other adapter the geometry checks still run and the pixel comparison ends as a named `golden-unverified` evidence skip, which fails in strict mode. See the `visual_render.rs` header.

Recorded baselines (strict, `CM_EVIDENCE` unset, GPU adapter of the reviewed golden set, `xvfb-run` present). The count changes as tests are added; these numbers apply only to the commits named:

| commit | passed | failed | ignored | skips |
| --- | --- | --- | --- | --- |
| `cab938c` | 335 | 0 | 0 | 0 |
| `44ad822` (P0) | 381 | 0 | 0 | 0 |
| `eca4e29` | 387 | 0 | 0 | 0 |
| the commit titled "README: test baseline of the corrected P0 head" | 392 | 0 | 0 | 0 |
| the commit titled "P1.6: native host integration and event QA" | 454 | 0 | 0 | 0 |
| the commit titled "P1: fixes from the independent review" | 457 | 0 | 0 | 0 |

## Events

Editor, document, line and marker events follow CM5's order: immediate events (`beforeChange`, `beforeSelectionChange`, `beforeCursorEnter`, `historyAdded`, `focus`, `blur`) run at once; the others are delayed to the end of the outermost operation (or, outside any operation, to the host's next `fire_orphan_delayed`). Handlers are typed (`Editor::on_change`, `Doc::on_change`, `LineHandle::on_delete`, `TextMarker::on_hide`, …) and return a `ListenerId` for `off`. Editor handlers receive a `Cx` with `&mut Editor` and the other editors the caller can reach; several editors that share documents or call each other live in an `EditorSet`. Event state is per thread. A document method called directly on a document attached to an editor parks the editor's part of the operation until the editor's next call or `flush_parked`. A panic in a handler still ends the operation; `panic = "abort"` builds are not supported. See `crates/cm-core/src/operation_group.rs` and `crates/cm-view/src/events.rs`; the order is checked against CM5 recordings in `crates/cm-view/tests/editor_events.rs` and `editor_set.rs`.

## C0 suite

The upstream CM5 browser test suite (1483 tests) runs headless against a rollup build of the vendored tree. Build it in a **disposable copy**, so the reference tree stays unbuilt:

```
cp -a oracle/cm5-build oracle/cm5-run.mjs <copy>/oracle/
cd <copy>/oracle/cm5-build && npm install --ignore-scripts && npm run build
CM5_PLAYWRIGHT_FROM=<dir with node_modules/playwright>/ node <copy>/oracle/cm5-run.mjs --json c0.json
```

Expected: "All passed (ran 1483 …)", and `c0.json` equals `oracle/cm5-c0-baseline.json`.

## License

The Rust workspace declares the MIT License (`Cargo.toml`). CodeMirror 5 is MIT, copyright Marijn Haverbeke and others. See `NOTICE`.
