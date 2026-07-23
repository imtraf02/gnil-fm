use std::{
    collections::{HashMap, HashSet},
    ffi::{OsStr, OsString},
    fmt,
    os::unix::ffi::{OsStrExt as _, OsStringExt as _},
    path::{Path, PathBuf},
};

use globset::Glob;
use url::Url;
use zbus::zvariant::OwnedValue;

pub type PortalOptions = HashMap<String, OwnedValue>;
pub type SerializedFilter = (String, Vec<(u32, String)>);
pub type SerializedChoice = (String, String, Vec<(String, String)>, String);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PickerRequestKind {
    Open(OpenFileOptions),
    Save(SaveFileOptions),
    SaveMany(SaveFilesOptions),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PickerRequest {
    pub handle: String,
    pub app_id: String,
    pub parent_window: String,
    pub title: String,
    pub kind: PickerRequestKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommonOptions {
    pub accept_label: Option<String>,
    pub modal: bool,
    pub choices: Vec<PortalChoice>,
    pub current_folder: Option<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenFileOptions {
    pub common: CommonOptions,
    pub multiple: bool,
    pub directory: bool,
    pub filters: Vec<PortalFilter>,
    pub current_filter: Option<PortalFilter>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SaveFileOptions {
    pub common: CommonOptions,
    pub current_name: Option<String>,
    pub current_file: Option<PathBuf>,
    pub filters: Vec<PortalFilter>,
    pub current_filter: Option<PortalFilter>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SaveFilesOptions {
    pub common: CommonOptions,
    pub files: Vec<OsString>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PortalFilter {
    pub label: String,
    pub rules: Vec<FilterRule>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FilterRule {
    Glob(String),
    Mime(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PortalChoice {
    pub id: String,
    pub label: String,
    pub options: Vec<(String, String)>,
    pub selected: String,
}

impl PortalChoice {
    #[must_use]
    pub fn is_boolean(&self) -> bool {
        self.options.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PickerOutcome {
    Accepted {
        paths: Vec<PathBuf>,
        choices: Vec<(String, String)>,
        current_filter: Option<PortalFilter>,
    },
    Cancelled,
    Failed(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PortalResponse {
    pub code: u32,
    pub uris: Vec<String>,
    pub choices: Vec<(String, String)>,
    pub current_filter: Option<SerializedFilter>,
}

impl PortalResponse {
    #[must_use]
    pub fn from_outcome(outcome: PickerOutcome) -> Self {
        match outcome {
            PickerOutcome::Accepted {
                paths,
                choices,
                current_filter,
            } => {
                let Some(uris) = paths.iter().map(|path| file_uri(path)).collect::<Option<Vec<_>>>()
                else {
                    return Self::error();
                };
                if uris.is_empty() {
                    return Self::error();
                }
                Self {
                    code: 0,
                    uris,
                    choices,
                    current_filter: current_filter.map(|filter| filter.serialize()),
                }
            }
            PickerOutcome::Cancelled => Self::cancelled(),
            PickerOutcome::Failed(_) => Self::error(),
        }
    }

    #[must_use]
    pub fn cancelled() -> Self {
        Self {
            code: 1,
            uris: Vec::new(),
            choices: Vec::new(),
            current_filter: None,
        }
    }

    #[must_use]
    pub fn error() -> Self {
        Self {
            code: 2,
            uris: Vec::new(),
            choices: Vec::new(),
            current_filter: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OptionError {
    message: String,
}

impl OptionError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for OptionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for OptionError {}

impl OpenFileOptions {
    pub fn parse(options: &PortalOptions) -> Result<Self, OptionError> {
        let filters = parse_filters(options, "filters")?.unwrap_or_default();
        let requested_filter = parse_filter(options, "current_filter")?;
        let current_filter = select_current_filter(&filters, requested_filter);
        Ok(Self {
            common: CommonOptions::parse(options)?,
            multiple: option(options, "multiple")?.unwrap_or(false),
            directory: option(options, "directory")?.unwrap_or(false),
            filters,
            current_filter,
        })
    }
}

impl SaveFileOptions {
    pub fn parse(options: &PortalOptions) -> Result<Self, OptionError> {
        let filters = parse_filters(options, "filters")?.unwrap_or_default();
        let requested_filter = parse_filter(options, "current_filter")?;
        let current_filter = select_current_filter(&filters, requested_filter);
        Ok(Self {
            common: CommonOptions::parse(options)?,
            current_name: option(options, "current_name")?,
            current_file: path_option(options, "current_file")?,
            filters,
            current_filter,
        })
    }
}

impl SaveFilesOptions {
    pub fn parse(options: &PortalOptions) -> Result<Self, OptionError> {
        let raw: Vec<Vec<u8>> = required_option(options, "files")?;
        if raw.is_empty() {
            return Err(OptionError::new("files must contain at least one name"));
        }
        let files = raw
            .iter()
            .map(|bytes| decode_nul_name(bytes))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            common: CommonOptions::parse(options)?,
            files,
        })
    }
}

impl CommonOptions {
    fn parse(options: &PortalOptions) -> Result<Self, OptionError> {
        Ok(Self {
            accept_label: option(options, "accept_label")?,
            modal: option(options, "modal")?.unwrap_or(true),
            choices: parse_choices(options)?,
            current_folder: path_option(options, "current_folder")?,
        })
    }
}

impl PortalFilter {
    #[must_use]
    pub fn matches(&self, path: &Path) -> bool {
        if path.is_dir() {
            return true;
        }
        let file_name = path.file_name().unwrap_or_else(|| OsStr::new(""));
        let mime = mime_guess::from_path(path).first_raw();
        self.rules.iter().any(|rule| match rule {
            FilterRule::Glob(pattern) => Glob::new(pattern)
                .ok()
                .is_some_and(|glob| glob.compile_matcher().is_match(file_name)),
            FilterRule::Mime(expected) => mime.is_some_and(|actual| mime_matches(expected, actual)),
        })
    }

    #[must_use]
    pub fn serialize(self) -> SerializedFilter {
        let rules = self
            .rules
            .into_iter()
            .map(|rule| match rule {
                FilterRule::Glob(value) => (0, value),
                FilterRule::Mime(value) => (1, value),
            })
            .collect();
        (self.label, rules)
    }
}

fn mime_matches(expected: &str, actual: &str) -> bool {
    if let Some(prefix) = expected.strip_suffix("/*") {
        actual
            .split_once('/')
            .is_some_and(|(kind, _)| kind.eq_ignore_ascii_case(prefix))
    } else {
        expected.eq_ignore_ascii_case(actual)
    }
}

fn select_current_filter(
    filters: &[PortalFilter],
    requested: Option<PortalFilter>,
) -> Option<PortalFilter> {
    match requested {
        Some(requested) if filters.is_empty() => Some(requested),
        Some(requested) => filters
            .iter()
            .find(|filter| **filter == requested)
            .cloned()
            .or_else(|| filters.first().cloned()),
        None => filters.first().cloned(),
    }
}

fn parse_filter(
    options: &PortalOptions,
    key: &str,
) -> Result<Option<PortalFilter>, OptionError> {
    let raw: Option<SerializedFilter> = option(options, key)?;
    raw.map(filter_from_serialized).transpose()
}

fn parse_filters(
    options: &PortalOptions,
    key: &str,
) -> Result<Option<Vec<PortalFilter>>, OptionError> {
    let raw: Option<Vec<SerializedFilter>> = option(options, key)?;
    raw.map(|filters| {
        filters
            .into_iter()
            .map(filter_from_serialized)
            .collect()
    })
    .transpose()
}

fn filter_from_serialized(raw: SerializedFilter) -> Result<PortalFilter, OptionError> {
    if raw.0.is_empty() || raw.1.is_empty() {
        return Err(OptionError::new("filters require a label and at least one rule"));
    }
    let rules = raw
        .1
        .into_iter()
        .map(|(kind, value)| {
            if value.is_empty() {
                return Err(OptionError::new("filter rule cannot be empty"));
            }
            match kind {
                0 => {
                    Glob::new(&value).map_err(|error| {
                        OptionError::new(format!("invalid glob filter {value:?}: {error}"))
                    })?;
                    Ok(FilterRule::Glob(value))
                }
                1 if valid_mime_filter(&value) => Ok(FilterRule::Mime(value)),
                1 => Err(OptionError::new(format!("invalid MIME filter {value:?}"))),
                _ => Err(OptionError::new(format!("unknown filter rule type {kind}"))),
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PortalFilter {
        label: raw.0,
        rules,
    })
}

fn valid_mime_filter(value: &str) -> bool {
    value
        .split_once('/')
        .is_some_and(|(kind, subtype)| !kind.is_empty() && !subtype.is_empty())
}

fn parse_choices(options: &PortalOptions) -> Result<Vec<PortalChoice>, OptionError> {
    let choices: Vec<SerializedChoice> = option(options, "choices")?.unwrap_or_default();
    let mut ids = HashSet::new();
    choices
        .into_iter()
        .map(|(id, label, values, initial)| {
            if id.is_empty() || label.is_empty() || !ids.insert(id.clone()) {
                return Err(OptionError::new("choice IDs and labels must be non-empty and unique"));
            }
            if values
                .iter()
                .any(|(value_id, value_label)| value_id.is_empty() || value_label.is_empty())
            {
                return Err(OptionError::new("choice option IDs and labels cannot be empty"));
            }
            let mut value_ids = HashSet::new();
            if values
                .iter()
                .any(|(value_id, _)| !value_ids.insert(value_id.clone()))
            {
                return Err(OptionError::new("choice option IDs must be unique"));
            }
            let selected = if values.is_empty() {
                if initial == "true" { "true" } else { "false" }.to_owned()
            } else if values.iter().any(|(value_id, _)| *value_id == initial) {
                initial
            } else {
                values[0].0.clone()
            };
            Ok(PortalChoice {
                id,
                label,
                options: values,
                selected,
            })
        })
        .collect()
}

fn path_option(options: &PortalOptions, key: &str) -> Result<Option<PathBuf>, OptionError> {
    let bytes: Option<Vec<u8>> = option(options, key)?;
    bytes.as_deref().map(decode_nul_path).transpose()
}

fn decode_nul_path(bytes: &[u8]) -> Result<PathBuf, OptionError> {
    let raw = decode_nul_bytes(bytes, "path")?;
    if raw.is_empty() {
        return Err(OptionError::new("path cannot be empty"));
    }
    let path = PathBuf::from(OsString::from_vec(raw.to_vec()));
    if !path.is_absolute() {
        return Err(OptionError::new("filesystem paths must be absolute"));
    }
    Ok(path)
}

fn decode_nul_name(bytes: &[u8]) -> Result<OsString, OptionError> {
    let raw = decode_nul_bytes(bytes, "file name")?;
    let name = OsStr::from_bytes(raw);
    if raw.is_empty() || raw.contains(&b'/') || name == OsStr::new(".") || name == OsStr::new("..") {
        return Err(OptionError::new("file names must be non-empty basenames"));
    }
    Ok(name.to_owned())
}

fn decode_nul_bytes<'a>(bytes: &'a [u8], label: &str) -> Result<&'a [u8], OptionError> {
    let Some((&0, raw)) = bytes.split_last() else {
        return Err(OptionError::new(format!("{label} must be NUL-terminated")));
    };
    if raw.contains(&0) {
        return Err(OptionError::new(format!("{label} contains an embedded NUL")));
    }
    Ok(raw)
}

fn option<T>(options: &PortalOptions, key: &str) -> Result<Option<T>, OptionError>
where
    T: TryFrom<OwnedValue>,
    T::Error: fmt::Display,
{
    options
        .get(key)
        .map(|value| {
            let cloned = value
                .try_clone()
                .map_err(|error| OptionError::new(format!("invalid {key}: {error}")))?;
            T::try_from(cloned).map_err(|error| OptionError::new(format!("invalid {key}: {error}")))
        })
        .transpose()
}

fn required_option<T>(options: &PortalOptions, key: &str) -> Result<T, OptionError>
where
    T: TryFrom<OwnedValue>,
    T::Error: fmt::Display,
{
    option(options, key)?.ok_or_else(|| OptionError::new(format!("missing required option {key}")))
}

#[must_use]
pub fn file_uri(path: &Path) -> Option<String> {
    Url::from_file_path(path).ok().map(|uri| uri.to_string())
}

#[must_use]
pub fn parent_handle(parent_window: &str) -> Option<&str> {
    parent_window
        .strip_prefix("wayland:")
        .filter(|handle| !handle.is_empty() && !handle.bytes().any(|byte| byte.is_ascii_control()))
}

#[must_use]
pub fn valid_save_name(name: &str) -> bool {
    !name.is_empty() && !name.contains('/') && name != "." && name != ".." && !name.contains('\0')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value<T: Into<zbus::zvariant::Value<'static>>>(value: T) -> OwnedValue {
        let value: zbus::zvariant::Value<'static> = value.into();
        OwnedValue::try_from(value).unwrap()
    }

    #[test]
    fn parses_open_options_and_selects_filter() {
        let filters = vec![(
            "Images".to_owned(),
            vec![(0, "*.[pP][nN][gG]".to_owned()), (1, "image/*".to_owned())],
        )];
        let mut options = PortalOptions::new();
        options.insert("multiple".into(), value(true));
        options.insert("filters".into(), value(filters.clone()));
        options.insert("current_filter".into(), value(filters[0].clone()));
        let parsed = OpenFileOptions::parse(&options).unwrap();
        assert!(parsed.multiple);
        assert_eq!(parsed.current_filter, parsed.filters.first().cloned());
    }

    #[test]
    fn choices_cover_combo_and_boolean_values() {
        let mut options = PortalOptions::new();
        options.insert(
            "choices".into(),
            value(vec![
                (
                    "encoding".to_owned(),
                    "Encoding".to_owned(),
                    vec![("utf8".to_owned(), "UTF-8".to_owned())],
                    String::new(),
                ),
                (
                    "copy".to_owned(),
                    "Save as copy".to_owned(),
                    Vec::new(),
                    "true".to_owned(),
                ),
            ]),
        );
        let parsed = CommonOptions::parse(&options).unwrap();
        assert_eq!(parsed.choices[0].selected, "utf8");
        assert_eq!(parsed.choices[1].selected, "true");
        assert!(parsed.choices[1].is_boolean());
    }

    #[test]
    fn decodes_non_utf8_paths_and_rejects_unsafe_names() {
        let mut bytes = b"/tmp/non-utf8-".to_vec();
        bytes.extend([0xff, 0]);
        assert_eq!(decode_nul_path(&bytes).unwrap().as_os_str().as_bytes(), &bytes[..bytes.len() - 1]);
        assert!(decode_nul_name(b"../escape\0").is_err());
        assert!(decode_nul_name(b"not-terminated").is_err());
    }

    #[test]
    fn filter_keeps_directories_and_matches_mime_wildcards() {
        let filter = PortalFilter {
            label: "Images".into(),
            rules: vec![FilterRule::Mime("image/*".into())],
        };
        assert!(filter.matches(Path::new("photo.png")));
        assert!(!filter.matches(Path::new("notes.txt")));
    }

    #[test]
    fn normalizes_file_uris_and_wayland_parents() {
        assert_eq!(file_uri(Path::new("/tmp/a b")), Some("file:///tmp/a%20b".into()));
        assert_eq!(file_uri(Path::new("relative/path")), None);
        assert_eq!(parent_handle("wayland:abc.123"), Some("abc.123"));
        assert_eq!(parent_handle("x11:123"), None);
    }

    #[test]
    fn response_never_returns_a_partial_uri_list() {
        let response = PortalResponse::from_outcome(PickerOutcome::Accepted {
            paths: vec![PathBuf::from("/tmp/valid"), PathBuf::from("relative")],
            choices: Vec::new(),
            current_filter: None,
        });
        assert_eq!(response.code, 2);
        assert!(response.uris.is_empty());
    }
}
