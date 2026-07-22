use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::SortSpec;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum TabRoot {
    Directory,
    Trash,
    Device { id: String, mount_root: PathBuf },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TabLocation {
    pub path: PathBuf,
    pub root: TabRoot,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TabState {
    pub path: PathBuf,
    pub root: TabRoot,
    pub back_history: Vec<TabLocation>,
    pub forward_history: Vec<TabLocation>,
    pub selected_path: Option<PathBuf>,
    pub sort: SortSpec,
    pub show_hidden: bool,
}

impl TabState {
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            root: TabRoot::Directory,
            back_history: Vec::new(),
            forward_history: Vec::new(),
            selected_path: None,
            sort: SortSpec::default(),
            show_hidden: false,
        }
    }

    pub fn navigate(&mut self, path: PathBuf) {
        self.navigate_location(TabLocation {
            path,
            root: TabRoot::Directory,
        });
    }

    pub fn navigate_trash(&mut self) {
        self.navigate_location(TabLocation {
            path: self.path.clone(),
            root: TabRoot::Trash,
        });
    }

    pub fn navigate_device(&mut self, id: String, mount_root: PathBuf) {
        self.navigate_location(TabLocation {
            path: mount_root.clone(),
            root: TabRoot::Device { id, mount_root },
        });
    }

    pub fn navigate_within_root(&mut self, path: PathBuf) {
        self.navigate_location(TabLocation {
            path,
            root: self.root.clone(),
        });
    }

    pub fn back(&mut self) -> bool {
        let Some(path) = self.back_history.pop() else {
            return false;
        };
        let current = self.location();
        self.forward_history.push(current);
        self.path = path.path;
        self.root = path.root;
        self.selected_path = None;
        true
    }

    pub fn forward(&mut self) -> bool {
        let Some(path) = self.forward_history.pop() else {
            return false;
        };
        let current = self.location();
        self.back_history.push(current);
        self.path = path.path;
        self.root = path.root;
        self.selected_path = None;
        true
    }

    pub fn up(&mut self) -> bool {
        if self.root == TabRoot::Trash {
            return false;
        }
        if let TabRoot::Device { mount_root, .. } = &self.root
            && self.path == *mount_root
        {
            return false;
        }
        let Some(parent) = self.path.parent().map(PathBuf::from) else {
            return false;
        };
        self.navigate_within_root(parent);
        true
    }

    #[must_use]
    pub fn location(&self) -> TabLocation {
        TabLocation {
            path: self.path.clone(),
            root: self.root.clone(),
        }
    }

    fn navigate_location(&mut self, location: TabLocation) {
        if location != self.location() {
            let current = self.location();
            self.path = location.path;
            self.root = location.root;
            self.back_history.push(current);
            self.forward_history.clear();
            self.selected_path = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_preserves_history() {
        let mut tab = TabState::new("/a".into());
        tab.navigate("/a/b".into());
        tab.navigate("/a/b/c".into());
        assert!(tab.back());
        assert_eq!(tab.path, PathBuf::from("/a/b"));
        assert!(tab.forward());
        assert_eq!(tab.path, PathBuf::from("/a/b/c"));
    }

    #[test]
    fn pseudo_roots_participate_in_history() {
        let mut tab = TabState::new("/home/person".into());
        tab.navigate_trash();
        assert_eq!(tab.root, TabRoot::Trash);
        assert!(tab.back());
        assert_eq!(tab.root, TabRoot::Directory);
        tab.navigate_device("usb".into(), "/run/media/usb".into());
        assert!(!tab.up());
    }
}
