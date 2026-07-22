use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeAppearance {
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ThemeColors {
    pub background: u32,
    pub surface: u32,
    pub surface_elevated: u32,
    pub border: u32,
    pub border_focused: u32,
    pub text_muted: u32,
    pub text: u32,
    pub text_emphasized: u32,
    pub accent: u32,
    pub accent_background: u32,
    pub accent_hover: u32,
    pub danger: u32,
    pub error: u32,
    pub warning: u32,
    pub git_added: u32,
    pub git_modified: u32,
    pub git_deleted: u32,
    pub git_untracked: u32,
}

impl ThemeColors {
    #[must_use]
    pub const fn dark() -> Self {
        Self {
            background: 0x11_14_12,
            surface: 0x17_1b_18,
            surface_elevated: 0x1d_22_1f,
            border: 0x25_2b_27,
            border_focused: 0x34_3c_37,
            text_muted: 0x8f_9a_93,
            text: 0xcb_d2_cd,
            text_emphasized: 0xec_f0_ed,
            accent: 0x8c_a8_94,
            accent_background: 0x26_34_2b,
            accent_hover: 0x31_42_36,
            danger: 0xd3_78_6c,
            error: 0xd3_9a_8c,
            warning: 0xc0_a8_75,
            git_added: 0x82_a9_8a,
            git_modified: 0xc0_a8_75,
            git_deleted: 0xd0_8c_80,
            git_untracked: 0x87_9c_ae,
        }
    }

    #[must_use]
    pub const fn light() -> Self {
        Self {
            background: 0xf5_f7_f5,
            surface: 0xee_f2_ef,
            surface_elevated: 0xe6_eb_e7,
            border: 0xd5_dd_d7,
            border_focused: 0xba_c7_bd,
            text_muted: 0x66_71_69,
            text: 0x34_40_39,
            text_emphasized: 0x17_20_19,
            accent: 0x55_78_64_u32,
            accent_background: 0xdc_e9_e0,
            accent_hover: 0xcd_df_d2,
            danger: 0xb9_53_4b,
            error: 0xa8_4c_45,
            warning: 0x8b_6a_2f,
            git_added: 0x4f_79_59,
            git_modified: 0x8b_6a_2f,
            git_deleted: 0xad_54_4d,
            git_untracked: 0x4f_70_86,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ThemeColorOverrides {
    pub background: Option<String>,
    pub surface: Option<String>,
    pub surface_elevated: Option<String>,
    pub border: Option<String>,
    pub border_focused: Option<String>,
    pub text_muted: Option<String>,
    pub text: Option<String>,
    pub text_emphasized: Option<String>,
    pub accent: Option<String>,
    pub accent_background: Option<String>,
    pub accent_hover: Option<String>,
    pub danger: Option<String>,
    pub error: Option<String>,
    pub warning: Option<String>,
    pub git_added: Option<String>,
    pub git_modified: Option<String>,
    pub git_deleted: Option<String>,
    pub git_untracked: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeFile {
    #[serde(default = "theme_schema_version")]
    pub schema_version: u32,
    #[serde(default, rename = "$schema")]
    pub schema: Option<String>,
    pub name: String,
    pub appearance: ThemeAppearance,
    #[serde(default)]
    pub colors: ThemeColorOverrides,
}

const fn theme_schema_version() -> u32 {
    1
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadedTheme {
    pub name: String,
    pub appearance: ThemeAppearance,
    pub colors: ThemeColors,
    pub source: Option<PathBuf>,
}

impl LoadedTheme {
    #[must_use]
    pub fn builtin(&self) -> bool {
        self.source.is_none()
    }
}

#[derive(Clone, Debug, Default)]
pub struct ThemeCatalog {
    themes: Vec<LoadedTheme>,
    pub errors: Vec<String>,
}

impl ThemeCatalog {
    #[must_use]
    pub fn load(directory: &Path) -> Self {
        let mut catalog = Self {
            themes: vec![builtin_light(), builtin_dark()],
            errors: Vec::new(),
        };
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return catalog,
            Err(error) => {
                catalog
                    .errors
                    .push(format!("{}: {error}", directory.display()));
                return catalog;
            }
        };
        let mut paths = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .collect::<Vec<_>>();
        paths.sort();
        let mut custom = BTreeMap::new();
        for path in paths {
            match load_theme_file(&path) {
                Ok(theme) => {
                    custom.insert((theme.appearance as u8, theme.name.to_lowercase()), theme);
                }
                Err(error) => catalog.errors.push(format!("{}: {error}", path.display())),
            }
        }
        for theme in custom.into_values() {
            if let Some(existing) = catalog.themes.iter_mut().find(|candidate| {
                candidate.appearance == theme.appearance
                    && candidate.name.eq_ignore_ascii_case(&theme.name)
            }) {
                *existing = theme;
            } else {
                catalog.themes.push(theme);
            }
        }
        catalog.themes.sort_by(|left, right| {
            left.appearance
                .cmp(&right.appearance)
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        });
        catalog
    }

    pub fn themes_for(&self, appearance: ThemeAppearance) -> impl Iterator<Item = &LoadedTheme> {
        self.themes
            .iter()
            .filter(move |theme| theme.appearance == appearance)
    }

    #[must_use]
    pub fn resolve(&self, name: &str, appearance: ThemeAppearance) -> (&LoadedTheme, bool) {
        if let Some(theme) = self.themes_for(appearance).find(|theme| theme.name == name) {
            return (theme, false);
        }
        let fallback = self
            .themes_for(appearance)
            .find(|theme| theme.builtin())
            .expect("theme catalog always contains a builtin per appearance");
        (fallback, true)
    }
}

fn builtin_light() -> LoadedTheme {
    LoadedTheme {
        name: "GNIL Light".into(),
        appearance: ThemeAppearance::Light,
        colors: ThemeColors::light(),
        source: None,
    }
}

fn builtin_dark() -> LoadedTheme {
    LoadedTheme {
        name: "GNIL Dark".into(),
        appearance: ThemeAppearance::Dark,
        colors: ThemeColors::dark(),
        source: None,
    }
}

fn load_theme_file(path: &Path) -> Result<LoadedTheme, String> {
    let source = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let theme: ThemeFile = serde_json::from_str(&source).map_err(|error| error.to_string())?;
    if theme.schema_version != 1 {
        return Err(format!(
            "unsupported schema_version {}",
            theme.schema_version
        ));
    }
    let name = theme.name.trim();
    if name.is_empty() {
        return Err("theme name must not be empty".into());
    }
    let base = match theme.appearance {
        ThemeAppearance::Light => ThemeColors::light(),
        ThemeAppearance::Dark => ThemeColors::dark(),
    };
    Ok(LoadedTheme {
        name: name.into(),
        appearance: theme.appearance,
        colors: apply_overrides(base, &theme.colors)?,
        source: Some(path.to_path_buf()),
    })
}

fn apply_overrides(
    mut colors: ThemeColors,
    overrides: &ThemeColorOverrides,
) -> Result<ThemeColors, String> {
    macro_rules! apply {
        ($field:ident) => {
            if let Some(value) = &overrides.$field {
                colors.$field = parse_hex_color(stringify!($field), value)?;
            }
        };
    }
    apply!(background);
    apply!(surface);
    apply!(surface_elevated);
    apply!(border);
    apply!(border_focused);
    apply!(text_muted);
    apply!(text);
    apply!(text_emphasized);
    apply!(accent);
    apply!(accent_background);
    apply!(accent_hover);
    apply!(danger);
    apply!(error);
    apply!(warning);
    apply!(git_added);
    apply!(git_modified);
    apply!(git_deleted);
    apply!(git_untracked);
    Ok(colors)
}

fn parse_hex_color(field: &str, value: &str) -> Result<u32, String> {
    let Some(hex) = value.strip_prefix('#') else {
        return Err(format!("colors.{field} must use #RRGGBB"));
    };
    if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("colors.{field} must use #RRGGBB"));
    }
    u32::from_str_radix(hex, 16).map_err(|_| format!("colors.{field} is invalid"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_theme_merges_with_appearance_defaults() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("forest.json"),
            r##"{
                "name": "Forest",
                "appearance": "dark",
                "colors": { "background": "#010203", "accent": "#abcdef" }
            }"##,
        )
        .unwrap();
        let catalog = ThemeCatalog::load(root.path());
        let (theme, fallback) = catalog.resolve("Forest", ThemeAppearance::Dark);
        assert!(!fallback);
        assert_eq!(theme.colors.background, 0x01_02_03);
        assert_eq!(theme.colors.accent, 0xab_cd_ef);
        assert_eq!(theme.colors.text, ThemeColors::dark().text);
    }

    #[test]
    fn invalid_file_is_reported_without_losing_builtins() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("broken.json"),
            r#"{"name":"Broken","appearance":"light","colors":{"accent":"red"}}"#,
        )
        .unwrap();
        let catalog = ThemeCatalog::load(root.path());
        assert_eq!(catalog.errors.len(), 1);
        assert!(catalog.resolve("missing", ThemeAppearance::Light).1);
    }

    #[test]
    fn shipped_example_matches_the_version_one_schema() {
        let theme: ThemeFile =
            serde_json::from_str(include_str!("../../../themes/forest-night.json")).unwrap();
        assert_eq!(theme.schema_version, 1);
        assert_eq!(theme.name, "Forest Night");
        assert_eq!(theme.appearance, ThemeAppearance::Dark);
    }
}
