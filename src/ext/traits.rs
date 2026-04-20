use crate::model::operation::Operation;
use crate::model::workspace::Workspace;
use anyhow::Result;

/// A page-level operation that can be applied via the TUI or CLI.
///
/// Implement this trait to add new operations to pdft.
/// Each operation should be self-contained and produce an inverse
/// Operation for undo support.
pub trait PageOperation: Send + Sync {
    /// Human-readable name for display.
    fn name(&self) -> &str;

    /// Short description for help text.
    fn description(&self) -> &str;

    /// Validate whether this operation can be applied.
    fn validate(&self, workspace: &Workspace) -> Result<()>;

    /// Apply the operation, returning an inverse for undo.
    fn apply(&self, workspace: &mut Workspace) -> Result<Operation>;

    /// Optional key binding hint for the TUI.
    fn keybinding(&self) -> Option<char> {
        None
    }
}
