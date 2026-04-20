use super::document::PdfDocument;
use super::operation::Operation;
use super::page_ref::*;
use anyhow::Result;
use std::path::Path;

/// Central model tracking open documents, page assignments, and undo history.
pub struct Workspace {
    pub documents: Vec<PdfDocument>,
    pub pages: Vec<PageSlot>,
    pub output_targets: Vec<OutputTarget>,
    pub history: Vec<Operation>,
    pub selected_page: usize,
}

impl Workspace {
    /// Create an empty workspace.
    pub fn new() -> Self {
        Self {
            documents: Vec::new(),
            pages: Vec::new(),
            output_targets: Vec::new(),
            history: Vec::new(),
            selected_page: 0,
        }
    }

    /// Open a PDF and add all its pages to the workspace.
    pub fn open(&mut self, path: &Path) -> Result<DocId> {
        let doc = PdfDocument::open(path)?;
        let doc_id = self.documents.len();
        let page_count = doc.page_count();
        self.documents.push(doc);

        let insert_at = self.pages.len();
        for page_num in 0..page_count {
            self.pages.push(PageSlot {
                source: PageRef { doc_id, page_num },
                output_target: None,
                marked_for_delete: false,
            });
        }

        self.history.push(Operation::AddDocument { doc_id });
        self.history.push(Operation::InsertPages {
            at: insert_at,
            source: doc_id,
            pages: (0..page_count).collect(),
        });

        Ok(doc_id)
    }

    /// Total number of pages in the working list.
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Toggle delete mark on a single page.
    pub fn toggle_delete(&mut self, index: usize) {
        self.toggle_delete_batch(&[index]);
    }

    /// Toggle delete mark on multiple pages as a single undoable operation.
    pub fn toggle_delete_batch(&mut self, indices: &[usize]) {
        let valid: Vec<usize> = indices.iter().copied().filter(|&i| i < self.pages.len()).collect();
        if valid.is_empty() {
            return;
        }
        for &i in &valid {
            self.pages[i].marked_for_delete = !self.pages[i].marked_for_delete;
        }
        self.history.push(Operation::MarkForDelete { indices: valid });
    }

    /// Assign pages to an output target.
    pub fn assign_output(&mut self, indices: &[usize], target: Option<OutputTargetId>) {
        let previous: Vec<_> = indices
            .iter()
            .map(|&i| self.pages[i].output_target)
            .collect();
        for &i in indices {
            if i < self.pages.len() {
                self.pages[i].output_target = target;
            }
        }
        self.history.push(Operation::AssignOutput {
            page_indices: indices.to_vec(),
            target,
            previous,
        });
    }

    /// Add a named output target, return its ID.
    pub fn add_output_target(&mut self, target: OutputTarget) -> OutputTargetId {
        let id = self.output_targets.len();
        self.output_targets.push(target);
        id
    }

    /// Undo the last operation. Returns true if something was undone.
    pub fn undo(&mut self) -> bool {
        let op = match self.history.pop() {
            Some(op) => op,
            None => return false,
        };
        match op {
            Operation::MarkForDelete { indices } => {
                // Toggle marks back
                for &i in &indices {
                    if i < self.pages.len() {
                        self.pages[i].marked_for_delete = !self.pages[i].marked_for_delete;
                    }
                }
            }
            Operation::InsertPages { at, pages, .. } => {
                // Remove the inserted pages
                let count = pages.len();
                if at + count <= self.pages.len() {
                    self.pages.drain(at..at + count);
                }
                // Clamp selected page
                if self.selected_page >= self.pages.len() {
                    self.selected_page = self.pages.len().saturating_sub(1);
                }
            }
            Operation::AddDocument { .. } => {
                // AddDocument always precedes InsertPages; pop that too
                // (nothing to reverse for AddDocument alone — the doc stays loaded)
            }
            Operation::AssignOutput {
                page_indices,
                previous,
                ..
            } => {
                for (i, prev) in page_indices.iter().zip(previous.iter()) {
                    if *i < self.pages.len() {
                        self.pages[*i].output_target = *prev;
                    }
                }
            }
        }
        true
    }

    /// Get the active (non-deleted) pages.
    #[allow(dead_code)]
    pub fn active_pages(&self) -> impl Iterator<Item = (usize, &PageSlot)> {
        self.pages
            .iter()
            .enumerate()
            .filter(|(_, p)| !p.marked_for_delete)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a workspace with `n` fake pages (no real PDF needed).
    fn test_workspace(n: usize) -> Workspace {
        let mut ws = Workspace::new();
        for i in 0..n {
            ws.pages.push(PageSlot {
                source: PageRef { doc_id: 0, page_num: i },
                output_target: None,
                marked_for_delete: false,
            });
        }
        ws
    }

    #[test]
    fn toggle_delete_marks_and_unmarks() {
        let mut ws = test_workspace(3);
        ws.toggle_delete(1);
        assert!(ws.pages[1].marked_for_delete);
        ws.toggle_delete(1);
        assert!(!ws.pages[1].marked_for_delete);
    }

    #[test]
    fn toggle_delete_out_of_bounds_is_noop() {
        let mut ws = test_workspace(2);
        ws.toggle_delete(10); // should not panic
        assert_eq!(ws.pages.len(), 2);
    }

    #[test]
    fn undo_toggle_delete() {
        let mut ws = test_workspace(3);
        ws.toggle_delete(1);
        assert!(ws.pages[1].marked_for_delete);
        assert!(ws.undo());
        assert!(!ws.pages[1].marked_for_delete);
    }

    #[test]
    fn assign_output_and_undo() {
        let mut ws = test_workspace(3);
        ws.output_targets.push(OutputTarget {
            path: std::path::PathBuf::new(),
            label: "a".into(),
        });
        ws.assign_output(&[0, 2], Some(0));
        assert_eq!(ws.pages[0].output_target, Some(0));
        assert_eq!(ws.pages[1].output_target, None);
        assert_eq!(ws.pages[2].output_target, Some(0));

        assert!(ws.undo());
        assert_eq!(ws.pages[0].output_target, None);
        assert_eq!(ws.pages[2].output_target, None);
    }

    #[test]
    fn undo_on_empty_history_returns_false() {
        let mut ws = test_workspace(1);
        assert!(!ws.undo());
    }

    #[test]
    fn active_pages_excludes_deleted() {
        let mut ws = test_workspace(4);
        ws.toggle_delete(1);
        ws.toggle_delete(3);
        let active: Vec<usize> = ws.active_pages().map(|(i, _)| i).collect();
        assert_eq!(active, vec![0, 2]);
    }

    #[test]
    fn page_count() {
        let ws = test_workspace(5);
        assert_eq!(ws.page_count(), 5);
    }

    #[test]
    fn toggle_delete_batch_single_undo() {
        let mut ws = test_workspace(5);
        // Delete pages 1,2,3 as a batch
        ws.toggle_delete_batch(&[1, 2, 3]);
        assert!(ws.pages[1].marked_for_delete);
        assert!(ws.pages[2].marked_for_delete);
        assert!(ws.pages[3].marked_for_delete);

        // Single undo should revert all three
        assert!(ws.undo());
        assert!(!ws.pages[1].marked_for_delete);
        assert!(!ws.pages[2].marked_for_delete);
        assert!(!ws.pages[3].marked_for_delete);

        // No more undo ops
        assert!(!ws.undo());
    }
}
