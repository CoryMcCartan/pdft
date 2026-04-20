use anyhow::{Context, Result};
use std::path::Path;

/// Extract specific pages from a PDF into a new file.
/// `page_indices` are 0-based.
pub fn split_pages(
    input: &Path,
    page_indices: &[usize],
    output: &Path,
) -> Result<()> {
    let doc = lopdf::Document::load(input)
        .with_context(|| format!("failed to load {}", input.display()))?;

    let page_count = doc.get_pages().len();

    // Validate indices
    for &idx in page_indices {
        if idx >= page_count {
            anyhow::bail!(
                "page {} out of range (document has {} pages)",
                idx + 1,
                page_count
            );
        }
    }

    // Strategy: clone the document, delete all pages NOT in our set
    let mut result = doc.clone();
    let pages_to_delete: Vec<u32> = (1..=page_count as u32)
        .filter(|&n| !page_indices.contains(&((n - 1) as usize)))
        .collect();

    if !pages_to_delete.is_empty() {
        result.delete_pages(&pages_to_delete);
        result.prune_objects();
    }

    result
        .save(output)
        .with_context(|| format!("failed to save {}", output.display()))?;

    println!(
        "Extracted {} page(s) from {} → {}",
        page_indices.len(),
        input.display(),
        output.display()
    );
    Ok(())
}
