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

    /// Toggle delete mark on the selected page.
    pub fn toggle_delete(&mut self, index: usize) {
        if index < self.pages.len() {
            self.pages[index].marked_for_delete = !self.pages[index].marked_for_delete;
            self.history.push(Operation::MarkForDelete {
                indices: vec![index],
            });
        }
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
            Operation::DeletePages { .. } | Operation::ReorderPage { .. } => {
                // Not yet used in the interactive flow
            }
        }
        true
    }

    /// Get the active (non-deleted) pages.
    pub fn active_pages(&self) -> impl Iterator<Item = (usize, &PageSlot)> {
        self.pages
            .iter()
            .enumerate()
            .filter(|(_, p)| !p.marked_for_delete)
    }
}
