# pdft

Vibe-coded terminal-based PDF viewer and editor. View PDF pages as images or
extracted text, mark pages for deletion, assign pages to output groups for
splitting, merge documents, and search text.

Works best in a terminal with image support (kitty, ghostty, etc.; falls back
to halfblock rendering otherwise).

## Installation

```
cargo install --path .
```

## Usage

Open a PDF in the interactive viewer:
```
pdft file.pdf
```

### CLI commands

```
pdft info file.pdf                        # page count and dimensions
pdft text file.pdf -p 3                   # extract text from page 3
pdft merge a.pdf b.pdf -o combined.pdf    # merge PDFs
pdft split file.pdf -p 1-3,7 -o out.pdf  # extract pages
pdft delete file.pdf -p 2,4-6 -o out.pdf # remove pages
```

### Interactive viewer

Navigation: `j`/`k` to move between pages, `g`/`G` for first/last, `:` to jump
to a page number.

Editing: `d` to mark pages for deletion, `v` for visual selection (extend with
`j`/`k`), `a` + letter to assign pages to an output group (e.g., `ab` assigns
to group "b"). Delete and group assignment are mutually exclusive. `u` to undo.

Saving: `s` prompts for an output path. If pages are assigned to groups, you'll
be prompted for each group's output file.

Other: `/` to search text, `n`/`N` for next/previous match, `t` to toggle
image/text view, `w` to cycle layout, `m` to merge another PDF, `?` for help.

## Dependencies

- [hayro](https://github.com/nickkuk/hayro) for PDF rendering and text extraction
- [lopdf](https://github.com/nickkuk/lopdf) for PDF manipulation
- [ratatui](https://ratatui.rs/) + [ratatui-image](https://github.com/benjajaja/ratatui-image) for the terminal UI
