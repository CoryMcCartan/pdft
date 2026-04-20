use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// A loaded PDF document, wrapping lopdf for manipulation.
/// Optionally retains the raw file bytes so hayro can parse without re-reading.
pub struct PdfDocument {
    pub path: PathBuf,
    pub label: String,
    doc: lopdf::Document,
    /// Raw PDF bytes, retained for hayro rendering.
    raw_bytes: Option<Vec<u8>>,
}

impl PdfDocument {
    /// Open a PDF from a file path, reading bytes once for both lopdf and hayro.
    pub fn open(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let doc = lopdf::Document::load_mem(&bytes)
            .with_context(|| format!("failed to parse PDF: {}", path.display()))?;
        let label = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        Ok(Self {
            path: path.to_owned(),
            label,
            doc,
            raw_bytes: Some(bytes),
        })
    }

    /// Number of pages in the document.
    pub fn page_count(&self) -> usize {
        self.doc.get_pages().len()
    }

    /// Get the page dimensions (width, height) in PDF points for each page.
    pub fn page_dimensions(&self) -> Vec<(f64, f64)> {
        let pages = self.doc.get_pages();
        let mut dims = Vec::with_capacity(pages.len());
        for page_num in 1..=pages.len() {
            let dim = (|| -> Option<(f64, f64)> {
                let &page_id = pages.get(&(page_num as u32))?;
                let obj = self.doc.get_object(page_id).ok()?;
                let dict = obj.as_dict().ok()?;
                let media_box = dict.get(b"MediaBox").ok()?.as_array().ok()?;
                if media_box.len() >= 4 {
                    let w = media_box[2].as_float().unwrap_or(612.0);
                    let h = media_box[3].as_float().unwrap_or(792.0);
                    Some((w as f64, h as f64))
                } else {
                    None
                }
            })();
            dims.push(dim.unwrap_or((612.0, 792.0)));
        }
        dims
    }

    /// Get the raw PDF bytes (for hayro rendering).
    pub fn raw_bytes(&self) -> Option<&[u8]> {
        self.raw_bytes.as_deref()
    }

    /// Clone the underlying lopdf document for manipulation.
    pub fn clone_lopdf(&self) -> lopdf::Document {
        self.doc.clone()
    }
}
