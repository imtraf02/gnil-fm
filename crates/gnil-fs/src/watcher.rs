use std::{
    path::{Path, PathBuf},
    sync::mpsc,
};

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};

#[derive(Debug)]
pub enum WatchEvent {
    Changed(Vec<PathBuf>),
    Error(String),
}

pub struct DirectoryWatcher {
    watcher: RecommendedWatcher,
    receiver: mpsc::Receiver<WatchEvent>,
}

impl DirectoryWatcher {
    pub fn watch(path: &Path) -> notify::Result<Self> {
        let (sender, receiver) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
            let event = match event {
                Ok(event) => WatchEvent::Changed(event.paths),
                Err(error) => WatchEvent::Error(error.to_string()),
            };
            let _ = sender.send(event);
        })?;
        watcher.watch(path, RecursiveMode::NonRecursive)?;
        Ok(Self { watcher, receiver })
    }

    #[must_use]
    pub fn try_recv(&self) -> Option<WatchEvent> {
        self.receiver.try_recv().ok()
    }

    pub fn change_path(&mut self, old: &Path, new: &Path) -> notify::Result<()> {
        self.watcher.unwatch(old)?;
        self.watcher.watch(new, RecursiveMode::NonRecursive)
    }
}
