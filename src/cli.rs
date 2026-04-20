use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "pdft", about = "Terminal PDF tool", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// PDF file to open in interactive mode
    #[arg(value_name = "FILE")]
    pub file: Option<PathBuf>,
}

#[derive(Subcommand)]
pub enum Command {
    /// View a PDF interactively
    View {
        /// PDF file to view
        file: PathBuf,
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

        /// Terminal columns (default: 80)
        #[arg(short = 'W', long, default_value = "80")]
        width: u16,

        /// Terminal rows (default: 60)
        #[arg(short = 'H', long, default_value = "60")]
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
