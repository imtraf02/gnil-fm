fn field_label(label: &'static str) -> AnyElement {
    div()
        .text_xs()
        .text_color(rgb(text_muted()))
        .child(label)
        .into_any_element()
}

fn helper_text(text: &'static str) -> AnyElement {
    div()
        .text_xs()
        .line_height(px(18.0))
        .text_color(rgb(text_muted()))
        .child(text)
        .into_any_element()
}

fn property_row(label: &'static str, value: impl Into<SharedString>) -> AnyElement {
    div()
        .min_h(px(34.0))
        .py_2()
        .border_b_1()
        .border_color(rgb(border()))
        .flex()
        .items_start()
        .justify_between()
        .gap_4()
        .child(div().text_xs().text_color(rgb(text_muted())).child(label))
        .child(
            div()
                .min_w_0()
                .text_xs()
                .text_color(rgb(theme_text()))
                .child(value.into()),
        )
        .into_any_element()
}

fn sidebar_section_label(label: &'static str) -> AnyElement {
    div()
        .px_3()
        .pt_3()
        .pb_1()
        .text_xs()
        .text_color(rgb(text_muted()))
        .child(label)
        .into_any_element()
}

fn trash_action_button(
    label: &'static str,
    danger: bool,
    enabled: bool,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(label)
        .h_8()
        .px_3()
        .rounded_md()
        .flex()
        .items_center()
        .text_xs()
        .text_color(if !enabled {
            rgb(border_focused())
        } else if danger {
            rgb(danger_color())
        } else {
            rgb(theme_text())
        })
        .bg(rgb(surface_elevated()))
        .when(enabled, |button| {
            button
                .cursor_pointer()
                .hover(|style| style.bg(rgb(border())))
        })
        .when(enabled, FocusableControl::with_focus_ring)
        .child(label)
}

fn trash_entry_as_file_entry(entry: &TrashEntry) -> FileEntry {
    FileEntry {
        path: entry.reference.info_path.clone(),
        name: entry.name.clone(),
        kind: entry.kind,
        hidden: false,
        metadata: EntryMetadata::Ready(FileMetadata {
            len: entry.len,
            ..FileMetadata::default()
        }),
        git_status: None,
    }
}

fn deletion_label(timestamp: i64) -> String {
    DateTime::from_timestamp(timestamp, 0).map_or_else(
        || "Unknown".into(),
        |date| {
            date.with_timezone(&Local)
                .format("%b %e, %Y %H:%M")
                .to_string()
        },
    )
}

const fn device_icon(kind: DeviceKind) -> &'static str {
    match kind {
        DeviceKind::Usb => "icons/device-usb.svg",
        DeviceKind::SolidState | DeviceKind::HardDisk | DeviceKind::Other => {
            "icons/device-drive.svg"
        }
    }
}

fn device_usage(device: &DeviceEntry) -> f32 {
    if device.size == 0 {
        return 0.0;
    }
    device.available.map_or(0.0, |available| {
        let used = device.size.saturating_sub(available);
        let basis_points = used.saturating_mul(10_000) / device.size;
        f32::from(u16::try_from(basis_points.min(10_000)).unwrap_or(10_000)) / 10_000.0
    })
}

fn device_capacity_label(device: &DeviceEntry) -> String {
    match device.available {
        Some(available) => format!(
            "{} free of {}",
            format_bytes(available),
            format_bytes(device.size)
        ),
        None if device.mount_path.is_none() => {
            format!("Not mounted · {}", format_bytes(device.size))
        }
        None => format_bytes(device.size),
    }
}

fn empty_space_menu_row(index: usize, enabled: bool, focused: bool) -> gpui::Stateful<gpui::Div> {
    div()
        .id(("empty-space-menu-entry", index))
        .h_8()
        .w_full()
        .px_2()
        .rounded_md()
        .flex()
        .items_center()
        .justify_between()
        .text_color(if enabled {
            rgb(theme_text())
        } else {
            rgb(border_focused())
        })
        .when(focused, |row| row.bg(rgb(border())))
        .when(enabled, |row| {
            row.cursor_pointer().hover(|style| style.bg(rgb(border())))
        })
        .when(enabled, FocusableControl::with_focus_ring)
}

fn sort_label(label: &'static str, field: SortField, sort: gnil_core::SortSpec) -> String {
    if sort.field != field {
        return label.into();
    }
    format!(
        "{label} {}",
        match sort.direction {
            SortDirection::Ascending => "↑",
            SortDirection::Descending => "↓",
        }
    )
}

fn terminal_candidates() -> Vec<(String, Vec<String>)> {
    let mut candidates = Vec::new();
    if let Ok(terminal) = env::var("TERMINAL") {
        let mut parts = terminal.split_whitespace();
        if let Some(program) = parts.next() {
            candidates.push((program.to_owned(), parts.map(str::to_owned).collect()));
        }
    }
    candidates.extend(
        [
            "xdg-terminal-exec",
            "foot",
            "kitty",
            "wezterm",
            "alacritty",
            "gnome-terminal",
            "konsole",
        ]
        .into_iter()
        .map(|program| (program.to_owned(), Vec::new())),
    );
    candidates
}

fn sheet_button(label: &'static str, primary: bool) -> gpui::Stateful<gpui::Div> {
    div()
        .id(("sheet-button", usize::from(primary)))
        .h_8()
        .px_4()
        .rounded_md()
        .flex()
        .items_center()
        .cursor_pointer()
        .text_xs()
        .bg(if primary {
            rgb(accent_background())
        } else {
            rgb(surface_elevated())
        })
        .text_color(if primary {
            rgb(text_emphasized())
        } else {
            rgb(theme_text())
        })
        .hover(|style| {
            style.bg(if primary {
                rgb(accent_hover())
            } else {
                rgb(border())
            })
        })
        .with_focus_ring()
        .child(label)
}

fn segmented_scope(scope: RenameScope, cx: &mut Context<FileManager>) -> AnyElement {
    let mut row = div()
        .h_8()
        .flex()
        .rounded_md()
        .bg(rgb(surface_elevated()))
        .p_1();
    for (label, candidate) in [
        ("Name", RenameScope::Stem),
        ("Extension", RenameScope::Extension),
        ("Full name", RenameScope::FullName),
    ] {
        row = row.child(
            div()
                .id(("rename-scope", candidate as usize))
                .with_focus_ring()
                .flex_1()
                .h_6()
                .rounded_md()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .text_xs()
                .when(scope == candidate, |style| {
                    style.bg(rgb(accent_background()))
                })
                .hover(|style| style.bg(rgb(border())))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(OperationSheet::BulkRename { scope, .. }) =
                        this.operation_sheet.as_mut()
                    {
                        *scope = candidate;
                        cx.notify();
                    }
                }))
                .child(label),
        );
    }
    row.into_any_element()
}

fn toggle_chip(
    id: &'static str,
    label: &'static str,
    enabled: bool,
    cx: &mut Context<FileManager>,
    toggle: impl Fn(&mut OperationSheet) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .with_focus_ring()
        .h_8()
        .px_3()
        .rounded_md()
        .flex()
        .items_center()
        .gap_2()
        .cursor_pointer()
        .text_xs()
        .bg(if enabled {
            rgb(accent_background())
        } else {
            rgb(surface_elevated())
        })
        .hover(|style| style.bg(rgb(border())))
        .on_click(cx.listener(move |this, _, _, cx| {
            if let Some(sheet) = this.operation_sheet.as_mut() {
                toggle(sheet);
                cx.notify();
            }
        }))
        .child(if enabled { "●" } else { "○" })
        .child(label)
        .into_any_element()
}

fn file_icon(entry: &FileEntry) -> AnyElement {
    div()
        .w(px(26.0))
        .flex()
        .items_center()
        .child(ui_icon(
            file_icon_asset(entry),
            IconSize::Compact,
            IconTone::Default,
        ))
        .into_any_element()
}

fn file_icon_asset(entry: &FileEntry) -> &'static str {
    match entry.kind {
        FileKind::Directory if entry.metadata().is_some_and(|metadata| metadata.readonly) => {
            "icons/folder-readonly.svg"
        }
        FileKind::Directory => "icons/folder-closed.svg",
        FileKind::Symlink if entry.is_directory_like() => "icons/folder-symlink.svg",
        FileKind::File | FileKind::Symlink => match entry
            .extension()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str()
        {
            "rs" | "js" | "jsx" | "ts" | "tsx" | "py" | "go" | "c" | "cc" | "cpp" | "h" | "hpp"
            | "java" | "kt" | "lua" | "sh" | "fish" | "nix" | "toml" | "json" | "yaml" | "yml" => {
                "icons/file-code.svg"
            }
            "txt" | "md" | "markdown" | "rst" | "log" | "csv" => "icons/file-text.svg",
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "avif" | "svg" | "bmp" | "tif" | "tiff"
            | "ico" => "icons/file-image.svg",
            "pdf" | "doc" | "docx" | "odt" | "rtf" | "epub" => "icons/file-document.svg",
            "zip" | "tar" | "gz" | "bz2" | "xz" | "zst" | "7z" | "rar" => "icons/file-archive.svg",
            "mp3" | "flac" | "wav" | "ogg" | "m4a" | "mp4" | "mkv" | "webm" | "mov" | "avi" => {
                "icons/file-media.svg"
            }
            _ => "icons/file-generic.svg",
        },
        FileKind::Other => "icons/file-generic.svg",
    }
}

fn size_label(entry: &FileEntry) -> String {
    if entry.kind == FileKind::Directory {
        entry
            .metadata()
            .and_then(|metadata| metadata.child_count)
            .map_or_else(|| "—".into(), item_count_label)
    } else {
        entry
            .metadata()
            .map_or_else(|| "—".into(), |metadata| format_bytes(metadata.len))
    }
}

fn item_count_label(count: u64) -> String {
    if count == 1 {
        "1 item".to_owned()
    } else {
        format!("{count} items")
    }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut divisor = 1_u64;
    let mut unit = 0;
    while bytes / divisor >= 1024 && unit < UNITS.len() - 1 {
        divisor = divisor.saturating_mul(1024);
        unit += 1;
    }
    let mut whole = bytes / divisor;
    let mut decimal = (bytes % divisor)
        .saturating_mul(10)
        .saturating_add(divisor / 2)
        / divisor;
    if decimal == 10 {
        whole = whole.saturating_add(1);
        decimal = 0;
    }
    if decimal == 0 {
        format!("{whole} {}", UNITS[unit])
    } else {
        format!("{whole}.{decimal} {}", UNITS[unit])
    }
}

fn modified_label(entry: &FileEntry) -> String {
    entry
        .metadata()
        .and_then(|metadata| metadata.modified_unix_ms)
        .and_then(DateTime::from_timestamp_millis)
        .map_or_else(
            || "—".into(),
            |date| {
                date.with_timezone(&Local)
                    .format("%b %e, %H:%M")
                    .to_string()
            },
        )
}

fn window_theme_appearance(window: &Window) -> ThemeAppearance {
    match window.appearance() {
        WindowAppearance::Light | WindowAppearance::VibrantLight => ThemeAppearance::Light,
        WindowAppearance::Dark | WindowAppearance::VibrantDark => ThemeAppearance::Dark,
    }
}

fn resolve_theme_appearance(
    mode: ThemeMode,
    system_appearance: ThemeAppearance,
) -> ThemeAppearance {
    match mode {
        ThemeMode::Light => ThemeAppearance::Light,
        ThemeMode::Dark => ThemeAppearance::Dark,
        ThemeMode::System => system_appearance,
    }
}
