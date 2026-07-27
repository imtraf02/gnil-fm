use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use crate::PreviewResult;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct PreviewCacheKey {
    path: PathBuf,
    len: u64,
    modified_seconds: i64,
    modified_nanoseconds: i64,
    device: u64,
    inode: u64,
}

impl PreviewCacheKey {
    fn read(path: &Path) -> Option<Self> {
        let metadata = fs::symlink_metadata(path).ok()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt as _;
            Some(Self {
                path: path.to_path_buf(),
                len: metadata.len(),
                modified_seconds: metadata.mtime(),
                modified_nanoseconds: metadata.mtime_nsec(),
                device: metadata.dev(),
                inode: metadata.ino(),
            })
        }
        #[cfg(not(unix))]
        {
            let modified = metadata
                .modified()
                .ok()?
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?;
            Some(Self {
                path: path.to_path_buf(),
                len: metadata.len(),
                modified_seconds: i64::try_from(modified.as_secs()).ok()?,
                modified_nanoseconds: i64::from(modified.subsec_nanos()),
                device: 0,
                inode: 0,
            })
        }
    }
}

struct CacheEntry {
    preview: PreviewResult,
    weight: usize,
    last_used: u64,
}

pub struct PreviewCache {
    entries: HashMap<PreviewCacheKey, CacheEntry>,
    capacity: usize,
    used: usize,
    clock: u64,
}

impl PreviewCache {
    #[must_use]
    pub fn new(capacity_mib: usize) -> Self {
        Self {
            entries: HashMap::new(),
            capacity: mib_to_bytes(capacity_mib),
            used: 0,
            clock: 0,
        }
    }

    pub fn set_capacity_mib(&mut self, capacity_mib: usize) {
        self.capacity = mib_to_bytes(capacity_mib);
        self.evict_to_capacity();
    }

    pub fn get(&mut self, path: &Path) -> Option<PreviewResult> {
        let key = PreviewCacheKey::read(path)?;
        self.clock = self.clock.wrapping_add(1);
        let entry = self.entries.get_mut(&key)?;
        entry.last_used = self.clock;
        Some(entry.preview.clone())
    }

    pub fn insert(&mut self, path: &Path, preview: PreviewResult) {
        let Some(key) = PreviewCacheKey::read(path) else {
            return;
        };
        self.clock = self.clock.wrapping_add(1);
        let weight = preview_weight(&preview);
        let stale_keys = self
            .entries
            .keys()
            .filter(|cached| cached.path == key.path && **cached != key)
            .cloned()
            .collect::<Vec<_>>();
        for stale_key in stale_keys {
            if let Some(stale) = self.entries.remove(&stale_key) {
                self.used = self.used.saturating_sub(stale.weight);
            }
        }
        if let Some(previous) = self.entries.remove(&key) {
            self.used = self.used.saturating_sub(previous.weight);
        }
        self.used = self.used.saturating_add(weight);
        self.entries.insert(
            key,
            CacheEntry {
                preview,
                weight,
                last_used: self.clock,
            },
        );
        self.evict_to_capacity();
    }

    fn evict_to_capacity(&mut self) {
        while self.used > self.capacity && self.entries.len() > 1 {
            let Some(key) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            if let Some(entry) = self.entries.remove(&key) {
                self.used = self.used.saturating_sub(entry.weight);
            }
        }
    }
}

fn mib_to_bytes(mib: usize) -> usize {
    mib.saturating_mul(1024 * 1024)
}

fn preview_weight(preview: &PreviewResult) -> usize {
    match preview {
        PreviewResult::Directory(_) | PreviewResult::Image(_) | PreviewResult::Metadata(_) => 256,
        PreviewResult::Text(text) => {
            let text_bytes = text
                .lines
                .iter()
                .flat_map(|line| &line.segments)
                .map(|segment| segment.text.capacity())
                .sum::<usize>();
            text_bytes
                .saturating_add(text.lines.len() * size_of::<crate::HighlightedLine>())
                .saturating_add(text.syntax.capacity())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::{MetadataPreview, PreviewResult};

    use super::PreviewCache;

    #[test]
    fn fingerprint_invalidates_changed_files() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("preview.txt");
        fs::write(&path, b"one").unwrap();
        let mut cache = PreviewCache::new(1);
        cache.insert(
            &path,
            PreviewResult::Metadata(MetadataPreview {
                mime: "text/plain".into(),
                len: 3,
                modified_unix_ms: None,
                readonly: false,
            }),
        );
        assert!(cache.get(&path).is_some());
        fs::write(&path, b"a different size").unwrap();
        assert!(cache.get(&path).is_none());
    }
}
