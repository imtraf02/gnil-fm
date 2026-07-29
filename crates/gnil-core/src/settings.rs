use std::{collections::HashSet, fs, io, path::PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    Light,
    #[default]
    Dark,
    System,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeymapProfile {
    #[default]
    Desktop,
    Yazi,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
// These are independent user preferences, not mutually exclusive states.
#[allow(clippy::struct_excessive_bools)]
pub struct AppSettings {
    pub schema_version: u32,
    pub theme: ThemeMode,
    pub light_theme: String,
    pub dark_theme: String,
    pub keymap: KeymapProfile,
    pub show_hidden: bool,
    pub hide_gitignored: bool,
    pub preview_enabled: bool,
    pub git_status_enabled: bool,
    pub auto_mount_removable: bool,
    pub reduced_motion: bool,
    pub worker_threads: usize,
    pub memory_cache_mib: usize,
    pub favorites: Vec<PathBuf>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            schema_version: 2,
            theme: ThemeMode::Dark,
            light_theme: "GNIL Light".into(),
            dark_theme: "GNIL Dark".into(),
            keymap: KeymapProfile::Desktop,
            show_hidden: false,
            hide_gitignored: true,
            preview_enabled: true,
            git_status_enabled: true,
            auto_mount_removable: true,
            reduced_motion: false,
            worker_threads: 4,
            memory_cache_mib: 128,
            favorites: Vec::new(),
        }
    }
}

impl AppSettings {
    pub fn normalize(&mut self) {
        self.schema_version = 2;
        if !matches!(self.worker_threads, 2 | 4 | 8 | 16) {
            self.worker_threads = 4;
        }
        if !matches!(self.memory_cache_mib, 64 | 128 | 256 | 512) {
            self.memory_cache_mib = 128;
        }
        let mut unique = HashSet::new();
        self.favorites.retain(|path| unique.insert(path.clone()));
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigPaths {
    pub config: PathBuf,
    pub keymap: PathBuf,
    pub session: PathBuf,
    pub cache: PathBuf,
    pub journal: PathBuf,
}

impl ConfigPaths {
    #[must_use]
    pub fn discover() -> Self {
        let config_root = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from(".config"))
            .join("gnil-fm");
        let state_root = dirs::state_dir()
            .unwrap_or_else(|| config_root.clone())
            .join("gnil-fm");
        let cache = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from(".cache"))
            .join("gnil-fm");
        Self {
            config: config_root.join("config.toml"),
            keymap: config_root.join("keymap.toml"),
            session: state_root.join("session.json"),
            journal: state_root.join("jobs.jsonl"),
            cache,
        }
    }

    pub fn load_settings(&self) -> Result<AppSettings, SettingsError> {
        match fs::read_to_string(&self.config) {
            Ok(source) => {
                let mut settings: AppSettings = toml::from_str(&source)?;
                settings.normalize();
                Ok(settings)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(AppSettings::default()),
            Err(error) => Err(error.into()),
        }
    }

    #[must_use]
    pub fn themes_dir(&self) -> PathBuf {
        self.config
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("themes")
    }

    pub fn save_settings(&self, settings: &AppSettings) -> Result<(), SettingsError> {
        let parent = self.config.parent().ok_or(SettingsError::NoParent)?;
        fs::create_dir_all(parent)?;
        let temporary = self.config.with_extension("toml.tmp");
        fs::write(&temporary, toml::to_string_pretty(settings)?)?;
        fs::rename(temporary, &self.config)?;
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("settings path has no parent")]
    NoParent,
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Decode(#[from] toml::de::Error),
    #[error(transparent)]
    Encode(#[from] toml::ser::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_atomically() {
        let root = tempfile::tempdir().unwrap();
        let paths = ConfigPaths {
            config: root.path().join("config/gnil-fm/config.toml"),
            keymap: root.path().join("keymap.toml"),
            session: root.path().join("session.json"),
            cache: root.path().join("cache"),
            journal: root.path().join("journal.jsonl"),
        };
        let settings = AppSettings {
            show_hidden: true,
            favorites: vec![PathBuf::from("/tmp/project"), PathBuf::from("/tmp/archive")],
            ..AppSettings::default()
        };
        paths.save_settings(&settings).unwrap();
        assert_eq!(paths.load_settings().unwrap(), settings);
    }

    #[test]
    fn legacy_settings_are_normalized_and_unknown_disk_cache_is_ignored() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("config.toml");
        fs::write(
            &config,
            "schema_version = 1\nworker_threads = 99\nmemory_cache_mib = 1\ndisk_cache_mib = 2048\n",
        )
        .unwrap();
        let paths = ConfigPaths {
            config,
            keymap: root.path().join("keymap.toml"),
            session: root.path().join("session.json"),
            cache: root.path().join("cache"),
            journal: root.path().join("journal.jsonl"),
        };
        let settings = paths.load_settings().unwrap();
        assert_eq!(settings.schema_version, 2);
        assert_eq!(settings.worker_threads, 4);
        assert_eq!(settings.memory_cache_mib, 128);
    }

    #[test]
    fn favorites_keep_user_order_and_drop_duplicates() {
        let mut settings = AppSettings {
            favorites: vec![
                PathBuf::from("/z"),
                PathBuf::from("/a"),
                PathBuf::from("/z"),
            ],
            ..AppSettings::default()
        };
        settings.normalize();
        assert_eq!(
            settings.favorites,
            [PathBuf::from("/z"), PathBuf::from("/a")]
        );
    }
}
