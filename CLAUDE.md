# pdft

Terminal PDF viewer and editor. Rust, using ratatui for TUI and hayro for PDF rendering.

## Build & Run

```
cargo run --bin pdft -- FILE          # open in TUI viewer
cargo run --bin pdft -- text FILE     # extract text to stdout
cargo run --bin pdft -- info FILE     # page count and dimensions
cargo run --bin pdft -- merge A B -o OUT
cargo run --bin pdft -- split FILE -p 1-3,5 -o OUT
cargo run --bin pdft -- delete FILE -p 2,4 -o OUT
```

There is also a `bench_render` binary. Use `--bin pdft` when building/running.

## Architecture

```
src/
  main.rs          CLI dispatch
  cli.rs           Clap arg parsing, page range syntax
  app.rs           Top-level App state, modes, save logic
  model/
    workspace.rs   Central model: documents, pages, undo history
    document.rs    PdfDocument (lopdf wrapper)
    page_ref.rs    PageSlot, PageRef, OutputTarget types
    operation.rs   Operation enum for undo
  render/
    renderer.rs    hayro PDF-to-image rendering
    cache.rs       LRU image cache (keyed by doc+page+resolution)
    text_layout.rs Text extraction via hayro Device trait
  tui/
    event_loop.rs  Main loop: draw, poll events, render pages/thumbnails
    input.rs       Key handling, dialog submission, visual mode, assign
    theme.rs       Color constants
    views/
      page_view.rs   Image display (ratatui-image) or text view with search highlighting
      sidebar.rs     Page list with delete/group/search/visual indicators
      thumbnail_bar.rs  Thumbnail strip and grid
      status_bar.rs  Filename, page info, search status
      dialog.rs      Bottom-bar input prompt (vim command-line style)
      help.rs        Full-screen help overlay
  ops/             CLI-only operations (delete, merge, split, save)
  ext/traits.rs    PageOperation trait (unused, for future extensibility)
```

## Key Concepts

- **hayro** renders PDF pages to bitmaps; also used for text extraction via the `Device` trait
- **lopdf** handles PDF manipulation (merge, delete pages, save)
- **ratatui-image** displays rendered pages (auto-detects kitty/sixel/halfblock protocol)
- Two rendering scales: main page at 2.0x, thumbnails at 0.5x, both cached
- Text extraction: hayro `Device` collects glyph positions + advance widths to detect word boundaries; y-positions snapped to detected line grid

## TUI Key Bindings

- `j/k` page nav, `g/G` first/last, `:` go to page number
- `v` visual select, `d/x` toggle delete, `a`+letter assign group (toggles; mutual exclusion with delete)
- `s` save (prompts per group if groups assigned), `m` merge, `u` undo
- `/` search, `n/N` next/prev match, empty search clears
- `t` image/text toggle, `w` cycle layout, `h/l` scroll text view
- `?` help, `q`/`Esc` quit (Esc exits visual mode first)

## Key Design Decisions

- Dialogs render in the bottom hint bar (vim-style), not as popups, to avoid image protocol artifacts
- Help is full-screen overlay with Clear
- Key debouncing: event loop drains queued key events, processes only the last
- Delete and group assignment are mutually exclusive on a page
- Group assignment toggles: assigning the same group again clears it
- Save with groups prompts for each output file path individually
