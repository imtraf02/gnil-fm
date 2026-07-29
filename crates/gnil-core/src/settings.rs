use std::{collections::HashSet, fs, io, path::PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileLayout {
    #[default]
    Details,
    Grid,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DetailsColumnWidths {
    pub name: f32,
    pub modified: f32,
    pub kind: f32,
    pub size: f32,
    pub trash_name: f32,
    pub trash_deleted: f32,
    pub trash_location: f32,
}

impl Default for DetailsColumnWidths {
    fn default() -> Self {
        Self {
            name: 360.0,
            modified: 156.0,
            kind: 148.0,
            size: 104.0,
            trash_name: 360.0,
            trash_deleted: 160.0,
            trash_location: 260.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    Light,
    #[default]
    Dark,
    System,
}

/// A user change layered over the built-in keymap.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeymapBindingKind {
    #[default]
    Bind,
    Unbind,
}

/// One user-defined keymap entry. `action` remains a string so newer actions survive an older
/// application version round-trip.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct KeymapBinding {
    pub action: String,
    pub keystrokes: String,
    #[serde(default)]
    pub kind: KeymapBindingKind,
}

/// Persistent keyboard customizations.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct KeymapOverrides {
    pub version: u32,
    pub bindings: Vec<KeymapBinding>,
}

impl Default for KeymapOverrides {
    fn default() -> Self {
        Self {
            version: 1,
            bindings: Vec::new(),
        }
    }
}

impl KeymapOverrides {
    pub fn normalize(&mut self) {
        self.version = 1;
        let mut seen = HashSet::new();
        self.bindings.reverse();
        self.bindings
            .retain(|binding| seen.insert((binding.action.clone(), binding.keystrokes.clone())));
        self.bindings.reverse();
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
// These are independent user preferences, not mutually exclusive states.
#[allow(clippy::struct_excessive_bools)]
pub struct AppSettings {
    pub schema_version: u32,
    pub file_layout: FileLayout,
    pub file_sort: crate::SortSpec,
    pub details_columns: DetailsColumnWidths,
    pub theme: ThemeMode,
    pub light_theme: String,
    pub dark_theme: String,
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
            schema_version: 4,
            file_layout: FileLayout::Details,
            file_sort: crate::SortSpec::default(),
            details_columns: DetailsColumnWidths::default(),
            theme: ThemeMode::Dark,
            light_theme: "GNIL Light".into(),
            dark_theme: "GNIL Dark".into(),
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

/// Settings returned by the compatibility-aware loader.
#[derive(Clone, Debug, PartialEq)]
pub struct LoadedSettings {
    pub settings: AppSettings,
    pub migrated_legacy_keymap: bool,
}

impl AppSettings {
    pub fn normalize(&mut self) {
        self.schema_version = 4;
        self.details_columns.name = self.details_columns.name.clamp(160.0, 640.0);
        self.details_columns.modified = self.details_columns.modified.clamp(100.0, 260.0);
        self.details_columns.kind = self.details_columns.kind.clamp(90.0, 240.0);
        self.details_columns.size = self.details_columns.size.clamp(80.0, 180.0);
        self.details_columns.trash_name = self.details_columns.trash_name.clamp(160.0, 640.0);
        self.details_columns.trash_deleted = self.details_columns.trash_deleted.clamp(110.0, 260.0);
        self.details_columns.trash_location =
            self.details_columns.trash_location.clamp(160.0, 520.0);
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
        Ok(self.load_settings_with_migration()?.settings)
    }

    pub fn load_settings_with_migration(&self) -> Result<LoadedSettings, SettingsError> {
        match fs::read_to_string(&self.config) {
            Ok(source) => {
                let mut source: toml::Value = toml::from_str(&source)?;
                // Keep this compatibility path for the current release only; new configurations
                // use the single default keymap plus explicit overrides.
                let migrated_legacy_keymap = source
                    .get("keymap")
                    .and_then(toml::Value::as_str)
                    .is_some_and(|value| value.eq_ignore_ascii_case("yazi"));
                if migrated_legacy_keymap {
                    source
                        .as_table_mut()
                        .expect("TOML document root must be a table")
                        .remove("keymap");
                }
                let mut settings: AppSettings = source.try_into()?;
                settings.normalize();
                if migrated_legacy_keymap {
                    self.save_settings(&settings)?;
                }
                Ok(LoadedSettings {
                    settings,
                    migrated_legacy_keymap,
                })
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(LoadedSettings {
                settings: AppSettings::default(),
                migrated_legacy_keymap: false,
            }),
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

    pub fn load_keymap(&self) -> Result<KeymapOverrides, SettingsError> {
        match fs::read_to_string(&self.keymap) {
            Ok(source) => {
                let mut keymap: KeymapOverrides = toml::from_str(&source)?;
                keymap.normalize();
                Ok(keymap)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(KeymapOverrides::default()),
            Err(error) => Err(error.into()),
        }
    }

    pub fn save_keymap(&self, keymap: &KeymapOverrides) -> Result<(), SettingsError> {
        let parent = self.keymap.parent().ok_or(SettingsError::NoParent)?;
        fs::create_dir_all(parent)?;
        let temporary = self.keymap.with_extension("toml.tmp");
        fs::write(&temporary, toml::to_string_pretty(keymap)?)?;
        fs::rename(temporary, &self.keymap)?;
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
        assert_eq!(settings.schema_version, 4);
        assert_eq!(settings.file_layout, FileLayout::Details);
        assert_eq!(settings.file_sort, crate::SortSpec::default());
        assert_eq!(settings.details_columns, DetailsColumnWidths::default());
        assert_eq!(settings.worker_threads, 4);
        assert_eq!(settings.memory_cache_mib, 128);
    }

    #[test]
    fn legacy_yazi_keymap_is_migrated_atomically() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("config.toml");
        fs::write(&config, "keymap = \"yazi\"\nshow_hidden = true\n").unwrap();
        let paths = ConfigPaths {
            config: config.clone(),
            keymap: root.path().join("keymap.toml"),
            session: root.path().join("session.json"),
            cache: root.path().join("cache"),
            journal: root.path().join("journal.jsonl"),
        };

        let loaded = paths.load_settings_with_migration().unwrap();

        assert!(loaded.migrated_legacy_keymap);
        assert!(loaded.settings.show_hidden);
        assert!(!fs::read_to_string(config).unwrap().contains("yazi"));
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

    #[test]
    fn layout_and_sort_round_trip() {
        let root = tempfile::tempdir().unwrap();
        let paths = ConfigPaths {
            config: root.path().join("config.toml"),
            keymap: root.path().join("keymap.toml"),
            session: root.path().join("session.json"),
            cache: root.path().join("cache"),
            journal: root.path().join("journal.jsonl"),
        };
        let settings = AppSettings {
            file_layout: FileLayout::Grid,
            file_sort: crate::SortSpec {
                field: crate::SortField::Modified,
                direction: crate::SortDirection::Descending,
                directories_first: true,
            },
            details_columns: DetailsColumnWidths {
                name: 412.0,
                kind: 132.0,
                ..DetailsColumnWidths::default()
            },
            ..AppSettings::default()
        };

        paths.save_settings(&settings).unwrap();

        assert_eq!(paths.load_settings().unwrap(), settings);
    }

    #[test]
    fn keymap_round_trip_is_atomic_and_keeps_last_duplicate() {
        let root = tempfile::tempdir().unwrap();
        let paths = ConfigPaths {
            config: root.path().join("config.toml"),
            keymap: root.path().join("config/keymap.toml"),
            session: root.path().join("session.json"),
            cache: root.path().join("cache"),
            journal: root.path().join("journal.jsonl"),
        };
        let mut keymap = KeymapOverrides {
            version: 8,
            bindings: vec![
                KeymapBinding {
                    action: "file.copy".into(),
                    keystrokes: "ctrl-c".into(),
                    kind: KeymapBindingKind::Bind,
                },
                KeymapBinding {
                    action: "file.copy".into(),
                    keystrokes: "ctrl-c".into(),
                    kind: KeymapBindingKind::Unbind,
                },
            ],
        };
        keymap.normalize();
        paths.save_keymap(&keymap).unwrap();

        assert_eq!(paths.load_keymap().unwrap(), keymap);
        assert_eq!(keymap.version, 1);
        assert_eq!(keymap.bindings.len(), 1);
        assert_eq!(keymap.bindings[0].kind, KeymapBindingKind::Unbind);
    }
}
