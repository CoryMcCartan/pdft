use super::page_ref::{DocId, OutputTargetId, PageNum};

/// A reversible operation on the workspace, stored for undo.
#[derive(Debug, Clone)]
pub enum Operation {
    MarkForDelete {
        /// Indices toggled.
        indices: Vec<usize>,
    },
    InsertPages {
        at: usize,
        #[allow(dead_code)]
        source: DocId,
        pages: Vec<PageNum>,
    },
    AssignOutput {
        page_indices: Vec<usize>,
        #[allow(dead_code)]
        target: Option<OutputTargetId>,
        /// Previous assignments for undo.
        previous: Vec<Option<OutputTargetId>>,
    },
    AddDocument {
        #[allow(dead_code)]
        doc_id: DocId,
    },
}
