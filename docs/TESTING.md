# Testing

This guide holds the details of the reference pin, the test suites and the oracle recordings. The README has the short form.

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

`crates/cm-web/tests/wasm_runtime.rs` builds `cm-web` for `wasm32-unknown-unknown` and runs the L-B scenarios through its API in Node. It needs the rustup target, `wasm-bindgen` at the Cargo.lock version and `node`. The browser runs need Playwright and a CM5 build made in a disposable copy (as for the C0 suite):

```
CM5_PLAYWRIGHT_FROM=<dir with node_modules/playwright>/ tools/web-test.sh <out dir> [<built cm5 dir>] [browsers...]
```

The browsers are `chromium`, `firefox` and `webkit`, and the emulated profiles `chromium-mobile` (Pixel 7) and `webkit-mobile` (iPhone 15). Emulation is not a phone, and Playwright's WebKit is not Safari.

Each browser ends `PASS`, `FAIL`, `BLOCKED` or `UNSUPPORTED` in `browser-run.json`, and the exit status is 0 only when all pass (1 for a failure, 2 for a missing tool or reference). Without the CM5 build, or with a directory that is not the pinned build, the browsers are `BLOCKED` and no comparison counts. A run passes only when all 60 required CM5 comparisons ran (`REQUIRED` in `browser-run.mjs`). Each browser is closed on every exit path, and a run that takes longer than `--browser-timeout` (300 s) fails. `crates/cm-web/tests/web/harness-selftest.mjs` checks these outcomes and the browser cleanup with broken references and injected faults.

The same script then runs `crates/cm-web/tests/web/metrics-lifecycle.mjs` (`metrics-lifecycle.json`). It covers the adapter's font measurement against CM5: a hidden editor, showing it, container and window resizes, `refresh()` after a font change, 0 and small measured widths, and listener and observer cleanup. Where CM5 keeps a stale line measurement until `refresh()`, the check compares with CM5 after `refresh()` and is recorded as known.

The Node run in `wasm_runtime.rs` has a 120 s deadline that the test enforces itself. It does not need a `timeout` program.

CI runs `CM_EVIDENCE=ci` with `CM_EVIDENCE_ALLOW=no-gpu-adapter,tool-absent`. Each permitted skip is one line in `CM_EVIDENCE_LOG`, which `tools/evidence_summary.py` checks.

Renderer goldens are exact only for the adapter of a reviewed golden set (`crates/cm-view/tests/visual/goldens/ADAPTER.txt`, `REVIEWED.txt`). On any other adapter the geometry checks still run and the pixel comparison ends as a named `golden-unverified` evidence skip, which fails in strict mode. See the `visual_render.rs` header.

## Modes

`cm-modes` holds the mode API of `src/modes.js` (`Registry`: `defineMode`, `defineMIME`, `resolveMode`, `getMode`, `extendMode`, helpers from `registerHelper`), the composite-mode addons `addon/mode/simple.js` (`simple`), `overlay.js` (`overlay`) and `multiplex.js` (`multiplex`), and `mode/meta.js` (`meta`: `modeInfo`, `findModeBy*`). Modes match with `cm_core::util::regexp::RegExp`, a JS `RegExp` on UTF-16 code units. A `modeInfo` entry is metadata, not an implementation: `ModeInfo::is_available` tells whether its mode is registered. Ported modes: clike, css, htmlmixed, javascript, rust, toml and xml (plus the native VRML mode); the other CM5 modes are not ported yet.

`crates/cm-modes/tests/mode_infra_oracle.rs` compares the infrastructure with CM5 recordings. To record again, build a disposable copy as for the C0 suite, then run `node oracle/cm5-mode-infra.mjs <copy>/oracle/cm5-build`. Its inputs come from `node oracle/cm5-mode-infra-cases.mjs`.

The rust mode (`mode/rust/rust.js`, a simple mode) is checked by `crates/cm-modes/tests/rust_oracle.rs` (tables, resolution, per-line state, indent and tokens, `runMode`) and `crates/cm-view/tests/rust_mode_editor.rs` (editor tokens, smart indent, electric `}`). Record them again with `node oracle/cm5-rust-mode.mjs <copy>/oracle/cm5-build` and `CM5_BUILD_DIR=<copy>/oracle/cm5-build node oracle/cm5-rust-editor.mjs`.

The clike mode (`mode/clike/clike.js`: C, C++, Java, C#, Scala, Kotlin, GLSL shaders, nesC, Objective-C, Objective-C++, Squirrel and Ceylon, 16 MIME types) is checked by `crates/cm-modes/tests/clike_oracle.rs` (every `def` configuration and its `hintWords`, mode properties, resolution, per-line state, indent and tokens, restarts from saved states, `runMode`) and `crates/cm-view/tests/clike_mode_editor.rs` (editor tokens, smart indent, `getHelpers`, typed electric input). Record them again with `node oracle/cm5-clike-mode.mjs <copy>/oracle/cm5-build` and `CM5_BUILD_DIR=<copy>/oracle/cm5-build node oracle/cm5-clike-editor.mjs`.

The longest line (`display.maxLine`) and the parser states its measurement leaves (`findMaxLine`, the change and collapsed-marker bookkeeping, the `endOperation_R2` measurement and the `updateHeightsInViewport` width check) are checked by `crates/cm-view/tests/maxline_state.rs` against `tests/data/cm5-maxline-state.json`. Record it again with `CM5_BUILD_DIR=<copy>/oracle/cm5-build node oracle/cm5-maxline-state.mjs`.

## C0 suite

The oracle scripts serve the CM5 build through `oracle/server-utils.mjs` (`safeResolve`: decoded path must stay inside the root after `realpath`, malformed URIs get 400). `node oracle/server-utils.test.mjs` checks it; `cargo test -p cm-view --test oracle_server_utils` runs the same check in the workspace suite. A file swapped for a symlink between the check and the read is not prevented (time-of-check race); the servers bind 127.0.0.1 only.

The upstream CM5 browser test suite (1483 tests) runs headless against a rollup build of the vendored tree. Build it in a **disposable copy**, so the reference tree stays unbuilt:

```
cp -a oracle/cm5-build oracle/cm5-run.mjs <copy>/oracle/
cd <copy>/oracle/cm5-build && npm install --ignore-scripts && npm run build
CM5_PLAYWRIGHT_FROM=<dir with node_modules/playwright>/ node <copy>/oracle/cm5-run.mjs --json c0.json
```

Expected: "All passed (ran 1483 …)", and `c0.json` equals `oracle/cm5-c0-baseline.json`.
