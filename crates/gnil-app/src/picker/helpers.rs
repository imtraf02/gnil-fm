fn request_initial_state(
    request: &PickerRequest,
    home: &Path,
) -> (
    PathBuf,
    Option<SuggestedName>,
    Vec<PortalFilter>,
    Option<PortalFilter>,
    Vec<PortalChoice>,
) {
    match &request.kind {
        PickerRequestKind::Open(options) => (
            options
                .common
                .current_folder
                .clone()
                .unwrap_or_else(|| home.to_path_buf()),
            None,
            picker_filters(&options.filters, options.current_filter.as_ref()),
            options.current_filter.clone(),
            options.common.choices.clone(),
        ),
        PickerRequestKind::Save(options) => {
            let current_file = options.current_file.as_ref();
            let directory = current_file
                .and_then(|path| path.parent().map(Path::to_path_buf))
                .or_else(|| options.common.current_folder.clone())
                .unwrap_or_else(|| home.to_path_buf());
            let name = current_file
                .and_then(|path| {
                    path.file_name().map(|name| SuggestedName {
                        display: name.to_string_lossy().into_owned(),
                        raw: Some(name.to_owned()),
                    })
                })
                .or_else(|| {
                    options
                        .current_name
                        .clone()
                        .map(|display| SuggestedName { display, raw: None })
                })
                .or_else(|| {
                    Some(SuggestedName {
                        display: String::new(),
                        raw: None,
                    })
                });
            (
                directory,
                name,
                picker_filters(&options.filters, options.current_filter.as_ref()),
                options.current_filter.clone(),
                options.common.choices.clone(),
            )
        }
        PickerRequestKind::SaveMany(options) => (
            options
                .common
                .current_folder
                .clone()
                .unwrap_or_else(|| home.to_path_buf()),
            None,
            Vec::new(),
            None,
            options.common.choices.clone(),
        ),
    }
}

struct SuggestedName {
    display: String,
    raw: Option<OsString>,
}

fn picker_filters(
    filters: &[PortalFilter],
    current_filter: Option<&PortalFilter>,
) -> Vec<PortalFilter> {
    if filters.is_empty() {
        current_filter.cloned().into_iter().collect()
    } else {
        filters.to_vec()
    }
}

fn accessible_directory(path: &Path) -> Result<PathBuf, String> {
    match fs::read_dir(path) {
        Ok(_) => Ok(path.to_path_buf()),
        Err(error) => Err(permission_message(path, &error)),
    }
}

fn permission_message(path: &Path, error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::PermissionDenied => format!("Permission denied: {}", path.display()),
        std::io::ErrorKind::NotFound => format!("Location no longer exists: {}", path.display()),
        _ => format!("Cannot open {}: {error}", path.display()),
    }
}

fn load_picker_theme(window: &Window) {
    let paths = ConfigPaths::discover();
    let settings = paths.load_settings().unwrap_or_default();
    let appearance = match settings.theme {
        ThemeMode::Light => ThemeAppearance::Light,
        ThemeMode::Dark => ThemeAppearance::Dark,
        ThemeMode::System => match window.appearance() {
            gpui::WindowAppearance::Light | gpui::WindowAppearance::VibrantLight => {
                ThemeAppearance::Light
            }
            gpui::WindowAppearance::Dark | gpui::WindowAppearance::VibrantDark => {
                ThemeAppearance::Dark
            }
        },
    };
    let name = match appearance {
        ThemeAppearance::Light => &settings.light_theme,
        ThemeAppearance::Dark => &settings.dark_theme,
    };
    let catalog = ThemeCatalog::load(&paths.themes_dir());
    set_active(catalog.resolve(name, appearance).0.colors);
}

fn picker_places() -> Vec<(String, PathBuf)> {
    let mut places = Vec::new();
    if let Some(home) = dirs::home_dir() {
        places.push(("Home".into(), home));
    }
    for (label, path) in [
        ("Desktop", dirs::desktop_dir()),
        ("Documents", dirs::document_dir()),
        ("Downloads", dirs::download_dir()),
        ("Pictures", dirs::picture_dir()),
        ("Music", dirs::audio_dir()),
        ("Videos", dirs::video_dir()),
    ] {
        if let Some(path) = path.filter(|path| path.exists()) {
            places.push((label.into(), path));
        }
    }
    places
}

fn recent_entries() -> Vec<FileEntry> {
    let Some(data_dir) = dirs::data_dir() else {
        return Vec::new();
    };
    let Ok(source) = fs::read(data_dir.join("recently-used.xbel")) else {
        return Vec::new();
    };
    let mut reader = Reader::from_reader(source.as_slice());
    reader.config_mut().trim_text(true);
    let mut paths = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) if element.name().as_ref() == b"bookmark" => {
                if let Some(path) = element
                    .attributes()
                    .filter_map(Result::ok)
                    .find(|attribute| attribute.key.as_ref() == b"href")
                    .and_then(|attribute| attribute.unescape_value().ok())
                    .and_then(|href| Url::parse(&href).ok())
                    .and_then(|uri| uri.to_file_path().ok())
                {
                    paths.push(path);
                    if paths.len() >= 200 {
                        break;
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    let mut seen = HashSet::new();
    paths
        .into_iter()
        .filter(|path| seen.insert(path.clone()))
        .filter_map(|path| entry_from_path(&path))
        .collect()
}

fn entry_from_path(path: &Path) -> Option<FileEntry> {
    let metadata = fs::symlink_metadata(path).ok()?;
    let file_type = metadata.file_type();
    let kind = if file_type.is_symlink() {
        FileKind::Symlink
    } else if file_type.is_dir() {
        FileKind::Directory
    } else if file_type.is_file() {
        FileKind::File
    } else {
        FileKind::Other
    };
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::MetadataExt as _;
        Some(metadata.mode())
    };
    #[cfg(not(unix))]
    let mode = None;
    Some(FileEntry {
        path: path.to_path_buf(),
        name: path.file_name()?.to_string_lossy().into_owned(),
        kind,
        hidden: path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with('.')),
        metadata: EntryMetadata::Ready(FileMetadata {
            len: metadata.len(),
            child_count: (kind == FileKind::Directory)
                .then(|| {
                    fs::read_dir(path)
                        .ok()
                        .and_then(|children| u64::try_from(children.count()).ok())
                })
                .flatten(),
            modified_unix_ms: None,
            mode,
            readonly: metadata.permissions().readonly(),
            symlink_target: file_type
                .is_symlink()
                .then(|| fs::read_link(path).ok())
                .flatten(),
            symlink_target_kind: file_type.is_symlink().then(|| {
                fs::metadata(path).map_or(FileKind::Other, |target| {
                    if target.is_dir() {
                        FileKind::Directory
                    } else if target.is_file() {
                        FileKind::File
                    } else {
                        FileKind::Other
                    }
                })
            }),
            mime: mime_guess::from_path(path).first_raw().map(str::to_owned),
        }),
        git_status: None,
    })
}

fn entry_is_directory(entry: &FileEntry) -> bool {
    entry.is_directory_like()
}

fn section_label(label: impl Into<SharedString>) -> gpui::Div {
    div()
        .h_7()
        .px_2()
        .flex()
        .items_center()
        .text_xs()
        .text_color(rgb(text_muted()))
        .child(label.into())
}

fn sidebar_item(label: impl Into<SharedString>, active: bool) -> gpui::Div {
    div()
        .h_8()
        .px_2()
        .rounded_md()
        .flex()
        .items_center()
        .text_sm()
        .cursor_pointer()
        .when(active, |row| row.bg(rgb(accent_background())))
        .when(!active, |row| {
            row.hover(|style| style.bg(rgb(surface_elevated())))
        })
        .child(label.into())
}

fn sidebar_item_with_icon(
    label: impl Into<SharedString>,
    icon: &'static str,
    active: bool,
) -> gpui::Div {
    sidebar_item("", active)
        .gap_2()
        .child(ui_icon(
            icon,
            IconSize::Compact,
            if active {
                IconTone::Accent
            } else {
                IconTone::Muted
            },
        ))
        .child(label.into())
}

fn nav_button(icon: &'static str, enabled: bool) -> gpui::Div {
    div()
        .h_8()
        .w_8()
        .rounded_md()
        .flex()
        .items_center()
        .justify_center()
        .text_sm()
        .text_color(rgb(if enabled { text() } else { border_focused() }))
        .when(enabled, |button| {
            button
                .cursor_pointer()
                .hover(|style| style.bg(rgb(surface_elevated())))
        })
        .child(ui_icon(
            icon,
            IconSize::Small,
            if enabled {
                IconTone::Default
            } else {
                IconTone::Muted
            },
        ))
}

fn secondary_button(label: impl Into<SharedString>) -> gpui::Div {
    div()
        .h_9()
        .px_3()
        .rounded_md()
        .border_1()
        .border_color(rgb(border_focused()))
        .bg(rgb(surface_elevated()))
        .flex()
        .items_center()
        .text_xs()
        .cursor_pointer()
        .hover(|style| style.bg(rgb(border())))
        .child(label.into())
}

fn primary_button(label: impl Into<SharedString>, enabled: bool) -> gpui::Div {
    div()
        .h_9()
        .px_4()
        .rounded_md()
        .bg(rgb(if enabled { accent() } else { border() }))
        .text_color(rgb(if enabled { background() } else { text_muted() }))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .flex()
        .items_center()
        .text_xs()
        .when(enabled, |button| {
            button.cursor_pointer().hover(|style| style.opacity(0.9))
        })
        .child(label.into())
}

fn file_icon_asset(entry: &FileEntry) -> &'static str {
    match entry.kind {
        FileKind::Directory if entry.metadata().is_some_and(|metadata| metadata.readonly) => {
            "icons/folder-readonly.svg"
        }
        FileKind::Directory => match entry.name.to_ascii_lowercase().as_str() {
            "downloads" => "icons/folder-downloads.svg",
            "pictures" | "images" | "photos" => "icons/folder-pictures.svg",
            "documents" | "docs" => "icons/folder-documents.svg",
            "videos" | "movies" => "icons/folder-videos.svg",
            "music" | "audio" => "icons/folder-music.svg",
            "desktop" => "icons/folder-desktop.svg",
            _ => "icons/folder-closed.svg",
        },
        FileKind::Symlink if entry.is_directory_like() => "icons/folder-symlink.svg",
        FileKind::File | FileKind::Symlink => match entry
            .path
            .extension()
            .and_then(OsStr::to_str)
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str()
        {
            "rs" | "js" | "jsx" | "ts" | "tsx" | "py" | "go" | "c" | "cpp" | "h" | "java"
            | "kt" | "lua" | "sh" | "nix" | "toml" | "json" | "yaml" => "icons/file-code.svg",
            "txt" | "md" | "rst" | "log" | "csv" => "icons/file-text.svg",
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "avif" | "svg" | "bmp" => {
                "icons/file-image.svg"
            }
            "pdf" | "doc" | "docx" | "odt" | "rtf" | "epub" => "icons/file-document.svg",
            "zip" | "tar" | "gz" | "bz2" | "xz" | "zst" | "7z" | "rar" => "icons/file-archive.svg",
            "mp3" | "flac" | "wav" | "ogg" | "mp4" | "mkv" | "webm" | "mov" => {
                "icons/file-media.svg"
            }
            _ => "icons/file-generic.svg",
        },
        FileKind::Other => "icons/file-generic.svg",
    }
}

#[allow(clippy::cast_precision_loss)]
fn size_label(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
