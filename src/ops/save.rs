use crate::model::workspace::Workspace;
use anyhow::{Context, Result};
use std::path::Path;

/// Execute all pending operations in the workspace and save results.
///
/// This handles:
/// - Removing pages marked for deletion
/// - Writing pages to their assigned output targets
/// - Writing unassigned (remaining) pages to the default output
pub fn save_workspace(workspace: &Workspace, default_output: &Path) -> Result<()> {
    // Collect active pages (not marked for delete)
    let active: Vec<_> = workspace.active_pages().collect();

    if active.is_empty() {
        anyhow::bail!("no pages remaining after deletions");
    }

    // Group pages by output target
    // None -> default output, Some(id) -> specific output target
    let mut groups: std::collections::BTreeMap<Option<usize>, Vec<usize>> =
        std::collections::BTreeMap::new();

    for (_, slot) in &active {
        groups
            .entry(slot.output_target)
            .or_default()
            .push(slot.source.page_num);
    }

    // For now, only handle single-document workspaces for save
    // Multi-document save requires the merge logic
    if workspace.documents.len() != 1 {
        // For multi-document, we'd need to merge first, then split
        // This is a simplification for Phase 1
        anyhow::bail!("save with multiple source documents not yet supported in non-interactive mode");
    }

    let source_doc = &workspace.documents[0];

    for (target_id, page_nums) in &groups {
        let output_path = match target_id {
            None => default_output.to_owned(),
            Some(id) => workspace.output_targets[*id].path.clone(),
        };

        let mut doc = source_doc.clone_lopdf();
        let page_count = doc.get_pages().len();
        let pages_to_delete: Vec<u32> = (1..=page_count as u32)
            .filter(|&n| !page_nums.contains(&((n - 1) as usize)))
            .collect();

        if !pages_to_delete.is_empty() {
            doc.delete_pages(&pages_to_delete);
        }

        doc.save(&output_path)
            .with_context(|| format!("failed to save {}", output_path.display()))?;

        println!(
            "Saved {} page(s) → {}",
            page_nums.len(),
            output_path.display()
        );
    }

    Ok(())
}
