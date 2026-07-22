use std::{
    borrow::Cow,
    collections::{BTreeMap, HashSet},
    env,
    fmt::Write as _,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

mod action_menu;
mod empty_space_menu;
mod path_input;
mod text_input;
mod theme_runtime;

use action_menu::{
    ActionMenuPlacement, ActionMenuState, FileMenuCommand, MenuAnimationState, MenuContext,
    MenuEntry, prepare_context_selection,
};
use empty_space_menu::{
    EmptySpaceMenuActivation, EmptySpaceMenuCapabilities, EmptySpaceMenuCommand,
    EmptySpaceMenuContext, EmptySpaceMenuEntry, EmptySpaceMenuState, EmptySpaceViewState,
};
use path_input::{
    PathInputState, PathSuggestion, PathTarget, completion_candidates, resolve_path_input,
    single_pasted_path, validate_path,
};
use text_input::{TextInput, TextInputEvent};
use theme_runtime::{
    accent, accent_background, accent_hover, background, border, border_focused,
    danger as danger_color, error as error_color, git_added, git_deleted, git_modified,
    git_untracked, surface, surface_elevated, text as theme_text, text_emphasized, text_muted,
};

use chrono::{DateTime, Local};
use gnil_clipboard::{
    FileClipboard, FileClipboardMode, GNOME_FILES_MIME, TEXT_MIME, URI_LIST_MIME,
    decode_file_clipboard, encode_file_clipboard,
};
use gnil_core::{
    AppSettings, ConfigPaths, ConflictDecision, DirectorySnapshot, FileEntry, FileKind,
    FileMetadata, FsOperation, GitStatus, JobProgress, KeymapProfile, PermissionChange, RenamePair,
    SelectionState, SortDirection, SortField, TabRoot, TabState, ThemeAppearance, ThemeCatalog,
    ThemeMode, TrashEntryRef, UndoRecord,
};
use gnil_fs::{
    DeviceEntry, DeviceKind, DeviceMonitor, OperationExecutor, ScanOptions, TrashEntry,
    eject_device, mount_device, scan_devices, scan_directory, scan_git_status, scan_trash,
    unmount_device,
};
use gnil_preview::{PreviewRequest, PreviewResult, PreviewService};
use gpui::{
    AnchoredPositionMode, Animation, AnimationExt as _, AnyElement, App, Application, AssetSource,
    Bounds, ClickEvent, ClipboardItem, Context, Corner, Div, Entity, FocusHandle, Focusable,
    KeyBinding, MouseButton, MouseDownEvent, PromptLevel, Render, ScrollStrategy, SharedString,
    Stateful, Subscription, UniformListScrollHandle, Window, WindowAppearance, WindowBounds,
    WindowOptions, actions, anchored, deferred, div, img, point, prelude::*, px, relative, rgb,
    size, uniform_list,
};

actions!(
    gnil,
    [
        SelectNext,
        SelectPrevious,
        SelectNextRange,
        SelectPreviousRange,
        ToggleSelection,
        OpenSelected,
        GoBack,
        GoForward,
        GoUp,
        TogglePreview,
        ToggleHidden,
        Refresh,
        CopySelected,
        CutSelected,
        CopyPathAbsolute,
        CopyPathRelative,
        ToggleActions,
        ToggleAppearance,
        OpenCreateSymlink,
        OpenPermissions,
        OpenRename,
        ExtractSelected,
        ExtractSelectedTo,
        CancelOperation,
        DismissSheet,
        ApplySheet,
        Paste,
        TrashSelected,
        DeleteSelected,
        Undo,
        MenuNext,
        MenuPrevious,
        MenuFirst,
        MenuLast,
        MenuActivate,
        MenuOpenSubmenu,
        MenuCloseSubmenu,
        DismissMenu,
        CreateFolder,
        CreateFile,
        SelectAllEntries,
        RestoreTrashSelected,
        EmptyTrash,
        ActivatePathInput,
        SubmitPathInput,
        DismissPathInput,
        CompletePathNext,
        CompletePathPrevious,
        PathHistoryPrevious,
        PathHistoryNext,
        PastePath,
        Quit,
    ]
);

const FILE_GIT_COLUMN_WIDTH: f32 = 24.0;
const FILE_SIZE_COLUMN_WIDTH: f32 = 92.0;
const FILE_MODIFIED_COLUMN_WIDTH: f32 = 132.0;

struct Assets;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RenameScope {
    Stem,
    Extension,
    FullName,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CreateEntryKind {
    Folder,
    File,
}

enum OperationSheet {
    CreateEntry {
        kind: CreateEntryKind,
        name: Entity<TextInput>,
    },
    Extract {
        sources: Vec<PathBuf>,
        destination: Entity<TextInput>,
    },
    FolderProperties {
        path: PathBuf,
        item_count: usize,
        file_bytes: u64,
        mode: Option<u32>,
        readonly: bool,
    },
    Symlink {
        target: Entity<TextInput>,
        name: Entity<TextInput>,
        relative: bool,
    },
    Permissions {
        paths: Vec<PathBuf>,
        original_modes: Vec<u32>,
        current_modes: Vec<u32>,
        octal: Entity<TextInput>,
    },
    Rename {
        from: PathBuf,
        name: Entity<TextInput>,
    },
    BulkRename {
        paths: Vec<PathBuf>,
        find: Entity<TextInput>,
        replace: Entity<TextInput>,
        prefix: Entity<TextInput>,
        suffix: Entity<TextInput>,
        start: Entity<TextInput>,
        padding: Entity<TextInput>,
        regex: bool,
        numbering: bool,
        scope: RenameScope,
    },
}

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        let bytes: Option<&'static [u8]> = match path {
            "brand/gnil-fm.svg" => Some(include_bytes!("../../../assets/brand/gnil-fm.svg")),
            "icons/folder-closed.svg" => {
                Some(include_bytes!("../../../assets/icons/folder-closed.svg"))
            }
            "icons/folder-open.svg" => {
                Some(include_bytes!("../../../assets/icons/folder-open.svg"))
            }
            "icons/folder-favorite.svg" => {
                Some(include_bytes!("../../../assets/icons/folder-favorite.svg"))
            }
            "icons/folder-symlink.svg" => {
                Some(include_bytes!("../../../assets/icons/folder-symlink.svg"))
            }
            "icons/folder-readonly.svg" => {
                Some(include_bytes!("../../../assets/icons/folder-readonly.svg"))
            }
            "icons/file-generic.svg" => {
                Some(include_bytes!("../../../assets/icons/file-generic.svg"))
            }
            "icons/file-code.svg" => Some(include_bytes!("../../../assets/icons/file-code.svg")),
            "icons/file-text.svg" => Some(include_bytes!("../../../assets/icons/file-text.svg")),
            "icons/file-image.svg" => Some(include_bytes!("../../../assets/icons/file-image.svg")),
            "icons/file-document.svg" => {
                Some(include_bytes!("../../../assets/icons/file-document.svg"))
            }
            "icons/file-archive.svg" => {
                Some(include_bytes!("../../../assets/icons/file-archive.svg"))
            }
            "icons/file-media.svg" => Some(include_bytes!("../../../assets/icons/file-media.svg")),
            "icons/empty-state.svg" => {
                Some(include_bytes!("../../../assets/icons/empty-state.svg"))
            }
            "icons/trash.svg" => Some(include_bytes!("../../../assets/icons/trash.svg")),
            "icons/device-usb.svg" => Some(include_bytes!("../../../assets/icons/device-usb.svg")),
            "icons/device-drive.svg" => {
                Some(include_bytes!("../../../assets/icons/device-drive.svg"))
            }
            "icons/device-eject.svg" => {
                Some(include_bytes!("../../../assets/icons/device-eject.svg"))
            }
            _ => None,
        };
        Ok(bytes.map(Cow::Borrowed))
    }

    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        match path {
            "brand" => Ok(vec!["gnil-fm.svg".into()]),
            "icons" => Ok(vec![
                "folder-closed.svg".into(),
                "folder-open.svg".into(),
                "folder-favorite.svg".into(),
                "folder-symlink.svg".into(),
                "folder-readonly.svg".into(),
                "file-generic.svg".into(),
                "file-code.svg".into(),
                "file-text.svg".into(),
                "file-image.svg".into(),
                "file-document.svg".into(),
                "file-archive.svg".into(),
                "file-media.svg".into(),
                "empty-state.svg".into(),
                "trash.svg".into(),
                "device-usb.svg".into(),
                "device-drive.svg".into(),
                "device-eject.svg".into(),
            ]),
            _ => Ok(Vec::new()),
        }
    }
}

#[allow(clippy::struct_excessive_bools)]
struct FileManager {
    focus_handle: FocusHandle,
    tab: TabState,
    snapshot: DirectorySnapshot,
    selection: SelectionState,
    preview: Option<PreviewResult>,
    preview_path: Option<PathBuf>,
    preview_visible: bool,
    loading: bool,
    error: Option<String>,
    generation: u64,
    places: Vec<(String, PathBuf)>,
    keymap: KeymapProfile,
    undo_stack: Vec<UndoRecord>,
    operation_running: bool,
    operation_cancel: Option<Arc<AtomicBool>>,
    operation_progress: Option<JobProgress>,
    operation_progress_rx: Option<crossbeam_channel::Receiver<JobProgress>>,
    status_message: Option<String>,
    clipboard: Option<FileClipboard>,
    action_menu: Option<ActionMenuState>,
    action_menu_serial: u64,
    empty_space_menu: Option<EmptySpaceMenuState>,
    empty_space_menu_serial: u64,
    operation_sheet: Option<OperationSheet>,
    path_input: PathInputState,
    _path_input_subscription: Subscription,
    pending_reveal: Option<PathBuf>,
    file_list_scroll: UniformListScrollHandle,
    reduced_motion: bool,
    git_status_enabled: bool,
    trash_entries: Vec<TrashEntry>,
    devices: Vec<DeviceEntry>,
    device_monitor: Option<DeviceMonitor>,
    devices_loading: bool,
    auto_mount_removable: bool,
    auto_mount_attempted: HashSet<String>,
    config_paths: ConfigPaths,
    settings: AppSettings,
    theme_catalog: ThemeCatalog,
    theme_appearance: ThemeAppearance,
    active_theme_name: String,
    appearance_menu_open: bool,
    appearance_menu_closing: bool,
    appearance_subscription: Option<Subscription>,
}

impl FileManager {
    fn new(path: &Path, system_appearance: ThemeAppearance, cx: &mut Context<Self>) -> Self {
        let config_paths = ConfigPaths::discover();
        let settings = config_paths.load_settings().unwrap_or_default();
        let theme_catalog = ThemeCatalog::load(&config_paths.themes_dir());
        let theme_appearance = resolve_theme_appearance(settings.theme, system_appearance);
        let requested_theme = selected_theme_name(&settings, theme_appearance);
        let (active_theme, _) = theme_catalog.resolve(requested_theme, theme_appearance);
        let active_theme_name = active_theme.name.clone();
        theme_runtime::set_active(active_theme.colors);
        let tab = TabState {
            show_hidden: settings.show_hidden,
            ..TabState::new(path.to_path_buf())
        };
        let snapshot = DirectorySnapshot {
            generation: 0,
            path: path.to_path_buf(),
            entries: Vec::new(),
            unreadable_entries: 0,
        };
        let path_input = cx.new(|cx| {
            TextInput::new("Enter a path", path.display().to_string(), cx)
                .with_key_context("PathInput")
        });
        let path_input_subscription =
            cx.subscribe(&path_input, |this, input, event: &TextInputEvent, cx| {
                if matches!(event, TextInputEvent::Changed)
                    && this.path_input.editing
                    && this.path_input.input_changed()
                {
                    input.update(cx, |input, cx| input.set_invalid(false, cx));
                    cx.notify();
                }
            });
        Self {
            focus_handle: cx.focus_handle(),
            tab,
            snapshot,
            selection: SelectionState::default(),
            preview: None,
            preview_path: None,
            preview_visible: settings.preview_enabled,
            loading: false,
            error: None,
            generation: 0,
            places: places(),
            keymap: settings.keymap,
            undo_stack: Vec::new(),
            operation_running: false,
            operation_cancel: None,
            operation_progress: None,
            operation_progress_rx: None,
            status_message: None,
            clipboard: None,
            action_menu: None,
            action_menu_serial: 0,
            empty_space_menu: None,
            empty_space_menu_serial: 0,
            operation_sheet: None,
            path_input: PathInputState::new(path_input, path.to_path_buf()),
            _path_input_subscription: path_input_subscription,
            pending_reveal: None,
            file_list_scroll: UniformListScrollHandle::new(),
            reduced_motion: settings.reduced_motion,
            git_status_enabled: settings.git_status_enabled,
            trash_entries: Vec::new(),
            devices: Vec::new(),
            device_monitor: DeviceMonitor::start().ok(),
            devices_loading: false,
            auto_mount_removable: settings.auto_mount_removable,
            auto_mount_attempted: HashSet::new(),
            config_paths,
            settings,
            theme_catalog,
            theme_appearance,
            active_theme_name,
            appearance_menu_open: false,
            appearance_menu_closing: false,
            appearance_subscription: None,
        }
    }

    fn load_directory(&mut self, cx: &mut Context<Self>) {
        self.action_menu = None;
        self.empty_space_menu = None;
        if self.path_input.editing {
            self.path_input.dismiss();
            self.path_input
                .input
                .update(cx, |input, cx| input.set_invalid(false, cx));
        }
        if self.tab.root == TabRoot::Trash {
            self.load_trash(cx);
            return;
        }
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        let path = self.tab.path.clone();
        let preserve_selection = self.snapshot.path == path;
        let show_hidden = self.tab.show_hidden;
        let sort = self.tab.sort;
        let git_status_enabled = self.git_status_enabled;
        self.loading = true;
        self.error = None;
        if !preserve_selection {
            self.selection.clear();
        }
        self.preview = None;
        self.preview_path = None;
        cx.notify();

        let task = cx.background_executor().spawn(async move {
            let mut snapshot = scan_directory(
                &path,
                ScanOptions {
                    generation,
                    show_hidden,
                    sort,
                },
            )?;
            if git_status_enabled {
                let git = scan_git_status(&path);
                for entry in &mut snapshot.entries {
                    entry.git_status = git.status_for_path(&entry.path);
                }
            }
            Ok::<_, std::io::Error>(snapshot)
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                this.loading = false;
                match result {
                    Ok(snapshot) => {
                        this.snapshot = snapshot;
                        if let Some(path) = this.pending_reveal.take() {
                            if let Some(index) = this
                                .snapshot
                                .entries
                                .iter()
                                .position(|entry| entry.path == path)
                            {
                                this.selection.select_only(index, &this.snapshot.entries);
                                this.tab.selected_path = Some(path);
                                this.file_list_scroll
                                    .scroll_to_item_strict(index, ScrollStrategy::Center);
                                this.load_preview(cx);
                            } else {
                                this.selection.clear();
                                this.error = Some("The file is no longer in this folder".into());
                            }
                        } else {
                            this.selection.retain_existing(&this.snapshot.entries);
                        }
                    }
                    Err(error) => {
                        this.pending_reveal = None;
                        this.error = Some(error.to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn load_trash(&mut self, cx: &mut Context<Self>) {
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        self.loading = true;
        self.error = None;
        self.selection.clear();
        self.preview = None;
        self.preview_path = None;
        cx.notify();
        let task = cx.background_executor().spawn(async move { scan_trash() });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation || this.tab.root != TabRoot::Trash {
                    return;
                }
                this.loading = false;
                match result {
                    Ok(trash) => {
                        this.snapshot.entries = trash
                            .entries
                            .iter()
                            .map(trash_entry_as_file_entry)
                            .collect();
                        this.snapshot.unreadable_entries = trash.unreadable_entries;
                        this.trash_entries = trash.entries;
                    }
                    Err(error) => {
                        this.snapshot.entries.clear();
                        this.trash_entries.clear();
                        this.error = Some(format!("Could not read Trash: {error}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn refresh_devices(&mut self, cx: &mut Context<Self>) {
        if self.devices_loading {
            return;
        }
        self.devices_loading = true;
        let task = cx.background_executor().spawn(async { scan_devices() });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.devices_loading = false;
                let Ok(devices) = result else {
                    return;
                };
                if let TabRoot::Device { id, .. } = &this.tab.root
                    && !devices
                        .iter()
                        .any(|device| &device.id == id && device.mount_path.is_some())
                {
                    this.snapshot.entries.clear();
                    this.selection.clear();
                    this.preview = None;
                    this.preview_path = None;
                    this.error = Some("Location no longer available".into());
                }
                let auto_mount: Vec<_> = devices
                    .iter()
                    .filter(|device| {
                        this.auto_mount_removable
                            && device.removable
                            && device.mount_path.is_none()
                            && !this.auto_mount_attempted.contains(&device.id)
                    })
                    .map(|device| device.id.clone())
                    .collect();
                this.auto_mount_attempted
                    .retain(|id| devices.iter().any(|device| &device.id == id));
                this.devices = devices;
                for id in auto_mount {
                    this.auto_mount_attempted.insert(id.clone());
                    Self::mount_device_in_background(id, false, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn schedule_device_monitor(&mut self, cx: &mut Context<Self>) {
        if self.device_monitor.is_none() {
            return;
        }
        let timer = cx.background_executor().timer(Duration::from_millis(750));
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                if this
                    .device_monitor
                    .as_ref()
                    .is_some_and(DeviceMonitor::take_changed)
                {
                    this.refresh_devices(cx);
                }
                this.schedule_device_monitor(cx);
            });
        })
        .detach();
    }

    fn open_device(&mut self, id: String, cx: &mut Context<Self>) {
        let mount_path = self
            .devices
            .iter()
            .find(|device| device.id == id)
            .and_then(|device| device.mount_path.clone());
        if let Some(mount_path) = mount_path {
            self.tab.navigate_device(id, mount_path);
            self.load_directory(cx);
        } else {
            Self::mount_device_in_background(id, true, cx);
        }
    }

    fn navigate_path(&mut self, path: PathBuf) {
        let stays_on_device = matches!(
            &self.tab.root,
            TabRoot::Device { mount_root, .. } if path.starts_with(mount_root)
        );
        if stays_on_device {
            self.tab.navigate_within_root(path);
        } else {
            self.tab.navigate(path);
        }
    }

    fn mount_device_in_background(id: String, navigate: bool, cx: &mut Context<Self>) {
        let task_id = id.clone();
        let task = cx
            .background_executor()
            .spawn(async move { mount_device(&task_id) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(path) => {
                    if navigate {
                        this.tab.navigate_device(id, path);
                        this.load_directory(cx);
                    } else {
                        this.refresh_devices(cx);
                    }
                }
                Err(error) => {
                    this.error = Some(format!("Could not mount device: {error}"));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn disconnect_device(id: String, drive_id: String, eject: bool, cx: &mut Context<Self>) {
        let task = cx.background_executor().spawn(async move {
            unmount_device(&id)?;
            if eject {
                eject_device(&drive_id)?;
            }
            Ok::<_, gnil_fs::DeviceError>(())
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(()) => this.refresh_devices(cx),
                Err(error) => {
                    this.error = Some(format!("Could not disconnect device: {error}"));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn select_from_click(&mut self, index: usize, event: &ClickEvent, cx: &mut Context<Self>) {
        let modifiers = event.modifiers();
        if modifiers.shift {
            self.selection.extend_to(index, &self.snapshot.entries);
        } else if modifiers.control || modifiers.platform {
            self.selection.toggle(index, &self.snapshot.entries);
        } else {
            self.selection.select_only(index, &self.snapshot.entries);
        }
        self.tab.selected_path = self
            .snapshot
            .entries
            .get(index)
            .map(|entry| entry.path.clone());
        self.load_preview(cx);
        cx.notify();
    }

    fn load_preview(&mut self, cx: &mut Context<Self>) {
        let Some(index) = self.selection.cursor else {
            return;
        };
        let path = if self.tab.root == TabRoot::Trash {
            self.trash_entries.get(index).map_or_else(
                || self.snapshot.entries[index].path.clone(),
                |entry| entry.reference.trashed_path.clone(),
            )
        } else {
            self.snapshot.entries[index].path.clone()
        };
        self.preview_path = Some(path.clone());
        self.preview = None;
        let task = cx.background_executor().spawn(async move {
            PreviewService::default()
                .preview(&PreviewRequest::initial(path.clone()))
                .map(|preview| (path, preview))
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok((path, preview)) if this.preview_path.as_ref() == Some(&path) => {
                        this.preview = Some(preview);
                    }
                    Err(error) => this.error = Some(error.to_string()),
                    _ => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn open_index(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.tab.root == TabRoot::Trash {
            if let Some(entry) = self.trash_entries.get(index)
                && let Err(error) = std::process::Command::new("xdg-open")
                    .arg(&entry.reference.trashed_path)
                    .spawn()
            {
                self.error = Some(error.to_string());
                cx.notify();
            }
            return;
        }
        let Some(entry) = self.snapshot.entries.get(index).cloned() else {
            return;
        };
        match entry.kind {
            FileKind::Directory => {
                self.tab.navigate_within_root(entry.path);
                self.load_directory(cx);
            }
            FileKind::Symlink if entry.path.is_dir() => {
                self.tab.navigate_within_root(entry.path);
                self.load_directory(cx);
            }
            _ => {
                if let Err(error) = std::process::Command::new("xdg-open")
                    .arg(&entry.path)
                    .spawn()
                {
                    self.error = Some(error.to_string());
                    cx.notify();
                }
            }
        }
    }

    fn select_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        self.selection.move_cursor(1, &self.snapshot.entries, false);
        self.selection_changed(cx);
    }

    fn select_previous(&mut self, _: &SelectPrevious, _: &mut Window, cx: &mut Context<Self>) {
        self.selection
            .move_cursor(-1, &self.snapshot.entries, false);
        self.selection_changed(cx);
    }

    fn select_next_range(&mut self, _: &SelectNextRange, _: &mut Window, cx: &mut Context<Self>) {
        self.selection.move_cursor(1, &self.snapshot.entries, true);
        self.selection_changed(cx);
    }

    fn select_previous_range(
        &mut self,
        _: &SelectPreviousRange,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selection.move_cursor(-1, &self.snapshot.entries, true);
        self.selection_changed(cx);
    }

    fn toggle_selection(&mut self, _: &ToggleSelection, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.selection.cursor {
            self.selection.toggle(index, &self.snapshot.entries);
            if self.keymap == KeymapProfile::Yazi {
                self.selection
                    .move_cursor_preserving_selection(1, &self.snapshot.entries);
            }
            self.selection_changed(cx);
        }
    }

    fn selection_changed(&mut self, cx: &mut Context<Self>) {
        self.tab.selected_path = self
            .selection
            .cursor
            .and_then(|index| self.snapshot.entries.get(index))
            .map(|entry| entry.path.clone());
        self.load_preview(cx);
        cx.notify();
    }

    fn open_selected(&mut self, _: &OpenSelected, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.selection.cursor {
            self.open_index(index, cx);
        }
    }

    fn go_back(&mut self, _: &GoBack, _: &mut Window, cx: &mut Context<Self>) {
        if self.tab.back() {
            self.load_directory(cx);
        }
    }

    fn go_forward(&mut self, _: &GoForward, _: &mut Window, cx: &mut Context<Self>) {
        if self.tab.forward() {
            self.load_directory(cx);
        }
    }

    fn go_up(&mut self, _: &GoUp, _: &mut Window, cx: &mut Context<Self>) {
        if self.tab.up() {
            self.load_directory(cx);
        }
    }

    fn toggle_preview(&mut self, _: &TogglePreview, _: &mut Window, cx: &mut Context<Self>) {
        self.preview_visible = !self.preview_visible;
        cx.notify();
    }

    fn toggle_hidden(&mut self, _: &ToggleHidden, _: &mut Window, cx: &mut Context<Self>) {
        if self.tab.root == TabRoot::Trash {
            return;
        }
        self.tab.show_hidden = !self.tab.show_hidden;
        self.load_directory(cx);
    }

    fn refresh(&mut self, _: &Refresh, _: &mut Window, cx: &mut Context<Self>) {
        self.load_directory(cx);
    }

    fn activate_path_input(
        &mut self,
        _: &ActivatePathInput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operation_sheet.is_some()
            || self.action_menu.is_some()
            || self.empty_space_menu.is_some()
            || self.appearance_menu_open
        {
            return;
        }
        if self.path_input.editing {
            self.path_input.input.update(cx, |input, cx| {
                input.select_all(cx);
                window.focus(&input.focus_handle(cx));
            });
            return;
        }
        let value = self.tab.path.display().to_string();
        self.path_input.begin(self.tab.path.clone());
        self.path_input.expect_programmatic_change();
        self.path_input.input.update(cx, |input, cx| {
            input.set_text(value, cx);
            input.set_invalid(false, cx);
            input.select_all(cx);
            window.focus(&input.focus_handle(cx));
        });
        cx.notify();
    }

    fn dismiss_path_input(
        &mut self,
        _: &DismissPathInput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.path_input.editing {
            return;
        }
        self.path_input.dismiss();
        self.path_input
            .input
            .update(cx, |input, cx| input.set_invalid(false, cx));
        window.focus(&self.focus_handle(cx));
        cx.notify();
    }

    fn complete_path_next(&mut self, _: &CompletePathNext, _: &mut Window, cx: &mut Context<Self>) {
        self.complete_path(false, cx);
    }

    fn complete_path_previous(
        &mut self,
        _: &CompletePathPrevious,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.complete_path(true, cx);
    }

    fn complete_path(&mut self, reverse: bool, cx: &mut Context<Self>) {
        if !self.path_input.editing || self.path_input.checking {
            return;
        }
        if !self.path_input.suggestions.is_empty() {
            if self.path_input.move_suggestion(reverse) {
                cx.notify();
            }
            return;
        }
        let input = self.path_input.input.read(cx).text().to_owned();
        let base_path = self.path_input.base_path.clone();
        let home_dir = dirs::home_dir();
        let show_hidden = self.tab.show_hidden;
        let generation = self.path_input.begin_request();
        self.path_input
            .input
            .update(cx, |input, cx| input.set_invalid(false, cx));
        cx.notify();
        let task = cx.background_executor().spawn(async move {
            completion_candidates(&input, &base_path, home_dir.as_deref(), show_hidden)
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(mut suggestions) if suggestions.len() == 1 => {
                    let suggestion = suggestions.pop().expect("single suggestion");
                    if this
                        .path_input
                        .apply_suggestions(generation, Vec::new(), reverse)
                    {
                        this.path_input.expect_programmatic_change();
                        this.path_input.input.update(cx, |input, cx| {
                            input.set_text(suggestion.input, cx);
                            input.set_invalid(false, cx);
                        });
                        cx.notify();
                    }
                }
                Ok(mut suggestions) => {
                    suggestions.truncate(8);
                    if this
                        .path_input
                        .apply_suggestions(generation, suggestions, reverse)
                    {
                        cx.notify();
                    }
                }
                Err(error) => {
                    if this.path_input.apply_error(generation, error) {
                        this.path_input
                            .input
                            .update(cx, |input, cx| input.set_invalid(true, cx));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    fn path_history_previous(
        &mut self,
        _: &PathHistoryPrevious,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.path_input.editing || self.path_input.checking {
            return;
        }
        let current = self.path_input.input.read(cx).text().to_owned();
        if let Some(value) = self.path_input.history_previous(&current) {
            self.set_path_input_text(value, cx);
        }
    }

    fn path_history_next(&mut self, _: &PathHistoryNext, _: &mut Window, cx: &mut Context<Self>) {
        if !self.path_input.editing || self.path_input.checking {
            return;
        }
        if let Some(value) = self.path_input.history_next() {
            self.set_path_input_text(value, cx);
        }
    }

    fn set_path_input_text(&mut self, value: String, cx: &mut Context<Self>) {
        self.path_input.expect_programmatic_change();
        self.path_input.input.update(cx, |input, cx| {
            input.set_text(value, cx);
            input.set_invalid(false, cx);
        });
        cx.notify();
    }

    fn paste_path(&mut self, _: &PastePath, window: &mut Window, cx: &mut Context<Self>) {
        if !self.path_input.editing || self.path_input.checking {
            return;
        }
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        match single_pasted_path(&text) {
            Ok(path) => {
                self.path_input.input.update(cx, |input, cx| {
                    input.replace_selection(&path, window, cx);
                    input.set_invalid(false, cx);
                });
            }
            Err(error) => self.set_path_input_error(error, cx),
        }
    }

    fn submit_path_input(
        &mut self,
        _: &SubmitPathInput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.path_input.editing || self.path_input.checking {
            return;
        }
        if let Some(suggestion) = self.path_input.focused_suggestion() {
            self.accept_path_suggestion(suggestion, cx);
            return;
        }
        let raw = self.path_input.input.read(cx).text().to_owned();
        let path = match resolve_path_input(
            &raw,
            &self.path_input.base_path,
            dirs::home_dir().as_deref(),
        ) {
            Ok(path) => path,
            Err(error) => {
                self.set_path_input_error(error, cx);
                return;
            }
        };
        let generation = self.path_input.begin_request();
        self.path_input
            .input
            .update(cx, |input, cx| input.set_invalid(false, cx));
        cx.notify();
        let task = cx
            .background_executor()
            .spawn(async move { validate_path(path) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if generation != this.path_input.generation || !this.path_input.editing {
                    return;
                }
                this.path_input.checking = false;
                match result {
                    Ok(target) => {
                        this.path_input.record_success(raw);
                        this.path_input.dismiss();
                        this.path_input
                            .input
                            .update(cx, |input, cx| input.set_invalid(false, cx));
                        match target {
                            PathTarget::Directory(path) => {
                                this.pending_reveal = None;
                                this.navigate_path(path);
                            }
                            PathTarget::File { path, parent } => {
                                this.pending_reveal = Some(path);
                                this.navigate_path(parent);
                            }
                        }
                        window.focus(&this.focus_handle(cx));
                        this.load_directory(cx);
                    }
                    Err(error) => {
                        this.path_input.error = Some(error);
                        this.path_input.input.update(cx, |input, cx| {
                            input.set_invalid(true, cx);
                        });
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    fn accept_path_suggestion(&mut self, suggestion: PathSuggestion, cx: &mut Context<Self>) {
        self.path_input.suggestions.clear();
        self.path_input.focused_suggestion = None;
        self.set_path_input_text(suggestion.input, cx);
    }

    fn set_path_input_error(&mut self, error: String, cx: &mut Context<Self>) {
        self.path_input.set_error(error);
        self.path_input
            .input
            .update(cx, |input, cx| input.set_invalid(true, cx));
        cx.notify();
    }

    fn toggle_actions(&mut self, _: &ToggleActions, _: &mut Window, cx: &mut Context<Self>) {
        if self.action_menu.is_some() {
            self.dismiss_action_menu(cx);
        } else {
            self.appearance_menu_open = false;
            self.open_action_menu(ActionMenuPlacement::Header, cx);
        }
    }

    fn toggle_appearance(&mut self, _: &ToggleAppearance, _: &mut Window, cx: &mut Context<Self>) {
        if self.appearance_menu_open && !self.appearance_menu_closing {
            self.dismiss_appearance_menu(cx);
        } else {
            self.action_menu = None;
            self.empty_space_menu = None;
            self.appearance_menu_open = true;
            self.appearance_menu_closing = false;
            cx.notify();
        }
    }

    fn dismiss_appearance_menu(&mut self, cx: &mut Context<Self>) {
        if !self.appearance_menu_open || self.appearance_menu_closing {
            return;
        }
        self.appearance_menu_closing = true;
        cx.notify();
        let timer = cx.background_executor().timer(Duration::from_millis(80));
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                if this.appearance_menu_closing {
                    this.appearance_menu_open = false;
                    this.appearance_menu_closing = false;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn set_theme_mode(&mut self, mode: ThemeMode, window: &Window, cx: &mut Context<Self>) {
        self.settings.theme = mode;
        self.apply_selected_theme(window_theme_appearance(window), cx);
        self.save_settings();
    }

    fn select_theme(&mut self, name: &str, cx: &mut Context<Self>) {
        match self.theme_appearance {
            ThemeAppearance::Light => name.clone_into(&mut self.settings.light_theme),
            ThemeAppearance::Dark => name.clone_into(&mut self.settings.dark_theme),
        }
        self.apply_theme_name(name, cx);
        self.save_settings();
    }

    fn reload_themes(&mut self, cx: &mut Context<Self>) {
        self.theme_catalog = ThemeCatalog::load(&self.config_paths.themes_dir());
        let requested = selected_theme_name(&self.settings, self.theme_appearance).to_owned();
        self.apply_theme_name(&requested, cx);
        if self.theme_catalog.errors.is_empty() {
            self.status_message = Some("Themes reloaded".into());
        } else {
            self.status_message = Some(format!(
                "{} theme file{} could not be loaded",
                self.theme_catalog.errors.len(),
                if self.theme_catalog.errors.len() == 1 {
                    ""
                } else {
                    "s"
                }
            ));
        }
        cx.notify();
    }

    fn apply_selected_theme(&mut self, system_appearance: ThemeAppearance, cx: &mut Context<Self>) {
        self.theme_appearance = resolve_theme_appearance(self.settings.theme, system_appearance);
        let requested = selected_theme_name(&self.settings, self.theme_appearance).to_owned();
        self.apply_theme_name(&requested, cx);
    }

    fn apply_theme_name(&mut self, requested: &str, cx: &mut Context<Self>) {
        let (theme, fallback) = self.theme_catalog.resolve(requested, self.theme_appearance);
        self.active_theme_name.clone_from(&theme.name);
        theme_runtime::set_active(theme.colors);
        if fallback {
            self.status_message = Some(format!(
                "Theme “{requested}” is unavailable; using {}",
                theme.name
            ));
        }
        cx.notify();
    }

    fn save_settings(&mut self) {
        if let Err(error) = self.config_paths.save_settings(&self.settings) {
            self.error = Some(format!("Could not save settings: {error}"));
        }
    }

    fn system_appearance_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.theme == ThemeMode::System {
            self.apply_selected_theme(window_theme_appearance(window), cx);
        }
    }

    fn open_action_menu(&mut self, placement: ActionMenuPlacement, cx: &mut Context<Self>) {
        if self.tab.root == TabRoot::Trash {
            return;
        }
        self.empty_space_menu = None;
        self.appearance_menu_open = false;
        let clipboard_valid = self.file_clipboard_from_system(cx).is_some();
        let context = MenuContext::from_selection(
            &self.selection,
            &self.snapshot.entries,
            clipboard_valid,
            self.operation_running,
        );
        self.action_menu_serial = self.action_menu_serial.wrapping_add(1);
        self.action_menu = Some(ActionMenuState::new(
            placement,
            context,
            self.action_menu_serial,
        ));
        cx.notify();
    }

    fn dismiss_action_menu(&mut self, cx: &mut Context<Self>) {
        let Some(menu) = self.action_menu.as_mut() else {
            return;
        };
        if menu.animation == MenuAnimationState::Closing {
            return;
        }
        menu.animation = MenuAnimationState::Closing;
        let serial = menu.serial;
        cx.notify();
        let timer = cx.background_executor().timer(Duration::from_millis(80));
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                if this.action_menu.as_ref().is_some_and(|menu| {
                    menu.serial == serial && menu.animation == MenuAnimationState::Closing
                }) {
                    this.action_menu = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn open_empty_space_menu(
        &mut self,
        position: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.action_menu = None;
        self.appearance_menu_open = false;
        self.selection.clear();
        self.tab.selected_path = None;
        self.preview = None;
        self.preview_path = None;
        let context = EmptySpaceMenuContext {
            capabilities: EmptySpaceMenuCapabilities {
                clipboard_valid: self.file_clipboard_from_system(cx).is_some(),
                operation_running: self.operation_running,
                has_entries: !self.snapshot.entries.is_empty(),
            },
            sort: self.tab.sort,
            view: EmptySpaceViewState {
                show_hidden: self.tab.show_hidden,
                git_status_enabled: self.git_status_enabled,
            },
        };
        self.empty_space_menu_serial = self.empty_space_menu_serial.wrapping_add(1);
        self.empty_space_menu = Some(EmptySpaceMenuState::new(
            position,
            context,
            self.empty_space_menu_serial,
        ));
        cx.notify();
    }

    fn dismiss_empty_space_menu(&mut self, cx: &mut Context<Self>) {
        let Some(menu) = self.empty_space_menu.as_mut() else {
            return;
        };
        if menu.animation == MenuAnimationState::Closing {
            return;
        }
        menu.animation = MenuAnimationState::Closing;
        let serial = menu.serial;
        cx.notify();
        let timer = cx.background_executor().timer(Duration::from_millis(80));
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                if this.empty_space_menu.as_ref().is_some_and(|menu| {
                    menu.serial == serial && menu.animation == MenuAnimationState::Closing
                }) {
                    this.empty_space_menu = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn dismiss_menu(&mut self, _: &DismissMenu, _: &mut Window, cx: &mut Context<Self>) {
        if self.appearance_menu_open {
            self.dismiss_appearance_menu(cx);
        } else if self
            .empty_space_menu
            .as_mut()
            .is_some_and(EmptySpaceMenuState::close_submenu)
        {
            cx.notify();
        } else if self.empty_space_menu.is_some() {
            self.dismiss_empty_space_menu(cx);
        } else {
            self.dismiss_action_menu(cx);
        }
    }

    fn menu_next(&mut self, _: &MenuNext, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(menu) = self.empty_space_menu.as_mut()
            && menu.animation != MenuAnimationState::Closing
        {
            menu.move_focus(1);
            cx.notify();
        } else if let Some(menu) = self.action_menu.as_mut()
            && menu.animation != MenuAnimationState::Closing
        {
            menu.move_focus(1);
            cx.notify();
        }
    }

    fn menu_previous(&mut self, _: &MenuPrevious, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(menu) = self.empty_space_menu.as_mut()
            && menu.animation != MenuAnimationState::Closing
        {
            menu.move_focus(-1);
            cx.notify();
        } else if let Some(menu) = self.action_menu.as_mut()
            && menu.animation != MenuAnimationState::Closing
        {
            menu.move_focus(-1);
            cx.notify();
        }
    }

    fn menu_first(&mut self, _: &MenuFirst, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(menu) = self.empty_space_menu.as_mut()
            && menu.animation != MenuAnimationState::Closing
        {
            menu.focus_first();
            cx.notify();
        } else if let Some(menu) = self.action_menu.as_mut()
            && menu.animation != MenuAnimationState::Closing
        {
            menu.focus_first();
            cx.notify();
        }
    }

    fn menu_last(&mut self, _: &MenuLast, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(menu) = self.empty_space_menu.as_mut()
            && menu.animation != MenuAnimationState::Closing
        {
            menu.focus_last();
            cx.notify();
        } else if let Some(menu) = self.action_menu.as_mut()
            && menu.animation != MenuAnimationState::Closing
        {
            menu.focus_last();
            cx.notify();
        }
    }

    fn menu_activate(&mut self, _: &MenuActivate, window: &mut Window, cx: &mut Context<Self>) {
        let empty_space_activation = self
            .empty_space_menu
            .as_ref()
            .filter(|menu| menu.animation != MenuAnimationState::Closing)
            .and_then(EmptySpaceMenuState::focused_activation);
        if let Some(activation) = empty_space_activation {
            match activation {
                EmptySpaceMenuActivation::Command(command) => {
                    self.dispatch_empty_space_command(command, window, cx);
                }
                EmptySpaceMenuActivation::Submenu(submenu) => {
                    if let Some(menu) = self.empty_space_menu.as_mut() {
                        menu.open_submenu(submenu);
                        cx.notify();
                    }
                }
            }
            return;
        }
        let command = self
            .action_menu
            .as_ref()
            .filter(|menu| menu.animation != MenuAnimationState::Closing)
            .and_then(ActionMenuState::focused_command);
        if let Some(command) = command {
            self.dispatch_menu_command(command, window, cx);
        }
    }

    fn menu_open_submenu(&mut self, _: &MenuOpenSubmenu, _: &mut Window, cx: &mut Context<Self>) {
        let submenu = self
            .empty_space_menu
            .as_ref()
            .and_then(EmptySpaceMenuState::focused_activation)
            .and_then(|activation| match activation {
                EmptySpaceMenuActivation::Submenu(submenu) => Some(submenu),
                EmptySpaceMenuActivation::Command(_) => None,
            });
        if let Some(submenu) = submenu
            && let Some(menu) = self.empty_space_menu.as_mut()
        {
            menu.open_submenu(submenu);
            cx.notify();
        }
    }

    fn menu_close_submenu(&mut self, _: &MenuCloseSubmenu, _: &mut Window, cx: &mut Context<Self>) {
        if self
            .empty_space_menu
            .as_mut()
            .is_some_and(EmptySpaceMenuState::close_submenu)
        {
            cx.notify();
        }
    }

    fn dispatch_menu_command(
        &mut self,
        command: FileMenuCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.action_menu = None;
        cx.notify();
        match command {
            FileMenuCommand::Open => {
                let paths = self.selection.effective_paths(&self.snapshot.entries);
                if let [path] = paths.as_slice()
                    && let Some(index) = self
                        .snapshot
                        .entries
                        .iter()
                        .position(|entry| &entry.path == path)
                {
                    self.open_index(index, cx);
                }
            }
            FileMenuCommand::Extract => {
                self.extract_selected(&ExtractSelected, window, cx);
            }
            FileMenuCommand::ExtractTo => {
                self.extract_selected_to(&ExtractSelectedTo, window, cx);
            }
            FileMenuCommand::Copy => self.copy_selected(&CopySelected, window, cx),
            FileMenuCommand::Cut => self.cut_selected(&CutSelected, window, cx),
            FileMenuCommand::Paste => self.paste(&Paste, window, cx),
            FileMenuCommand::Rename => self.open_rename(&OpenRename, window, cx),
            FileMenuCommand::CreateSymlink => {
                self.open_create_symlink(&OpenCreateSymlink, window, cx);
            }
            FileMenuCommand::Permissions => {
                self.open_permissions(&OpenPermissions, window, cx);
            }
            FileMenuCommand::CopyPathAbsolute => {
                self.copy_path_absolute(&CopyPathAbsolute, window, cx);
            }
            FileMenuCommand::CopyPathRelative => {
                self.copy_path_relative(&CopyPathRelative, window, cx);
            }
            FileMenuCommand::Trash => self.trash_selected(&TrashSelected, window, cx),
            FileMenuCommand::DeletePermanently => {
                self.delete_selected(&DeleteSelected, window, cx);
            }
        }
    }

    fn dispatch_empty_space_command(
        &mut self,
        command: EmptySpaceMenuCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.empty_space_menu = None;
        match command {
            EmptySpaceMenuCommand::NewFolder => self.create_folder(&CreateFolder, window, cx),
            EmptySpaceMenuCommand::NewFile => self.create_file(&CreateFile, window, cx),
            EmptySpaceMenuCommand::Paste => self.paste(&Paste, window, cx),
            EmptySpaceMenuCommand::Refresh => self.refresh(&Refresh, window, cx),
            EmptySpaceMenuCommand::SortField(field) => {
                self.tab.sort.field = field;
                self.load_directory(cx);
            }
            EmptySpaceMenuCommand::SortDirection(direction) => {
                self.tab.sort.direction = direction;
                self.load_directory(cx);
            }
            EmptySpaceMenuCommand::ToggleHidden => {
                self.toggle_hidden(&ToggleHidden, window, cx);
            }
            EmptySpaceMenuCommand::ToggleGitStatus => self.toggle_git_status(cx),
            EmptySpaceMenuCommand::SelectAll => self.select_all_entries(cx),
            EmptySpaceMenuCommand::OpenTerminal => self.open_terminal_here(cx),
            EmptySpaceMenuCommand::FolderProperties => self.open_folder_properties(cx),
        }
    }

    fn create_folder(&mut self, _: &CreateFolder, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab.root == TabRoot::Trash {
            return;
        }
        self.open_create_entry(CreateEntryKind::Folder, window, cx);
    }

    fn create_file(&mut self, _: &CreateFile, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab.root == TabRoot::Trash {
            return;
        }
        self.open_create_entry(CreateEntryKind::File, window, cx);
    }

    fn selected_archives(&self) -> Vec<PathBuf> {
        let paths = self.selection.effective_paths(&self.snapshot.entries);
        if !paths.is_empty()
            && paths
                .iter()
                .all(|path| gnil_fs::is_archive_candidate(path))
        {
            paths
        } else {
            Vec::new()
        }
    }

    fn extract_selected(&mut self, _: &ExtractSelected, _: &mut Window, cx: &mut Context<Self>) {
        if self.tab.root == TabRoot::Trash || self.operation_running {
            return;
        }
        let sources = self.selected_archives();
        if sources.is_empty() {
            return;
        }
        self.start_operation(
            FsOperation::ExtractArchives {
                sources,
                destination: self.tab.path.clone(),
            },
            "Preparing archives…".into(),
            false,
            cx,
        );
    }

    fn extract_selected_to(
        &mut self,
        _: &ExtractSelectedTo,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tab.root == TabRoot::Trash || self.operation_running {
            return;
        }
        let sources = self.selected_archives();
        if sources.is_empty() {
            return;
        }
        let destination = cx.new(|cx| {
            TextInput::new(
                "Destination folder",
                self.tab.path.display().to_string(),
                cx,
            )
        });
        destination.update(cx, |input, cx| {
            input.select_all(cx);
            window.focus(&input.focus_handle(cx));
        });
        self.operation_sheet = Some(OperationSheet::Extract {
            sources,
            destination,
        });
        self.action_menu = None;
        cx.notify();
    }

    fn cancel_operation(&mut self, _: &CancelOperation, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(cancel) = &self.operation_cancel {
            cancel.store(true, Ordering::Relaxed);
            self.status_message = Some("Cancelling…".into());
            cx.notify();
        }
    }

    fn open_create_entry(
        &mut self,
        kind: CreateEntryKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operation_running {
            return;
        }
        let placeholder = match kind {
            CreateEntryKind::Folder => "Folder name",
            CreateEntryKind::File => "File name",
        };
        let name = cx.new(|cx| TextInput::new(placeholder, "", cx));
        name.update(cx, |input, cx| window.focus(&input.focus_handle(cx)));
        self.operation_sheet = Some(OperationSheet::CreateEntry { kind, name });
        self.action_menu = None;
        self.empty_space_menu = None;
        cx.notify();
    }

    fn select_all_entries(&mut self, cx: &mut Context<Self>) {
        self.selection.select_all(&self.snapshot.entries);
        self.selection_changed(cx);
    }

    fn toggle_git_status(&mut self, cx: &mut Context<Self>) {
        self.git_status_enabled = !self.git_status_enabled;
        self.load_directory(cx);
    }

    fn sort_from_header(&mut self, field: SortField, cx: &mut Context<Self>) {
        if self.tab.sort.field == field {
            self.tab.sort.direction = match self.tab.sort.direction {
                SortDirection::Ascending => SortDirection::Descending,
                SortDirection::Descending => SortDirection::Ascending,
            };
        } else {
            self.tab.sort.field = field;
            self.tab.sort.direction = SortDirection::Ascending;
        }
        self.load_directory(cx);
    }

    fn open_terminal_here(&mut self, cx: &mut Context<Self>) {
        let mut last_error = None;
        for (program, arguments) in terminal_candidates() {
            match std::process::Command::new(&program)
                .args(arguments)
                .current_dir(&self.tab.path)
                .spawn()
            {
                Ok(_) => {
                    self.status_message = Some("Opened terminal in this folder".into());
                    self.error = None;
                    cx.notify();
                    return;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    last_error = Some(error);
                }
                Err(error) => {
                    last_error = Some(error);
                    break;
                }
            }
        }
        self.error = Some(last_error.map_or_else(
            || "No terminal command is configured".into(),
            |error| format!("Could not open a terminal: {error}"),
        ));
        cx.notify();
    }

    fn open_folder_properties(&mut self, cx: &mut Context<Self>) {
        let metadata = std::fs::symlink_metadata(&self.tab.path).ok();
        #[cfg(unix)]
        let mode = metadata.as_ref().map(|metadata| {
            use std::os::unix::fs::MetadataExt as _;
            metadata.mode() & 0o7777
        });
        #[cfg(not(unix))]
        let mode = None;
        self.operation_sheet = Some(OperationSheet::FolderProperties {
            path: self.tab.path.clone(),
            item_count: self.snapshot.entries.len(),
            file_bytes: self
                .snapshot
                .entries
                .iter()
                .filter(|entry| entry.kind == FileKind::File)
                .fold(0_u64, |total, entry| {
                    total.saturating_add(entry.metadata.len)
                }),
            mode,
            readonly: metadata.is_some_and(|metadata| metadata.permissions().readonly()),
        });
        cx.notify();
    }

    fn open_create_symlink(
        &mut self,
        _: &OpenCreateSymlink,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tab.root == TabRoot::Trash {
            return;
        }
        let target = cx.new(|cx| TextInput::new("Target path", "", cx));
        let name = cx.new(|cx| TextInput::new("Link name", "", cx));
        self.operation_sheet = Some(OperationSheet::Symlink {
            target,
            name,
            relative: true,
        });
        self.action_menu = None;
        cx.notify();
    }

    fn open_permissions(&mut self, _: &OpenPermissions, _: &mut Window, cx: &mut Context<Self>) {
        if self.tab.root == TabRoot::Trash {
            return;
        }
        let selected = self.selection.effective_paths(&self.snapshot.entries);
        let mut paths = Vec::new();
        let mut modes = Vec::new();
        let mut skipped_links = 0;
        for path in selected {
            let Some(entry) = self
                .snapshot
                .entries
                .iter()
                .find(|entry| entry.path == path)
            else {
                continue;
            };
            if entry.kind == FileKind::Symlink {
                skipped_links += 1;
                continue;
            }
            if let Some(mode) = entry.metadata.mode {
                paths.push(path);
                modes.push(mode & 0o7777);
            }
        }
        if paths.is_empty() {
            self.error = Some("Select at least one non-symlink item".into());
            cx.notify();
            return;
        }
        let common_mode = modes
            .first()
            .copied()
            .filter(|first| modes.iter().all(|mode| mode == first));
        let value = common_mode.map_or_else(String::new, |mode| format!("{mode:04o}"));
        let octal = cx.new(|cx| TextInput::new("mixed — use permission toggles", value, cx));
        self.operation_sheet = Some(OperationSheet::Permissions {
            paths,
            original_modes: modes.clone(),
            current_modes: modes,
            octal,
        });
        self.action_menu = None;
        if skipped_links > 0 {
            self.status_message = Some(format!(
                "Excluded {skipped_links} symlink{} from chmod",
                if skipped_links == 1 { "" } else { "s" }
            ));
        }
        cx.notify();
    }

    fn open_rename(&mut self, _: &OpenRename, _: &mut Window, cx: &mut Context<Self>) {
        if self.tab.root == TabRoot::Trash {
            return;
        }
        let paths = self.selection.effective_paths(&self.snapshot.entries);
        if paths.is_empty() {
            return;
        }
        self.operation_sheet = if let [path] = paths.as_slice() {
            let value = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_owned();
            let name = cx.new(|cx| TextInput::new("New name", value, cx));
            Some(OperationSheet::Rename {
                from: path.clone(),
                name,
            })
        } else {
            Some(OperationSheet::BulkRename {
                paths,
                find: cx.new(|cx| TextInput::new("Find", "", cx)),
                replace: cx.new(|cx| TextInput::new("Replace", "", cx)),
                prefix: cx.new(|cx| TextInput::new("Prefix", "", cx)),
                suffix: cx.new(|cx| TextInput::new("Suffix", "", cx)),
                start: cx.new(|cx| TextInput::new("Start", "1", cx)),
                padding: cx.new(|cx| TextInput::new("Padding", "2", cx)),
                regex: false,
                numbering: false,
                scope: RenameScope::Stem,
            })
        };
        self.action_menu = None;
        cx.notify();
    }

    fn dismiss_sheet(&mut self, _: &DismissSheet, _: &mut Window, cx: &mut Context<Self>) {
        self.operation_sheet = None;
        self.error = None;
        cx.notify();
    }

    fn toggle_permission_bit(&mut self, bit: u32, cx: &mut Context<Self>) {
        let Some(OperationSheet::Permissions {
            current_modes,
            octal,
            ..
        }) = self.operation_sheet.as_mut()
        else {
            return;
        };
        let all_set = current_modes.iter().all(|mode| mode & bit != 0);
        for mode in current_modes.iter_mut() {
            if all_set {
                *mode &= !bit;
            } else {
                *mode |= bit;
            }
        }
        let common = current_modes
            .first()
            .copied()
            .filter(|first| current_modes.iter().all(|mode| mode == first));
        octal.update(cx, |input, cx| {
            input.set_text(
                common.map_or_else(String::new, |mode| format!("{mode:04o}")),
                cx,
            );
        });
        cx.notify();
    }

    fn apply_sheet(&mut self, _: &ApplySheet, _: &mut Window, cx: &mut Context<Self>) {
        if self.operation_running
            || matches!(
                self.operation_sheet,
                Some(OperationSheet::FolderProperties { .. })
            )
        {
            return;
        }
        let Some(sheet) = self.operation_sheet.take() else {
            return;
        };
        let operation = operation_from_sheet(&sheet, &self.tab.path, cx);
        match operation {
            Ok(operation) => {
                self.error = None;
                self.start_operation(operation, "Applying changes…".into(), false, cx);
            }
            Err(error) => {
                if let OperationSheet::Extract { destination, .. } = &sheet {
                    destination.update(cx, |input, cx| input.set_invalid(true, cx));
                }
                self.operation_sheet = Some(sheet);
                self.error = Some(error);
                cx.notify();
            }
        }
    }

    fn start_operation(
        &mut self,
        operation: FsOperation,
        progress_message: String,
        clear_clipboard_on_success: bool,
        cx: &mut Context<Self>,
    ) {
        if self.operation_running {
            return;
        }
        self.operation_running = true;
        let cancel = Arc::new(AtomicBool::new(false));
        let (progress_tx, progress_rx) = crossbeam_channel::unbounded();
        self.operation_cancel = Some(cancel.clone());
        self.operation_progress = None;
        self.operation_progress_rx = Some(progress_rx);
        self.error = None;
        self.status_message = Some(progress_message);
        cx.notify();

        let extraction = matches!(operation, FsOperation::ExtractArchives { .. });
        let task = cx.background_executor().spawn(async move {
            OperationExecutor.execute_with_progress(&operation, &cancel, &mut |progress| {
                let _ = progress_tx.send(progress);
            })
        });
        self.poll_operation_progress(cx);
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.operation_running = false;
                this.operation_cancel = None;
                this.operation_progress_rx = None;
                match result {
                    Ok(outcome) => {
                        if clear_clipboard_on_success {
                            this.clipboard = None;
                        }
                        let undo_available = outcome.undo.is_some();
                        if let Some(undo) = outcome.undo {
                            this.undo_stack.push(undo);
                        }
                        if extraction && !undo_available {
                            this.status_message = Some(format!(
                                "Extracted {} archive{} · too many entries to undo",
                                outcome.affected_paths.len(),
                                if outcome.affected_paths.len() == 1 {
                                    ""
                                } else {
                                    "s"
                                }
                            ));
                        } else {
                            this.status_message = Some(format!(
                                "Updated {} item{}",
                                outcome.affected_paths.len(),
                                if outcome.affected_paths.len() == 1 {
                                    ""
                                } else {
                                    "s"
                                }
                            ));
                        }
                        this.pending_reveal = outcome.affected_paths.first().cloned();
                        this.operation_progress = None;
                        this.load_directory(cx);
                    }
                    Err(error) => {
                        this.status_message = None;
                        this.operation_progress = None;
                        this.error = Some(error.to_string());
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    fn poll_operation_progress(&mut self, cx: &mut Context<Self>) {
        if let Some(receiver) = &self.operation_progress_rx {
            while let Ok(progress) = receiver.try_recv() {
                self.operation_progress = Some(progress);
            }
        }
        if !self.operation_running {
            return;
        }
        let timer = cx.background_executor().timer(Duration::from_millis(50));
        cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                this.poll_operation_progress(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn copy_selected(&mut self, _: &CopySelected, _: &mut Window, cx: &mut Context<Self>) {
        if self.tab.root == TabRoot::Trash {
            return;
        }
        let paths = self.selection.effective_paths(&self.snapshot.entries);
        if paths.is_empty() {
            return;
        }
        self.set_file_clipboard(FileClipboardMode::Copy, paths, cx);
    }

    fn cut_selected(&mut self, _: &CutSelected, _: &mut Window, cx: &mut Context<Self>) {
        if self.tab.root == TabRoot::Trash {
            return;
        }
        let paths = self.selection.effective_paths(&self.snapshot.entries);
        if paths.is_empty() {
            return;
        }
        self.set_file_clipboard(FileClipboardMode::Cut, paths, cx);
    }

    fn set_file_clipboard(
        &mut self,
        mode: FileClipboardMode,
        paths: Vec<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let clipboard = FileClipboard::new(mode, paths);
        match clipboard_text(&clipboard) {
            Ok(text) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                let action = match mode {
                    FileClipboardMode::Copy => "Copied",
                    FileClipboardMode::Cut => "Cut",
                };
                self.status_message = Some(format!(
                    "{action} {} item{}",
                    clipboard.paths.len(),
                    if clipboard.paths.len() == 1 { "" } else { "s" }
                ));
                self.clipboard = Some(clipboard);
            }
            Err(error) => self.error = Some(error),
        }
        cx.notify();
    }

    fn copy_path_absolute(&mut self, _: &CopyPathAbsolute, _: &mut Window, cx: &mut Context<Self>) {
        self.copy_paths_as_text(false, cx);
    }

    fn copy_path_relative(&mut self, _: &CopyPathRelative, _: &mut Window, cx: &mut Context<Self>) {
        self.copy_paths_as_text(true, cx);
    }

    fn copy_paths_as_text(&mut self, relative: bool, cx: &mut Context<Self>) {
        if self.tab.root == TabRoot::Trash {
            return;
        }
        let paths = self.selection.effective_paths(&self.snapshot.entries);
        if paths.is_empty() {
            return;
        }
        let result = paths
            .iter()
            .map(|path| {
                let path = if relative {
                    path.strip_prefix(&self.tab.path)
                        .unwrap_or(path)
                        .to_path_buf()
                } else {
                    std::path::absolute(path).map_err(|error| error.to_string())?
                };
                path.into_os_string().into_string().map_err(|path| {
                    format!("Path is not valid UTF-8: {}", PathBuf::from(path).display())
                })
            })
            .collect::<Result<Vec<_>, _>>();
        match result {
            Ok(paths) => {
                cx.write_to_clipboard(ClipboardItem::new_string(paths.join("\n")));
                self.status_message = Some(format!(
                    "Copied {} {}path{}",
                    paths.len(),
                    if relative { "relative " } else { "" },
                    if paths.len() == 1 { "" } else { "s" }
                ));
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
        cx.notify();
    }

    fn file_clipboard_from_system(&self, cx: &mut Context<Self>) -> Option<FileClipboard> {
        let system_text = cx.read_from_clipboard().and_then(|item| item.text());
        let internal = self
            .clipboard
            .clone()
            .filter(|clipboard| clipboard_text(clipboard).ok().as_ref() == system_text.as_ref());
        internal.or_else(|| system_text.as_deref().and_then(clipboard_from_text))
    }

    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if self.tab.root == TabRoot::Trash {
            return;
        }
        let Some(clipboard) = self.file_clipboard_from_system(cx) else {
            self.status_message = Some("Clipboard contains no local files".into());
            cx.notify();
            return;
        };
        let (operation, message, clear_clipboard) = match clipboard.mode {
            FileClipboardMode::Copy => (
                FsOperation::Copy {
                    sources: clipboard.paths,
                    destination: self.tab.path.clone(),
                    conflict: ConflictDecision::KeepBoth,
                },
                "Copying…".into(),
                false,
            ),
            FileClipboardMode::Cut => (
                FsOperation::Move {
                    sources: clipboard.paths,
                    destination: self.tab.path.clone(),
                    conflict: ConflictDecision::Ask,
                },
                "Moving…".into(),
                true,
            ),
        };
        self.start_operation(operation, message, clear_clipboard, cx);
    }

    fn trash_selected(&mut self, _: &TrashSelected, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab.root == TabRoot::Trash {
            self.purge_selected_trash(window, cx);
            return;
        }
        let paths = self.selection.effective_paths(&self.snapshot.entries);
        if paths.is_empty() {
            return;
        }
        if self.operation_running {
            return;
        }
        let count = paths.len();
        let subject = selection_subject(&paths);
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("Move {subject} to Trash?"),
            Some("You can restore it with Ctrl+Z while gnil-fm remains open."),
            &["Move to Trash", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await.ok() != Some(0) {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                this.start_operation(
                    FsOperation::Trash { paths },
                    format!(
                        "Moving {count} item{} to Trash…",
                        if count == 1 { "" } else { "s" }
                    ),
                    false,
                    cx,
                );
            });
        })
        .detach();
    }

    fn delete_selected(&mut self, _: &DeleteSelected, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab.root == TabRoot::Trash {
            self.purge_selected_trash(window, cx);
            return;
        }
        let paths = self.selection.effective_paths(&self.snapshot.entries);
        if paths.is_empty() {
            return;
        }
        if self.operation_running {
            return;
        }
        let count = paths.len();
        let subject = selection_subject(&paths);
        let answer = window.prompt(
            PromptLevel::Critical,
            &format!("Permanently delete {subject}?"),
            Some("This action cannot be undone."),
            &["Delete Permanently", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await.ok() != Some(0) {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                this.start_operation(
                    FsOperation::DeletePermanently { paths },
                    format!(
                        "Deleting {count} item{}…",
                        if count == 1 { "" } else { "s" }
                    ),
                    false,
                    cx,
                );
            });
        })
        .detach();
    }

    fn restore_trash_selected(
        &mut self,
        _: &RestoreTrashSelected,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entries = self.selected_trash_entries();
        if entries.is_empty() || self.operation_running {
            return;
        }
        let conflicts = entries
            .iter()
            .filter(|entry| entry.original_path.exists())
            .count();
        if conflicts == 0 {
            self.start_operation(
                FsOperation::RestoreTrash {
                    entries,
                    replace_existing: false,
                },
                "Restoring from Trash…".into(),
                false,
                cx,
            );
            return;
        }
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!(
                "Replace {conflicts} existing item{}?",
                if conflicts == 1 { "" } else { "s" }
            ),
            Some("Restoring will permanently replace the items currently at their original paths."),
            &["Replace and Restore", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await.ok() != Some(0) {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                this.start_operation(
                    FsOperation::RestoreTrash {
                        entries,
                        replace_existing: true,
                    },
                    "Replacing and restoring…".into(),
                    false,
                    cx,
                );
            });
        })
        .detach();
    }

    fn purge_selected_trash(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let entries = self.selected_trash_entries();
        if entries.is_empty() || self.operation_running {
            return;
        }
        let count = entries.len();
        let answer = window.prompt(
            PromptLevel::Critical,
            &format!(
                "Permanently delete {count} trash item{}?",
                if count == 1 { "" } else { "s" }
            ),
            Some("This action cannot be undone."),
            &["Delete Permanently", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await.ok() != Some(0) {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                this.start_operation(
                    FsOperation::PurgeTrash { entries },
                    "Deleting from Trash…".into(),
                    false,
                    cx,
                );
            });
        })
        .detach();
    }

    fn empty_trash(&mut self, _: &EmptyTrash, window: &mut Window, cx: &mut Context<Self>) {
        if self.trash_entries.is_empty() || self.operation_running {
            return;
        }
        let entries: Vec<_> = self
            .trash_entries
            .iter()
            .map(|entry| entry.reference.clone())
            .collect();
        let answer = window.prompt(
            PromptLevel::Critical,
            "Empty Trash?",
            Some("Every item in every available trash location will be permanently deleted."),
            &["Empty Trash", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await.ok() != Some(0) {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                this.start_operation(
                    FsOperation::PurgeTrash { entries },
                    "Emptying Trash…".into(),
                    false,
                    cx,
                );
            });
        })
        .detach();
    }

    fn selected_trash_entries(&self) -> Vec<TrashEntryRef> {
        let selected: HashSet<_> = self
            .selection
            .effective_paths(&self.snapshot.entries)
            .into_iter()
            .collect();
        self.trash_entries
            .iter()
            .filter(|entry| selected.contains(&entry.reference.info_path))
            .map(|entry| entry.reference.clone())
            .collect()
    }

    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if self.operation_running {
            return;
        }
        let Some(record) = self.undo_stack.last().cloned() else {
            self.status_message = Some("Nothing to undo".into());
            cx.notify();
            return;
        };
        let label = record.label.clone();
        self.operation_running = true;
        self.error = None;
        self.status_message = Some(format!("Undoing {label}…"));
        cx.notify();
        let task = cx
            .background_executor()
            .spawn(async move { OperationExecutor.undo(&record) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.operation_running = false;
                match result {
                    Ok(()) => {
                        this.undo_stack.pop();
                        this.status_message = Some(format!("Undid {label}"));
                        this.load_directory(cx);
                    }
                    Err(error) => {
                        this.status_message = None;
                        this.error = Some(format!("Could not undo: {error}"));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    // Sidebar groups stay together so their spacing and hierarchy remain visually auditable.
    #[allow(clippy::too_many_lines)]
    fn render_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .w(px(218.0))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(rgb(surface()))
            .border_r_1()
            .border_color(rgb(border()))
            .child(
                div()
                    .h(px(58.0))
                    .flex()
                    .items_center()
                    .px_4()
                    .gap_3()
                    .child(img("brand/gnil-fm.svg").size_8())
                    .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child("gnil")),
            )
            .child(
                div()
                    .px_3()
                    .pt_3()
                    .pb_1()
                    .text_xs()
                    .text_color(rgb(text_muted()))
                    .child("PLACES"),
            )
            .children(
                self.places
                    .iter()
                    .enumerate()
                    .map(|(index, (label, path))| {
                        let path = path.clone();
                        let active = self.tab.root == TabRoot::Directory
                            && place_is_active(&self.tab.path, &path);
                        let icon = if active {
                            "icons/folder-open.svg"
                        } else if label == "Home" {
                            "icons/folder-favorite.svg"
                        } else {
                            "icons/folder-closed.svg"
                        };
                        div()
                            .id(("place", index))
                            .mx_2()
                            .h(px(34.0))
                            .px_3()
                            .rounded_md()
                            .flex()
                            .items_center()
                            .gap_3()
                            .text_sm()
                            .cursor_pointer()
                            .when(active, |style| {
                                style
                                    .bg(rgb(accent_background()))
                                    .text_color(rgb(text_emphasized()))
                            })
                            .when(!active, |style| style.text_color(rgb(theme_text())))
                            .hover(|style| style.bg(rgb(border())))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.tab.navigate(path.clone());
                                this.load_directory(cx);
                            }))
                            .child(img(icon).size_5())
                            .child(label.clone())
                    }),
            )
            .child(sidebar_section_label("DEVICES"))
            .children(self.devices.iter().enumerate().map(|(index, device)| {
                let id = device.id.clone();
                let disconnect_id = device.id.clone();
                let drive_id = device.drive_id.clone();
                let eject = device.can_eject;
                let active =
                    matches!(&self.tab.root, TabRoot::Device { id, .. } if id == &device.id);
                let usage = device_usage(device);
                div()
                    .id(("device", index))
                    .mx_2()
                    .min_h(px(46.0))
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .flex()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .when(active, |row| row.bg(rgb(accent_background())))
                    .hover(|style| style.bg(rgb(border())))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_device(id.clone(), cx);
                    }))
                    .child(img(device_icon(device.kind)).size_5())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .truncate()
                                    .text_xs()
                                    .text_color(rgb(theme_text()))
                                    .child(device.label.clone()),
                            )
                            .child(
                                div()
                                    .h(px(3.0))
                                    .w_full()
                                    .rounded_full()
                                    .bg(rgb(border_focused()))
                                    .child(
                                        div()
                                            .h_full()
                                            .w(relative(usage))
                                            .rounded_full()
                                            .bg(rgb(accent())),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(9.0))
                                    .text_color(rgb(text_muted()))
                                    .child(device_capacity_label(device)),
                            ),
                    )
                    .when(device.mount_path.is_some(), |row| {
                        row.child(
                            div()
                                .id(("disconnect-device", index))
                                .size_7()
                                .rounded_md()
                                .flex()
                                .items_center()
                                .justify_center()
                                .hover(|style| style.bg(rgb(border_focused())))
                                .on_click(cx.listener(move |_this, _, _, cx| {
                                    cx.stop_propagation();
                                    Self::disconnect_device(
                                        disconnect_id.clone(),
                                        drive_id.clone(),
                                        eject,
                                        cx,
                                    );
                                }))
                                .child(img("icons/device-eject.svg").size_4()),
                        )
                    })
            }))
            .when(self.devices.is_empty(), |sidebar| {
                sidebar.child(
                    div()
                        .mx_3()
                        .py_1()
                        .text_xs()
                        .text_color(rgb(border_focused()))
                        .child(if self.devices_loading {
                            "Looking for devices…"
                        } else {
                            "No external devices"
                        }),
                )
            })
            .child(sidebar_section_label("TRASH"))
            .child(
                div()
                    .id("trash-place")
                    .mx_2()
                    .h(px(34.0))
                    .px_3()
                    .rounded_md()
                    .flex()
                    .items_center()
                    .gap_3()
                    .cursor_pointer()
                    .text_sm()
                    .when(self.tab.root == TabRoot::Trash, |row| {
                        row.bg(rgb(accent_background()))
                            .text_color(rgb(text_emphasized()))
                    })
                    .when(self.tab.root != TabRoot::Trash, |row| {
                        row.text_color(rgb(theme_text()))
                    })
                    .hover(|style| style.bg(rgb(border())))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.tab.navigate_trash();
                        this.load_directory(cx);
                    }))
                    .child(img("icons/trash.svg").size_5())
                    .child("Trash"),
            )
            .child(div().flex_1())
            .child(
                div()
                    .mx_3()
                    .mb_3()
                    .p_3()
                    .rounded_lg()
                    .bg(rgb(surface_elevated()))
                    .text_xs()
                    .text_color(rgb(text_muted()))
                    .child("Keyboard-first")
                    .child(
                        div()
                            .mt_1()
                            .text_color(rgb(theme_text()))
                            .child("Navigate · preview · organize"),
                    ),
            )
            .into_any_element()
    }

    // Declarative GPUI trees are more legible kept together than split into state-free fragments.
    #[allow(clippy::too_many_lines)]
    fn render_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let is_trash = self.tab.root == TabRoot::Trash;
        let can_go_up = match &self.tab.root {
            TabRoot::Trash => false,
            TabRoot::Device { mount_root, .. } => self.tab.path != *mount_root,
            TabRoot::Directory => self.tab.path.parent().is_some(),
        };
        let button = |id: &'static str, label: &'static str, enabled: bool| {
            div()
                .id(id)
                .size_8()
                .rounded_md()
                .flex()
                .items_center()
                .justify_center()
                .text_color(if enabled {
                    rgb(theme_text())
                } else {
                    rgb(border_focused())
                })
                .when(enabled, |style| {
                    style
                        .cursor_pointer()
                        .hover(|style| style.bg(rgb(border())))
                })
                .child(label)
        };
        div()
            .h(px(58.0))
            .w_full()
            .flex_none()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .border_b_1()
            .border_color(rgb(border()))
            .child(
                button("back", "‹", !self.tab.back_history.is_empty()).on_click(cx.listener(
                    |this, _, window, cx| {
                        this.go_back(&GoBack, window, cx);
                    },
                )),
            )
            .child(
                button("forward", "›", !self.tab.forward_history.is_empty()).on_click(cx.listener(
                    |this, _, window, cx| {
                        this.go_forward(&GoForward, window, cx);
                    },
                )),
            )
            .child(
                button("up", "↑", can_go_up).on_click(cx.listener(|this, _, window, cx| {
                    this.go_up(&GoUp, window, cx);
                })),
            )
            .child(
                div()
                    .ml_2()
                    .h(px(34.0))
                    .flex_1()
                    .min_w_0()
                    .relative()
                    .child(self.render_path_field(cx)),
            )
            .child(
                div()
                    .id("toggle-hidden")
                    .h_8()
                    .px_3()
                    .rounded_md()
                    .flex()
                    .items_center()
                    .text_xs()
                    .text_color(if is_trash {
                        rgb(border_focused())
                    } else if self.tab.show_hidden {
                        rgb(accent())
                    } else {
                        rgb(text_muted())
                    })
                    .when(!is_trash, |control| {
                        control
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(border())))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_hidden(&ToggleHidden, window, cx);
                            }))
                    })
                    .child("Hidden"),
            )
            .child(
                div()
                    .id("toggle-preview")
                    .h_8()
                    .px_3()
                    .rounded_md()
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .text_xs()
                    .text_color(if self.preview_visible {
                        rgb(accent())
                    } else {
                        rgb(text_muted())
                    })
                    .hover(|style| style.bg(rgb(border())))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_preview(&TogglePreview, window, cx);
                    }))
                    .child("Preview"),
            )
            .child(
                div()
                    .id("appearance")
                    .h_8()
                    .w(px(88.0))
                    .relative()
                    .rounded_md()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .text_xs()
                    .text_color(if self.appearance_menu_open {
                        rgb(accent())
                    } else {
                        rgb(theme_text())
                    })
                    .hover(|style| style.bg(rgb(border())))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_appearance(&ToggleAppearance, window, cx);
                    }))
                    .child(if self.appearance_menu_open {
                        "Theme ↑"
                    } else {
                        "Theme ↓"
                    })
                    .when(self.appearance_menu_open, |trigger| {
                        trigger.child(self.render_appearance_menu(cx))
                    }),
            )
            .child(
                div()
                    .id("actions")
                    .h_8()
                    .w(px(80.0))
                    .relative()
                    .rounded_md()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_xs()
                    .text_color(if is_trash {
                        rgb(border_focused())
                    } else if self.action_menu.is_some() {
                        rgb(accent())
                    } else {
                        rgb(theme_text())
                    })
                    .when(!is_trash, |control| {
                        control
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(border())))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_actions(&ToggleActions, window, cx);
                            }))
                    })
                    .child(if self.action_menu.is_some() {
                        "Actions ↑"
                    } else {
                        "Actions ↓"
                    })
                    .when(
                        matches!(
                            self.action_menu.as_ref().map(|menu| menu.placement),
                            Some(ActionMenuPlacement::Header)
                        ),
                        |trigger| trigger.child(self.render_header_action_menu(cx)),
                    ),
            )
            .child(
                div()
                    .id("undo")
                    .h_8()
                    .px_3()
                    .rounded_md()
                    .flex()
                    .items_center()
                    .text_xs()
                    .text_color(if self.undo_stack.is_empty() {
                        rgb(border_focused())
                    } else {
                        rgb(theme_text())
                    })
                    .when(!self.undo_stack.is_empty(), |style| {
                        style
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(border())))
                    })
                    .on_click(cx.listener(|this, _, window, cx| this.undo(&Undo, window, cx)))
                    .child("Undo"),
            )
            .into_any_element()
    }

    fn render_path_field(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.path_input.editing {
            let input = self.path_input.input.clone();
            let checking = self.path_input.checking;
            let field = div()
                .id("path-input-layer")
                .size_full()
                .relative()
                .child(input)
                .when(checking, |field| {
                    field.child(
                        div()
                            .absolute()
                            .right_2()
                            .top(px(8.0))
                            .text_xs()
                            .text_color(rgb(text_muted()))
                            .child("Checking…"),
                    )
                })
                .when(
                    self.path_input.error.is_some() || !self.path_input.suggestions.is_empty(),
                    |field| field.child(self.render_path_feedback(cx)),
                );
            if self.reduced_motion {
                field.into_any_element()
            } else {
                field
                    .with_animation(
                        "path-input-enter",
                        Animation::new(Duration::from_millis(120))
                            .with_easing(gpui::ease_out_quint()),
                        |field, delta| field.opacity(delta).top(px(2.0 * (1.0 - delta))),
                    )
                    .into_any_element()
            }
        } else {
            let is_trash = self.tab.root == TabRoot::Trash;
            let breadcrumb = div()
                .id("breadcrumb-path")
                .size_full()
                .min_w_0()
                .rounded_md()
                .bg(rgb(surface_elevated()))
                .border_1()
                .border_color(rgb(border()))
                .px_3()
                .flex()
                .items_center()
                .gap_3()
                .text_sm()
                .text_color(rgb(theme_text()))
                .hover(|style| style.border_color(rgb(border_focused())))
                .when(!is_trash, |field| {
                    field
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.activate_path_input(&ActivatePathInput, window, cx);
                        }))
                })
                .child(div().flex_1().min_w_0().truncate().child(if is_trash {
                    "Trash".into()
                } else {
                    self.tab.path.display().to_string()
                }))
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(rgb(text_muted()))
                        .child(if is_trash { "Virtual view" } else { "Ctrl+L" }),
                );
            if self.reduced_motion {
                breadcrumb.into_any_element()
            } else {
                breadcrumb
                    .with_animation(
                        "breadcrumb-enter",
                        Animation::new(Duration::from_millis(100))
                            .with_easing(gpui::ease_out_quint()),
                        gpui::Styled::opacity,
                    )
                    .into_any_element()
            }
        }
    }

    fn render_path_feedback(&self, cx: &mut Context<Self>) -> AnyElement {
        let content = if let Some(error) = self.path_input.error.clone() {
            div()
                .w(px(420.0))
                .max_w_full()
                .px_3()
                .py_2()
                .rounded_lg()
                .border_1()
                .border_color(rgb(danger_color()))
                .bg(rgb(surface()))
                .shadow_lg()
                .text_xs()
                .text_color(rgb(danger_color()))
                .child(error)
                .into_any_element()
        } else {
            self.path_input
                .suggestions
                .iter()
                .take(8)
                .cloned()
                .enumerate()
                .fold(
                    div()
                        .w(px(420.0))
                        .max_w_full()
                        .p_1()
                        .rounded_lg()
                        .border_1()
                        .border_color(rgb(border_focused()))
                        .bg(rgb(surface()))
                        .shadow_lg()
                        .text_xs()
                        .occlude()
                        .on_any_mouse_down(|_, _, cx| cx.stop_propagation()),
                    |menu, (index, suggestion)| {
                        let focused = self.path_input.focused_suggestion == Some(index);
                        let clicked = suggestion.clone();
                        menu.child(
                            div()
                                .id(("path-suggestion", index))
                                .h(px(30.0))
                                .w_full()
                                .px_2()
                                .rounded_md()
                                .flex()
                                .items_center()
                                .cursor_pointer()
                                .text_color(rgb(theme_text()))
                                .when(focused, |row| row.bg(rgb(border())))
                                .hover(|row| row.bg(rgb(border())))
                                .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                                    if *hovered {
                                        this.path_input.focus_suggestion(index);
                                        cx.notify();
                                    }
                                }))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.accept_path_suggestion(clicked.clone(), cx);
                                }))
                                .child(suggestion.label),
                        )
                    },
                )
                .into_any_element()
        };
        deferred(
            anchored()
                .position_mode(AnchoredPositionMode::Local)
                .position(point(px(0.0), px(38.0)))
                .anchor(Corner::TopLeft)
                .snap_to_window()
                .child(content),
        )
        .with_priority(11)
        .into_any_element()
    }

    #[allow(clippy::too_many_lines)]
    fn render_appearance_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let selected_mode = self.settings.theme;
        let mode_row = [
            (ThemeMode::System, "System"),
            (ThemeMode::Light, "Light"),
            (ThemeMode::Dark, "Dark"),
        ]
        .into_iter()
        .enumerate()
        .fold(
            div()
                .h_8()
                .w_full()
                .p_1()
                .flex()
                .rounded_md()
                .bg(rgb(surface_elevated())),
            |row, (index, (mode, label))| {
                row.child(
                    div()
                        .id(("theme-mode", index))
                        .h_full()
                        .flex_1()
                        .rounded_md()
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .text_xs()
                        .text_color(if selected_mode == mode {
                            rgb(text_emphasized())
                        } else {
                            rgb(text_muted())
                        })
                        .when(selected_mode == mode, |button| {
                            button.bg(rgb(accent_background()))
                        })
                        .hover(|button| button.bg(rgb(border())))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.set_theme_mode(mode, window, cx);
                        }))
                        .child(label),
                )
            },
        );
        let themes = self
            .theme_catalog
            .themes_for(self.theme_appearance)
            .cloned()
            .collect::<Vec<_>>();
        let selected_name = self.active_theme_name.clone();
        let theme_rows = themes.into_iter().enumerate().fold(
            div().w_full().flex().flex_col().gap_1(),
            |list, (index, theme)| {
                let name = theme.name.clone();
                let selected = name == selected_name;
                let builtin = theme.builtin();
                list.child(
                    div()
                        .id(("theme-choice", index))
                        .h(px(36.0))
                        .w_full()
                        .px_2()
                        .rounded_md()
                        .flex()
                        .items_center()
                        .gap_2()
                        .cursor_pointer()
                        .when(selected, |row| row.bg(rgb(accent_background())))
                        .hover(|row| row.bg(rgb(border())))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.select_theme(&name, cx);
                        }))
                        .child(
                            div()
                                .size_3()
                                .rounded_full()
                                .border_1()
                                .border_color(rgb(theme.colors.border_focused))
                                .bg(rgb(theme.colors.accent)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_xs()
                                .text_color(rgb(theme_text()))
                                .child(theme.name),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(if selected { accent() } else { text_muted() }))
                                .child(if selected {
                                    "●"
                                } else if builtin {
                                    "Built-in"
                                } else {
                                    "JSON"
                                }),
                        ),
                )
            },
        );
        let theme_path = self.config_paths.themes_dir().display().to_string();
        let error_summary = (!self.theme_catalog.errors.is_empty()).then(|| {
            format!(
                "{} invalid theme file{} — Reload after fixing JSON",
                self.theme_catalog.errors.len(),
                if self.theme_catalog.errors.len() == 1 {
                    ""
                } else {
                    "s"
                }
            )
        });
        let panel = div()
            .id("appearance-menu")
            .w(px(304.0))
            .p_3()
            .rounded_lg()
            .border_1()
            .border_color(rgb(border_focused()))
            .bg(rgb(surface()))
            .shadow_lg()
            .occlude()
            .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgb(text_emphasized()))
                            .child("Appearance"),
                    )
                    .child(
                        div()
                            .id("close-appearance")
                            .size_6()
                            .rounded_md()
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .text_color(rgb(text_muted()))
                            .hover(|button| button.bg(rgb(border())).text_color(rgb(theme_text())))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.dismiss_appearance_menu(cx);
                            }))
                            .child("×"),
                    ),
            )
            .child(mode_row)
            .child(div().text_xs().text_color(rgb(text_muted())).child(
                match self.theme_appearance {
                    ThemeAppearance::Light => "LIGHT THEMES",
                    ThemeAppearance::Dark => "DARK THEMES",
                },
            ))
            .child(theme_rows)
            .when_some(error_summary, |panel, error| {
                panel.child(
                    div()
                        .rounded_md()
                        .p_2()
                        .bg(rgb(surface_elevated()))
                        .text_xs()
                        .text_color(rgb(error_color()))
                        .child(error),
                )
            })
            .child(
                div()
                    .pt_2()
                    .border_t_1()
                    .border_color(rgb(border()))
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(rgb(text_muted()))
                            .child(theme_path),
                    )
                    .child(
                        div()
                            .id("reload-themes")
                            .h_7()
                            .px_2()
                            .rounded_md()
                            .flex()
                            .items_center()
                            .cursor_pointer()
                            .text_xs()
                            .text_color(rgb(theme_text()))
                            .bg(rgb(surface_elevated()))
                            .hover(|button| button.bg(rgb(border())))
                            .on_click(cx.listener(|this, _, _, cx| this.reload_themes(cx)))
                            .child("Reload"),
                    ),
            );
        let panel = if self.reduced_motion {
            panel.into_any_element()
        } else if self.appearance_menu_closing {
            panel
                .with_animation(
                    "appearance-menu-closing",
                    Animation::new(Duration::from_millis(80)).with_easing(gpui::quadratic),
                    |panel, delta| panel.opacity(1.0 - delta).top(px(2.0 * delta)),
                )
                .into_any_element()
        } else {
            panel
                .with_animation(
                    "appearance-menu-opening",
                    Animation::new(Duration::from_millis(120)).with_easing(gpui::ease_out_quint()),
                    |panel, delta| panel.opacity(delta).top(px(3.0 * (1.0 - delta))),
                )
                .into_any_element()
        };
        deferred(
            anchored()
                .position_mode(AnchoredPositionMode::Local)
                .position(point(px(88.0), px(36.0)))
                .anchor(Corner::TopRight)
                .snap_to_window()
                .child(panel),
        )
        .with_priority(12)
        .into_any_element()
    }

    fn render_header_action_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        deferred(
            anchored()
                .position_mode(AnchoredPositionMode::Local)
                .position(point(px(80.0), px(36.0)))
                .anchor(Corner::TopRight)
                .snap_to_window()
                .child(self.render_action_menu_panel(cx)),
        )
        .with_priority(10)
        .into_any_element()
    }

    fn render_context_action_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(ActionMenuPlacement::Cursor(position)) =
            self.action_menu.as_ref().map(|menu| menu.placement)
        else {
            return div().into_any_element();
        };
        deferred(
            anchored()
                .position(position)
                .anchor(Corner::TopLeft)
                .snap_to_window()
                .child(self.render_action_menu_panel(cx)),
        )
        .with_priority(10)
        .into_any_element()
    }

    fn render_context_menu_backdrop(cx: &mut Context<Self>) -> AnyElement {
        deferred(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .occlude()
                .on_any_mouse_down(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    if this.appearance_menu_open {
                        this.dismiss_appearance_menu(cx);
                    } else if this.empty_space_menu.is_some() {
                        this.dismiss_empty_space_menu(cx);
                    } else {
                        this.dismiss_action_menu(cx);
                    }
                })),
        )
        .with_priority(9)
        .into_any_element()
    }

    fn render_empty_space_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(menu) = self.empty_space_menu.as_ref() else {
            return div().into_any_element();
        };
        let position = menu.position;
        let animation = menu.animation;
        let root =
            Self::render_empty_space_panel(menu.root_entries.clone(), menu.focused_root, false, cx);
        let panels = div()
            .relative()
            .flex()
            .items_start()
            .gap_1()
            .child(root)
            .when(menu.submenu.is_some(), |panels| {
                let submenu = Self::render_empty_space_panel(
                    menu.submenu_entries.clone(),
                    menu.focused_submenu,
                    true,
                    cx,
                );
                if self.reduced_motion {
                    panels.child(submenu)
                } else {
                    panels.child(
                        submenu.with_animation(
                            "empty-space-submenu-opening",
                            Animation::new(Duration::from_millis(100))
                                .with_easing(gpui::ease_out_quint()),
                            |panel, delta| panel.opacity(delta).left(px(3.0 * (1.0 - delta))),
                        ),
                    )
                }
            });
        let panels = if self.reduced_motion {
            panels.into_any_element()
        } else {
            match animation {
                MenuAnimationState::Opening => panels
                    .with_animation(
                        "empty-space-menu-opening",
                        Animation::new(Duration::from_millis(120))
                            .with_easing(gpui::ease_out_quint()),
                        |panels, delta| panels.opacity(delta).top(px(3.0 * (1.0 - delta))),
                    )
                    .into_any_element(),
                MenuAnimationState::Closing => panels
                    .with_animation(
                        "empty-space-menu-closing",
                        Animation::new(Duration::from_millis(80)).with_easing(gpui::quadratic),
                        |panels, delta| panels.opacity(1.0 - delta).top(px(2.0 * delta)),
                    )
                    .into_any_element(),
            }
        };
        deferred(
            anchored()
                .position(position)
                .anchor(Corner::TopLeft)
                .snap_to_window()
                .child(panels),
        )
        .with_priority(10)
        .into_any_element()
    }

    fn render_empty_space_panel(
        entries: Vec<EmptySpaceMenuEntry>,
        focused: Option<usize>,
        is_submenu: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        entries.into_iter().enumerate().fold(
            div()
                .id(if is_submenu {
                    "empty-space-submenu"
                } else {
                    "empty-space-menu"
                })
                .w(px(264.0))
                .p_1()
                .rounded_lg()
                .border_1()
                .border_color(rgb(border_focused()))
                .bg(rgb(surface()))
                .shadow_lg()
                .text_xs()
                .occlude()
                .on_any_mouse_down(|_, _, cx| cx.stop_propagation()),
            |panel, (index, entry)| {
                Self::render_empty_space_entry(panel, index, entry, focused, is_submenu, cx)
            },
        )
    }

    fn render_empty_space_entry(
        panel: Stateful<Div>,
        index: usize,
        entry: EmptySpaceMenuEntry,
        focused: Option<usize>,
        is_submenu: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        match entry {
            EmptySpaceMenuEntry::Separator => panel.child(
                div()
                    .h(px(9.0))
                    .px_2()
                    .flex()
                    .items_center()
                    .child(div().h(px(1.0)).w_full().bg(rgb(border()))),
            ),
            EmptySpaceMenuEntry::Action {
                command,
                label,
                shortcut,
                enabled,
                checked,
            } => panel.child(
                empty_space_menu_row(index, enabled, focused == Some(index))
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if !*hovered {
                            return;
                        }
                        if let Some(menu) = this.empty_space_menu.as_mut() {
                            if is_submenu {
                                menu.focus_submenu(index);
                            } else {
                                menu.focus_root(index);
                                menu.close_submenu();
                            }
                            cx.notify();
                        }
                    }))
                    .when(enabled, |row| {
                        row.on_click(cx.listener(move |this, _, window, cx| {
                            this.dispatch_empty_space_command(command, window, cx);
                        }))
                    })
                    .child(label)
                    .child(
                        div()
                            .ml_4()
                            .text_color(if enabled {
                                rgb(text_muted())
                            } else {
                                rgb(border_focused())
                            })
                            .child(if checked {
                                "●"
                            } else {
                                shortcut.unwrap_or("")
                            }),
                    ),
            ),
            EmptySpaceMenuEntry::Submenu {
                submenu,
                label,
                enabled,
            } => panel.child(
                empty_space_menu_row(index, enabled, focused == Some(index))
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered && let Some(menu) = this.empty_space_menu.as_mut() {
                            menu.focus_root(index);
                            if enabled {
                                menu.open_submenu(submenu);
                            }
                            cx.notify();
                        }
                    }))
                    .when(enabled, |row| {
                        row.on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(menu) = this.empty_space_menu.as_mut() {
                                menu.open_submenu(submenu);
                                cx.notify();
                            }
                        }))
                    })
                    .child(label)
                    .child(div().ml_4().text_color(rgb(text_muted())).child("›")),
            ),
        }
    }

    fn render_action_menu_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(menu) = self.action_menu.as_ref() else {
            return div().into_any_element();
        };
        let focused = menu.focused;
        let animation = menu.animation;
        let entries = menu.entries.clone();
        let panel = entries.into_iter().enumerate().fold(
            div()
                .id("file-action-menu")
                .w(px(264.0))
                .p_1()
                .relative()
                .rounded_lg()
                .border_1()
                .border_color(rgb(border_focused()))
                .bg(rgb(surface()))
                .shadow_lg()
                .text_xs()
                .occlude()
                .on_any_mouse_down(|_, _, cx| cx.stop_propagation()),
            |panel, (index, entry)| {
                Self::render_action_menu_entry(panel, index, entry, focused, cx)
            },
        );
        match animation {
            MenuAnimationState::Opening => panel
                .with_animation(
                    "file-action-menu-opening",
                    Animation::new(Duration::from_millis(120)).with_easing(gpui::ease_out_quint()),
                    |panel, delta| panel.opacity(delta).top(px(3.0 * (1.0 - delta))),
                )
                .into_any_element(),
            MenuAnimationState::Closing => panel
                .with_animation(
                    "file-action-menu-closing",
                    Animation::new(Duration::from_millis(80)).with_easing(gpui::quadratic),
                    |panel, delta| panel.opacity(1.0 - delta).top(px(2.0 * delta)),
                )
                .into_any_element(),
        }
    }

    fn render_action_menu_entry(
        panel: Stateful<Div>,
        index: usize,
        entry: MenuEntry,
        focused: Option<usize>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        match entry {
            MenuEntry::Separator => panel.child(
                div()
                    .h(px(9.0))
                    .px_2()
                    .flex()
                    .items_center()
                    .child(div().h(px(1.0)).w_full().bg(rgb(border()))),
            ),
            MenuEntry::Action {
                command,
                label,
                shortcut,
                enabled,
                danger,
            } => panel.child(
                div()
                    .id(("file-menu-entry", index))
                    .h_8()
                    .w_full()
                    .px_2()
                    .rounded_md()
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_color(if enabled {
                        if danger {
                            rgb(danger_color())
                        } else {
                            rgb(theme_text())
                        }
                    } else {
                        rgb(border_focused())
                    })
                    .when(focused == Some(index), |row| row.bg(rgb(border())))
                    .when(enabled, |row| {
                        row.cursor_pointer()
                            .hover(|style| style.bg(rgb(border())))
                            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                                if *hovered && let Some(menu) = this.action_menu.as_mut() {
                                    menu.focus(index);
                                    cx.notify();
                                }
                            }))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.dispatch_menu_command(command, window, cx);
                            }))
                    })
                    .child(label)
                    .when_some(shortcut, |row, shortcut| {
                        row.child(
                            div()
                                .ml_4()
                                .text_color(if enabled {
                                    rgb(text_muted())
                                } else {
                                    rgb(border_focused())
                                })
                                .child(shortcut),
                        )
                    }),
            ),
        }
    }

    #[allow(clippy::too_many_lines)]
    fn render_operation_sheet(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let read_only = matches!(
            self.operation_sheet,
            Some(OperationSheet::FolderProperties { .. })
        );
        let (title, subtitle, body) = match self.operation_sheet.as_ref().expect("active sheet") {
            OperationSheet::Extract {
                sources,
                destination,
            } => {
                let body = div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(field_label("DESTINATION FOLDER"))
                    .child(destination.clone())
                    .child(helper_text(
                        "Each archive creates a named output. Existing items are never merged or replaced.",
                    ))
                    .into_any_element();
                (
                    "Extract to…",
                    format!(
                        "{} archive{}",
                        sources.len(),
                        if sources.len() == 1 { "" } else { "s" }
                    ),
                    body,
                )
            }
            OperationSheet::CreateEntry { kind, name } => {
                let title = match kind {
                    CreateEntryKind::Folder => "New folder",
                    CreateEntryKind::File => "New file",
                };
                let body = div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(field_label("NAME"))
                    .child(name.clone())
                    .child(helper_text(
                        "Created in the current folder; an existing item is never replaced.",
                    ))
                    .into_any_element();
                (title, self.tab.path.display().to_string(), body)
            }
            OperationSheet::FolderProperties {
                path,
                item_count,
                file_bytes,
                mode,
                readonly,
            } => {
                let body = div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(property_row("Path", path.display().to_string()))
                    .child(property_row("Loaded items", item_count.to_string()))
                    .child(property_row("Visible file size", format_bytes(*file_bytes)))
                    .child(property_row(
                        "Permissions",
                        mode.map_or_else(|| "Unavailable".into(), |mode| format!("{mode:04o}")),
                    ))
                    .child(property_row(
                        "Access",
                        if *readonly { "Read only" } else { "Read and write" },
                    ))
                    .child(helper_text(
                        "Item count and size reflect the currently loaded view, including its hidden-file setting.",
                    ))
                    .into_any_element();
                (
                    "Folder properties",
                    path.file_name()
                        .unwrap_or(path.as_os_str())
                        .to_string_lossy()
                        .into_owned(),
                    body,
                )
            }
            OperationSheet::Symlink {
                target,
                name,
                relative,
            } => {
                let relative = *relative;
                let target = target.clone();
                let name = name.clone();
                let body = div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(field_label("TARGET"))
                    .child(target)
                    .child(field_label("LINK NAME"))
                    .child(name)
                    .child(
                        div()
                            .id("relative-link")
                            .h_8()
                            .px_2()
                            .rounded_md()
                            .flex()
                            .items_center()
                            .gap_2()
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(border())))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(OperationSheet::Symlink { relative, .. }) =
                                    this.operation_sheet.as_mut()
                                {
                                    *relative = !*relative;
                                    cx.notify();
                                }
                            }))
                            .child(if relative { "●" } else { "○" })
                            .child("Store relative target"),
                    )
                    .child(helper_text(
                        "Dangling targets are allowed; existing paths are never replaced.",
                    ))
                    .into_any_element();
                ("Create symlink", "Link from this folder".to_owned(), body)
            }
            OperationSheet::Permissions {
                current_modes,
                octal,
                paths,
                ..
            } => {
                let modes = current_modes.clone();
                let octal = octal.clone();
                let mut grid = div().flex().flex_col().gap_1();
                for (label, bits) in [
                    ("Owner", [0o400, 0o200, 0o100]),
                    ("Group", [0o040, 0o020, 0o010]),
                    ("Other", [0o004, 0o002, 0o001]),
                ] {
                    let mut row = div().h_8().flex().items_center().gap_1().child(
                        div()
                            .w(px(54.0))
                            .text_xs()
                            .text_color(rgb(text_muted()))
                            .child(label),
                    );
                    for (permission, bit) in ["Read", "Write", "Execute"].into_iter().zip(bits) {
                        let all = modes.iter().all(|mode| mode & bit != 0);
                        let none = modes.iter().all(|mode| mode & bit == 0);
                        let marker = if all {
                            "●"
                        } else if none {
                            "○"
                        } else {
                            "−"
                        };
                        row = row.child(
                            div()
                                .id(("permission-bit", bit as usize))
                                .flex_1()
                                .h_7()
                                .rounded_md()
                                .flex()
                                .items_center()
                                .justify_center()
                                .gap_1()
                                .cursor_pointer()
                                .text_xs()
                                .bg(if all {
                                    rgb(accent_background())
                                } else {
                                    rgb(surface_elevated())
                                })
                                .hover(|style| style.bg(rgb(border())))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.toggle_permission_bit(bit, cx);
                                }))
                                .child(marker)
                                .child(permission),
                        );
                    }
                    grid = grid.child(row);
                }
                let body = div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(grid)
                    .child(field_label("OCTAL MODE"))
                    .child(octal)
                    .child(helper_text(
                        "Non-recursive. Symlinks are excluded so their targets cannot be changed.",
                    ))
                    .into_any_element();
                (
                    "Permissions",
                    format!(
                        "{} item{}",
                        paths.len(),
                        if paths.len() == 1 { "" } else { "s" }
                    ),
                    body,
                )
            }
            OperationSheet::Rename { name, .. } => {
                let body = div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(field_label("NEW NAME"))
                    .child(name.clone())
                    .child(helper_text("The item stays in its current folder."))
                    .into_any_element();
                ("Rename", "One item".to_owned(), body)
            }
            sheet @ OperationSheet::BulkRename {
                find,
                replace,
                prefix,
                suffix,
                start,
                padding,
                regex,
                numbering,
                scope,
                paths,
            } => {
                let preview = bulk_rename_preview(sheet, cx);
                let preview_rows = match &preview {
                    Ok(pairs) => pairs
                        .iter()
                        .take(6)
                        .map(|pair| {
                            div().text_xs().text_color(rgb(theme_text())).child(format!(
                                "{}  →  {}",
                                pair.from.file_name().unwrap_or_default().to_string_lossy(),
                                pair.to.file_name().unwrap_or_default().to_string_lossy()
                            ))
                        })
                        .collect::<Vec<_>>(),
                    Err(_) => Vec::new(),
                };
                let error = preview.err();
                let regex_enabled = *regex;
                let numbering_enabled = *numbering;
                let active_scope = *scope;
                let body = div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(segmented_scope(active_scope, cx))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(find.clone())
                            .child(replace.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(prefix.clone())
                            .child(suffix.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(toggle_chip("regex", "Regex", regex_enabled, cx, |sheet| {
                                if let OperationSheet::BulkRename { regex, .. } = sheet {
                                    *regex = !*regex;
                                }
                            }))
                            .child(toggle_chip(
                                "numbering",
                                "Numbering",
                                numbering_enabled,
                                cx,
                                |sheet| {
                                    if let OperationSheet::BulkRename { numbering, .. } = sheet {
                                        *numbering = !*numbering;
                                    }
                                },
                            )),
                    )
                    .when(numbering_enabled, |view| {
                        view.child(
                            div()
                                .flex()
                                .gap_2()
                                .child(start.clone())
                                .child(padding.clone()),
                        )
                    })
                    .child(field_label("PREVIEW"))
                    .child(
                        div()
                            .p_2()
                            .rounded_md()
                            .bg(rgb(background()))
                            .flex()
                            .flex_col()
                            .gap_1()
                            .when_some(error, |view, error| {
                                view.child(
                                    div().text_xs().text_color(rgb(error_color())).child(error),
                                )
                            })
                            .children(preview_rows),
                    )
                    .into_any_element();
                (
                    "Bulk rename",
                    format!("{} items · live preview", paths.len()),
                    body,
                )
            }
        };

        let footer = div()
            .h(px(58.0))
            .px_4()
            .flex()
            .items_center()
            .justify_end()
            .gap_2()
            .border_t_1()
            .border_color(rgb(border()))
            .when(read_only, |footer| {
                footer.child(sheet_button("Close", true).on_click(cx.listener(
                    |this, _, window, cx| {
                        this.dismiss_sheet(&DismissSheet, window, cx);
                    },
                )))
            })
            .when(!read_only, |footer| {
                footer
                    .child(sheet_button("Cancel", false).on_click(cx.listener(
                        |this, _, window, cx| {
                            this.dismiss_sheet(&DismissSheet, window, cx);
                        },
                    )))
                    .child(sheet_button("Apply", true).on_click(cx.listener(
                        |this, _, window, cx| {
                            this.apply_sheet(&ApplySheet, window, cx);
                        },
                    )))
            });

        div()
            .w(px(382.0))
            .min_w(px(340.0))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(rgb(surface()))
            .border_l_1()
            .border_color(rgb(border()))
            .child(
                div()
                    .h(px(58.0))
                    .px_4()
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(rgb(border()))
                    .child(
                        div()
                            .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(title))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(text_muted()))
                                    .child(subtitle),
                            ),
                    )
                    .child(
                        div()
                            .id("dismiss-sheet")
                            .size_8()
                            .rounded_md()
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(border())))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.dismiss_sheet(&DismissSheet, window, cx);
                            }))
                            .child("×"),
                    ),
            )
            .child(div().flex_1().min_h_0().p_4().child(body))
            .child(footer)
            .into_any_element()
    }

    // The virtual list, contextual toolbar and column header form one declarative GPUI tree.
    #[allow(clippy::too_many_lines)]
    fn render_trash_list(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let selection = self.selection.clone();
        let entries = Arc::new(self.trash_entries.clone());
        let count = entries.len();
        let has_selection = self.selection.selected_count() > 0;
        let body = if count == 0 {
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_2()
                .child(img("icons/trash.svg").size(px(96.0)))
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(theme_text()))
                        .child(if self.loading {
                            "Reading Trash…"
                        } else {
                            "Trash is empty"
                        }),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(text_muted()))
                        .child("Deleted items from available devices appear together here."),
                )
                .into_any_element()
        } else {
            uniform_list(
                "trash-list",
                count,
                cx.processor(move |_this, range: std::ops::Range<usize>, _window, cx| {
                    range
                        .map(|index| {
                            let entry = entries[index].clone();
                            let file_entry = trash_entry_as_file_entry(&entry);
                            let highlighted = selection.is_highlighted(index, &file_entry);
                            div()
                                .id(("trash-entry", index))
                                .mx_2()
                                .h(px(42.0))
                                .px_3()
                                .rounded_md()
                                .flex()
                                .items_center()
                                .gap_2()
                                .cursor_pointer()
                                .when(highlighted, |row| {
                                    row.bg(rgb(accent_background()))
                                        .text_color(rgb(text_emphasized()))
                                })
                                .when(!highlighted, |row| {
                                    row.text_color(rgb(theme_text()))
                                        .hover(|style| style.bg(rgb(surface_elevated())))
                                })
                                .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                                    this.select_from_click(index, event, cx);
                                    if event.click_count() >= 2 {
                                        this.open_index(index, cx);
                                    }
                                }))
                                .child(file_icon(&file_entry))
                                .child(div().flex_1().min_w_0().truncate().child(entry.name))
                                .child(
                                    div()
                                        .w(px(142.0))
                                        .flex_none()
                                        .text_xs()
                                        .text_color(rgb(text_muted()))
                                        .child(deletion_label(entry.deletion_unix)),
                                )
                                .child(
                                    div()
                                        .w(px(280.0))
                                        .flex_none()
                                        .truncate()
                                        .text_xs()
                                        .text_color(rgb(text_muted()))
                                        .child(entry.original_path.display().to_string()),
                                )
                        })
                        .collect()
                }),
            )
            .track_scroll(self.file_list_scroll.clone())
            .h_full()
            .into_any_element()
        };
        div()
            .flex_1()
            .h_full()
            .min_w(px(420.0))
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(42.0))
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(rgb(border()))
                    .child(
                        trash_action_button("Restore", false, has_selection).on_click(cx.listener(
                            |this, _, window, cx| {
                                this.restore_trash_selected(&RestoreTrashSelected, window, cx);
                            },
                        )),
                    )
                    .child(
                        trash_action_button("Delete Permanently", true, has_selection).on_click(
                            cx.listener(|this, _, window, cx| {
                                this.purge_selected_trash(window, cx);
                            }),
                        ),
                    )
                    .child(div().flex_1())
                    .child(
                        trash_action_button("Empty Trash", true, count > 0).on_click(cx.listener(
                            |this, _, window, cx| {
                                this.empty_trash(&EmptyTrash, window, cx);
                            },
                        )),
                    ),
            )
            .child(
                div()
                    .h(px(32.0))
                    .px_4()
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(rgb(border()))
                    .text_xs()
                    .text_color(rgb(text_muted()))
                    .child(div().flex_1().child("NAME"))
                    .child(div().w(px(142.0)).flex_none().child("DELETED"))
                    .child(div().w(px(280.0)).flex_none().child("ORIGINAL LOCATION")),
            )
            .child(body)
            .into_any_element()
    }

    #[allow(clippy::too_many_lines)]
    fn render_file_list(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.tab.root == TabRoot::Trash {
            return self.render_trash_list(cx);
        }
        let selection = self.selection.clone();
        let entries = Arc::new(self.snapshot.entries.clone());
        let count = entries.len();
        let git_status_enabled = self.git_status_enabled;
        let sort = self.tab.sort;
        let body = if count == 0 {
            let title = if self.loading {
                "Opening folder…"
            } else if self.error.is_some() {
                "Could not read this folder"
            } else {
                "This folder is quiet"
            };
            let subtitle = if self.loading || self.error.is_some() {
                ""
            } else {
                "Drop or create a file to get started."
            };
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_2()
                .child(img("icons/empty-state.svg").size(px(184.0)))
                .child(
                    div()
                        .mt_2()
                        .text_sm()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(theme_text()))
                        .child(title),
                )
                .when(!subtitle.is_empty(), |view| {
                    view.child(
                        div()
                            .text_xs()
                            .text_color(rgb(text_muted()))
                            .child(subtitle),
                    )
                })
                .into_any_element()
        } else {
            uniform_list(
                "file-list",
                count,
                cx.processor(move |_this, range: std::ops::Range<usize>, _window, cx| {
                    range
                        .map(|index| {
                            let entry = entries[index].clone();
                            let open_entry = entry.clone();
                            let highlighted = selection.is_highlighted(index, &entry);
                            let row = div()
                                .id(("entry", index))
                                .w_full()
                                .h(px(36.0))
                                .px_2()
                                .rounded_md()
                                .flex()
                                .items_center()
                                .cursor_pointer()
                                .text_sm()
                                .when(highlighted, |style| {
                                    style
                                        .bg(rgb(accent_background()))
                                        .text_color(rgb(text_emphasized()))
                                })
                                .when(!highlighted, |style| {
                                    style
                                        .text_color(rgb(theme_text()))
                                        .hover(|style| style.bg(rgb(surface_elevated())))
                                })
                                .on_mouse_down(
                                    MouseButton::Right,
                                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                                        cx.stop_propagation();
                                        let changed = prepare_context_selection(
                                            &mut this.selection,
                                            &this.snapshot.entries,
                                            index,
                                        );
                                        if changed {
                                            this.selection_changed(cx);
                                        }
                                        this.open_action_menu(
                                            ActionMenuPlacement::Cursor(event.position),
                                            cx,
                                        );
                                    }),
                                )
                                .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                                    this.select_from_click(index, event, cx);
                                    if event.click_count() >= 2 {
                                        this.open_index(index, cx);
                                    }
                                }))
                                .child(file_icon(&entry))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .child(open_entry.name.clone()),
                                )
                                .when(git_status_enabled, |row| {
                                    row.child(
                                        div()
                                            .w(px(FILE_GIT_COLUMN_WIDTH))
                                            .flex_none()
                                            .text_xs()
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(git_color(entry.git_status))
                                            .child(git_label(entry.git_status)),
                                    )
                                })
                                .child(
                                    div()
                                        .w(px(FILE_SIZE_COLUMN_WIDTH))
                                        .flex_none()
                                        .text_xs()
                                        .text_color(rgb(text_muted()))
                                        .child(size_label(&entry)),
                                )
                                .child(
                                    div()
                                        .w(px(FILE_MODIFIED_COLUMN_WIDTH))
                                        .flex_none()
                                        .text_xs()
                                        .text_color(rgb(text_muted()))
                                        .child(modified_label(&entry)),
                                );
                            div().w_full().h(px(36.0)).px_2().child(row)
                        })
                        .collect()
                }),
            )
            .track_scroll(self.file_list_scroll.clone())
            .h_full()
            .into_any_element()
        };
        div()
            .flex_1()
            .h_full()
            .min_w(px(320.0))
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(32.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .px_4()
                    .border_b_1()
                    .border_color(rgb(border()))
                    .text_xs()
                    .text_color(rgb(text_muted()))
                    .child(
                        div()
                            .id("sort-name")
                            .flex_1()
                            .min_w_0()
                            .cursor_pointer()
                            .text_color(if sort.field == SortField::Name {
                                rgb(accent())
                            } else {
                                rgb(text_muted())
                            })
                            .hover(|style| style.text_color(rgb(theme_text())))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.sort_from_header(SortField::Name, cx);
                            }))
                            .child(sort_label("NAME", SortField::Name, sort)),
                    )
                    .when(git_status_enabled, |header| {
                        header.child(div().w(px(FILE_GIT_COLUMN_WIDTH)).flex_none().child("GIT"))
                    })
                    .child(
                        div()
                            .id("sort-size")
                            .w(px(FILE_SIZE_COLUMN_WIDTH))
                            .flex_none()
                            .cursor_pointer()
                            .text_color(if sort.field == SortField::Size {
                                rgb(accent())
                            } else {
                                rgb(text_muted())
                            })
                            .hover(|style| style.text_color(rgb(theme_text())))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.sort_from_header(SortField::Size, cx);
                            }))
                            .child(sort_label("SIZE", SortField::Size, sort)),
                    )
                    .child(
                        div()
                            .id("sort-modified")
                            .w(px(FILE_MODIFIED_COLUMN_WIDTH))
                            .flex_none()
                            .cursor_pointer()
                            .text_color(if sort.field == SortField::Modified {
                                rgb(accent())
                            } else {
                                rgb(text_muted())
                            })
                            .hover(|style| style.text_color(rgb(theme_text())))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.sort_from_header(SortField::Modified, cx);
                            }))
                            .child(sort_label("MODIFIED", SortField::Modified, sort)),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(|this, event: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.open_empty_space_menu(event.position, cx);
                        }),
                    )
                    .child(body),
            )
            .into_any_element()
    }

    fn render_preview(&self) -> AnyElement {
        let content = match (&self.preview_path, &self.preview) {
            (None, _) => empty_preview(),
            (Some(_path), None) => div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(rgb(text_muted()))
                .child("Reading preview…")
                .into_any_element(),
            (Some(path), Some(preview)) => preview_content(path, preview),
        };
        div()
            .w(px(318.0))
            .min_w(px(240.0))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(rgb(surface()))
            .border_l_1()
            .border_color(rgb(border()))
            .child(
                div()
                    .h(px(42.0))
                    .flex_none()
                    .px_4()
                    .flex()
                    .items_center()
                    .text_xs()
                    .text_color(rgb(text_muted()))
                    .child("PREVIEW"),
            )
            .child(content)
            .into_any_element()
    }

    fn render_status(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let mut message = if let Some(error) = &self.error {
            error.clone()
        } else if self.operation_running {
            self.status_message
                .clone()
                .unwrap_or_else(|| "Working…".into())
        } else if let Some(message) = &self.status_message {
            message.clone()
        } else if self.loading {
            "Scanning folder…".into()
        } else if self.selection.selected_count() > 0 {
            format!("{} selected", self.selection.selected_count())
        } else {
            format!("{} items", self.snapshot.entries.len())
        };
        if let Some(progress) = &self.operation_progress {
            let item_progress = progress.total_items.map_or_else(
                || progress.completed_items.to_string(),
                |total| format!("{} / {total}", progress.completed_items),
            );
            let byte_progress = if progress.completed_bytes > 0 {
                format!(" · {}", format_bytes(progress.completed_bytes))
            } else {
                String::new()
            };
            message = format!("Extracting {item_progress}{byte_progress}");
        }
        div()
            .h(px(30.0))
            .w_full()
            .flex_none()
            .px_3()
            .flex()
            .items_center()
            .justify_between()
            .border_t_1()
            .border_color(rgb(border()))
            .text_xs()
            .text_color(if self.error.is_some() {
                rgb(error_color())
            } else {
                rgb(text_muted())
            })
            .child(message)
            .when(
                self.operation_running && self.operation_cancel.is_some(),
                |status| {
                    status.child(
                        div()
                            .id("cancel-operation")
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .cursor_pointer()
                            .text_color(rgb(theme_text()))
                            .hover(|style| style.bg(rgb(surface_elevated())))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.cancel_operation(&CancelOperation, window, cx);
                            }))
                            .child("Cancel"),
                    )
                },
            )
            .when(!self.operation_running, |status| {
                status.child("Ctrl+Shift+C Copy Path · Del Trash · Ctrl+Z Undo")
            })
            .into_any_element()
    }

    fn render_workspace(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let right_panel = if self.operation_sheet.is_some() {
            Some(self.render_operation_sheet(cx))
        } else if self.preview_visible {
            Some(self.render_preview())
        } else {
            None
        };
        div()
            .size_full()
            .flex()
            .child(self.render_sidebar(cx))
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(self.render_header(cx))
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .child(self.render_file_list(cx))
                            .when_some(right_panel, gpui::ParentElement::child),
                    )
                    .child(self.render_status(cx)),
            )
            .into_any_element()
    }
}

impl Focusable for FileManager {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for FileManager {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("gnil-root")
            .relative()
            .key_context(
                if self.action_menu.is_some()
                    || self.empty_space_menu.is_some()
                    || self.appearance_menu_open
                {
                    "ActionMenu"
                } else {
                    match self.keymap {
                        KeymapProfile::Desktop => "FileManager",
                        KeymapProfile::Yazi => "YaziFileManager",
                    }
                },
            )
            .track_focus(&self.focus_handle(cx))
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_previous))
            .on_action(cx.listener(Self::select_next_range))
            .on_action(cx.listener(Self::select_previous_range))
            .on_action(cx.listener(Self::toggle_selection))
            .on_action(cx.listener(Self::open_selected))
            .on_action(cx.listener(Self::go_back))
            .on_action(cx.listener(Self::go_forward))
            .on_action(cx.listener(Self::go_up))
            .on_action(cx.listener(Self::toggle_preview))
            .on_action(cx.listener(Self::toggle_hidden))
            .on_action(cx.listener(Self::refresh))
            .on_action(cx.listener(Self::copy_selected))
            .on_action(cx.listener(Self::cut_selected))
            .on_action(cx.listener(Self::copy_path_absolute))
            .on_action(cx.listener(Self::copy_path_relative))
            .on_action(cx.listener(Self::toggle_actions))
            .on_action(cx.listener(Self::toggle_appearance))
            .on_action(cx.listener(Self::open_create_symlink))
            .on_action(cx.listener(Self::open_permissions))
            .on_action(cx.listener(Self::open_rename))
            .on_action(cx.listener(Self::extract_selected))
            .on_action(cx.listener(Self::extract_selected_to))
            .on_action(cx.listener(Self::cancel_operation))
            .on_action(cx.listener(Self::dismiss_sheet))
            .on_action(cx.listener(Self::apply_sheet))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::trash_selected))
            .on_action(cx.listener(Self::delete_selected))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::menu_next))
            .on_action(cx.listener(Self::menu_previous))
            .on_action(cx.listener(Self::menu_first))
            .on_action(cx.listener(Self::menu_last))
            .on_action(cx.listener(Self::menu_activate))
            .on_action(cx.listener(Self::menu_open_submenu))
            .on_action(cx.listener(Self::menu_close_submenu))
            .on_action(cx.listener(Self::dismiss_menu))
            .on_action(cx.listener(Self::create_folder))
            .on_action(cx.listener(Self::create_file))
            .on_action(cx.listener(Self::restore_trash_selected))
            .on_action(cx.listener(Self::empty_trash))
            .on_action(cx.listener(Self::activate_path_input))
            .on_action(cx.listener(Self::submit_path_input))
            .on_action(cx.listener(Self::dismiss_path_input))
            .on_action(cx.listener(Self::complete_path_next))
            .on_action(cx.listener(Self::complete_path_previous))
            .on_action(cx.listener(Self::path_history_previous))
            .on_action(cx.listener(Self::path_history_next))
            .on_action(cx.listener(Self::paste_path))
            .on_action(cx.listener(|this, _: &SelectAllEntries, _, cx| {
                this.select_all_entries(cx);
            }))
            .size_full()
            .flex()
            .bg(rgb(background()))
            .text_color(rgb(text_emphasized()))
            .font_family("Noto Sans")
            .child(self.render_workspace(cx))
            .when(
                self.action_menu.is_some()
                    || self.empty_space_menu.is_some()
                    || self.appearance_menu_open,
                |root| root.child(Self::render_context_menu_backdrop(cx)),
            )
            .when(self.empty_space_menu.is_some(), |root| {
                root.child(self.render_empty_space_menu(cx))
            })
            .when(
                matches!(
                    self.action_menu.as_ref().map(|menu| menu.placement),
                    Some(ActionMenuPlacement::Cursor(_))
                ),
                |root| root.child(self.render_context_action_menu(cx)),
            )
    }
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
    ] {
        if let Some(path) = path.filter(|path| path.exists()) {
            places.push((label.into(), path));
        }
    }
    places
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
        .child(label)
}

fn trash_entry_as_file_entry(entry: &TrashEntry) -> FileEntry {
    FileEntry {
        path: entry.reference.info_path.clone(),
        name: entry.name.clone(),
        kind: entry.kind,
        hidden: false,
        metadata: FileMetadata {
            len: entry.len,
            ..FileMetadata::default()
        },
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
        .child(img(file_icon_asset(entry)).size_5())
        .into_any_element()
}

fn file_icon_asset(entry: &FileEntry) -> &'static str {
    match entry.kind {
        FileKind::Directory if entry.metadata.readonly => "icons/folder-readonly.svg",
        FileKind::Directory => "icons/folder-closed.svg",
        FileKind::Symlink if entry.path.is_dir() => "icons/folder-symlink.svg",
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

fn git_label(status: Option<GitStatus>) -> &'static str {
    match status {
        Some(GitStatus::Modified) => "M",
        Some(GitStatus::Added) => "A",
        Some(GitStatus::Deleted) => "D",
        Some(GitStatus::Untracked) => "U",
        Some(GitStatus::Conflicted) => "!",
        None => "",
    }
}

fn git_color(status: Option<GitStatus>) -> gpui::Hsla {
    match status {
        Some(GitStatus::Added) => rgb(git_added()).into(),
        Some(GitStatus::Deleted | GitStatus::Conflicted) => rgb(git_deleted()).into(),
        Some(GitStatus::Modified) => rgb(git_modified()).into(),
        Some(GitStatus::Untracked) => rgb(git_untracked()).into(),
        None => rgb(border_focused()).into(),
    }
}

fn size_label(entry: &FileEntry) -> String {
    if entry.kind == FileKind::Directory {
        "—".into()
    } else {
        format_bytes(entry.metadata.len)
    }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut divisor = 1_u64;
    let mut unit = 0;
    while bytes / divisor >= 1024 && unit < UNITS.len() - 1 {
        divisor = divisor.saturating_mul(1024);
        unit += 1;
    }
    let whole = bytes / divisor;
    let decimal = (bytes % divisor).saturating_mul(10) / divisor;
    format!("{whole}.{decimal} {}", UNITS[unit])
}

fn modified_label(entry: &FileEntry) -> String {
    entry
        .metadata
        .modified_unix_ms
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

fn selected_theme_name(settings: &AppSettings, appearance: ThemeAppearance) -> &str {
    match appearance {
        ThemeAppearance::Light => &settings.light_theme,
        ThemeAppearance::Dark => &settings.dark_theme,
    }
}

fn open_main_window(initial_path: &Path, cx: &mut App) {
    let bounds = Bounds::centered(None, size(px(1180.0), px(760.0)), cx);
    let window = cx
        .open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some("gnil-fm".into()),
                    ..Default::default()
                }),
                app_id: Some("gnil-fm".into()),
                ..Default::default()
            },
            |window, cx| {
                let system_appearance = window_theme_appearance(window);
                cx.new(|cx| {
                    let mut manager = FileManager::new(initial_path, system_appearance, cx);
                    manager.load_directory(cx);
                    manager.refresh_devices(cx);
                    manager.schedule_device_monitor(cx);
                    manager
                })
            },
        )
        .expect("open gnil-fm window");
    window
        .update(cx, |manager, window, cx| {
            manager.appearance_subscription =
                Some(cx.observe_window_appearance(window, FileManager::system_appearance_changed));
            window.focus(&manager.focus_handle(cx));
        })
        .ok();
    cx.activate(true);
}

fn main() {
    if env::args().any(|argument| argument == "--version" || argument == "-V") {
        println!("gnil-fm {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let initial_path = initial_path();
    Application::new()
        .with_assets(Assets)
        .run(move |cx: &mut App| {
            text_input::bind_keys(cx);
            cx.bind_keys([
                KeyBinding::new("down", MenuNext, Some("ActionMenu")),
                KeyBinding::new("j", MenuNext, Some("ActionMenu")),
                KeyBinding::new("up", MenuPrevious, Some("ActionMenu")),
                KeyBinding::new("k", MenuPrevious, Some("ActionMenu")),
                KeyBinding::new("home", MenuFirst, Some("ActionMenu")),
                KeyBinding::new("end", MenuLast, Some("ActionMenu")),
                KeyBinding::new("enter", MenuActivate, Some("ActionMenu")),
                KeyBinding::new("space", MenuActivate, Some("ActionMenu")),
                KeyBinding::new("right", MenuOpenSubmenu, Some("ActionMenu")),
                KeyBinding::new("left", MenuCloseSubmenu, Some("ActionMenu")),
                KeyBinding::new("escape", DismissMenu, Some("ActionMenu")),
                KeyBinding::new("enter", SubmitPathInput, Some("PathInput")),
                KeyBinding::new("escape", DismissPathInput, Some("PathInput")),
                KeyBinding::new("tab", CompletePathNext, Some("PathInput")),
                KeyBinding::new("shift-tab", CompletePathPrevious, Some("PathInput")),
                KeyBinding::new("up", PathHistoryPrevious, Some("PathInput")),
                KeyBinding::new("down", PathHistoryNext, Some("PathInput")),
                KeyBinding::new("ctrl-v", PastePath, Some("PathInput")),
                KeyBinding::new("ctrl-l", ActivatePathInput, Some("PathInput")),
                KeyBinding::new("down", SelectNext, Some("FileManager")),
                KeyBinding::new("up", SelectPrevious, Some("FileManager")),
                KeyBinding::new("shift-down", SelectNextRange, Some("FileManager")),
                KeyBinding::new("shift-up", SelectPreviousRange, Some("FileManager")),
                KeyBinding::new("ctrl-space", ToggleSelection, Some("FileManager")),
                KeyBinding::new("enter", OpenSelected, Some("FileManager")),
                KeyBinding::new("space", TogglePreview, Some("FileManager")),
                KeyBinding::new("alt-left", GoBack, Some("FileManager")),
                KeyBinding::new("alt-right", GoForward, Some("FileManager")),
                KeyBinding::new("alt-up", GoUp, Some("FileManager")),
                KeyBinding::new("ctrl-h", ToggleHidden, Some("FileManager")),
                KeyBinding::new("ctrl-l", ActivatePathInput, Some("FileManager")),
                KeyBinding::new("f5", Refresh, Some("FileManager")),
                KeyBinding::new("ctrl-c", CopySelected, Some("FileManager")),
                KeyBinding::new("ctrl-x", CutSelected, Some("FileManager")),
                KeyBinding::new("ctrl-shift-c", CopyPathAbsolute, Some("FileManager")),
                KeyBinding::new("ctrl-alt-c", CopyPathRelative, Some("FileManager")),
                KeyBinding::new("ctrl-shift-l", OpenCreateSymlink, Some("FileManager")),
                KeyBinding::new("alt-enter", OpenPermissions, Some("FileManager")),
                KeyBinding::new("f2", OpenRename, Some("FileManager")),
                KeyBinding::new("ctrl-e", ExtractSelected, Some("FileManager")),
                KeyBinding::new("ctrl-shift-e", ExtractSelectedTo, Some("FileManager")),
                KeyBinding::new("ctrl-v", Paste, Some("FileManager")),
                KeyBinding::new("ctrl-a", SelectAllEntries, Some("FileManager")),
                KeyBinding::new("ctrl-shift-n", CreateFolder, Some("FileManager")),
                KeyBinding::new("delete", TrashSelected, Some("FileManager")),
                KeyBinding::new("shift-delete", DeleteSelected, Some("FileManager")),
                KeyBinding::new("ctrl-z", Undo, Some("FileManager")),
                KeyBinding::new("ctrl-shift-t", ToggleAppearance, Some("FileManager")),
                KeyBinding::new("ctrl-q", Quit, None),
                KeyBinding::new("j", SelectNext, Some("YaziFileManager")),
                KeyBinding::new("k", SelectPrevious, Some("YaziFileManager")),
                KeyBinding::new("space", ToggleSelection, Some("YaziFileManager")),
                KeyBinding::new("f3", TogglePreview, Some("YaziFileManager")),
                KeyBinding::new("l", OpenSelected, Some("YaziFileManager")),
                KeyBinding::new("h", GoUp, Some("YaziFileManager")),
                KeyBinding::new("ctrl-l", ActivatePathInput, Some("YaziFileManager")),
                KeyBinding::new("y", CopySelected, Some("YaziFileManager")),
                KeyBinding::new("x", CutSelected, Some("YaziFileManager")),
                KeyBinding::new("p", Paste, Some("YaziFileManager")),
                KeyBinding::new("ctrl-a", SelectAllEntries, Some("YaziFileManager")),
                KeyBinding::new("ctrl-shift-n", CreateFolder, Some("YaziFileManager")),
                KeyBinding::new("d", TrashSelected, Some("YaziFileManager")),
                KeyBinding::new("u", Undo, Some("YaziFileManager")),
                KeyBinding::new("ctrl-shift-t", ToggleAppearance, Some("YaziFileManager")),
                KeyBinding::new("ctrl-shift-l", OpenCreateSymlink, Some("YaziFileManager")),
                KeyBinding::new("alt-enter", OpenPermissions, Some("YaziFileManager")),
                KeyBinding::new("f2", OpenRename, Some("YaziFileManager")),
                KeyBinding::new("ctrl-e", ExtractSelected, Some("YaziFileManager")),
                KeyBinding::new("ctrl-shift-e", ExtractSelectedTo, Some("YaziFileManager")),
                KeyBinding::new("escape", DismissSheet, None),
                KeyBinding::new("ctrl-enter", ApplySheet, None),
            ]);
            cx.on_action(|_: &Quit, cx| cx.quit());
            open_main_window(&initial_path, cx);
        });
}

#[cfg(test)]
mod tests {
    use super::{place_is_active, resolve_theme_appearance};
    use gnil_core::{ThemeAppearance, ThemeMode};
    use std::path::Path;

    #[test]
    fn places_use_exact_path_matching() {
        let home = Path::new("/home/person");
        let downloads = Path::new("/home/person/Downloads");

        assert!(place_is_active(downloads, downloads));
        assert!(!place_is_active(downloads, home));
        assert!(!place_is_active(
            Path::new("/home/person/Downloads/subfolder"),
            downloads,
        ));
        assert!(place_is_active(home, home));
    }

    #[test]
    fn explicit_theme_mode_overrides_system_appearance() {
        assert_eq!(
            resolve_theme_appearance(ThemeMode::System, ThemeAppearance::Light),
            ThemeAppearance::Light
        );
        assert_eq!(
            resolve_theme_appearance(ThemeMode::Dark, ThemeAppearance::Light),
            ThemeAppearance::Dark
        );
        assert_eq!(
            resolve_theme_appearance(ThemeMode::Light, ThemeAppearance::Dark),
            ThemeAppearance::Light
        );
    }
}
