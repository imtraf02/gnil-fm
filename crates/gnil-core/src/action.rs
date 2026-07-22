use serde::{Deserialize, Serialize};

/// Stable identifier used by the command palette and configurable keymaps.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct ActionId(pub &'static str);

pub mod actions {
    use super::ActionId;

    pub const BACK: ActionId = ActionId("navigation.back");
    pub const FORWARD: ActionId = ActionId("navigation.forward");
    pub const UP: ActionId = ActionId("navigation.up");
    pub const OPEN: ActionId = ActionId("file.open");
    pub const COPY: ActionId = ActionId("file.copy");
    pub const CUT: ActionId = ActionId("file.cut");
    pub const PASTE: ActionId = ActionId("file.paste");
    pub const RENAME: ActionId = ActionId("file.rename");
    pub const BULK_RENAME: ActionId = ActionId("file.bulk_rename");
    pub const CREATE_SYMLINK: ActionId = ActionId("file.create_symlink");
    pub const SET_PERMISSIONS: ActionId = ActionId("file.set_permissions");
    pub const COPY_PATH: ActionId = ActionId("file.copy_path");
    pub const COPY_RELATIVE_PATH: ActionId = ActionId("file.copy_relative_path");
    pub const TRASH: ActionId = ActionId("file.trash");
    pub const DELETE_PERMANENTLY: ActionId = ActionId("file.delete_permanently");
    pub const UNDO: ActionId = ActionId("file.undo");
    pub const TOGGLE_PREVIEW: ActionId = ActionId("preview.toggle");
    pub const SEARCH_FILES: ActionId = ActionId("search.files");
    pub const TOGGLE_HIDDEN: ActionId = ActionId("view.toggle_hidden");
    pub const TOGGLE_YAZI: ActionId = ActionId("keymap.toggle_yazi");
}
