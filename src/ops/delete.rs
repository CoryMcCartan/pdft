use anyhow::{Context, Result};
use std::path::Path;

/// Delete pages from a lopdf document and save.
/// `page_indices` are 0-based.
pub fn delete_pages(
    input: &Path,
    page_indices: &[usize],
    output: &Path,
) -> Result<()> {
    let mut doc = lopdf::Document::load(input)
        .with_context(|| format!("failed to load {}", input.display()))?;

    let page_count = doc.get_pages().len();

    // lopdf delete_pages expects 1-indexed page numbers
    let mut to_delete: Vec<u32> = page_indices
        .iter()
        .map(|&i| (i + 1) as u32)
        .filter(|&n| n >= 1 && n <= page_count as u32)
        .collect();
    to_delete.sort_unstable();
    to_delete.dedup();

    if to_delete.is_empty() {
        anyhow::bail!("no valid pages to delete");
    }
    if to_delete.len() >= page_count {
        anyhow::bail!("cannot delete all pages from a document");
    }

    doc.delete_pages(&to_delete);
    doc.prune_objects();
    doc.save(output)
        .with_context(|| format!("failed to save {}", output.display()))?;

    println!(
        "Deleted {} page(s) from {} → {}",
        to_delete.len(),
        input.display(),
        output.display()
    );
    Ok(())
}
