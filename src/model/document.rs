use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// A loaded PDF document, wrapping lopdf for manipulation.
pub struct PdfDocument {
    pub path: PathBuf,
    pub label: String,
    doc: lopdf::Document,
}

impl PdfDocument {
    /// Open a PDF from a file path using lopdf.
    pub fn open(path: &Path) -> Result<Self> {
        let doc = lopdf::Document::load(path)
            .with_context(|| format!("failed to load PDF: {}", path.display()))?;
        let label = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        Ok(Self {
            path: path.to_owned(),
            label,
            doc,
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

    /// Access the underlying lopdf document.
    pub fn lopdf(&self) -> &lopdf::Document {
        &self.doc
    }

    /// Take ownership of the lopdf document (consumes self).
    pub fn into_lopdf(self) -> lopdf::Document {
        self.doc
    }

    /// Clone the underlying lopdf document for manipulation.
    pub fn clone_lopdf(&self) -> lopdf::Document {
        self.doc.clone()
    }
}
