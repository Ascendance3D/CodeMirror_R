# Embedding

This guide describes the host services and the event model for a program that embeds the editor.

## Targets and host services

`cm-core`, `cm-modes` and `cm-view --no-default-features` are the shared engine. They build for native targets and for `wasm32-unknown-unknown`. `cm-view` features:

- `gpu` (default): the wgpu renderer and the winit front end (`cm-edit`, `cm-render`). Native only.
- `clipboard-native` (default): the system clipboard through arboard, on Linux and the BSDs, Windows and macOS. On other targets it adds nothing, and `native_clipboard::SystemClipboard` is an in-memory clipboard. `gpu` alone no longer includes it.

The host supplies these services:

- **Time:** `cm_core::clock::set_override` / `set_host_clock`. On `wasm32-unknown-unknown` there is no system clock. Install `Date.now` there; without it the time is unknown, and `+`-origin edits do not merge in the history. See the `clock` module documentation.
- **Files:** `cm_view::fileio::FileHost` (`StdFs`, `MemFiles`, or your own) through `App::open_with`. `read` returns `Ok(None)` only for a missing file, which opens empty. Every other failure is an `Err`, and `App::open_with` returns it without building an editor. `StdFs` treats only `NotFound` as missing. On `wasm32-unknown-unknown` every `StdFs` call returns `Unsupported`, so the browser supplies its own host (`cm-web`'s `JsFiles`).
- **Clipboard:** `cm_view::app::Clipboard`.

## Events

Editor, document, line and marker events follow CM5's order: immediate events (`beforeChange`, `beforeSelectionChange`, `beforeCursorEnter`, `historyAdded`, `focus`, `blur`) run at once; the others are delayed to the end of the outermost operation (or, outside any operation, to the host's next `fire_orphan_delayed`). Handlers are typed (`Editor::on_change`, `Doc::on_change`, `LineHandle::on_delete`, `TextMarker::on_hide`, …) and return a `ListenerId` for `off`. Editor handlers receive a `Cx` with `&mut Editor` and the other editors the caller can reach; several editors that share documents or call each other live in an `EditorSet`. Event state is per thread. A document method called directly on a document attached to an editor parks the editor's part of the operation until the editor's next call or `flush_parked`. A panic in a handler still ends the operation; `panic = "abort"` builds are not supported. See `crates/cm-core/src/operation_group.rs` and `crates/cm-view/src/events.rs`; the order is checked against CM5 recordings in `crates/cm-view/tests/editor_events.rs` and `editor_set.rs`.
