use crate::model::page_ref::{OutputTarget, OutputTargetId};
use crate::model::workspace::Workspace;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// View mode for the main page viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Image,
    Text,
}

/// Layout mode controlling which panels are visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutMode {
    /// Main page view + thumbnail strip
    Normal,
    /// Thumbnail strip only (no main page)
    ThumbnailsOnly,
    /// Main page view only (no thumbnails)
    NoThumbnails,
}

/// Focus area in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Sidebar,
}

/// Modes the app can be in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    MergeInput,
    SaveInput,
    SaveConfirm,
    SaveGroupInput,
    SearchInput,
    GotoPage,
}

/// Text scroll command from input to event loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextScroll {
    None,
    Lines(i16),
    #[allow(dead_code)]
    Top,
    #[allow(dead_code)]
    Bottom,
}

/// Search state for text search across pages.
pub struct SearchState {
    pub query: String,
    /// Pages that contain matches, with match count per page.
    pub matches: Vec<(usize, usize)>,
    /// Index into `matches` for current match position.
    pub current_match: usize,
    /// For the current page: vertical positions of matches as fractions [0.0, 1.0].
    /// Updated when the page changes.
    pub current_page_match_positions: Vec<f32>,
}

/// Top-level application state shared between TUI components.
pub struct App {
    pub workspace: Workspace,
    pub view_mode: ViewMode,
    pub layout_mode: LayoutMode,
    #[allow(dead_code)]
    pub focus: Focus,
    pub mode: Mode,
    pub should_quit: bool,
    pub status_message: Option<String>,
    pub show_help: bool,
    pub search: Option<SearchState>,
    /// Pending text scroll command from input handler (applied by event loop).
    pub text_scroll: TextScroll,
    /// Path pending overwrite confirmation.
    pub pending_save_path: Option<PathBuf>,
    /// Grouped save state: list of (group_label, pages_1indexed, output_path).
    /// Populated during save, consumed one at a time via SaveGroupInput prompts.
    pub pending_group_saves: Vec<(String, Vec<u32>, PathBuf)>,
    /// Visual selection anchor (page index where `v` was pressed).
    pub visual_anchor: Option<usize>,
    /// Waiting for group letter after pressing `a`.
    pub pending_assign: bool,
    /// Waiting for second key after pressing `g`.
    pub pending_g: bool,
    /// Signal to scroll text view to the current search match.
    pub scroll_to_match: bool,
}

impl App {
    pub fn new() -> Self {
        Self {
            workspace: Workspace::new(),
            view_mode: ViewMode::Image,
            layout_mode: LayoutMode::Normal,
            focus: Focus::Sidebar,
            mode: Mode::Normal,
            should_quit: false,
            status_message: None,
            show_help: false,
            search: None,
            text_scroll: TextScroll::None,
            pending_save_path: None,
            pending_group_saves: Vec::new(),
            visual_anchor: None,
            pending_assign: false,
            pending_g: false,
            scroll_to_match: false,
        }
    }

    pub fn open_file(&mut self, path: &Path) -> Result<()> {
        self.workspace.open(path)?;
        self.status_message = Some(format!("Opened {}", path.display()));
        Ok(())
    }

    pub fn current_page(&self) -> usize {
        self.workspace.selected_page
    }

    /// Get the visual selection range (inclusive), or just the current page if not in visual mode.
    pub fn selected_range(&self) -> (usize, usize) {
        match self.visual_anchor {
            Some(anchor) => {
                let a = anchor.min(self.workspace.selected_page);
                let b = anchor.max(self.workspace.selected_page);
                (a, b)
            }
            None => {
                let p = self.workspace.selected_page;
                (p, p)
            }
        }
    }

    /// Check if a page index is in the current visual selection.
    pub fn is_selected(&self, idx: usize) -> bool {
        let (a, b) = self.selected_range();
        idx >= a && idx <= b
    }

    /// Get or create an output target for a group letter, returning its ID.
    pub fn get_or_create_group(&mut self, letter: char) -> OutputTargetId {
        let label = letter.to_string();
        if let Some(id) = self
            .workspace
            .output_targets
            .iter()
            .position(|t| t.label == label)
        {
            id
        } else {
            self.workspace.add_output_target(OutputTarget {
                path: PathBuf::new(), // filled at save time
                label,
            })
        }
    }

    pub fn page_count(&self) -> usize {
        self.workspace.page_count()
    }

    pub fn next_page(&mut self) {
        let max = self.page_count().saturating_sub(1);
        self.workspace.selected_page = (self.workspace.selected_page + 1).min(max);
    }

    pub fn prev_page(&mut self) {
        self.workspace.selected_page = self.workspace.selected_page.saturating_sub(1);
    }

    pub fn undo(&mut self) -> bool {
        self.workspace.undo()
    }

    pub fn toggle_view_mode(&mut self) {
        self.view_mode = match self.view_mode {
            ViewMode::Image => ViewMode::Text,
            ViewMode::Text => ViewMode::Image,
        };
    }

    pub fn cycle_layout(&mut self) {
        self.layout_mode = match self.layout_mode {
            LayoutMode::Normal => LayoutMode::NoThumbnails,
            LayoutMode::NoThumbnails => LayoutMode::ThumbnailsOnly,
            LayoutMode::ThumbnailsOnly => LayoutMode::Normal,
        };
    }

    /// Save to a specific output path.
    /// If pages have group assignments, produces one file per group
    /// (e.g., base_a.pdf, base_b.pdf) plus one for unassigned pages.
    pub fn save_to(&mut self, path: &Path) -> Result<()> {
        if self.workspace.documents.is_empty() {
            anyhow::bail!("no documents loaded");
        }

        self.save_single(path)
    }

    /// Save all non-deleted pages to a single file.
    fn save_single(&mut self, path: &Path) -> Result<()> {
        let deleted_indices: Vec<usize> = self
            .workspace
            .pages
            .iter()
            .enumerate()
            .filter(|(_, p)| p.marked_for_delete)
            .map(|(i, _)| i)
            .collect();

        let pages_to_delete: Vec<u32> = deleted_indices.iter().map(|&i| (i + 1) as u32).collect();

        let mut doc = self.workspace.documents[0].clone_lopdf();
        if !pages_to_delete.is_empty() {
            doc.delete_pages(&pages_to_delete);
            doc.prune_objects();
        }
        doc.save(path)
            .with_context(|| format!("failed to save {}", path.display()))?;

        let remaining = self.workspace.pages.len() - pages_to_delete.len();
        self.status_message = Some(format!("Saved {} pages → {}", remaining, path.display()));

        // Only clear marks after successful save
        for &i in &deleted_indices {
            self.workspace.pages[i].marked_for_delete = false;
        }

        Ok(())
    }

    /// Build the list of grouped saves and populate `pending_group_saves`.
    /// Returns suggested default paths for each group.
    pub fn prepare_group_saves(&mut self, base_path: &Path) {
        let stem = base_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let ext = base_path
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        let parent = base_path.parent().unwrap_or(Path::new("."));

        let mut groups: std::collections::BTreeMap<Option<OutputTargetId>, Vec<u32>> =
            std::collections::BTreeMap::new();

        for (i, page) in self.workspace.pages.iter().enumerate() {
            if page.marked_for_delete {
                continue;
            }
            groups
                .entry(page.output_target)
                .or_default()
                .push((i + 1) as u32);
        }

        self.pending_group_saves.clear();
        for (target_id, keep_pages) in groups {
            let (label, default_path) = match target_id {
                None => ("unassigned".to_string(), base_path.to_path_buf()),
                Some(id) => {
                    let lbl = self
                        .workspace
                        .output_targets
                        .get(id)
                        .map(|t| t.label.clone())
                        .unwrap_or_else(|| "x".to_string());
                    let path = parent.join(format!("{stem}_{lbl}{ext}"));
                    (format!("group '{lbl}'"), path)
                }
            };
            self.pending_group_saves.push((label, keep_pages, default_path));
        }
    }

    /// Save a single group: keep only `keep_pages` (1-indexed) and write to `path`.
    pub fn save_group(&mut self, keep_pages: &[u32], path: &Path) -> Result<()> {
        let all_pages: Vec<u32> = (1..=self.workspace.pages.len() as u32).collect();
        let delete_pages: Vec<u32> = all_pages
            .iter()
            .filter(|p| !keep_pages.contains(p))
            .copied()
            .collect();

        let mut doc = self.workspace.documents[0].clone_lopdf();
        if !delete_pages.is_empty() {
            doc.delete_pages(&delete_pages);
            doc.prune_objects();
        }
        doc.save(path)
            .with_context(|| format!("failed to save {}", path.display()))?;
        Ok(())
    }

    /// Get the original file path of the first document.
    pub fn original_path(&self) -> Option<PathBuf> {
        self.workspace.documents.first().map(|d| d.path.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::page_ref::{PageRef, PageSlot};

    fn app_with_pages(n: usize) -> App {
        let mut app = App::new();
        for i in 0..n {
            app.workspace.pages.push(PageSlot {
                source: PageRef { doc_id: 0, page_num: i },
                output_target: None,
                marked_for_delete: false,
            });
        }
        app
    }

    #[test]
    fn next_page_clamps_at_end() {
        let mut app = app_with_pages(3);
        app.workspace.selected_page = 2;
        app.next_page();
        assert_eq!(app.current_page(), 2);
    }

    #[test]
    fn prev_page_clamps_at_zero() {
        let mut app = app_with_pages(3);
        app.workspace.selected_page = 0;
        app.prev_page();
        assert_eq!(app.current_page(), 0);
    }

    #[test]
    fn navigation_through_pages() {
        let mut app = app_with_pages(5);
        assert_eq!(app.current_page(), 0);
        app.next_page();
        assert_eq!(app.current_page(), 1);
        app.next_page();
        assert_eq!(app.current_page(), 2);
        app.prev_page();
        assert_eq!(app.current_page(), 1);
    }

    #[test]
    fn selected_range_without_visual() {
        let mut app = app_with_pages(5);
        app.workspace.selected_page = 3;
        assert_eq!(app.selected_range(), (3, 3));
    }

    #[test]
    fn selected_range_with_visual_anchor() {
        let mut app = app_with_pages(5);
        app.visual_anchor = Some(1);
        app.workspace.selected_page = 3;
        assert_eq!(app.selected_range(), (1, 3));

        // Anchor after cursor
        app.visual_anchor = Some(4);
        app.workspace.selected_page = 2;
        assert_eq!(app.selected_range(), (2, 4));
    }

    #[test]
    fn is_selected_in_visual_range() {
        let mut app = app_with_pages(5);
        app.visual_anchor = Some(1);
        app.workspace.selected_page = 3;
        assert!(!app.is_selected(0));
        assert!(app.is_selected(1));
        assert!(app.is_selected(2));
        assert!(app.is_selected(3));
        assert!(!app.is_selected(4));
    }

    #[test]
    fn toggle_view_mode() {
        let mut app = App::new();
        assert_eq!(app.view_mode, ViewMode::Image);
        app.toggle_view_mode();
        assert_eq!(app.view_mode, ViewMode::Text);
        app.toggle_view_mode();
        assert_eq!(app.view_mode, ViewMode::Image);
    }

    #[test]
    fn cycle_layout() {
        let mut app = App::new();
        assert_eq!(app.layout_mode, LayoutMode::Normal);
        app.cycle_layout();
        assert_eq!(app.layout_mode, LayoutMode::NoThumbnails);
        app.cycle_layout();
        assert_eq!(app.layout_mode, LayoutMode::ThumbnailsOnly);
        app.cycle_layout();
        assert_eq!(app.layout_mode, LayoutMode::Normal);
    }

    #[test]
    fn get_or_create_group_reuses_existing() {
        let mut app = app_with_pages(3);
        let id1 = app.get_or_create_group('a');
        let id2 = app.get_or_create_group('a');
        assert_eq!(id1, id2);

        let id3 = app.get_or_create_group('b');
        assert_ne!(id1, id3);
    }
}
