# pdft

Vibe-coded terminal PDF viewer and editor. View PDF pages as images or
extracted text, fill form fields, add text and signatures, mark pages for
deletion, assign pages to output groups for splitting, merge documents, and
search text.

Works best in a terminal with image support (kitty, ghostty, iTerm2, sixel; falls back to text mode in terminals without graphics protocol support).

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
pdft info file.pdf                        # metadata, page count, dimensions
pdft text file.pdf                        # extract text (auto-fits terminal width)
pdft text file.pdf -p 3                   # extract text from page 3
pdft merge a.pdf b.pdf -o combined.pdf    # merge PDFs
pdft split file.pdf -p 1-3,7 -o out.pdf  # extract pages
pdft delete file.pdf -p 2,4-6 -o out.pdf # remove pages
```

### Viewer options

```
pdft view file.pdf -p 5           # open at page 5
pdft view file.pdf --text         # start in text mode
```

### Interactive viewer keybindings

| Key | Action |
|-----|--------|
| `j`/`k` | Next/previous page |
| `gg`/`G` | First/last page |
| `:` | Go to page number |
| `v` | Visual select mode |
| `d`/`x` | Toggle delete mark (clears group) |
| `a`+letter | Assign to group (e.g., `ab` for group "b") |
| `u` | Undo |
| `s` | Save |
| `m` | Merge another PDF |
| `/` | Search text |
| `n`/`N` | Next/previous search match |
| `[`/`]` | Previous/next comment |
| `t` | Toggle image/text view |
| `w` | Cycle layout (no thumbnails/thumbnails only/normal) |
| `2` | Cycle spread mode (off/book/paired) |
| `A` | Add text to page (click or coordinates) |
| `S` | Place signature PNG (from `$PDFT_SIGNATURE` or prompt) |
| `F` | Fill form fields (Tab/Shift-Tab to navigate) |
| `?` | Help overlay |
| `q`/`Esc` | Quit (Esc exits visual/form mode first) |

### Form filling

Press `F` to enter form mode. Tab through fields, Enter to edit. Values are
saved to the PDF's form data and will appear in any PDF viewer. Supports text
fields and checkboxes. Radio button options are shown in the prompt.

### Signatures and text

Press `S` to stamp a PNG signature. The path is read from `$PDFT_SIGNATURE`
environment variable or prompted on first use. Click to place or type `:` for
percentage coordinates (e.g., `50,80` for center-left at 80% down).

Press `A` to add text (11pt Courier). Same placement interface.

### Mouse support

- Click sidebar or thumbnails to navigate
- Click page in signature/text/form placement modes
- Scroll wheel for text view scrolling

## Dependencies

- [hayro](https://github.com/LaurenzV/hayro) for PDF rendering and text extraction
- [lopdf](https://github.com/J-F-Liu/lopdf) for PDF manipulation
- [ratatui](https://ratatui.rs/) + [ratatui-image](https://github.com/benjajaja/ratatui-image) for the terminal UI
