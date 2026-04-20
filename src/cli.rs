use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "pdft", about = "Terminal PDF tool", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// PDF file
    #[arg(value_name = "FILE", global = true)]
    pub file: Option<PathBuf>,
}

#[derive(Subcommand)]
pub enum Command {
    /// View a PDF interactively
    View {
        /// PDF file to view
        file: PathBuf,

        /// Force halfblock rendering (no kitty/sixel graphics)
        #[arg(long)]
        halfblock: bool,

        /// Start in text mode
        #[arg(long)]
        text: bool,

        /// Open at page number (1-indexed)
        #[arg(short, long)]
        page: Option<usize>,

        /// Watch the file for changes and reload automatically
        #[arg(long)]
        watch: bool,
    },

    /// Merge multiple PDFs into one
    Merge {
        /// Input PDF files
        #[arg(required = true)]
        files: Vec<PathBuf>,

        /// Output file path
        #[arg(short, long)]
        output: PathBuf,
    },

    /// Extract page ranges into a new PDF
    Split {
        /// Input PDF file
        file: PathBuf,

        /// Page ranges to extract (e.g., "1-3,5,7-9")
        #[arg(short, long, value_parser = parse_page_ranges)]
        pages: Vec<Vec<PageRange>>,

        /// Output file path
        #[arg(short, long)]
        output: PathBuf,
    },

    /// Delete pages from a PDF
    Delete {
        /// Input PDF file
        file: PathBuf,

        /// Page numbers/ranges to delete (e.g., "1-3,5,7-9")
        #[arg(short, long, value_parser = parse_page_ranges)]
        pages: Vec<Vec<PageRange>>,

        /// Output file path (defaults to overwriting input)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Print PDF metadata and page count
    Info {
        /// PDF file to inspect
        file: PathBuf,
    },

    /// Extract text from a PDF page and print to terminal
    Text {
        /// PDF file
        file: PathBuf,

        /// Page number (1-indexed, default: 1)
        #[arg(short, long, default_value = "1")]
        page: usize,

        /// Terminal columns (auto-detected if omitted)
        #[arg(short = 'W', long, default_value = "0", hide_default_value = true)]
        width: u16,

        /// Terminal rows (auto-detected if omitted)
        #[arg(short = 'H', long, default_value = "0", hide_default_value = true)]
        height: u16,
    },
}

/// A range of pages (1-indexed, inclusive on both ends).
#[derive(Debug, Clone)]
pub struct PageRange {
    pub start: usize,
    pub end: usize,
}

impl PageRange {
    /// Expand into 0-indexed page numbers.
    pub fn to_indices(&self) -> impl Iterator<Item = usize> {
        (self.start - 1)..self.end
    }
}

/// Parse a comma-separated list of page ranges like "1-3,5,7-9".
fn parse_page_ranges(s: &str) -> Result<Vec<PageRange>, String> {
    let mut ranges = Vec::new();
    for part in s.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((a, b)) = part.split_once('-') {
            let start: usize = a.trim().parse().map_err(|_| format!("invalid page number: {a}"))?;
            let end: usize = b.trim().parse().map_err(|_| format!("invalid page number: {b}"))?;
            if start == 0 || end == 0 {
                return Err("page numbers are 1-indexed".into());
            }
            if start > end {
                return Err(format!("invalid range: {start}-{end}"));
            }
            ranges.push(PageRange { start, end });
        } else {
            let n: usize = part.parse().map_err(|_| format!("invalid page number: {part}"))?;
            if n == 0 {
                return Err("page numbers are 1-indexed".into());
            }
            ranges.push(PageRange { start: n, end: n });
        }
    }
    if ranges.is_empty() {
        return Err("no page ranges specified".into());
    }
    Ok(ranges)
}

/// Collect all page ranges into a sorted, deduplicated list of 0-indexed page indices.
pub fn collect_page_indices(ranges: &[PageRange]) -> Vec<usize> {
    let mut indices: Vec<usize> = ranges.iter().flat_map(|r| r.to_indices()).collect();
    indices.sort_unstable();
    indices.dedup();
    indices
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_page() {
        let ranges = parse_page_ranges("3").unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].start, 3);
        assert_eq!(ranges[0].end, 3);
    }

    #[test]
    fn parse_range() {
        let ranges = parse_page_ranges("2-5").unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].start, 2);
        assert_eq!(ranges[0].end, 5);
    }

    #[test]
    fn parse_comma_separated() {
        let ranges = parse_page_ranges("1,3-5,7").unwrap();
        assert_eq!(ranges.len(), 3);
    }

    #[test]
    fn parse_with_whitespace() {
        let ranges = parse_page_ranges(" 1 , 3 - 5 ").unwrap();
        assert_eq!(ranges.len(), 2);
    }

    #[test]
    fn parse_rejects_zero() {
        assert!(parse_page_ranges("0").is_err());
        assert!(parse_page_ranges("0-3").is_err());
        assert!(parse_page_ranges("1-0").is_err());
    }

    #[test]
    fn parse_rejects_reversed_range() {
        assert!(parse_page_ranges("5-3").is_err());
    }

    #[test]
    fn parse_rejects_empty() {
        assert!(parse_page_ranges("").is_err());
        assert!(parse_page_ranges(",,,").is_err());
    }

    #[test]
    fn parse_rejects_non_numeric() {
        assert!(parse_page_ranges("abc").is_err());
    }

    #[test]
    fn to_indices_converts_to_zero_based() {
        let r = PageRange { start: 1, end: 3 };
        let indices: Vec<usize> = r.to_indices().collect();
        assert_eq!(indices, vec![0, 1, 2]);
    }

    #[test]
    fn collect_deduplicates_and_sorts() {
        let ranges = parse_page_ranges("3,1-3,5").unwrap();
        let indices = collect_page_indices(&ranges);
        assert_eq!(indices, vec![0, 1, 2, 4]);
    }
}
