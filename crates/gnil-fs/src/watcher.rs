use std::path::{Path, PathBuf};

use notify::{
    Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
    event::{MetadataKind, ModifyKind, RenameMode},
};

#[derive(Debug)]
pub enum WatchEvent {
    Upsert(Vec<PathBuf>),
    Remove(Vec<PathBuf>),
    Rename { from: PathBuf, to: PathBuf },
    Rescan,
    Error(String),
}

pub struct DirectoryWatcher {
    _watcher: RecommendedWatcher,
    receiver: async_channel::Receiver<WatchEvent>,
}

impl DirectoryWatcher {
    pub fn watch(path: &Path) -> notify::Result<Self> {
        let (sender, receiver) = async_channel::unbounded();
        let mut watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
            let event = match event {
                Ok(event) => classify_event(event),
                Err(error) => Some(WatchEvent::Error(error.to_string())),
            };
            if let Some(event) = event {
                let _ = sender.try_send(event);
            }
        })?;
        watcher.watch(path, RecursiveMode::NonRecursive)?;
        Ok(Self {
            _watcher: watcher,
            receiver,
        })
    }

    #[must_use]
    pub fn events(&self) -> async_channel::Receiver<WatchEvent> {
        self.receiver.clone()
    }
}

fn classify_event(event: Event) -> Option<WatchEvent> {
    match event.kind {
        EventKind::Access(_)
        | EventKind::Modify(ModifyKind::Metadata(MetadataKind::AccessTime))
        | EventKind::Other => None,
        EventKind::Remove(_) => Some(WatchEvent::Remove(event.paths)),
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) if event.paths.len() == 2 => {
            Some(WatchEvent::Rename {
                from: event.paths[0].clone(),
                to: event.paths[1].clone(),
            })
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
            Some(WatchEvent::Remove(event.paths))
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::To)) => {
            Some(WatchEvent::Upsert(event.paths))
        }
        EventKind::Modify(ModifyKind::Name(
            RenameMode::Both | RenameMode::Any | RenameMode::Other,
        ))
        | EventKind::Any => Some(WatchEvent::Rescan),
        EventKind::Create(_) | EventKind::Modify(_) => Some(WatchEvent::Upsert(event.paths)),
    }
}

#[cfg(test)]
mod tests {
    use notify::event::{AccessKind, CreateKind, ModifyKind, RemoveKind};

    use super::*;

    #[test]
    fn ignores_non_mutating_access_events_from_directory_scans() {
        assert!(classify_event(Event::new(EventKind::Access(AccessKind::Any))).is_none());
        assert!(classify_event(Event::new(EventKind::Other)).is_none());
        assert!(
            classify_event(Event::new(EventKind::Modify(ModifyKind::Metadata(
                MetadataKind::AccessTime
            ))))
            .is_none()
        );
    }

    #[test]
    fn classifies_filesystem_mutations_for_incremental_updates() {
        let created = PathBuf::from("/tmp/created");
        let modified = PathBuf::from("/tmp/modified");
        let removed = PathBuf::from("/tmp/removed");
        assert!(matches!(
            classify_event(
                Event::new(EventKind::Create(CreateKind::Any)).add_path(created.clone())
            ),
            Some(WatchEvent::Upsert(paths)) if paths == [created]
        ));
        assert!(matches!(
            classify_event(
                Event::new(EventKind::Modify(ModifyKind::Any)).add_path(modified.clone())
            ),
            Some(WatchEvent::Upsert(paths)) if paths == [modified]
        ));
        assert!(matches!(
            classify_event(
                Event::new(EventKind::Remove(RemoveKind::Any)).add_path(removed.clone())
            ),
            Some(WatchEvent::Remove(paths)) if paths == [removed]
        ));
    }

    #[test]
    fn keeps_both_sides_of_a_rename() {
        let from = PathBuf::from("/tmp/from");
        let to = PathBuf::from("/tmp/to");
        assert!(matches!(
            classify_event(
                Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
                    .add_path(from.clone())
                    .add_path(to.clone())
            ),
            Some(WatchEvent::Rename {
                from: actual_from,
                to: actual_to,
            }) if actual_from == from && actual_to == to
        ));
    }
}
