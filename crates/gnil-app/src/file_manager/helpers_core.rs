fn stable_path_id(path: &Path) -> u64 {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    hasher.finish()
}

fn sanitized_external_paths(paths: &ExternalPaths) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    paths
        .paths()
        .iter()
        .filter(|path| std::fs::symlink_metadata(path).is_ok())
        .filter(|path| seen.insert((*path).clone()))
        .cloned()
        .collect()
}

fn can_internal_drop_value(
    payload: &FileDragPayload,
    target: &DropTarget,
    operation_running: bool,
) -> bool {
    if !payload.visual.lifted()
        || internal_drop_intent(payload, target, payload.visual.copy(), operation_running).is_err()
    {
        return false;
    }
    match target {
        DropTarget::Directory(destination) => {
            std::fs::canonicalize(destination).is_ok_and(|canonical_destination| {
                !payload.paths.iter().any(|path| {
                    path.parent()
                        .and_then(|parent| std::fs::canonicalize(parent).ok())
                        .is_some_and(|parent| parent == canonical_destination)
                }) && gnil_fs::validate_transfer_destination(&payload.paths, destination).is_ok()
            })
        }
        DropTarget::Trash => true,
    }
}

fn can_external_drop_value(
    external: &ExternalPaths,
    target: &DropTarget,
    operation_running: bool,
) -> bool {
    let paths = sanitized_external_paths(external);
    if external_drop_intent(&paths, target, operation_running).is_err() {
        return false;
    }
    match target {
        DropTarget::Directory(destination) => {
            gnil_fs::validate_transfer_destination(&paths, destination).is_ok()
        }
        DropTarget::Trash => false,
    }
}

fn can_drop_value(value: &dyn std::any::Any, target: &DropTarget, operation_running: bool) -> bool {
    value
        .downcast_ref::<FileDragPayload>()
        .is_some_and(|payload| can_internal_drop_value(payload, target, operation_running))
        || value
            .downcast_ref::<ExternalPaths>()
            .is_some_and(|paths| can_external_drop_value(paths, target, operation_running))
}

fn preview_content(path: &Path, preview: &PreviewResult) -> AnyElement {
    let title = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let body = match preview {
        PreviewResult::Directory(directory) => div()
            .p_4()
            .text_sm()
            .text_color(rgb(theme_text()))
            .child(format!("{} items", directory.child_count))
            .into_any_element(),
        PreviewResult::Image(image) if image.decode_allowed => div()
            .p_4()
            .gap_3()
            .flex()
            .flex_col()
            .items_center()
            .child(
                img(Arc::<Path>::from(path))
                    .max_w_full()
                    .max_h(px(270.0))
                    .rounded_lg(),
            )
            .child(div().text_xs().text_color(rgb(text_muted())).child(format!(
                "{} × {} · {}",
                image.width,
                image.height,
                image.format.to_uppercase()
            )))
            .into_any_element(),
        PreviewResult::Image(image) => div()
            .p_4()
            .text_sm()
            .text_color(rgb(text_muted()))
            .child(format!(
                "{} × {} · too large to decode safely",
                image.width, image.height
            ))
            .into_any_element(),
        PreviewResult::Text(text) => {
            let plain = text
                .lines
                .iter()
                .take(80)
                .flat_map(|line| line.segments.iter().map(|segment| segment.text.as_str()))
                .collect::<String>();
            div()
                .id("text-preview-scroll")
                .p_4()
                .overflow_y_scroll()
                .text_xs()
                .font_family("Noto Sans Mono")
                .line_height(px(18.0))
                .text_color(rgb(theme_text()))
                .child(plain)
                .when(text.truncated, |style| {
                    style.child(
                        div()
                            .mt_4()
                            .text_color(rgb(accent()))
                            .child("Preview truncated at 2 MiB"),
                    )
                })
                .into_any_element()
        }
        PreviewResult::Metadata(metadata) => div()
            .p_4()
            .flex()
            .flex_col()
            .gap_2()
            .text_sm()
            .text_color(rgb(theme_text()))
            .child(metadata.mime.clone())
            .child(format_bytes(metadata.len))
            .when(metadata.readonly, |style| style.child("Read only"))
            .into_any_element(),
    };
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .child(
            div()
                .px_4()
                .pb_3()
                .text_sm()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(text_emphasized()))
                .child(title),
        )
        .child(body)
        .into_any_element()
}

fn empty_preview() -> AnyElement {
    div()
        .flex_1()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_3()
        .px_6()
        .text_center()
        .child(div().text_3xl().text_color(rgb(accent())).child("◇"))
        .child(
            div()
                .text_sm()
                .text_color(rgb(theme_text()))
                .child("Select a file to preview"),
        )
        .child(
            div()
                .text_xs()
                .text_color(rgb(text_muted()))
                .child("Space toggles this panel"),
        )
        .into_any_element()
}

fn places() -> Vec<(String, PathBuf)> {
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

fn favorite_availability(favorites: &[PathBuf]) -> HashMap<PathBuf, bool> {
    favorites
        .iter()
        .map(|favorite| (favorite.clone(), favorite.is_dir()))
        .collect()
}

fn place_is_active(current_path: &Path, place_path: &Path) -> bool {
    current_path == place_path
}

fn initial_path() -> PathBuf {
    env::args_os()
        .skip(1)
        .find(|argument| !argument.to_string_lossy().starts_with('-'))
        .and_then(|argument| {
            let text = argument.to_string_lossy();
            if text.starts_with("file://") {
                url::Url::parse(&text).ok()?.to_file_path().ok()
            } else {
                Some(PathBuf::from(argument))
            }
        })
        .filter(|path| path.is_dir())
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn clipboard_text(clipboard: &FileClipboard) -> Result<String, String> {
    let payload = encode_file_clipboard(clipboard).map_err(|error| error.to_string())?;
    let bytes = payload
        .into_iter()
        .find(|payload| payload.mime_type == TEXT_MIME)
        .map(|payload| payload.bytes)
        .ok_or_else(|| "clipboard text payload is missing".to_owned())?;
    String::from_utf8(bytes).map_err(|error| error.to_string())
}

fn clipboard_from_text(text: &str) -> Option<FileClipboard> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with("copy\n") || trimmed.starts_with("cut\n") {
        let payloads = BTreeMap::from([(GNOME_FILES_MIME.to_owned(), trimmed.as_bytes().to_vec())]);
        return decode_file_clipboard(&payloads).ok().flatten();
    }
    if trimmed
        .lines()
        .filter(|line| !line.trim().is_empty())
        .all(|line| line.trim().starts_with("file://"))
    {
        let payloads = BTreeMap::from([(URI_LIST_MIME.to_owned(), trimmed.as_bytes().to_vec())]);
        return decode_file_clipboard(&payloads).ok().flatten();
    }

    let paths: Vec<_> = trimmed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .collect();
    (!paths.is_empty() && paths.iter().all(|path| path.is_absolute()))
        .then(|| FileClipboard::new(FileClipboardMode::Copy, paths))
}

fn selection_subject(paths: &[PathBuf]) -> String {
    if let [path] = paths {
        return format!(
            "“{}”",
            path.file_name().unwrap_or_default().to_string_lossy()
        );
    }
    format!("{} items", paths.len())
}

#[allow(clippy::too_many_lines)]
fn operation_from_sheet(
    sheet: &OperationSheet,
    current_directory: &Path,
    cx: &App,
) -> Result<FsOperation, String> {
    match sheet {
        OperationSheet::Extract {
            sources,
            destination,
        } => {
            let text = destination.read(cx).text().trim().to_owned();
            if text.is_empty() {
                return Err("Destination folder is required".into());
            }
            let expanded = if text == "~" {
                dirs::home_dir().ok_or_else(|| "Home directory is unavailable".to_owned())?
            } else if let Some(rest) = text.strip_prefix("~/") {
                dirs::home_dir()
                    .ok_or_else(|| "Home directory is unavailable".to_owned())?
                    .join(rest)
            } else {
                PathBuf::from(text)
            };
            let destination = if expanded.is_absolute() {
                expanded
            } else {
                current_directory.join(expanded)
            };
            match std::fs::metadata(&destination) {
                Ok(metadata) if metadata.is_dir() => {}
                Ok(_) => return Err("Destination must be a folder".into()),
                Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                    return Err("Destination folder is not accessible".into());
                }
                Err(_) => return Err("Destination folder does not exist".into()),
            }
            Ok(FsOperation::ExtractArchives {
                sources: sources.clone(),
                destination,
            })
        }
        OperationSheet::CreateEntry { kind, name } => {
            create_entry_operation(*kind, name, current_directory, cx)
        }
        OperationSheet::FolderProperties { .. } => {
            Err("Folder properties do not apply changes".into())
        }
        OperationSheet::Symlink {
            target,
            name,
            relative,
        } => {
            let target_text = target.read(cx).text().trim().to_owned();
            let name = name.read(cx).text().trim().to_owned();
            validate_file_name(&name)?;
            if target_text.is_empty() {
                return Err("Target path is required".into());
            }
            let expanded_target = if let Some(rest) = target_text.strip_prefix("~/") {
                dirs::home_dir()
                    .ok_or_else(|| "Home directory is unavailable".to_owned())?
                    .join(rest)
            } else {
                PathBuf::from(&target_text)
            };
            let absolute_target = if expanded_target.is_absolute() {
                expanded_target
            } else {
                current_directory.join(expanded_target)
            };
            let link_path = current_directory.join(name);
            let stored_target = if *relative {
                lexical_relative_path(&absolute_target, current_directory)?
            } else {
                std::path::absolute(absolute_target).map_err(|error| error.to_string())?
            };
            Ok(FsOperation::CreateSymlink {
                link_path,
                target: stored_target,
            })
        }
        OperationSheet::Permissions {
            paths,
            original_modes,
            current_modes,
            octal,
        } => {
            let octal = octal.read(cx).text().trim().to_owned();
            let change = if octal.is_empty() {
                let mut set = 0;
                let mut clear = 0;
                for bit_index in 0..12 {
                    let bit = 1 << bit_index;
                    let changed = original_modes
                        .iter()
                        .zip(current_modes)
                        .any(|(before, after)| before & bit != after & bit);
                    if changed && current_modes.iter().all(|mode| mode & bit != 0) {
                        set |= bit;
                    } else if changed && current_modes.iter().all(|mode| mode & bit == 0) {
                        clear |= bit;
                    }
                }
                if set == 0 && clear == 0 {
                    return Err("No permission changes to apply".into());
                }
                PermissionChange::Mask { set, clear }
            } else {
                let mode = u32::from_str_radix(octal.trim_start_matches("0o"), 8)
                    .map_err(|_| "Enter an octal mode such as 0644".to_owned())?;
                if mode > 0o7777 {
                    return Err("Permission mode must be between 0000 and 7777".into());
                }
                PermissionChange::Exact(mode)
            };
            Ok(FsOperation::SetPermissions {
                paths: paths.clone(),
                change,
            })
        }
        OperationSheet::Rename { from, name } => {
            let name = name.read(cx).text().trim().to_owned();
            validate_file_name(&name)?;
            let to = from
                .parent()
                .ok_or_else(|| "Cannot rename a filesystem root".to_owned())?
                .join(name);
            if to == *from {
                return Err("The name is unchanged".into());
            }
            Ok(FsOperation::Rename {
                from: from.clone(),
                to,
            })
        }
        OperationSheet::BulkRename { .. } => Ok(FsOperation::BulkRename {
            pairs: bulk_rename_preview(sheet, cx)?,
        }),
    }
}

fn create_entry_operation(
    kind: CreateEntryKind,
    name: &Entity<TextInput>,
    current_directory: &Path,
    cx: &App,
) -> Result<FsOperation, String> {
    let name = name.read(cx).text().trim().to_owned();
    validate_file_name(&name)?;
    let path = current_directory.join(name);
    match kind {
        CreateEntryKind::Folder => Ok(FsOperation::CreateDirectory { path }),
        CreateEntryKind::File => Ok(FsOperation::CreateFile { path }),
    }
}

#[allow(clippy::too_many_lines)]
fn bulk_rename_preview(sheet: &OperationSheet, cx: &App) -> Result<Vec<RenamePair>, String> {
    let OperationSheet::BulkRename {
        paths,
        find,
        replace,
        prefix,
        suffix,
        start,
        padding,
        regex,
        numbering,
        scope,
    } = sheet
    else {
        return Err("Bulk rename is not active".into());
    };
    let find = find.read(cx).text().to_owned();
    let replace = replace.read(cx).text().to_owned();
    let prefix = prefix.read(cx).text().to_owned();
    let suffix = suffix.read(cx).text().to_owned();
    let start = start
        .read(cx)
        .text()
        .trim()
        .parse::<usize>()
        .map_err(|_| "Numbering start must be a positive integer".to_owned())?;
    let padding = padding
        .read(cx)
        .text()
        .trim()
        .parse::<usize>()
        .map_err(|_| "Padding must be an integer".to_owned())?
        .min(12);
    let expression = (*regex && !find.is_empty())
        .then(|| regex::Regex::new(&find).map_err(|error| format!("Invalid regex: {error}")))
        .transpose()?;

    let mut pairs = Vec::with_capacity(paths.len());
    let mut destinations = HashSet::with_capacity(paths.len());
    let sources: HashSet<_> = paths.iter().cloned().collect();
    for (index, from) in paths.iter().enumerate() {
        let original = from
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("Name is not valid UTF-8: {}", from.display()))?;
        let path = Path::new(original);
        let stem = path
            .file_stem()
            .and_then(|part| part.to_str())
            .unwrap_or(original);
        let extension = path
            .extension()
            .and_then(|part| part.to_str())
            .unwrap_or("");
        let component = match scope {
            RenameScope::Stem => stem,
            RenameScope::Extension => {
                if extension.is_empty() {
                    return Err(format!("{} has no extension", from.display()));
                }
                extension
            }
            RenameScope::FullName => original,
        };
        let replaced = if let Some(expression) = &expression {
            expression
                .replace_all(component, replace.as_str())
                .into_owned()
        } else if find.is_empty() {
            component.to_owned()
        } else {
            component.replace(&find, &replace)
        };
        let mut transformed = format!("{prefix}{replaced}{suffix}");
        if *numbering {
            let number = start + index;
            transformed.push('-');
            write!(&mut transformed, "{number:0padding$}")
                .expect("writing to a String cannot fail");
        }
        let new_name = match scope {
            RenameScope::Stem if !extension.is_empty() => format!("{transformed}.{extension}"),
            RenameScope::Extension => format!("{stem}.{transformed}"),
            _ => transformed,
        };
        validate_file_name(&new_name)?;
        let to = from
            .parent()
            .ok_or_else(|| format!("Path has no parent: {}", from.display()))?
            .join(new_name);
        if !destinations.insert(to.clone()) {
            return Err(format!("Two items would become {}", to.display()));
        }
        if std::fs::symlink_metadata(&to).is_ok() && !sources.contains(&to) {
            return Err(format!("Destination already exists: {}", to.display()));
        }
        if to != *from {
            pairs.push(RenamePair {
                from: from.clone(),
                to,
            });
        }
    }
    if pairs.is_empty() {
        return Err("The rename rules do not change any names".into());
    }
    Ok(pairs)
}

fn validate_file_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\0') {
        return Err("Enter one valid file name without path separators".into());
    }
    Ok(())
}

fn lexical_relative_path(target: &Path, base: &Path) -> Result<PathBuf, String> {
    let target = std::path::absolute(target).map_err(|error| error.to_string())?;
    let base = std::path::absolute(base).map_err(|error| error.to_string())?;
    let target_components: Vec<_> = target.components().collect();
    let base_components: Vec<_> = base.components().collect();
    let common = target_components
        .iter()
        .zip(&base_components)
        .take_while(|(left, right)| left == right)
        .count();
    if common == 0 {
        return Err("Target and link directory do not share a filesystem root".into());
    }
    let mut relative = PathBuf::new();
    for component in &base_components[common..] {
        if matches!(component, std::path::Component::Normal(_)) {
            relative.push("..");
        }
    }
    for component in &target_components[common..] {
        relative.push(component.as_os_str());
    }
    if relative.as_os_str().is_empty() {
        relative.push(".");
    }
    Ok(relative)
}
