# pdft

Terminal PDF viewer and editor. Rust, using ratatui for TUI and hayro for PDF rendering.

## Build & Run

```
cargo run --bin pdft -- FILE               # open in TUI viewer
cargo run --bin pdft -- view FILE --text   # open in text mode
cargo run --bin pdft -- view FILE -p 5     # open at page 5
cargo run --bin pdft -- text FILE          # extract text to stdout
cargo run --bin pdft -- info FILE          # metadata, page count, dimensions
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
  app.rs           Top-level App state, modes, SpreadMode, save logic
  model/
    workspace.rs   Central model: documents, pages, undo history
    document.rs    PdfDocument (lopdf wrapper), form fields, signature/text embed
    page_ref.rs    PageSlot, PageRef, OutputTarget, Comment types
    operation.rs   Operation enum for undo
  render/
    renderer.rs    hayro PDF-to-image rendering (scales to fill terminal)
    cache.rs       LRU image cache (keyed by doc+page+resolution)
    text_layout.rs Text extraction via hayro Device trait
  tui/
    event_loop.rs  Main loop: draw, poll events, render pages/thumbnails/spread
    input.rs       Key/mouse handling, dialog submission, form filling
    theme.rs       Color constants
    views/
      page_view.rs   Image display (single/spread) or text view with highlighting
      sidebar.rs     Page list with delete/group/search/comment/visual indicators
      thumbnail_bar.rs  Thumbnail strip and grid with markers
      status_bar.rs  Filename, page info, mode, form field info, comments
      dialog.rs      Bottom-bar input prompt (vim command-line style)
      help.rs        Full-screen help overlay
  ops/             CLI-only operations (delete, merge, split)
```

## Key Concepts

- **hayro** renders PDF pages to bitmaps; also used for text extraction via the `Device` trait
- **lopdf** handles PDF manipulation (merge, delete pages, form fields, image/text embedding, save)
- **ratatui-image** displays rendered pages (auto-detects kitty/sixel/halfblock protocol)
- Rendering scales to fill available terminal area (2.0x minimum quality floor)
- Text extraction: hayro `Device` collects glyph positions + advance widths to detect word boundaries; y-positions snapped to detected line grid
- Form field values set via `/V` with appearance stream generation for visual rendering
- Signature embedding: PNG decoded, split to RGB+SMask, compressed, added as XObject
- `prune_objects()` called after page deletion to reduce file size

## TUI Key Bindings

- `j/k` page nav, `gg/G` first/last, `:` go to page number
- `v` visual select, `d/x` toggle delete, `a`+letter assign group
- `s` save, `m` merge, `u` undo
- `/` search, `n/N` next/prev match (relative to current page)
- `[/]` prev/next comment
- `t` image/text toggle, `w` cycle layout, `2` cycle spread (off/book/paired)
- `A` add text, `S` place signature, `F` fill form fields
- `?` help, `q`/`Esc` quit
- Mouse: click sidebar/thumbnails, scroll in text mode, click-to-place for stamps/forms

## Key Design Decisions

- Dialogs render in the bottom hint bar (vim-style), not as popups, to avoid image protocol artifacts
- Help is full-screen overlay with Clear
- Key debouncing: event loop drains queued key events, processes only the last
- Delete and group assignment are mutually exclusive on a page
- Group assignment toggles: assigning the same group again clears it
- Save with groups skips initial path prompt, goes straight to per-group prompts
- Form field tab order: sorted by page, then visual row (10pt grid snap), then x
- Spread view: Book mode (cover alone, then pairs) and Paired mode (1-2, 3-4, ...)
- `refresh_bytes()` clones doc before serializing to avoid mutating lopdf internal state
- Defaults to text mode when no graphics protocol detected (halfblock)
- Terminal resize events handled (invalidate caches, re-render)
