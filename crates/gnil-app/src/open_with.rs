use std::{
    collections::{HashMap, HashSet},
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use freedesktop_desktop_entry::{
    DesktopEntry, Iter, current_desktop, default_paths, get_languages_from_env,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DesktopApplication {
    pub(crate) desktop_id: String,
    pub(crate) name: String,
    pub(crate) generic_name: Option<String>,
    pub(crate) desktop_file: PathBuf,
    pub(crate) is_default: bool,
    pub(crate) compatible: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OpenWithCatalog {
    pub(crate) mime_type: String,
    pub(crate) suggested: Vec<DesktopApplication>,
    pub(crate) all: Vec<DesktopApplication>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct MimeAssociations {
    defaults: HashMap<String, Vec<String>>,
    added: HashMap<String, Vec<String>>,
    removed: HashMap<String, Vec<String>>,
}

pub(crate) fn discover_applications(path: &Path, metadata_mime: Option<&str>) -> OpenWithCatalog {
    let mime_type = mime_type_for_path(path, metadata_mime);
    let locales = get_languages_from_env();
    let desktops = current_desktop().unwrap_or_default();
    let association_files = mimeapps_paths(&desktops);
    let associations = association_files
        .iter()
        .filter_map(|path| fs::read_to_string(path).ok())
        .map(|contents| parse_mimeapps(&contents))
        .collect::<Vec<_>>();
    let entries = Iter::new(default_paths()).entries(Some(&locales));

    build_catalog(&mime_type, entries, &associations, &desktops, &locales)
}

pub(crate) fn launch_application(app: &DesktopApplication, path: &Path) -> Result<(), String> {
    let locales = get_languages_from_env();
    let entry = DesktopEntry::from_path(&app.desktop_file, Some(&locales))
        .map_err(|error| format!("Could not read {}: {error}", app.desktop_id))?;
    let (program, arguments) = launch_command(&entry, path, &locales)?;
    let mut command = Command::new(program);
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(directory) = entry.path().filter(|path| !path.is_empty()) {
        command.current_dir(directory);
    }
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Could not open with {}: {error}", app.name))
}

pub(crate) fn set_default_application(
    app: &DesktopApplication,
    mime_type: &str,
) -> Result<(), String> {
    let output = Command::new("xdg-mime")
        .args(["default", app.desktop_id.as_str(), mime_type])
        .output()
        .map_err(|error| format!("Could not run xdg-mime: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Err(if detail.is_empty() {
        format!("xdg-mime exited with {}", output.status)
    } else {
        format!("Could not set the default app: {detail}")
    })
}

pub(crate) fn filter_applications<'a>(
    applications: &'a [DesktopApplication],
    query: &str,
) -> Vec<&'a DesktopApplication> {
    let query = query.trim().to_lowercase();
    applications
        .iter()
        .filter(|application| {
            query.is_empty()
                || application.name.to_lowercase().contains(&query)
                || application
                    .generic_name
                    .as_ref()
                    .is_some_and(|name| name.to_lowercase().contains(&query))
        })
        .collect()
}

pub(crate) fn mime_type_for_path(path: &Path, metadata_mime: Option<&str>) -> String {
    metadata_mime
        .filter(|mime| !mime.trim().is_empty())
        .map(str::to_owned)
        .or_else(|| mime_guess::from_path(path).first_raw().map(str::to_owned))
        .unwrap_or_else(|| "application/octet-stream".to_owned())
}

fn build_catalog(
    mime_type: &str,
    entries: impl IntoIterator<Item = DesktopEntry>,
    association_files: &[MimeAssociations],
    desktops: &[String],
    locales: &[String],
) -> OpenWithCatalog {
    let (defaults, associated, removed) = resolve_associations(mime_type, association_files);
    let default_id = defaults.first();
    let associated_ids = defaults
        .iter()
        .chain(&associated)
        .filter(|id| !removed.contains(*id))
        .cloned()
        .collect::<Vec<_>>();
    let associated_set = associated_ids.iter().collect::<HashSet<_>>();
    let mut seen = HashSet::new();
    let mut applications = Vec::new();

    for entry in entries {
        let desktop_id = desktop_file_id(entry.id());
        if !seen.insert(desktop_id.clone()) || entry.hidden() {
            continue;
        }
        let compatible = !removed.contains(&desktop_id)
            && (associated_set.contains(&desktop_id)
                || entry
                    .mime_type()
                    .is_some_and(|types| types.contains(&mime_type)));
        if !entry_is_launchable(&entry, desktops) || (entry.no_display() && !compatible) {
            continue;
        }
        let Some(name) = entry.name(locales).filter(|name| !name.trim().is_empty()) else {
            continue;
        };
        applications.push((
            entry.no_display(),
            DesktopApplication {
                is_default: default_id == Some(&desktop_id),
                compatible,
                desktop_id,
                name: name.into_owned(),
                generic_name: entry
                    .generic_name(locales)
                    .filter(|name| !name.trim().is_empty())
                    .map(std::borrow::Cow::into_owned),
                desktop_file: entry.path,
            },
        ));
    }

    let by_id = applications
        .iter()
        .map(|(_, app)| (app.desktop_id.as_str(), app))
        .collect::<HashMap<_, _>>();
    let mut suggested = associated_ids
        .iter()
        .filter_map(|id| by_id.get(id.as_str()).map(|app| (*app).clone()))
        .collect::<Vec<_>>();
    let mut remaining = applications
        .iter()
        .filter(|(_, app)| app.compatible)
        .map(|(_, app)| app.clone())
        .filter(|app| {
            !suggested
                .iter()
                .any(|candidate| candidate.desktop_id == app.desktop_id)
        })
        .collect::<Vec<_>>();
    remaining.sort_by(app_name_order);
    suggested.extend(remaining);

    let mut all = applications
        .into_iter()
        .filter_map(|(no_display, app)| (!no_display).then_some(app))
        .collect::<Vec<_>>();
    all.sort_by(app_name_order);

    OpenWithCatalog {
        mime_type: mime_type.to_owned(),
        suggested,
        all,
    }
}

fn desktop_file_id(app_id: &str) -> String {
    if app_id.ends_with(".desktop") {
        app_id.to_owned()
    } else {
        format!("{app_id}.desktop")
    }
}

fn app_name_order(left: &DesktopApplication, right: &DesktopApplication) -> std::cmp::Ordering {
    left.name
        .to_lowercase()
        .cmp(&right.name.to_lowercase())
        .then_with(|| left.desktop_id.cmp(&right.desktop_id))
}

fn entry_is_launchable(entry: &DesktopEntry, desktops: &[String]) -> bool {
    if entry.type_() != Some("Application") || entry.terminal() || entry.exec().is_none() {
        return false;
    }
    if entry.only_show_in().is_some_and(|only| {
        desktops.is_empty()
            || !only.iter().any(|candidate| {
                desktops
                    .iter()
                    .any(|desktop| candidate.eq_ignore_ascii_case(desktop))
            })
    }) {
        return false;
    }
    if entry.not_show_in().is_some_and(|excluded| {
        excluded.iter().any(|candidate| {
            desktops
                .iter()
                .any(|desktop| candidate.eq_ignore_ascii_case(desktop))
        })
    }) {
        return false;
    }
    entry.try_exec().is_none_or(executable_exists)
}

fn executable_exists(executable: &str) -> bool {
    let executable = Path::new(executable);
    if executable.components().count() > 1 {
        return executable.is_file();
    }
    env::var_os("PATH").is_some_and(|path| {
        env::split_paths(&path).any(|directory| directory.join(executable).is_file())
    })
}

fn launch_command(
    entry: &DesktopEntry,
    path: &Path,
    locales: &[String],
) -> Result<(String, Vec<String>), String> {
    let uri = url::Url::from_file_path(path)
        .map_err(|()| format!("Could not create a file URI for {}", path.display()))?;
    let local_path = path.to_string_lossy();
    let resource = if entry
        .exec()
        .is_some_and(|exec| exec.contains("%f") || exec.contains("%F"))
    {
        local_path.as_ref()
    } else {
        uri.as_str()
    };
    let arguments = entry
        .parse_exec_with_uris(&[resource], locales)
        .map_err(|error| format!("Invalid Exec entry for {}: {error}", entry.id()))?;
    let mut arguments = arguments.into_iter();
    let program = arguments
        .next()
        .ok_or_else(|| format!("The Exec entry for {} is empty", entry.id()))?;
    Ok((program, arguments.collect()))
}

fn mimeapps_paths(desktops: &[String]) -> Vec<PathBuf> {
    let config_home = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".config")));
    let config_dirs = env::var_os("XDG_CONFIG_DIRS").map_or_else(
        || vec![PathBuf::from("/etc/xdg")],
        |dirs| env::split_paths(&dirs).collect::<Vec<_>>(),
    );
    let data_home = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".local/share")));
    let data_dirs = env::var_os("XDG_DATA_DIRS").map_or_else(
        || {
            vec![
                PathBuf::from("/usr/local/share"),
                PathBuf::from("/usr/share"),
            ]
        },
        |dirs| env::split_paths(&dirs).collect::<Vec<_>>(),
    );
    let mut paths = Vec::new();
    if let Some(config_home) = config_home {
        add_mimeapps_paths(&mut paths, &config_home, desktops);
    }
    for directory in config_dirs {
        add_mimeapps_paths(&mut paths, &directory, desktops);
    }
    if let Some(data_home) = data_home {
        add_mimeapps_paths(&mut paths, &data_home.join("applications"), desktops);
    }
    for directory in data_dirs {
        add_mimeapps_paths(&mut paths, &directory.join("applications"), desktops);
    }
    paths
}

fn add_mimeapps_paths(paths: &mut Vec<PathBuf>, root: &Path, desktops: &[String]) {
    paths.extend(
        desktops
            .iter()
            .map(|desktop| root.join(format!("{desktop}-mimeapps.list"))),
    );
    paths.push(root.join("mimeapps.list"));
}

fn parse_mimeapps(contents: &str) -> MimeAssociations {
    #[derive(Clone, Copy)]
    enum Section {
        Default,
        Added,
        Removed,
        Other,
    }
    let mut associations = MimeAssociations::default();
    let mut section = Section::Other;
    for line in contents.lines().map(str::trim) {
        section = match line {
            "[Default Applications]" => Section::Default,
            "[Added Associations]" => Section::Added,
            "[Removed Associations]" => Section::Removed,
            line if line.starts_with('[') => Section::Other,
            _ => {
                if !line.is_empty()
                    && !line.starts_with('#')
                    && let Some((mime_type, applications)) = line.split_once('=')
                {
                    let applications = applications
                        .split(';')
                        .map(str::trim)
                        .filter(|application| !application.is_empty())
                        .map(str::to_owned)
                        .collect::<Vec<_>>();
                    let target = match section {
                        Section::Default => &mut associations.defaults,
                        Section::Added => &mut associations.added,
                        Section::Removed => &mut associations.removed,
                        Section::Other => continue,
                    };
                    target.insert(mime_type.trim().to_owned(), applications);
                }
                section
            }
        };
    }
    associations
}

fn resolve_associations(
    mime_type: &str,
    files: &[MimeAssociations],
) -> (Vec<String>, Vec<String>, HashSet<String>) {
    let mut defaults = Vec::new();
    let mut added = Vec::new();
    let mut decisions = HashMap::new();
    for file in files {
        if let Some(applications) = file.removed.get(mime_type) {
            for application in applications {
                decisions.entry(application.clone()).or_insert(false);
            }
        }
        if let Some(applications) = file.defaults.get(mime_type) {
            extend_associations(&mut defaults, applications, &mut decisions);
        }
        if let Some(applications) = file.added.get(mime_type) {
            extend_associations(&mut added, applications, &mut decisions);
        }
    }
    let removed = decisions
        .into_iter()
        .filter_map(|(application, associated)| (!associated).then_some(application))
        .collect();
    (defaults, added, removed)
}

fn extend_associations(
    target: &mut Vec<String>,
    applications: &[String],
    decisions: &mut HashMap<String, bool>,
) {
    for application in applications {
        let associated = *decisions.entry(application.clone()).or_insert(true);
        if associated && !target.contains(application) {
            target.push(application.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn desktop_entry(
        root: &Path,
        id: &str,
        name: &str,
        mime_types: &str,
        extra: &str,
    ) -> DesktopEntry {
        let path = root.join(format!("{id}.desktop"));
        let contents = format!(
            "[Desktop Entry]\nType=Application\nName={name}\nExec=/bin/echo %u\nMimeType={mime_types}\n{extra}"
        );
        fs::write(&path, contents).expect("write desktop fixture");
        DesktopEntry::from_path(path, Some(&["en"])).expect("parse desktop fixture")
    }

    #[test]
    fn parses_and_resolves_mimeapps_in_precedence_order() {
        let high = parse_mimeapps(
            "[Default Applications]\ntext/plain=zed.desktop;\n\
             [Added Associations]\ntext/plain=zed.desktop;code.desktop;\n",
        );
        let low = parse_mimeapps(
            "[Default Applications]\ntext/plain=legacy.desktop;\n\
             [Removed Associations]\ntext/plain=blocked.desktop;\n",
        );

        let (defaults, added, removed) = resolve_associations("text/plain", &[high, low]);

        assert_eq!(defaults, ["zed.desktop", "legacy.desktop"]);
        assert_eq!(added, ["zed.desktop", "code.desktop"]);
        assert!(removed.contains("blocked.desktop"));
    }

    #[test]
    fn lower_priority_removal_does_not_override_a_higher_association() {
        let high = parse_mimeapps("[Added Associations]\ntext/plain=preferred.desktop;\n");
        let low = parse_mimeapps(
            "[Removed Associations]\ntext/plain=preferred.desktop;blocked.desktop;\n",
        );

        let (_, added, removed) = resolve_associations("text/plain", &[high, low]);

        assert_eq!(added, ["preferred.desktop"]);
        assert!(!removed.contains("preferred.desktop"));
        assert!(removed.contains("blocked.desktop"));
    }

    #[test]
    fn catalog_prioritizes_default_and_keeps_hidden_compatible_app() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let root = temporary.path();
        let entries = vec![
            desktop_entry(root, "viewer", "Viewer", "image/png;", ""),
            desktop_entry(
                root,
                "default",
                "Default App",
                "image/png;",
                "NoDisplay=true\n",
            ),
            desktop_entry(root, "notes", "Notes", "text/plain;", ""),
        ];
        let associations = parse_mimeapps(
            "[Default Applications]\nimage/png=default.desktop;\n\
             [Added Associations]\nimage/png=viewer.desktop;\n",
        );

        let catalog = build_catalog(
            "image/png",
            entries,
            &[associations],
            &[],
            &["en".to_owned()],
        );

        assert_eq!(catalog.suggested[0].desktop_id, "default.desktop");
        assert!(catalog.suggested[0].is_default);
        assert_eq!(catalog.suggested[1].desktop_id, "viewer.desktop");
        assert!(
            !catalog
                .all
                .iter()
                .any(|application| application.desktop_id == "default.desktop")
        );
        assert!(
            catalog
                .all
                .iter()
                .any(|application| application.desktop_id == "notes.desktop")
        );
    }

    #[test]
    fn launch_arguments_do_not_invoke_a_shell() {
        let temporary = tempfile::tempdir().expect("temporary directory");
        let entry = desktop_entry(temporary.path(), "safe", "Safe", "text/plain;", "");
        let path = Path::new("/tmp/report;touch-injected.txt");

        let (program, arguments) =
            launch_command(&entry, path, &["en".to_owned()]).expect("launch command");

        assert_eq!(program, "/bin/echo");
        assert_eq!(arguments.len(), 1);
        assert!(arguments[0].starts_with("file:///tmp/report;touch-injected.txt"));
    }

    #[test]
    fn search_matches_name_and_generic_name_case_insensitively() {
        let applications = vec![DesktopApplication {
            desktop_id: "org.example.Editor.desktop".to_owned(),
            name: "Paper Plane".to_owned(),
            generic_name: Some("Text Editor".to_owned()),
            desktop_file: PathBuf::from("/tmp/editor.desktop"),
            is_default: false,
            compatible: true,
        }];

        assert_eq!(filter_applications(&applications, "paper").len(), 1);
        assert_eq!(filter_applications(&applications, "EDITOR").len(), 1);
        assert!(filter_applications(&applications, "image").is_empty());
    }
}
