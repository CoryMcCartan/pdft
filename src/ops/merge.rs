use anyhow::{Context, Result};
use lopdf::Document;
use std::path::Path;

/// Merge multiple PDF files into a single output file.
pub fn merge_files(inputs: &[&Path], output: &Path) -> Result<()> {
    if inputs.len() < 2 {
        anyhow::bail!("need at least 2 files to merge");
    }

    let documents: Vec<Document> = inputs
        .iter()
        .map(|p| {
            Document::load(p).with_context(|| format!("failed to load {}", p.display()))
        })
        .collect::<Result<_>>()?;

    let mut merged = merge_documents(&documents)?;
    merged
        .save(output)
        .with_context(|| format!("failed to save {}", output.display()))?;

    let total_pages: usize = documents.iter().map(|d| d.get_pages().len()).sum();
    println!(
        "Merged {} files ({} pages) → {}",
        inputs.len(),
        total_pages,
        output.display()
    );
    Ok(())
}

/// Merge multiple lopdf documents into one.
pub fn merge_documents(documents: &[Document]) -> Result<Document> {
    let mut merged = documents[0].clone();

    for doc in &documents[1..] {
        let mut doc_clone = doc.clone();
        let max_id = merged.max_id;
        doc_clone.renumber_objects_with(max_id + 1);

        // Transfer all objects
        for (id, obj) in &doc_clone.objects {
            merged.objects.insert(*id, obj.clone());
        }

        // Get page references from the source document
        let source_pages = doc_clone.get_pages();
        let mut page_ids: Vec<_> = source_pages.into_iter().collect();
        page_ids.sort_by_key(|&(num, _)| num);

        // Find the Pages object in the merged catalog
        let catalog = merged.catalog().context("no catalog")?;
        let pages_ref = catalog
            .get(b"Pages")
            .ok()
            .and_then(|p| p.as_reference().ok())
            .context("failed to find Pages reference in catalog")?;

        // Add page refs to Kids array and update Parent pointers
        for (_, page_id) in &page_ids {
            // Set the page's Parent to point to merged Pages node
            if let Ok(page_obj) = merged.get_object_mut(*page_id) {
                if let Ok(page_dict) = page_obj.as_dict_mut() {
                    page_dict.set("Parent", lopdf::Object::Reference(pages_ref));
                }
            }
        }

        // Now mutate the Pages dict
        let pages_obj = merged
            .get_object_mut(pages_ref)
            .context("Pages object not found")?;
        let pages_dict = pages_obj
            .as_dict_mut()
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let kids = pages_dict
            .get_mut(b"Kids")
            .map_err(|e| anyhow::anyhow!("{e}"))
            .and_then(|k| k.as_array_mut().map_err(|e| anyhow::anyhow!("{e}")))
            .context("Pages has no Kids array")?;

        for (_, page_id) in &page_ids {
            kids.push(lopdf::Object::Reference(*page_id));
        }

        let new_count = kids.len();
        pages_dict.set("Count", lopdf::Object::Integer(new_count as i64));

        merged.max_id = merged
            .objects
            .keys()
            .map(|&(id, _)| id)
            .max()
            .unwrap_or(0);
    }

    Ok(merged)
}
