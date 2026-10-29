# CodeMirror_R

CodeMirror_R ports the behavior of [CodeMirror 5](https://codemirror.net/5/) (version 5.65.21) to Rust. Tests compare it with the pinned upstream source and with recordings of upstream CodeMirror.

It has a shared editor engine, a native desktop editor (wgpu and winit), and a browser adapter for `wasm32-unknown-unknown`.

## Status

The project is under active development. No release is published, and the API can change.

It does not have full CodeMirror 5 compatibility. The lists below describe the current state. The [issue tracker](https://github.com/Ascendance3D/CodeMirror_R/issues) has the details.

### Working

- Document model: B-tree line storage, changes, undo and redo history with event merging, selections, marked text, line classes, line widgets, gutters and gutter markers, and linked documents. Text positions use UTF-16 offsets, as in JavaScript.
- Mode API: `defineMode`, `defineMIME`, `resolveMode`, `getMode`, `extendMode`, helpers, `runMode`, overlays (`addOverlay`, `removeOverlay`), and the `simple`, `overlay`, `multiplex` and `meta` mode addons.
- Language modes: JavaScript, XML, CSS, HTML mixed, C-like (16 MIME types), Rust and TOML, plus a native VRML mode.
- Editor: commands, the `basic`, `pcDefault`, `macDefault` and `emacsy` key maps, cursor movement, measurement, highlighting, mouse input, and editor events in CodeMirror order.
- Native editor `cm-edit`, with system clipboard support.
- Browser adapter `cm-web`: DOM rendering, input, clipboard and the host clock.

### Missing or incomplete

- 114 of the 121 upstream language mode directories ([#25](https://github.com/Ascendance3D/CodeMirror_R/issues/25)).
- The other addons, for example search, fold, hint, lint, dialog and comment ([#26](https://github.com/Ascendance3D/CodeMirror_R/issues/26)).
- The 65 themes ([#28](https://github.com/Ascendance3D/CodeMirror_R/issues/28)), the Emacs and Sublime key maps ([#27](https://github.com/Ascendance3D/CodeMirror_R/issues/27)) and the Vim key map.
- Inline widgets and `addWidget` ([#29](https://github.com/Ascendance3D/CodeMirror_R/issues/29)), collapsed ranges ([#30](https://github.com/Ascendance3D/CodeMirror_R/issues/30)), and right-to-left layout, cursor and movement ([#32](https://github.com/Ascendance3D/CodeMirror_R/issues/32); the bidi ordering helpers are done).
- Touch input ([#46](https://github.com/Ascendance3D/CodeMirror_R/issues/46)), complete drag and drop ([#47](https://github.com/Ascendance3D/CodeMirror_R/issues/47)) and screen reader support ([#48](https://github.com/Ascendance3D/CodeMirror_R/issues/48)).
- `swapDoc` ([#39](https://github.com/Ascendance3D/CodeMirror_R/issues/39)) and some mode API details ([#57](https://github.com/Ascendance3D/CodeMirror_R/issues/57)).

## Targets

| target | crates | tested |
| --- | --- | --- |
| Linux x86_64 | all except `cm-web` | CI, on a GitHub-hosted runner without a GPU |
| Windows, macOS | all except `cm-web` | not tested |
| `wasm32-unknown-unknown` | `cm-core`, `cm-modes`, `cm-view --no-default-features`, `cm-web` | built by the tests; browser runs are local only ([#33](https://github.com/Ascendance3D/CodeMirror_R/issues/33)) |

## Requirements

- Rust 1.95.0, the version that CI uses. The workspace uses the 2024 edition.
- On Linux, for the native editor: `pkg-config`, `libwayland-dev`, `libxkbcommon-dev` and a Vulkan driver (for example `mesa-vulkan-drivers`).
- Optional, for some tests: Node.js, `wasm-bindgen` at the `Cargo.lock` version with the `wasm32-unknown-unknown` target, `xvfb-run` and Python 3. Without them, those tests fail in strict mode or skip by name in CI mode.

## Build

```sh
cargo build --workspace
cargo build --release -p cm-view --bin cm-edit
```

## Test

```sh
tools/fetch-upstream.sh
cargo test --workspace --no-fail-fast
```

`tools/fetch-upstream.sh` downloads the pinned CodeMirror 5 source into `upstream/` (gitignored) and checks both reference trees. By default the tests are strict: a missing tool, GPU adapter or private input fails the test. CI uses CI mode, where these tests skip by name:

```sh
CM_EVIDENCE=ci CM_EVIDENCE_ALLOW=no-gpu-adapter,tool-absent,corpus-items,cm5-cmd-ops-baseline.json cargo test --workspace --no-fail-fast
```

`corpus-items` (historical Mall item files) and `cm5-cmd-ops-baseline.json` (it embeds their text) are private inputs. They have no redistribution permission and are not in the repository. `oracle/private-inputs.tsv` lists them with their sizes and sha256. The owner keeps a private copy. Put it at `oracle/<name>`, or set `CM_PRIVATE_INPUTS` to the directory that holds it. Without them, `vrml_tokens_match_js_vrml_mode`, `javascript_tokens_match_cm5_runmode` (only its 253 corpus files) and `cmd_fuzz_oracle` fail in strict mode.

[docs/TESTING.md](docs/TESTING.md) describes the reference pin, the browser runs, the upstream CodeMirror test suite (1483 tests) and how to record the oracle data again.

## Usage

Open a file in the native editor:

```sh
./target/release/cm-edit Cargo.toml --mode text/x-toml
```

Other options are `--read-only`, `--wrap` and `--no-line-numbers`. `--script <steps>` replays input for tests.

Tokenize text with a mode, as CodeMirror's `runMode` does:

```rust
use cm_modes::{run_mode_collect, Registry, Spec};

let registry = Registry::with_all_modes();
for (text, style) in run_mode_collect(&registry, "name = \"value\"\n", &Spec::from("text/x-toml")) {
    println!("{text:?} {style:?}");
}
```

The crates are not on crates.io. Use a Git or path dependency. [docs/EMBEDDING.md](docs/EMBEDDING.md) describes the host services (time, files, clipboard) and the event model.

## Repository layout

| path | contents |
| --- | --- |
| `crates/cm-core` | document model and utilities, no UI |
| `crates/cm-modes` | mode API, tokenization loop and language modes |
| `crates/cm-view` | commands, key maps, measurement, input, and the native editor (`cm-edit`, `cm-render`) |
| `crates/cm-web` | browser adapter (`wasm32-unknown-unknown`, wasm-bindgen) |
| `crates/cm-evidence` | test evidence policy: a test that cannot run fails or skips by name |
| `oracle/` | vendored CodeMirror 5.65.21 tree, recording scripts and baselines |
| `tools/` | reference pin (`reference.lock`), fetch and verification scripts |
| `docs/` | testing and embedding guides, provenance records |

## Upstream and attribution

CodeMirror is copyright Marijn Haverbeke and others, under the MIT License. `oracle/cm5-build/` is an unchanged copy of CodeMirror 5.65.21 (commit `4ca3d05`), with its license in `oracle/cm5-build/LICENSE`. [NOTICE](NOTICE) lists all third-party material, and [PROVENANCE.md](PROVENANCE.md) records where this repository came from.

## License

MIT. See [LICENSE](LICENSE).

## Contributing

Bug reports and pull requests are welcome. For a large change, please open an [issue](https://github.com/Ascendance3D/CodeMirror_R/issues) first.

- Run `cargo fmt --all` and the test commands above before you open a pull request.
- Compare new behavior with CodeMirror 5.65.21, and add a test that records the upstream result.
- Do not change the reference pin, `oracle/cm5-build/` or a baseline file to make a test pass.
- A maintainer approves CI runs for pull requests from outside contributors. The AI review job needs a repository secret, so it does not work for pull requests from forks.

CI: [GitHub Actions](https://github.com/Ascendance3D/CodeMirror_R/actions).
