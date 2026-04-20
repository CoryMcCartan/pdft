use super::page_ref::{DocId, OutputTargetId, PageNum};

/// A reversible operation on the workspace, stored for undo.
#[derive(Debug, Clone)]
pub enum Operation {
    DeletePages {
        /// Indices into Workspace::pages that were removed.
        indices: Vec<usize>,
    },
    MarkForDelete {
        /// Indices toggled.
        indices: Vec<usize>,
    },
    InsertPages {
        at: usize,
        source: DocId,
        pages: Vec<PageNum>,
    },
    ReorderPage {
        from: usize,
        to: usize,
    },
    AssignOutput {
        page_indices: Vec<usize>,
        target: Option<OutputTargetId>,
        /// Previous assignments for undo.
        previous: Vec<Option<OutputTargetId>>,
    },
    AddDocument {
        doc_id: DocId,
    },
}
