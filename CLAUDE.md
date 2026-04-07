# philcom — Claude context

A terminal file manager (NC/Total Commander/Far Manager clone) written in Rust with Ratatui.

## Stack

- **Rust** + **Ratatui 0.29** + **crossterm 0.28**
- Config: TOML at `~/.config/philcom/config.toml`
- No database, no network — pure filesystem tool

## Project structure

```
src/
  main.rs      — terminal setup/teardown, entry point
  app.rs       — App struct, all state, event loop, key/mouse handling
  ui.rs        — all rendering (panels, menu, dialogs, viewer, buttons)
  panel.rs     — Panel + TabState structs: directory listing, navigation, tabs
  theme.rs     — Theme struct with 4 built-in themes (dark/light/monokai/nord)
  config.rs    — Config struct, TOML load/save, default button definitions
  editor.rs    — Built-in text editor state and key handling
```

## Key architecture decisions

- **App state** holds `left_panel_rect`, `right_panel_rect`, `menu_item_rects`, `button_rects` — populated during render and used for mouse hit-testing in the next event cycle.
- **Dialog system**: `Option<Dialog>` enum in App. Key handling dispatches to `handle_dialog_key` first if a dialog is open.
- **Viewer**: `Option<ViewerState>` — full-screen takeover, returns to normal on Esc/F3/q. Holds `lines: Vec<String>` for text mode and `raw_bytes: Vec<u8>` for hex mode. `hex_mode: bool` toggles with `h`; binary files auto-open in hex mode.
- **Editor**: `Option<EditorState>` — full-screen takeover (F4).
- **Tabs**: `Panel` holds `Vec<TabState>` + `active_tab: usize`. All panel ops delegate via `tab()` / `tab_mut()` accessors. Tab bar rendered in block title as a `Line` of spans.
- **Tab rects**: calculated geometrically in `tab_rects_for()` in ui.rs, stored in `left_tab_rects` / `right_tab_rects` for mouse hit-testing.
- **Double-click**: tracked via `last_click: Option<(col, row, Instant)>` — 400ms window.
- **Panel resize**: `split_percent: u16` (default 50), changed via Ctrl+Left/Right.
- **File search**: background thread sends results via `mpsc` channel; drained each frame in `run()` before `terminal.draw()`. Results shown in a fake panel (`SearchResultsPanel`) replacing the active panel.
- **Shell execution**: `pending_command: Option<String>` and `pending_shell: bool` are set during key handling and executed at the top of the `run()` loop where terminal access is available.
- **Bookmarks**: stored in `config.toml` as `bookmarks: Vec<String>` with `#[serde(default)]`.

## Implemented features

- Two-panel navigation, tabs per panel (Ctrl+T/W, mouse)
- F3 file viewer — line numbers, wrap, text selection, copy to clipboard; `h` toggles hex view (16 bytes/row, drag-select rows, copy as hex string)
- F4 built-in editor — Ctrl+S save, clipboard, unsaved-changes guard
- F5 Copy, F6 Move/Rename, F7 Mkdir, F8 Delete
- Batch operations (Space to mark)
- File search — wildcard, grep-in-files, hex bytes; browsable results panel
- Command line — shell commands, `cd` navigation, `q`/`quit`/`exit`
- Directory history (Alt+H) and bookmarks (Ctrl+D / F9)
- File type coloring + executable filter
- 4 themes (dark/light/monokai/nord), configurable buttons
- Mouse: click, double-click, scroll, drag selection, context menus
- Ctrl+O — spawn interactive shell, return on exit

## Build & run

```bash
cargo build
cargo run
```

## User preferences

- Keyboard shortcuts and mouse must both work everywhere
- Themes: dark default, configurable via TOML
- Style: concise code, no over-engineering
