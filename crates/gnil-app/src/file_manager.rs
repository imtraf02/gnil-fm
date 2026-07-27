use std::{
    collections::{BTreeMap, HashMap, HashSet},
    env,
    fmt::Write as _,
    hash::{DefaultHasher, Hash, Hasher},
    ops::Range,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use crate::action_menu::{
    ActionMenuPlacement, ActionMenuState, FileMenuCommand, MenuAnimationState, MenuContext,
    MenuEntry, prepare_context_selection,
};
use crate::assets::Assets;
use crate::device_refresh::{DEVICE_MONITOR_INTERVAL, DeviceRefresh};
use crate::empty_space_menu::{
    EmptySpaceMenuActivation, EmptySpaceMenuCapabilities, EmptySpaceMenuCommand,
    EmptySpaceMenuContext, EmptySpaceMenuEntry, EmptySpaceMenuState, EmptySpaceViewState,
};
use crate::path_input::{
    PathInputState, PathSuggestion, PathTarget, completion_candidates, resolve_path_input,
    single_pasted_path, validate_path,
};
use crate::pointer_interaction::{
    ActiveFileDrag, DropIntent, DropTarget, FileDragPayload, PointerInteraction, RUBBER_EDGE_ZONE,
    RowPressState, RubberBandState, RubberFrameGate, band_span, endpoint_index,
    external_drop_intent, internal_drop_intent, merge_from_modifiers, movement_crossed_threshold,
    rubber_highlighted,
};
use crate::text_input::{self, TextInput, TextInputEvent};
use crate::theme_runtime::{
    self, accent, accent_background, accent_hover, background, border, border_focused,
    danger as danger_color, error as error_color, git_added, git_deleted, git_modified,
    git_untracked, surface, surface_elevated, text as theme_text, text_emphasized, text_muted,
};
use crate::ui::control::FocusableControl;
use crate::ui::icon::{IconSize, IconTone, ui_icon};
use crate::ui::overlay::{
    APPEARANCE_PRIORITY, BACKDROP_PRIORITY, MENU_PRIORITY, MODAL_PRIORITY, OverlayMotionState,
    PATH_MENU_PRIORITY, animate_overlay,
};

use chrono::{DateTime, Local};
use gnil_clipboard::{
    FileClipboard, FileClipboardMode, GNOME_FILES_MIME, TEXT_MIME, URI_LIST_MIME,
    decode_file_clipboard, encode_file_clipboard,
};
use gnil_core::{
    AppSettings, ConfigPaths, ConflictDecision, DirectoryLoadPhase, DirectorySnapshot,
    EntryMetadata, FileEntry, FileKind, FileMetadata, FsOperation, GitStatus, JobProgress,
    KeymapProfile, PermissionChange, RenamePair, SelectionState, SortDirection, SortField, TabRoot,
    TabState, ThemeAppearance, ThemeCatalog, ThemeMode, TrashEntryRef, UndoRecord,
};
use gnil_fs::{
    DeviceEntry, DeviceKind, DirectoryWatcher, OperationExecutor, ScanOptions, TrashEntry,
    WatchEvent, eject_device, mount_device, scan_devices, scan_directory_progressive, scan_trash,
    unmount_device,
};
use gnil_preview::{PreviewCache, PreviewError, PreviewRequest, PreviewResult, PreviewService};
use gpui::{
    AnchoredPositionMode, Animation, AnimationExt as _, AnyElement, AnyView, App, Application,
    Bounds, ClickEvent, ClipboardItem, Context, Corner, CursorStyle, Div, DragMoveEvent, Entity,
    ExternalPaths, FocusHandle, Focusable, Hsla, KeyBinding, ModifiersChangedEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, PromptLevel, Render, ScrollStrategy,
    SharedString, Stateful, StyleRefinement, Subscription, UniformListScrollHandle, Window,
    WindowAppearance, WindowBounds, WindowOptions, actions, anchored, deferred, div, img, point,
    prelude::*, px, relative, rgb, size, uniform_list,
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
        CancelPointerInteraction,
        ActivatePathInput,
        SubmitPathInput,
        DismissPathInput,
        CompletePathNext,
        CompletePathPrevious,
        PathHistoryPrevious,
        PathHistoryNext,
        PastePath,
        ToggleSettings,
        DismissSettings,
        Quit,
    ]
);

const FILE_GIT_COLUMN_WIDTH: f32 = 24.0;
const FILE_SIZE_COLUMN_WIDTH: f32 = 92.0;
const FILE_MODIFIED_COLUMN_WIDTH: f32 = 132.0;
const FILE_ROW_HEIGHT: f32 = 36.0;
const TRASH_ROW_HEIGHT: f32 = 42.0;

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

struct DragGhost {
    payload: FileDragPayload,
    cursor_offset: gpui::Point<Pixels>,
}

#[derive(Default)]
struct DragPayloadCache {
    single: HashMap<PathBuf, FileDragPayload>,
    single_snapshot: Option<std::sync::Weak<DirectorySnapshot>>,
    selected: HashMap<PathBuf, FileDragPayload>,
    selected_revision: u64,
    selected_generation: u64,
}

impl Render for DragGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let lifted = self.payload.visual.lifted();
        let copy = self.payload.visual.copy();
        let count = self.payload.paths.len();
        div()
            .pl(self.cursor_offset.x - px(12.0))
            .pt(self.cursor_offset.y - px(18.0))
            .opacity(if lifted { 1.0 } else { 0.0 })
            .child(
                div()
                    .h(px(38.0))
                    .max_w(px(236.0))
                    .px_2()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(if copy { accent() } else { border_focused() }))
                    .bg(rgb(surface_elevated()))
                    .shadow_md()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .text_color(rgb(text_emphasized()))
                    .child(ui_icon(
                        self.payload.first_icon,
                        IconSize::Compact,
                        IconTone::Default,
                    ))
                    .child(
                        div()
                            .max_w(px(150.0))
                            .truncate()
                            .child(self.payload.first_name.clone()),
                    )
                    .when(count > 1, |ghost| {
                        ghost.child(
                            div()
                                .h_5()
                                .min_w(px(24.0))
                                .px_1()
                                .rounded_full()
                                .bg(rgb(accent_background()))
                                .flex()
                                .items_center()
                                .justify_center()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child(count.to_string()),
                        )
                    })
                    .when(copy, |ghost| {
                        ghost.child(div().text_color(rgb(accent())).child("Copy"))
                    }),
            )
    }
}

#[allow(clippy::struct_excessive_bools)]
struct FileManager {
    focus_handle: FocusHandle,
    surfaces: FileManagerSurfaces,
    _surface_subscription: Subscription,
    perf_trace: PerfTrace,
    tab: TabState,
    snapshot: Arc<DirectorySnapshot>,
    selection: SelectionState,
    selected_paths_cache: Arc<[PathBuf]>,
    selected_paths_cache_revision: u64,
    selected_paths_cache_generation: u64,
    drag_payload_cache: DragPayloadCache,
    preview: Option<PreviewResult>,
    preview_path: Option<PathBuf>,
    preview_request_path: Option<PathBuf>,
    preview_generation: u64,
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
    operation_sheet_closing: bool,
    path_input: PathInputState,
    _path_input_subscription: Subscription,
    pending_reveal: Option<PathBuf>,
    file_list_scroll: UniformListScrollHandle,
    rendered_file_range: Range<usize>,
    pointer_interaction: PointerInteraction,
    pointer_serial: u64,
    reduced_motion: bool,
    git_status_enabled: bool,
    trash_entries: Vec<TrashEntry>,
    devices: Vec<DeviceEntry>,
    device_refresh: DeviceRefresh,
    device_scan_error: Option<String>,
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
    settings_open: bool,
    settings_closing: bool,
    settings_category: SettingsCategory,
    scan_cancel: Option<Arc<AtomicBool>>,
    preview_cancel: Option<Arc<AtomicBool>>,
    preview_service: Arc<PreviewService>,
    preview_cache: Arc<Mutex<PreviewCache>>,
    directory_watcher: Option<DirectoryWatcher>,
    watched_path: Option<PathBuf>,
    watcher_polling: bool,
    watcher_refresh_at: Option<Instant>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum SettingsCategory {
    #[default]
    Appearance,
    Keymap,
    FileView,
    Performance,
}

impl FileManager {
    fn create_path_input(path: &Path, cx: &mut Context<Self>) -> (Entity<TextInput>, Subscription) {
        let input = cx.new(|cx| {
            TextInput::new("Enter a path", path.display().to_string(), cx)
                .with_key_context("PathInput")
        });
        let subscription = cx.subscribe(&input, |this, input, event: &TextInputEvent, cx| {
            if matches!(event, TextInputEvent::Changed)
                && this.path_input.editing
                && this.path_input.input_changed()
            {
                input.update(cx, |input, cx| input.set_invalid(false, cx));
                cx.notify();
            }
        });
        (input, subscription)
    }

    fn new(path: &Path, system_appearance: ThemeAppearance, cx: &mut Context<Self>) -> Self {
        let config_paths = ConfigPaths::discover();
        let settings = config_paths.load_settings().unwrap_or_default();
        let theme_catalog = ThemeCatalog::load(&config_paths.themes_dir());
        let theme_appearance = resolve_theme_appearance(settings.theme, system_appearance);
        let requested_theme = selected_theme_name(&settings, theme_appearance);
        let (active_theme, _) = theme_catalog.resolve(requested_theme, theme_appearance);
        let active_theme_name = active_theme.name.clone();
        let memory_cache_mib = settings.memory_cache_mib;
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
            phase: DirectoryLoadPhase::Complete,
        };
        let (path_input, path_input_subscription) = Self::create_path_input(path, cx);
        let (surfaces, surface_subscription) = FileManagerSurfaces::install(cx);
        Self {
            focus_handle: cx.focus_handle(),
            surfaces,
            _surface_subscription: surface_subscription,
            perf_trace: PerfTrace::from_env(),
            tab,
            snapshot: Arc::new(snapshot),
            selection: SelectionState::default(),
            selected_paths_cache: Arc::from([]),
            selected_paths_cache_revision: 0,
            selected_paths_cache_generation: 0,
            drag_payload_cache: DragPayloadCache::default(),
            preview: None,
            preview_path: None,
            preview_request_path: None,
            preview_generation: 0,
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
            operation_sheet_closing: false,
            path_input: PathInputState::new(path_input, path.to_path_buf()),
            _path_input_subscription: path_input_subscription,
            pending_reveal: None,
            file_list_scroll: UniformListScrollHandle::new(),
            rendered_file_range: 0..0,
            pointer_interaction: PointerInteraction::Idle,
            pointer_serial: 0,
            reduced_motion: settings.reduced_motion,
            git_status_enabled: settings.git_status_enabled,
            trash_entries: Vec::new(),
            devices: Vec::new(),
            device_refresh: DeviceRefresh::new(),
            device_scan_error: None,
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
            settings_open: false,
            settings_closing: false,
            settings_category: SettingsCategory::Appearance,
            scan_cancel: None,
            preview_cancel: None,
            preview_service: Arc::new(PreviewService::default()),
            preview_cache: Arc::new(Mutex::new(PreviewCache::new(memory_cache_mib))),
            directory_watcher: None,
            watched_path: None,
            watcher_polling: false,
            watcher_refresh_at: None,
        }
    }
}

include!("file_manager/loading.rs");
include!("file_manager/perf_trace.rs");
include!("file_manager/rubber_band.rs");
include!("file_manager/interaction.rs");
include!("file_manager/operations.rs");
include!("file_manager/view_surface.rs");
include!("file_manager/view_sidebar.rs");
include!("file_manager/view_header.rs");
include!("file_manager/view_appearance.rs");
include!("file_manager/view_menus.rs");
include!("file_manager/view_operation_sheet.rs");
include!("file_manager/view_settings.rs");
include!("file_manager/view_lists.rs");
include!("file_manager/view_trash.rs");
include!("file_manager/view_workspace.rs");
include!("file_manager/helpers_core.rs");
include!("file_manager/helpers_ui.rs");
include!("file_manager/helpers_window.rs");

pub fn run() {
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
                KeyBinding::new(
                    "escape",
                    CancelPointerInteraction,
                    Some("PointerInteraction"),
                ),
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
                KeyBinding::new("ctrl-,", ToggleSettings, Some("FileManager")),
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
                KeyBinding::new("ctrl-,", ToggleSettings, Some("YaziFileManager")),
                KeyBinding::new("escape", DismissSheet, None),
                KeyBinding::new("escape", DismissSettings, None),
                KeyBinding::new("ctrl-enter", ApplySheet, None),
            ]);
            cx.on_action(|_: &Quit, cx| cx.quit());
            open_main_window(&initial_path, cx);
        });
}

#[cfg(test)]
mod tests {
    use super::{FileManager, PointerInteraction, place_is_active, resolve_theme_appearance};
    use gnil_core::{
        DirectoryLoadPhase, DirectorySnapshot, EntryMetadata, FileEntry, FileKind, ThemeAppearance,
        ThemeMode,
    };
    use gpui::{Modifiers, MouseButton, TestAppContext, point, px};
    use std::{
        path::{Path, PathBuf},
        sync::Arc,
    };

    fn test_snapshot(entry_count: usize) -> Arc<DirectorySnapshot> {
        Arc::new(DirectorySnapshot {
            generation: 1,
            path: PathBuf::from("/tmp"),
            entries: (0..entry_count)
                .map(|index| FileEntry {
                    path: PathBuf::from(format!("/tmp/file-{index}.txt")),
                    name: format!("file-{index}.txt"),
                    kind: FileKind::File,
                    hidden: false,
                    metadata: EntryMetadata::Pending,
                    git_status: None,
                })
                .collect(),
            unreadable_entries: 0,
            phase: DirectoryLoadPhase::Complete,
        })
    }

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

    #[gpui::test]
    fn rubber_band_mouse_move_uses_a_legal_frame_callback(cx: &mut TestAppContext) {
        let (manager, cx) = cx.add_window_view(|_, cx| {
            let mut manager = FileManager::new(Path::new("/tmp"), ThemeAppearance::Dark, cx);
            manager.snapshot = test_snapshot(2);
            manager
        });
        let viewport = cx.read(|cx| {
            manager
                .read(cx)
                .file_list_scroll
                .0
                .borrow()
                .base_handle
                .bounds()
        });
        let origin = point(viewport.left() + px(20.0), viewport.bottom() - px(20.0));
        let current = point(origin.x + px(24.0), origin.y - px(24.0));

        cx.simulate_mouse_down(origin, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(current, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(current, MouseButton::Left, Modifiers::none());

        assert!(cx.read(|cx| matches!(
            manager.read(cx).pointer_interaction,
            PointerInteraction::Idle
        )));
    }

    #[gpui::test]
    fn row_hover_does_not_render_cached_sibling_surfaces(cx: &mut TestAppContext) {
        let (manager, cx) = cx.add_window_view(|_, cx| {
            let mut manager = FileManager::new(Path::new("/tmp"), ThemeAppearance::Dark, cx);
            manager.snapshot = test_snapshot(2);
            manager
        });
        let viewport = cx.read(|cx| {
            manager
                .read(cx)
                .file_list_scroll
                .0
                .borrow()
                .base_handle
                .bounds()
        });
        let (sidebar, file_list) = cx.read(|cx| {
            let manager = manager.read(cx);
            (
                manager.surfaces.sidebar.clone(),
                manager.surfaces.file_list.clone(),
            )
        });
        let sidebar_before = cx.read(|cx| sidebar.read(cx).render_count);
        let list_before = cx.read(|cx| file_list.read(cx).render_count);

        cx.simulate_mouse_move(
            point(viewport.left() + px(30.0), viewport.top() + px(18.0)),
            None,
            Modifiers::none(),
        );
        cx.simulate_mouse_move(
            point(viewport.left() + px(30.0), viewport.top() + px(54.0)),
            None,
            Modifiers::none(),
        );

        assert_eq!(cx.read(|cx| sidebar.read(cx).render_count), sidebar_before);
        assert!(cx.read(|cx| file_list.read(cx).render_count) > list_before);
    }

    #[gpui::test]
    fn file_list_uses_exact_space_left_by_preview(cx: &mut TestAppContext) {
        let (manager, cx) = cx.add_window_view(|_, cx| {
            let mut manager = FileManager::new(Path::new("/tmp"), ThemeAppearance::Dark, cx);
            manager.snapshot = test_snapshot(2);
            manager.preview_visible = true;
            manager
        });

        let list_with_preview = cx
            .debug_bounds("file-list-surface-frame")
            .expect("file-list surface should be laid out");
        let preview = cx
            .debug_bounds("preview-surface")
            .expect("preview surface should be laid out");
        assert_eq!(list_with_preview.right(), preview.left());

        manager.update(cx, |manager, cx| {
            manager.preview_visible = false;
            cx.notify();
        });
        cx.run_until_parked();

        let expanded_list = cx
            .debug_bounds("file-list-surface-frame")
            .expect("expanded file-list surface should be laid out");
        assert_eq!(expanded_list.right(), preview.right());
        assert_eq!(
            expanded_list.size.width - list_with_preview.size.width,
            preview.size.width
        );
    }
}
