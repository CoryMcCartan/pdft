use std::path::PathBuf;

/// Index into Workspace::documents.
pub type DocId = usize;

/// 0-based page index within the source document.
pub type PageNum = usize;

/// Reference to a specific page in a specific document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageRef {
    pub doc_id: DocId,
    pub page_num: PageNum,
}

/// A page in the working page list.
#[derive(Debug, Clone)]
pub struct PageSlot {
    pub source: PageRef,
    pub output_target: Option<OutputTargetId>,
    pub marked_for_delete: bool,
}

/// Index into Workspace::output_targets.
pub type OutputTargetId = usize;

/// A named output file for split operations.
#[derive(Debug, Clone)]
pub struct OutputTarget {
    pub path: PathBuf,
    pub label: String,
}
