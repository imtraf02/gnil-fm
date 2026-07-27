use std::{
    path::{Path, PathBuf},
    sync::mpsc,
};

use notify::{
    Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
    event::{MetadataKind, ModifyKind},
};

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
                Ok(event) if should_refresh(&event) => WatchEvent::Changed(event.paths),
                Ok(_) => return,
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

fn should_refresh(event: &Event) -> bool {
    match event.kind {
        EventKind::Access(_)
        | EventKind::Modify(ModifyKind::Metadata(MetadataKind::AccessTime))
        | EventKind::Other => false,
        EventKind::Any | EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use notify::event::{AccessKind, CreateKind, ModifyKind, RemoveKind};

    use super::*;

    #[test]
    fn ignores_non_mutating_access_events_from_directory_scans() {
        assert!(!should_refresh(&Event::new(EventKind::Access(
            AccessKind::Any
        ))));
        assert!(!should_refresh(&Event::new(EventKind::Other)));
        assert!(!should_refresh(&Event::new(EventKind::Modify(
            ModifyKind::Metadata(MetadataKind::AccessTime)
        ))));
    }

    #[test]
    fn refreshes_for_filesystem_mutations() {
        assert!(should_refresh(&Event::new(EventKind::Create(
            CreateKind::Any
        ))));
        assert!(should_refresh(&Event::new(EventKind::Modify(
            ModifyKind::Any
        ))));
        assert!(should_refresh(&Event::new(EventKind::Remove(
            RemoveKind::Any
        ))));
    }
}
