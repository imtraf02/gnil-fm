use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
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
    ActionMenuActivation, ActionMenuPlacement, ActionMenuState, FileMenuCommand, FileMenuSubmenu,
    MenuAnimationState, MenuContext, MenuEntry, MenuFocusTarget, MenuToolbarAction,
    prepare_context_selection,
};
use crate::assets::Assets;
use crate::device_refresh::{DEVICE_MONITOR_INTERVAL, DeviceRefresh};
use crate::empty_space_menu::{
    EmptySpaceMenuActivation, EmptySpaceMenuCapabilities, EmptySpaceMenuCommand,
    EmptySpaceMenuContext, EmptySpaceMenuEntry, EmptySpaceMenuState, EmptySpaceSubmenu,
    EmptySpaceViewState,
};
use crate::open_with::{
    DesktopApplication, OpenWithCatalog, discover_applications, filter_applications,
    launch_application, set_default_application,
};
use crate::path_input::{
    PathInputState, PathSuggestion, PathTarget, completion_candidates, resolve_path_input,
    single_pasted_path, validate_path,
};
use crate::pointer_interaction::{
    ActiveFileDrag, DropIntent, DropTarget, FileDragPayload, PointerInteraction, RUBBER_EDGE_ZONE,
    RowPressState, RubberBandState, RubberFrameGate, band_span, endpoint_index,
    external_drop_intent, grid_band_indices, internal_drop_intent, merge_from_modifiers,
    movement_crossed_threshold, rubber_highlighted,
};
use crate::text_input::{self, TextInput, TextInputEvent};
use crate::theme_runtime::{
    self, accent, accent_background, accent_hover, background, border, border_focused,
    danger as danger_color, error as error_color, surface, surface_elevated, text as theme_text,
    text_emphasized, text_muted,
};
use crate::ui::control::FocusableControl;
use crate::ui::icon::{IconSize, IconTone, ui_icon};
use crate::ui::overlay::{
    APPEARANCE_PRIORITY, MENU_PRIORITY, MODAL_PRIORITY, OverlayMotionState, PATH_MENU_PRIORITY,
    animate_overlay,
};

use chrono::{DateTime, Local};
use gnil_clipboard::{
    FileClipboard, FileClipboardMode, GNOME_FILES_MIME, TEXT_MIME, URI_LIST_MIME,
    decode_file_clipboard, encode_file_clipboard,
};
use gnil_core::{
    ActionId, AppSettings, ConfigPaths, ConflictDecision, DirectoryLoadPhase, DirectorySnapshot,
    EntryMetadata, FileEntry, FileKind, FileLayout, FileMetadata, FsOperation, JobProgress,
    KeymapOverrides, PermissionChange, RenamePair, SelectionState, SortDirection, SortField,
    TabRoot, TabState, ThemeAppearance, ThemeCatalog, ThemeMode, TrashEntryRef, UndoRecord,
    natural_cmp,
};
use gnil_fs::{
    DeviceEntry, DeviceKind, DirectoryWatcher, OperationExecutor, ScanOptions, SearchOptions,
    TrashEntry, WatchEvent, eject_device, mount_device, scan_devices, scan_directory_progressive,
    scan_entry, scan_trash, search_paths, unmount_device,
};
use gnil_preview::{
    PreviewCache, PreviewError, PreviewRequest, PreviewResult, PreviewService, ThumbnailRequest,
    ThumbnailService,
};
use gpui::{
    AnchoredPositionMode, Animation, AnimationExt as _, AnyElement, AnyView, App, Application,
    Bounds, ClickEvent, ClipboardItem, Context, Corner, CursorStyle, Div, DragMoveEvent, Entity,
    ExternalPaths, FocusHandle, Focusable, Global, Hsla, KeyBinding, ModifiersChangedEvent,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, PromptLevel, Render,
    ScrollStrategy, SharedString, Stateful, StyleRefinement, Subscription, UniformListScrollHandle,
    Window, WindowAppearance, WindowBounds, WindowOptions, actions, anchored, deferred, div, img,
    point, prelude::*, px, relative, rgb, size, uniform_list,
};

actions!(
    gnil,
    [
        SelectNext,
        SelectPrevious,
        SelectLeft,
        SelectRight,
        SelectNextRange,
        SelectPreviousRange,
        SelectLeftRange,
        SelectRightRange,
        ToggleSelection,
        OpenSelected,
        OpenWithSelected,
        ConfirmOpenWith,
        DismissOpenWith,
        OpenWithNext,
        OpenWithPrevious,
        ToggleOpenWithDefault,
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
        ConfirmInlineRename,
        CancelInlineRename,
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
        HandleEscape,
        CancelPointerInteraction,
        ActivatePathInput,
        SubmitPathInput,
        DismissPathInput,
        CompletePathNext,
        CompletePathPrevious,
        PathHistoryPrevious,
        PathHistoryNext,
        PastePath,
        ActivateFileSearch,
        DismissFileSearch,
        FileSearchNext,
        FileSearchPrevious,
        OpenFileSearchResult,
        ToggleFavorite,
        ToggleSettings,
        OpenKeymap,
        FocusSettingsSearch,
        DismissSettings,
        Quit,
    ]
);

const FILE_FAVORITE_ACTION_WIDTH: f32 = 24.0;
const FILE_ROW_HEIGHT: f32 = 36.0;
const TRASH_ROW_HEIGHT: f32 = 42.0;
const GRID_ROW_HEIGHT: f32 = 190.0;
const GRID_CARD_TARGET_WIDTH: f32 = 160.0;
const GRID_CARD_GAP: f32 = 12.0;
const GRID_HORIZONTAL_PADDING: f32 = 12.0;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FileManagerOpenRequest {
    pub(crate) directory: PathBuf,
    pub(crate) reveal: Vec<PathBuf>,
    pub(crate) show_properties: bool,
}

impl FileManagerOpenRequest {
    pub(crate) fn new(directory: PathBuf, reveal: Vec<PathBuf>, show_properties: bool) -> Self {
        Self {
            directory,
            reveal,
            show_properties,
        }
    }

    pub(crate) fn browse(directory: PathBuf) -> Self {
        Self::new(directory, Vec::new(), false)
    }
}

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DetailColumn {
    Name,
    Modified,
    Kind,
    Size,
    TrashName,
    TrashDeleted,
    TrashLocation,
}

#[derive(Clone, Copy, Debug)]
struct ColumnResizeState {
    column: DetailColumn,
    origin_x: f32,
    initial_width: f32,
}

struct FileSearchState {
    input: Entity<TextInput>,
    root: PathBuf,
    home_root: PathBuf,
    scope: FileSearchScope,
    open: bool,
    closing: bool,
    loading: bool,
    generation: u64,
    cancel: Option<Arc<AtomicBool>>,
    results: Vec<FileEntry>,
    focused: Option<usize>,
    error: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum FileSearchScope {
    #[default]
    CurrentFolder,
    Home,
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

struct InlineRenameState {
    path: PathBuf,
    input: Entity<TextInput>,
    _input_subscription: Subscription,
    _blur_subscription: Subscription,
}

struct OpenWithChooserState {
    path: PathBuf,
    file_name: String,
    mime_type: String,
    catalog: Option<OpenWithCatalog>,
    search: Entity<TextInput>,
    _search_subscription: Subscription,
    selected_desktop_id: Option<String>,
    always_use: bool,
    loading: bool,
    error: Option<String>,
    motion: OverlayMotionState,
    serial: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum GridPreviewState {
    Loading,
    Thumbnail(PathBuf),
    Text(Vec<String>),
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct GridPreviewRequest {
    key: PathBuf,
    source: PathBuf,
}

struct DragGhost {
    payload: FileDragPayload,
    cursor_offset: gpui::Point<Pixels>,
}

#[derive(Clone)]
struct FavoriteDragPayload {
    path: PathBuf,
    label: String,
}

struct FavoriteDragGhost {
    label: String,
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

impl Render for FavoriteDragGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .pl(self.cursor_offset.x - px(12.0))
            .pt(self.cursor_offset.y - px(17.0))
            .child(
                div()
                    .h(px(34.0))
                    .max_w(px(204.0))
                    .px_2()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(border_focused()))
                    .bg(rgb(surface_elevated()))
                    .shadow_md()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .text_color(rgb(text_emphasized()))
                    .child(ui_icon(
                        "icons/folder-favorite.svg",
                        IconSize::Compact,
                        IconTone::Accent,
                    ))
                    .child(div().max_w(px(150.0)).truncate().child(self.label.clone())),
            )
    }
}

#[allow(clippy::struct_excessive_bools)]
struct FileManager {
    focus_handle: FocusHandle,
    surfaces: FileManagerSurfaces,
    perf_trace: PerfTrace,
    tab: TabState,
    snapshot: Arc<DirectorySnapshot>,
    selection: SelectionState,
    selected_paths_cache: Arc<[PathBuf]>,
    selected_paths_cache_revision: u64,
    selected_paths_cache_generation: u64,
    drag_payload_cache: DragPayloadCache,
    favorite_availability: HashMap<PathBuf, bool>,
    favorite_paths: Arc<HashSet<PathBuf>>,
    preview: Option<PreviewResult>,
    preview_path: Option<PathBuf>,
    preview_request_path: Option<PathBuf>,
    preview_generation: u64,
    preview_debounce_generation: u64,
    preview_visible: bool,
    loading: bool,
    error: Option<String>,
    generation: u64,
    places: Vec<(String, PathBuf)>,
    undo_stack: Vec<UndoRecord>,
    operation_running: bool,
    operation_cancel: Option<Arc<AtomicBool>>,
    operation_progress: Option<JobProgress>,
    operation_progress_rx: Option<crossbeam_channel::Receiver<JobProgress>>,
    status_message: Option<String>,
    clipboard: Option<FileClipboard>,
    action_menu: Option<ActionMenuState>,
    action_menu_serial: u64,
    open_with_catalogs: HashMap<String, OpenWithCatalog>,
    open_with_chooser: Option<OpenWithChooserState>,
    open_with_serial: u64,
    empty_space_menu: Option<EmptySpaceMenuState>,
    empty_space_menu_serial: u64,
    operation_sheet: Option<OperationSheet>,
    operation_sheet_closing: bool,
    inline_rename: Option<InlineRenameState>,
    path_input: PathInputState,
    _path_input_subscription: Subscription,
    file_search: FileSearchState,
    _file_search_subscription: Subscription,
    pending_reveal: Vec<PathBuf>,
    pending_show_properties: bool,
    file_list_scroll: UniformListScrollHandle,
    rendered_file_range: Range<usize>,
    grid_columns: usize,
    grid_card_width: f32,
    grid_previews: HashMap<PathBuf, GridPreviewState>,
    grid_preview_queue: VecDeque<GridPreviewRequest>,
    grid_preview_inflight: HashSet<PathBuf>,
    grid_preview_generation: u64,
    grid_preview_cancel: Arc<AtomicBool>,
    pointer_interaction: PointerInteraction,
    pointer_serial: u64,
    column_resize: Option<ColumnResizeState>,
    reduced_motion: bool,
    trash_entries: Arc<Vec<TrashEntry>>,
    devices: Vec<DeviceEntry>,
    device_refresh: DeviceRefresh,
    device_scan_error: Option<String>,
    auto_mount_removable: bool,
    auto_mount_attempted: HashSet<String>,
    config_paths: ConfigPaths,
    preferences: Entity<PreferencesState>,
    preferences_subscription: Option<Subscription>,
    settings: AppSettings,
    theme_catalog: ThemeCatalog,
    theme_appearance: ThemeAppearance,
    active_theme_name: String,
    appearance_menu_open: bool,
    appearance_menu_closing: bool,
    command_bar_menu: Option<CommandBarMenuState>,
    command_bar_menu_serial: u64,
    command_bar_width_tier: CommandBarWidthTier,
    appearance_subscription: Option<Subscription>,
    scan_cancel: Option<Arc<AtomicBool>>,
    preview_cancel: Option<Arc<AtomicBool>>,
    preview_service: Arc<PreviewService>,
    preview_cache: Arc<Mutex<PreviewCache>>,
    thumbnail_service: Arc<ThumbnailService>,
    directory_watcher: Option<DirectoryWatcher>,
    watched_path: Option<PathBuf>,
    watcher_refresh_at: Option<Instant>,
    pending_watch_changes: PendingWatchChanges,
    watcher_apply_running: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum SettingsCategory {
    #[default]
    Appearance,
    Keymap,
    FileView,
    Performance,
}

struct PreferencesState {
    config_paths: ConfigPaths,
    settings: AppSettings,
    keymap: KeymapOverrides,
    keymap_error: Option<String>,
    migrated_legacy_keymap: bool,
}

impl PreferencesState {
    fn load(config_paths: ConfigPaths) -> Self {
        let loaded = config_paths
            .load_settings_with_migration()
            .unwrap_or_else(|_| gnil_core::LoadedSettings {
                settings: AppSettings::default(),
                migrated_legacy_keymap: false,
            });
        let (keymap, keymap_error) = match config_paths.load_keymap() {
            Ok(keymap) => {
                let error = PreferencesWindow::keymap_validation_message(&keymap);
                (keymap, error)
            }
            Err(error) => (
                KeymapOverrides::default(),
                Some(format!("Could not load keymap: {error}")),
            ),
        };
        Self {
            config_paths,
            settings: loaded.settings,
            keymap,
            keymap_error,
            migrated_legacy_keymap: loaded.migrated_legacy_keymap,
        }
    }

    fn save_settings(&mut self, settings: AppSettings) -> Result<(), String> {
        self.settings = settings;
        self.settings.normalize();
        self.config_paths
            .save_settings(&self.settings)
            .map_err(|error| error.to_string())
    }

    fn save_keymap(&mut self, mut keymap: KeymapOverrides) -> Result<(), String> {
        keymap.normalize();
        self.config_paths
            .save_keymap(&keymap)
            .map_err(|error| error.to_string())?;
        self.keymap_error = PreferencesWindow::keymap_validation_message(&keymap);
        self.keymap = keymap;
        Ok(())
    }
}

struct PreferencesRegistry {
    state: Entity<PreferencesState>,
    window: Option<gpui::WindowHandle<PreferencesWindow>>,
}

impl Global for PreferencesRegistry {}

struct PreferencesWindow {
    focus_handle: FocusHandle,
    preferences: Entity<PreferencesState>,
    settings: AppSettings,
    config_paths: ConfigPaths,
    keymap_overrides: KeymapOverrides,
    keymap_error: Option<String>,
    settings_category: SettingsCategory,
    settings_search: Entity<TextInput>,
    _settings_search_subscription: Subscription,
    keymap_search: Entity<TextInput>,
    _keymap_search_subscription: Subscription,
    keymap_capture_subscription: Option<Subscription>,
    keymap_edit: Option<KeymapEditState>,
    keymap_conflict: Option<KeymapConflictState>,
    preferences_subscription: Option<Subscription>,
    reduced_motion: bool,
}

fn preferences_state(cx: &mut App) -> Entity<PreferencesState> {
    if !cx.has_global::<PreferencesRegistry>() {
        let config_paths = ConfigPaths::discover();
        let state = cx.new(|_| PreferencesState::load(config_paths));
        cx.set_global(PreferencesRegistry {
            state: state.clone(),
            window: None,
        });
    }
    cx.global::<PreferencesRegistry>().state.clone()
}

fn open_preferences_window(category: SettingsCategory, cx: &mut App) {
    let preferences = preferences_state(cx);
    if let Some(handle) = cx.global::<PreferencesRegistry>().window
        && handle
            .update(cx, |settings, window, cx| {
                settings.settings_category = category;
                settings
                    .settings_search
                    .update(cx, |input, cx| input.set_text("", cx));
                window.activate_window();
                window.focus(&settings.focus_handle(cx));
            })
            .is_ok()
    {
        return;
    }
    cx.global_mut::<PreferencesRegistry>().window = None;

    let bounds = Bounds::centered(None, size(px(1000.0), px(720.0)), cx);
    match cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(780.0), px(560.0))),
            window_background: gpui::WindowBackgroundAppearance::Opaque,
            titlebar: Some(gpui::TitlebarOptions {
                title: Some("gnil Settings".into()),
                ..Default::default()
            }),
            app_id: Some("gnil-fm".into()),
            ..Default::default()
        },
        |window, cx| {
            cx.new(|cx| {
                let mut preferences_window = PreferencesWindow::new(&preferences, cx);
                preferences_window.settings_category = category;
                window.focus(&preferences_window.focus_handle(cx));
                preferences_window
            })
        },
    ) {
        Ok(handle) => {
            cx.global_mut::<PreferencesRegistry>().window = Some(handle);
            cx.activate(true);
        }
        Err(error) => {
            eprintln!("Could not open preferences window: {error}");
        }
    }
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

    fn create_file_search(cx: &mut Context<Self>) -> (Entity<TextInput>, Subscription) {
        let input = cx.new(|cx| {
            TextInput::new("Search files", "", cx)
                .with_key_context("SearchInput")
                .with_leading_icon("icons/action-search.svg")
                .with_trailing_action_space()
        });
        let subscription = cx.subscribe(&input, |this, input, event: &TextInputEvent, cx| {
            if matches!(event, TextInputEvent::Changed) && this.file_search.open {
                let query = input.read(cx).text().to_owned();
                this.update_file_search(&query, cx);
            }
        });
        (input, subscription)
    }

    #[allow(clippy::too_many_lines)]
    fn new(path: &Path, system_appearance: ThemeAppearance, cx: &mut Context<Self>) -> Self {
        let preferences = preferences_state(cx);
        let (config_paths, settings, keymap, migrated_legacy_keymap) = {
            let preference_snapshot = preferences.read(cx);
            (
                preference_snapshot.config_paths.clone(),
                preference_snapshot.settings.clone(),
                preference_snapshot.keymap.clone(),
                preference_snapshot.migrated_legacy_keymap,
            )
        };
        let theme_catalog = ThemeCatalog::load(&config_paths.themes_dir());
        let theme_appearance = resolve_theme_appearance(settings.theme, system_appearance);
        let requested_theme = selected_theme_name(&settings, theme_appearance);
        let (active_theme, _) = theme_catalog.resolve(requested_theme, theme_appearance);
        let active_theme_name = active_theme.name.clone();
        let memory_cache_mib = settings.memory_cache_mib;
        theme_runtime::set_active(active_theme.colors);
        let tab = TabState {
            show_hidden: settings.show_hidden,
            sort: settings.file_sort,
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
        let (file_search_input, file_search_subscription) = Self::create_file_search(cx);
        let surfaces = FileManagerSurfaces::install(cx);
        let favorite_availability = favorite_availability(&settings.favorites);
        let favorite_paths = Arc::new(settings.favorites.iter().cloned().collect());
        let mut manager = Self {
            focus_handle: cx.focus_handle(),
            surfaces,
            perf_trace: PerfTrace::from_env(),
            tab,
            snapshot: Arc::new(snapshot),
            selection: SelectionState::default(),
            selected_paths_cache: Arc::from([]),
            selected_paths_cache_revision: 0,
            selected_paths_cache_generation: 0,
            drag_payload_cache: DragPayloadCache::default(),
            favorite_availability,
            favorite_paths,
            preview: None,
            preview_path: None,
            preview_request_path: None,
            preview_generation: 0,
            preview_debounce_generation: 0,
            preview_visible: settings.preview_enabled,
            loading: false,
            error: None,
            generation: 0,
            places: places(),
            undo_stack: Vec::new(),
            operation_running: false,
            operation_cancel: None,
            operation_progress: None,
            operation_progress_rx: None,
            status_message: migrated_legacy_keymap
                .then(|| "Migrated legacy keyboard profile to the default keymap.".into()),
            clipboard: None,
            action_menu: None,
            action_menu_serial: 0,
            open_with_catalogs: HashMap::new(),
            open_with_chooser: None,
            open_with_serial: 0,
            empty_space_menu: None,
            empty_space_menu_serial: 0,
            operation_sheet: None,
            operation_sheet_closing: false,
            inline_rename: None,
            path_input: PathInputState::new(path_input, path.to_path_buf()),
            _path_input_subscription: path_input_subscription,
            file_search: FileSearchState::new(
                file_search_input,
                path.to_path_buf(),
                dirs::home_dir().unwrap_or_else(|| path.to_path_buf()),
            ),
            _file_search_subscription: file_search_subscription,
            pending_reveal: Vec::new(),
            pending_show_properties: false,
            file_list_scroll: UniformListScrollHandle::new(),
            rendered_file_range: 0..0,
            grid_columns: 1,
            grid_card_width: GRID_CARD_TARGET_WIDTH,
            grid_previews: HashMap::new(),
            grid_preview_queue: VecDeque::new(),
            grid_preview_inflight: HashSet::new(),
            grid_preview_generation: 0,
            grid_preview_cancel: Arc::new(AtomicBool::new(false)),
            pointer_interaction: PointerInteraction::Idle,
            pointer_serial: 0,
            column_resize: None,
            reduced_motion: settings.reduced_motion,
            trash_entries: Arc::new(Vec::new()),
            devices: Vec::new(),
            device_refresh: DeviceRefresh::new(),
            device_scan_error: None,
            auto_mount_removable: settings.auto_mount_removable,
            auto_mount_attempted: HashSet::new(),
            config_paths,
            preferences: preferences.clone(),
            preferences_subscription: None,
            settings,
            theme_catalog,
            theme_appearance,
            active_theme_name,
            appearance_menu_open: false,
            appearance_menu_closing: false,
            command_bar_menu: None,
            command_bar_menu_serial: 0,
            command_bar_width_tier: CommandBarWidthTier::Full,
            appearance_subscription: None,
            scan_cancel: None,
            preview_cancel: None,
            preview_service: Arc::new(PreviewService::default()),
            preview_cache: Arc::new(Mutex::new(PreviewCache::new(memory_cache_mib))),
            thumbnail_service: Arc::new(ThumbnailService),
            directory_watcher: None,
            watched_path: None,
            watcher_refresh_at: None,
            pending_watch_changes: PendingWatchChanges::default(),
            watcher_apply_running: false,
        };
        manager.preferences_subscription = Some(cx.observe(&preferences, |this, state, cx| {
            let (settings, keymap) = {
                let state = state.read(cx);
                (state.settings.clone(), state.keymap.clone())
            };
            this.sync_preferences(&settings, &keymap, cx);
        }));
        PreferencesWindow::install_keybindings(&keymap, cx);
        manager.refresh_favorite_availability(cx);
        manager
    }
}

include!("file_manager/loading.rs");
include!("file_manager/directory_watcher.rs");
include!("file_manager/perf_trace.rs");
include!("file_manager/rubber_band.rs");
include!("file_manager/file_search.rs");
include!("file_manager/interaction.rs");
include!("file_manager/open_with.rs");
include!("file_manager/operations.rs");
include!("file_manager/command_bar.rs");
include!("file_manager/menu_geometry.rs");
include!("file_manager/grid_preview.rs");
include!("file_manager/detail_columns.rs");
include!("file_manager/view_surface.rs");
include!("file_manager/view_sidebar.rs");
include!("file_manager/view_header.rs");
include!("file_manager/view_command_bar.rs");
include!("file_manager/view_appearance.rs");
include!("file_manager/view_submenus.rs");
include!("file_manager/view_menus.rs");
include!("file_manager/view_operation_sheet.rs");
include!("file_manager/view_open_with.rs");
include!("file_manager/view_settings.rs");
include!("file_manager/keymap.rs");
include!("file_manager/view_lists.rs");
include!("file_manager/view_trash.rs");
include!("file_manager/view_grid.rs");
include!("file_manager/view_workspace.rs");
include!("file_manager/helpers_core.rs");
include!("file_manager/helpers_ui.rs");
include!("file_manager/helpers_window.rs");

pub fn run() {
    if env::args().any(|argument| argument == "--version" || argument == "-V") {
        println!("gnil-fm {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let request = initial_open_request();
    Application::new()
        .with_assets(Assets)
        .run(move |cx: &mut App| {
            cx.on_action(|_: &Quit, cx| cx.quit());
            if let Err(error) = open_main_window(request, cx) {
                eprintln!("Could not open gnil-fm window: {error}");
            }
        });
}

#[cfg(test)]
mod tests {
    use super::{
        ActionMenuPlacement, ActionMenuState, ActivateFileSearch, CommandBarMenuKind,
        EmptySpaceMenuCapabilities, EmptySpaceMenuContext, EmptySpaceMenuState, EmptySpaceSubmenu,
        EmptySpaceViewState, FileManager, FileSearchScope, HandleEscape, MenuAnimationState,
        MenuContext, OpenRename, OperationSheet, PointerInteraction, ToggleFavorite,
        ToggleSettings, breadcrumb_segments, favorite_availability, format_bytes, place_is_active,
        resolve_theme_appearance, size_label, visible_breadcrumb_segments,
    };
    use gnil_core::{
        ConfigPaths, DirectoryLoadPhase, DirectorySnapshot, EntryMetadata, FileEntry, FileKind,
        FileMetadata, SortSpec, ThemeAppearance, ThemeMode,
    };
    use gpui::{ExternalPaths, Modifiers, MouseButton, TestAppContext, point, px, size};
    use std::{
        path::{Path, PathBuf},
        sync::Arc,
        time::Duration,
    };

    use crate::open_with::DesktopApplication;

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

    fn test_config_paths(root: &Path) -> ConfigPaths {
        ConfigPaths {
            config: root.join("config.toml"),
            keymap: root.join("keymap.toml"),
            session: root.join("session.json"),
            cache: root.join("cache"),
            journal: root.join("journal.jsonl"),
        }
    }

    #[test]
    fn default_keymap_keeps_standard_navigation() {
        let defaults = super::PreferencesWindow::default_keymap_entries().collect::<Vec<_>>();

        assert!(defaults.contains(&("selection.next", "down")));
        assert!(defaults.contains(&("file.copy", "ctrl-c")));
        assert!(defaults.contains(&("settings.open_keymap", "ctrl-k ctrl-s")));
        assert!(!defaults.contains(&("selection.next", "j")));
    }

    #[gpui::test]
    fn settings_opens_one_native_preferences_window(cx: &mut TestAppContext) {
        let (manager, cx) = cx.add_window_view(|_, cx| {
            FileManager::new(Path::new("/tmp"), ThemeAppearance::Dark, cx)
        });

        cx.update(|window, cx| {
            manager.update(cx, |manager, cx| {
                manager.toggle_settings(&ToggleSettings, window, cx);
                manager.toggle_settings(&ToggleSettings, window, cx);
            });
        });

        assert_eq!(cx.windows().len(), 2);
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
    fn favorite_availability_is_optimistic_before_background_probe() {
        let missing = PathBuf::from("/definitely-not-a-mounted-favorite");
        assert_eq!(
            favorite_availability(std::slice::from_ref(&missing)).get(&missing),
            Some(&true)
        );
    }

    #[test]
    fn size_column_uses_item_count_for_folders_and_bytes_for_files() {
        let folder = FileEntry {
            path: PathBuf::from("/tmp/folder"),
            name: "folder".to_owned(),
            kind: FileKind::Directory,
            hidden: false,
            metadata: EntryMetadata::Ready(FileMetadata {
                child_count: Some(12),
                ..FileMetadata::default()
            }),
            git_status: None,
        };
        let file = FileEntry {
            path: PathBuf::from("/tmp/file"),
            name: "file".to_owned(),
            kind: FileKind::File,
            hidden: false,
            metadata: EntryMetadata::Ready(FileMetadata {
                len: 128 * 1024,
                ..FileMetadata::default()
            }),
            git_status: None,
        };

        assert_eq!(size_label(&folder), "12 items");
        assert_eq!(size_label(&file), "128 KB");
        assert_eq!(format_bytes(4 * 1024 * 1024 + (1024 * 1024 / 5)), "4.2 MB");
    }

    #[test]
    fn breadcrumb_segments_keep_each_ancestor_target() {
        assert_eq!(
            breadcrumb_segments(Path::new("/home/person/Documents")),
            vec![
                ("/".to_owned(), PathBuf::from("/")),
                ("home".to_owned(), PathBuf::from("/home")),
                ("person".to_owned(), PathBuf::from("/home/person")),
                (
                    "Documents".to_owned(),
                    PathBuf::from("/home/person/Documents")
                ),
            ]
        );
    }

    #[test]
    fn long_breadcrumbs_keep_root_and_nearest_ancestors() {
        assert_eq!(
            visible_breadcrumb_segments(Path::new("/home/person/Projects/gnil-fm/crates")),
            vec![
                (0, "/".to_owned(), Some(PathBuf::from("/"))),
                (usize::MAX, "…".to_owned(), None),
                (
                    4,
                    "gnil-fm".to_owned(),
                    Some(PathBuf::from("/home/person/Projects/gnil-fm"))
                ),
                (
                    5,
                    "crates".to_owned(),
                    Some(PathBuf::from("/home/person/Projects/gnil-fm/crates"))
                ),
            ]
        );
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

    #[test]
    fn external_paths_encode_as_uri_list() {
        let paths = ExternalPaths::new([PathBuf::from("/tmp/a file.txt"), PathBuf::from("/tmp/β")]);
        assert_eq!(
            String::from_utf8(paths.to_uri_list().unwrap()).unwrap(),
            "file:///tmp/a%20file.txt\r\nfile:///tmp/%CE%B2\r\n"
        );
        assert!(ExternalPaths::default().to_uri_list().is_err());
    }

    #[gpui::test]
    fn file_search_uses_the_current_folder_and_reveals_a_file(cx: &mut TestAppContext) {
        let temp = tempfile::tempdir().unwrap();
        let current_folder = temp.path().join("workspace");
        let reports = current_folder.join("Documents/reports");
        std::fs::create_dir_all(&reports).unwrap();
        let report = reports.join("annual-report.txt");
        std::fs::write(&report, b"report").unwrap();
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("annual-outside.txt"), b"other").unwrap();
        let initial_path = current_folder.clone();
        let (manager, cx) = cx.add_window_view(move |_, cx| {
            FileManager::new(&initial_path, ThemeAppearance::Dark, cx)
        });

        cx.update(|window, cx| {
            manager.update(cx, |manager, cx| {
                manager.activate_file_search(&ActivateFileSearch, window, cx);
            });
        });
        assert_eq!(
            cx.read(|cx| manager.read(cx).file_search.root.clone()),
            current_folder
        );
        let search_field = cx
            .debug_bounds("file-search-input-layer")
            .expect("search input should be visible");
        let close = cx
            .debug_bounds("file-search-close")
            .expect("close button should be visible");
        assert!(close.left() >= search_field.left());
        assert!(close.right() <= search_field.right());
        assert!(close.top() >= search_field.top());
        assert!(close.bottom() <= search_field.bottom());
        manager.update(cx, |manager, cx| {
            manager
                .file_search
                .input
                .update(cx, |input, cx| input.set_text("annual", cx));
        });
        cx.executor().advance_clock(Duration::from_millis(121));
        cx.run_until_parked();

        assert!(cx.read(|cx| {
            let manager = manager.read(cx);
            manager.file_search.open
                && !manager.file_search.loading
                && manager.file_search.results.len() == 1
                && manager.file_search.results[0].path == report
        }));

        cx.update(|window, cx| {
            manager.update(cx, |manager, cx| {
                manager.activate_file_search_result(0, window, cx);
            });
        });
        cx.run_until_parked();

        assert_eq!(cx.read(|cx| manager.read(cx).tab.path.clone()), reports);
        assert!(!cx.read(|cx| manager.read(cx).file_search.open));
    }

    #[gpui::test]
    fn navigation_publishes_the_new_empty_folder_before_scanning(cx: &mut TestAppContext) {
        let temp = tempfile::tempdir().unwrap();
        let initial_path = temp.path().join("initial");
        let next_path = temp.path().join("next");
        std::fs::create_dir_all(&initial_path).unwrap();
        std::fs::create_dir_all(&next_path).unwrap();
        std::fs::write(next_path.join("file.txt"), b"x").unwrap();
        let initial_for_view = initial_path.clone();
        let (manager, cx) = cx.add_window_view(move |_, cx| {
            let mut manager = FileManager::new(&initial_for_view, ThemeAppearance::Dark, cx);
            manager.snapshot = test_snapshot(2);
            manager.selection.select_only(1, &manager.snapshot.entries);
            manager
        });

        manager.update(cx, |manager, cx| {
            manager.tab.navigate(next_path.clone());
            manager.load_directory(cx);
            assert_eq!(manager.snapshot.path, next_path);
            assert!(manager.snapshot.entries.is_empty());
            assert_eq!(manager.snapshot.phase, DirectoryLoadPhase::Discovered);
            assert!(manager.loading);
            assert_eq!(manager.selection.selected_count(), 0);
            assert_eq!(manager.rendered_file_range, 0..0);
        });
    }

    #[gpui::test]
    fn external_reveal_selects_multiple_siblings_after_loading(cx: &mut TestAppContext) {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first.txt");
        let second = temp.path().join("second.txt");
        std::fs::write(&first, b"first").unwrap();
        std::fs::write(&second, b"second").unwrap();
        let initial_path = temp.path().to_path_buf();
        let first_for_view = first.clone();
        let second_for_view = second.clone();
        let (manager, cx) = cx.add_window_view(move |_, cx| {
            let mut manager = FileManager::new(&initial_path, ThemeAppearance::Dark, cx);
            manager.pending_reveal = vec![first_for_view, second_for_view];
            manager.load_directory(cx);
            manager
        });

        cx.run_until_parked();

        assert!(cx.read(|cx| {
            let manager = manager.read(cx);
            manager.snapshot.phase == DirectoryLoadPhase::Complete
                && manager.selection.selected_count() == 2
                && manager.selection.contains_path(&first)
                && manager.selection.contains_path(&second)
                && manager.pending_reveal.is_empty()
        }));
    }

    #[gpui::test]
    fn external_properties_waits_for_metadata_and_opens_existing_sheet(cx: &mut TestAppContext) {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("report.txt");
        std::fs::write(&file, b"report").unwrap();
        let initial_path = temp.path().to_path_buf();
        let file_for_view = file.clone();
        let (manager, cx) = cx.add_window_view(move |_, cx| {
            let mut manager = FileManager::new(&initial_path, ThemeAppearance::Dark, cx);
            manager.pending_reveal = vec![file_for_view];
            manager.pending_show_properties = true;
            manager.load_directory(cx);
            manager
        });

        cx.run_until_parked();

        assert!(cx.read(|cx| {
            let manager = manager.read(cx);
            manager.snapshot.phase == DirectoryLoadPhase::Complete
                && manager.selection.contains_path(&file)
                && !manager.pending_show_properties
                && matches!(
                    manager.operation_sheet,
                    Some(OperationSheet::Permissions { .. })
                )
        }));
    }

    #[gpui::test]
    fn directory_grid_card_does_not_queue_preview_io(cx: &mut TestAppContext) {
        let (manager, cx) = cx.add_window_view(|_, cx| {
            FileManager::new(Path::new("/tmp"), ThemeAppearance::Dark, cx)
        });
        let directory = FileEntry {
            path: PathBuf::from("/tmp/folder"),
            name: "folder".into(),
            kind: FileKind::Directory,
            hidden: false,
            metadata: EntryMetadata::Ready(FileMetadata::default()),
            git_status: None,
        };

        manager.update(cx, |manager, cx| {
            let _ = manager.render_grid_preview(&directory, &directory.path, cx);
            assert!(manager.grid_preview_queue.is_empty());
            assert!(manager.grid_preview_inflight.is_empty());
            assert!(!manager.grid_previews.contains_key(&directory.path));
        });
    }

    #[gpui::test]
    fn file_search_stays_open_inside_and_closes_on_outside_mouse_down(cx: &mut TestAppContext) {
        let (manager, cx) = cx.add_window_view(|_, cx| {
            let mut manager = FileManager::new(Path::new("/tmp"), ThemeAppearance::Dark, cx);
            manager.reduced_motion = true;
            manager
        });
        cx.update(|window, cx| {
            manager.update(cx, |manager, cx| {
                manager.activate_file_search(&ActivateFileSearch, window, cx);
            });
        });

        let search = cx
            .debug_bounds("file-search-input-layer")
            .expect("search input should be visible");
        cx.simulate_mouse_down(
            point(search.left() + px(20.0), search.top() + px(10.0)),
            MouseButton::Left,
            Modifiers::none(),
        );
        assert!(cx.read(|cx| manager.read(cx).file_search.open));

        cx.simulate_mouse_down(
            point(search.left() - px(20.0), search.top() + px(10.0)),
            MouseButton::Left,
            Modifiers::none(),
        );
        assert!(!cx.read(|cx| manager.read(cx).file_search.open));
    }

    #[gpui::test]
    fn action_menu_does_not_block_an_outside_file_click(cx: &mut TestAppContext) {
        let (manager, cx) = cx.add_window_view(|_, cx| {
            let mut manager = FileManager::new(Path::new("/tmp"), ThemeAppearance::Dark, cx);
            manager.snapshot = test_snapshot(2);
            manager.selection.select_only(1, &manager.snapshot.entries);
            manager.action_menu_serial = 1;
            manager.action_menu = Some(ActionMenuState::new(
                ActionMenuPlacement::Cursor(point(px(640.0), px(420.0))),
                MenuContext {
                    selected_count: 1,
                    ..MenuContext::default()
                },
                manager.action_menu_serial,
            ));
            manager
        });
        cx.run_until_parked();

        let row = cx
            .debug_bounds("details-row-0")
            .expect("the first file row should be visible outside the menu");
        let position = point(row.left() + px(12.0), row.top() + px(12.0));
        cx.simulate_mouse_down(position, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(position, MouseButton::Left, Modifiers::none());

        assert!(cx.read(|cx| {
            let manager = manager.read(cx);
            manager
                .selection
                .contains_path(Path::new("/tmp/file-0.txt"))
                && manager
                    .action_menu
                    .as_ref()
                    .is_some_and(|menu| menu.animation == MenuAnimationState::Closing)
        }));

        cx.executor().advance_clock(Duration::from_millis(81));
        cx.run_until_parked();
        assert!(cx.read(|cx| manager.read(cx).action_menu.is_none()));
    }

    #[gpui::test]
    fn clicking_an_open_command_menu_trigger_does_not_reopen_it(cx: &mut TestAppContext) {
        let (manager, cx) = cx.add_window_view(|_, cx| {
            FileManager::new(Path::new("/tmp"), ThemeAppearance::Dark, cx)
        });
        manager.update(cx, |manager, cx| {
            manager.toggle_command_bar_menu(CommandBarMenuKind::Sort, cx);
        });
        assert!(cx.read(|cx| manager.read(cx).command_bar_menu.is_some()));

        manager.update(cx, |manager, cx| {
            // The menu receives pointer-down-out before the trigger's click handler.
            manager.dismiss_command_bar_menu(cx);
            manager.toggle_command_bar_menu(CommandBarMenuKind::Sort, cx);
        });
        assert!(cx.read(|cx| {
            manager
                .read(cx)
                .command_bar_menu
                .as_ref()
                .is_some_and(|menu| menu.animation == MenuAnimationState::Closing)
        }));

        cx.executor().advance_clock(Duration::from_millis(81));
        cx.run_until_parked();
        assert!(cx.read(|cx| manager.read(cx).command_bar_menu.is_none()));
    }

    #[gpui::test]
    fn file_search_can_switch_between_current_folder_and_home(cx: &mut TestAppContext) {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let current = home.join("Projects/workspace");
        std::fs::create_dir_all(&current).unwrap();
        let home_result = home.join("home-only-result.txt");
        std::fs::write(&home_result, b"home").unwrap();
        let initial_path = current.clone();
        let (manager, cx) = cx.add_window_view(move |_, cx| {
            let mut manager = FileManager::new(&initial_path, ThemeAppearance::Dark, cx);
            manager.file_search.home_root = home;
            manager
        });

        cx.update(|window, cx| {
            manager.update(cx, |manager, cx| {
                manager.activate_file_search(&ActivateFileSearch, window, cx);
                manager.select_file_search_scope(FileSearchScope::Home, cx);
                manager
                    .file_search
                    .input
                    .update(cx, |input, cx| input.set_text("home-only", cx));
            });
        });
        cx.executor().advance_clock(Duration::from_millis(121));
        cx.run_until_parked();

        assert!(cx.read(|cx| {
            let search = &manager.read(cx).file_search;
            search.scope == FileSearchScope::Home
                && search.root == temp.path().join("home")
                && search.results.len() == 1
                && search.results[0].path == home_result
        }));
    }

    #[gpui::test]
    fn escape_cancels_inline_rename_before_other_active_layers(cx: &mut TestAppContext) {
        let (manager, cx) = cx.add_window_view(|_, cx| {
            let mut manager = FileManager::new(Path::new("/tmp"), ThemeAppearance::Dark, cx);
            manager.reduced_motion = true;
            manager.snapshot = test_snapshot(1);
            manager.file_search.open = true;
            manager.selection.select_only(0, &manager.snapshot.entries);
            manager
        });

        cx.update(|window, cx| {
            manager.update(cx, |manager, cx| {
                manager.open_rename(&OpenRename, window, cx);
                assert!(manager.inline_rename.is_some());
                assert!(manager.operation_sheet.is_none());
                manager.handle_escape(&HandleEscape, window, cx);
            });
        });

        assert!(cx.read(|cx| {
            let manager = manager.read(cx);
            manager.inline_rename.is_none()
                && manager.file_search.open
                && manager.selection.selected_count() == 1
        }));
    }

    #[gpui::test]
    fn escape_closes_one_layer_then_clears_selection(cx: &mut TestAppContext) {
        let (manager, cx) = cx.add_window_view(|_, cx| {
            let mut manager = FileManager::new(Path::new("/tmp"), ThemeAppearance::Dark, cx);
            manager.reduced_motion = true;
            manager.snapshot = test_snapshot(1);
            manager.selection.select_only(0, &manager.snapshot.entries);
            manager.file_search.open = true;
            manager.operation_sheet = Some(OperationSheet::FolderProperties {
                path: PathBuf::from("/tmp"),
                item_count: 1,
                file_bytes: 0,
                mode: None,
                readonly: false,
            });
            manager
                .file_search
                .input
                .update(cx, |input, cx| input.set_text("needle", cx));
            manager
        });

        cx.update(|window, cx| {
            manager.update(cx, |manager, cx| {
                manager.handle_escape(&HandleEscape, window, cx);
            });
        });
        assert!(cx.read(|cx| {
            let manager = manager.read(cx);
            !manager.file_search.open
                && manager.file_search.input.read(cx).text().is_empty()
                && manager.operation_sheet.is_some()
                && manager.selection.selected_count() == 1
        }));

        cx.update(|window, cx| {
            manager.update(cx, |manager, cx| {
                manager.handle_escape(&HandleEscape, window, cx);
            });
        });
        assert!(cx.read(|cx| manager.read(cx).operation_sheet.is_none()));
        assert_eq!(cx.read(|cx| manager.read(cx).selection.selected_count()), 1);

        cx.update(|window, cx| {
            manager.update(cx, |manager, cx| {
                manager.handle_escape(&HandleEscape, window, cx);
            });
        });
        assert_eq!(cx.read(|cx| manager.read(cx).selection.selected_count()), 0);

        cx.update(|window, cx| {
            manager.update(cx, |manager, cx| {
                manager.handle_escape(&HandleEscape, window, cx);
            });
        });
        assert_eq!(cx.read(|cx| manager.read(cx).selection.selected_count()), 0);
    }

    #[gpui::test]
    fn favorites_toggle_and_keep_drag_reorder(cx: &mut TestAppContext) {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        let initial_path = temp.path().to_path_buf();
        let config_paths = test_config_paths(temp.path());
        let first_entry = first.clone();
        let (manager, cx) = cx.add_window_view(move |_, cx| {
            let mut manager = FileManager::new(&initial_path, ThemeAppearance::Dark, cx);
            manager.config_paths = config_paths;
            manager.settings.favorites.clear();
            manager.favorite_availability.clear();
            manager.snapshot = Arc::new(DirectorySnapshot {
                generation: 1,
                path: initial_path,
                entries: vec![FileEntry {
                    path: first_entry,
                    name: "first".into(),
                    kind: FileKind::Directory,
                    hidden: false,
                    metadata: EntryMetadata::Ready(FileMetadata::default()),
                    git_status: None,
                }],
                unreadable_entries: 0,
                phase: DirectoryLoadPhase::Complete,
            });
            manager.selection.select_only(0, &manager.snapshot.entries);
            manager
        });

        cx.update(|window, cx| {
            manager.update(cx, |manager, cx| {
                manager.toggle_selected_favorite(&ToggleFavorite, window, cx);
                manager.toggle_favorite_path(second.clone(), cx);
                manager.reorder_favorite_path(&first, 1, cx);
            });
        });

        assert_eq!(
            cx.read(|cx| manager.read(cx).settings.favorites.clone()),
            [second.clone(), first.clone()]
        );
        assert_eq!(
            test_config_paths(temp.path())
                .load_settings()
                .unwrap()
                .favorites,
            [second, first]
        );
    }

    #[gpui::test]
    fn unavailable_favorite_prompts_before_removal(cx: &mut TestAppContext) {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("moved-folder");
        let initial_path = temp.path().to_path_buf();
        let config_paths = test_config_paths(temp.path());
        let favorite = missing.clone();
        let (manager, cx) = cx.add_window_view(move |_, cx| {
            let mut manager = FileManager::new(&initial_path, ThemeAppearance::Dark, cx);
            manager.config_paths = config_paths;
            manager.settings.favorites = vec![favorite.clone()];
            manager.favorite_availability.insert(favorite, false);
            manager
        });

        cx.update(|window, cx| {
            manager.update(cx, |manager, cx| {
                manager.open_favorite(missing.clone(), window, cx);
            });
        });
        assert!(cx.has_pending_prompt());
        cx.simulate_prompt_answer("Remove from Favorites");
        cx.run_until_parked();

        assert!(cx.read(|cx| manager.read(cx).settings.favorites.is_empty()));
    }

    #[gpui::test]
    fn details_header_drag_resizes_and_persists_the_column(cx: &mut TestAppContext) {
        let temp = tempfile::tempdir().unwrap();
        let initial_path = temp.path().to_path_buf();
        let config_paths = test_config_paths(temp.path());
        let saved_paths = config_paths.clone();
        let (manager, cx) = cx.add_window_view(move |_, cx| {
            let mut manager = FileManager::new(&initial_path, ThemeAppearance::Dark, cx);
            manager.config_paths = config_paths;
            manager.settings.file_layout = super::FileLayout::Details;
            manager.settings.details_columns.name = 360.0;
            manager.snapshot = test_snapshot(2);
            manager
        });
        let initial_width = cx.read(|cx| manager.read(cx).settings.details_columns.name);
        let handle = cx
            .debug_bounds("column-resize-0")
            .expect("name column resize handle should be rendered");
        let origin = point(
            handle.left() + handle.size.width / 2.0,
            handle.top() + handle.size.height / 2.0,
        );
        let current = point(origin.x + px(72.0), origin.y);

        cx.simulate_mouse_down(origin, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(current, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(current, MouseButton::Left, Modifiers::none());

        let resized_width = cx.read(|cx| manager.read(cx).settings.details_columns.name);
        assert!((resized_width - (initial_width + 72.0)).abs() < 0.01);
        assert!(cx.read(|cx| manager.read(cx).column_resize.is_none()));
        let persisted_width = saved_paths.load_settings().unwrap().details_columns.name;
        assert!((persisted_width - (initial_width + 72.0)).abs() < 0.01);
        let expected_row_width = cx.read(|cx| {
            let columns = manager.read(cx).settings.details_columns;
            columns.name + columns.modified + columns.kind + columns.size + 16.0
        });
        let row_width = f32::from(
            cx.debug_bounds("details-row-0")
                .expect("details row should be rendered")
                .size
                .width,
        );
        assert!(
            (row_width - expected_row_width).abs() < 1.0,
            "row width {row_width} should match columns {expected_row_width}"
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
    fn selected_rows_start_one_native_file_drag(cx: &mut TestAppContext) {
        let (manager, cx) = cx.add_window_view(|_, cx| {
            let mut manager = FileManager::new(Path::new("/tmp"), ThemeAppearance::Dark, cx);
            manager.settings.file_layout = super::FileLayout::Details;
            manager.snapshot = test_snapshot(3);
            manager.selection.select_only(0, &manager.snapshot.entries);
            manager.selection.toggle(1, &manager.snapshot.entries);
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
        let origin = point(viewport.left() + px(30.0), viewport.top() + px(18.0));

        cx.simulate_mouse_down(origin, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(
            point(origin.x + px(6.0), origin.y),
            MouseButton::Left,
            Modifiers::control(),
        );
        cx.simulate_mouse_move(
            point(origin.x + px(10.0), origin.y),
            MouseButton::Left,
            Modifiers::control(),
        );

        let paths = cx
            .take_external_file_drag()
            .expect("row drag should start a native file drag");
        assert_eq!(
            paths.paths(),
            [
                PathBuf::from("/tmp/file-0.txt"),
                PathBuf::from("/tmp/file-1.txt")
            ]
        );
        assert!(cx.read(|cx| matches!(
            &manager.read(cx).pointer_interaction,
            PointerInteraction::FileDrag(drag) if drag.copy
        )));

        cx.finish_external_file_drag();

        assert!(cx.read(|cx| matches!(
            manager.read(cx).pointer_interaction,
            PointerInteraction::Idle
        )));
        assert_eq!(cx.read(|cx| manager.read(cx).selection.selected_count()), 2);
    }

    #[gpui::test]
    fn dragging_an_unselected_row_exports_only_that_row(cx: &mut TestAppContext) {
        let (manager, cx) = cx.add_window_view(|_, cx| {
            let mut manager = FileManager::new(Path::new("/tmp"), ThemeAppearance::Dark, cx);
            manager.settings.file_layout = super::FileLayout::Details;
            manager.snapshot = test_snapshot(3);
            manager.selection.select_only(0, &manager.snapshot.entries);
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
        let origin = point(viewport.left() + px(30.0), viewport.top() + px(54.0));

        cx.simulate_mouse_down(origin, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(
            point(origin.x + px(6.0), origin.y),
            MouseButton::Left,
            Modifiers::none(),
        );
        cx.simulate_mouse_move(
            point(origin.x + px(10.0), origin.y),
            MouseButton::Left,
            Modifiers::none(),
        );

        let paths = cx
            .take_external_file_drag()
            .expect("row drag should start a native file drag");
        assert_eq!(paths.paths(), [PathBuf::from("/tmp/file-1.txt")]);
        assert_eq!(
            cx.read(|cx| {
                let manager = manager.read(cx);
                manager.selection.effective_paths(&manager.snapshot.entries)
            }),
            vec![PathBuf::from("/tmp/file-1.txt")]
        );

        cx.finish_external_file_drag();

        assert_eq!(
            cx.read(|cx| {
                let manager = manager.read(cx);
                manager.selection.effective_paths(&manager.snapshot.entries)
            }),
            vec![PathBuf::from("/tmp/file-0.txt")]
        );
    }

    #[gpui::test]
    fn running_operation_disables_native_file_drag(cx: &mut TestAppContext) {
        let (manager, cx) = cx.add_window_view(|_, cx| {
            let mut manager = FileManager::new(Path::new("/tmp"), ThemeAppearance::Dark, cx);
            manager.snapshot = test_snapshot(2);
            manager.operation_running = true;
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
        let origin = point(viewport.left() + px(30.0), viewport.top() + px(18.0));

        cx.simulate_mouse_down(origin, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(
            point(origin.x + px(10.0), origin.y),
            MouseButton::Left,
            Modifiers::none(),
        );

        assert!(cx.take_external_file_drag().is_none());
    }

    #[gpui::test]
    fn rubber_band_motion_inside_one_span_only_renders_overlay(cx: &mut TestAppContext) {
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
        let first = point(origin.x + px(12.0), origin.y - px(12.0));
        let second = point(first.x + px(4.0), first.y - px(4.0));

        cx.simulate_mouse_down(origin, MouseButton::Left, Modifiers::none());
        manager.update(cx, |manager, cx| {
            manager.update_rubber_band(first, false, cx);
        });
        cx.run_until_parked();

        let (file_list, rubber_band) = cx.read(|cx| {
            let manager = manager.read(cx);
            (
                manager.surfaces.file_list.clone(),
                manager.surfaces.rubber_band.clone(),
            )
        });
        let list_before = cx.read(|cx| file_list.read(cx).render_count);
        let rubber_before = cx.read(|cx| rubber_band.read(cx).render_count);

        manager.update(cx, |manager, cx| {
            manager.update_rubber_band(second, false, cx);
        });
        cx.run_until_parked();

        assert_eq!(cx.read(|cx| file_list.read(cx).render_count), list_before);
        assert!(cx.read(|cx| rubber_band.read(cx).render_count) > rubber_before);

        let list_before_crossing = cx.read(|cx| file_list.read(cx).render_count);
        let rubber_before_crossing = cx.read(|cx| rubber_band.read(cx).render_count);
        let row_position = point(origin.x + px(8.0), viewport.top() + px(54.0));
        manager.update(cx, |manager, cx| {
            manager.update_rubber_band(row_position, false, cx);
        });
        cx.run_until_parked();

        assert!(cx.read(|cx| file_list.read(cx).render_count) > list_before_crossing);
        assert!(cx.read(|cx| rubber_band.read(cx).render_count) > rubber_before_crossing);
        let band = cx
            .debug_bounds("rubber-band")
            .expect("rubber band should be visible after crossing the drag threshold");
        assert_eq!(band.top(), row_position.y);
        assert_eq!(band.bottom(), origin.y);

        cx.simulate_mouse_up(row_position, MouseButton::Left, Modifiers::none());
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
    fn selection_change_only_renders_dependent_surfaces(cx: &mut TestAppContext) {
        let (manager, cx) = cx.add_window_view(|_, cx| {
            let mut manager = FileManager::new(Path::new("/tmp"), ThemeAppearance::Dark, cx);
            manager.snapshot = test_snapshot(2);
            manager.preview_visible = false;
            manager
        });
        let (sidebar, file_list, preview, status) = cx.read(|cx| {
            let manager = manager.read(cx);
            (
                manager.surfaces.sidebar.clone(),
                manager.surfaces.file_list.clone(),
                manager.surfaces.preview.clone(),
                manager.surfaces.status.clone(),
            )
        });
        let sidebar_before = cx.read(|cx| sidebar.read(cx).render_count);
        let list_before = cx.read(|cx| file_list.read(cx).render_count);
        let preview_before = cx.read(|cx| preview.read(cx).render_count);
        let status_before = cx.read(|cx| status.read(cx).render_count);

        manager.update(cx, |manager, cx| {
            manager.selection.select_only(1, &manager.snapshot.entries);
            manager.selection_changed(cx);
        });
        cx.run_until_parked();

        assert_eq!(cx.read(|cx| sidebar.read(cx).render_count), sidebar_before);
        assert_eq!(cx.read(|cx| preview.read(cx).render_count), preview_before);
        assert!(cx.read(|cx| file_list.read(cx).render_count) > list_before);
        assert!(cx.read(|cx| status.read(cx).render_count) > status_before);
    }

    #[gpui::test]
    fn keyboard_preview_waits_for_debounce_window(cx: &mut TestAppContext) {
        let (manager, cx) = cx.add_window_view(|_, cx| {
            let mut manager = FileManager::new(Path::new("/tmp"), ThemeAppearance::Dark, cx);
            manager.snapshot = test_snapshot(2);
            manager.preview_visible = true;
            manager
        });

        manager.update(cx, |manager, cx| {
            manager.selection.select_only(1, &manager.snapshot.entries);
            manager.selection_changed_debounced(cx);
        });
        cx.run_until_parked();
        let generation_before_timer = cx.read(|cx| manager.read(cx).preview_generation);

        cx.executor().advance_clock(Duration::from_millis(49));
        cx.run_until_parked();
        assert_eq!(
            cx.read(|cx| manager.read(cx).preview_generation),
            generation_before_timer
        );

        cx.executor().advance_clock(Duration::from_millis(1));
        cx.run_until_parked();
        assert!(
            cx.read(|cx| manager.read(cx).preview_generation) > generation_before_timer,
            "preview loading should start after the debounce window"
        );
    }

    include!("file_manager/tests/menu_submenus.rs");

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

    #[gpui::test]
    fn preview_toolbar_button_toggles_the_right_panel(cx: &mut TestAppContext) {
        let (manager, cx) = cx.add_window_view(|_, cx| {
            let mut manager = FileManager::new(Path::new("/tmp"), ThemeAppearance::Dark, cx);
            manager.preview_visible = false;
            manager
        });
        cx.run_until_parked();

        let toggle = cx
            .debug_bounds("preview-panel-toggle")
            .expect("preview panel toggle should be visible at the right edge of the toolbar");
        let command_bar = cx.read(|cx| manager.read(cx).surfaces.command_bar.clone());
        let renders_before = cx.read(|cx| command_bar.read(cx).render_count);
        let position = point(toggle.left() + px(12.0), toggle.top() + px(12.0));

        cx.simulate_click(position, Modifiers::none());
        assert!(cx.read(|cx| manager.read(cx).preview_visible));
        let preview = cx
            .debug_bounds("preview-surface")
            .expect("preview panel should be laid out after enabling it");
        assert!(cx.read(|cx| command_bar.read(cx).render_count) > renders_before);

        cx.simulate_click(position, Modifiers::none());
        assert!(!cx.read(|cx| manager.read(cx).preview_visible));
        let expanded_list = cx
            .debug_bounds("file-list-surface-frame")
            .expect("file list should expand after hiding the preview panel");
        assert_eq!(expanded_list.right(), preview.right());
    }

    #[gpui::test]
    fn command_bar_menu_is_attached_to_its_trigger_without_an_icon_gutter(cx: &mut TestAppContext) {
        let (manager, cx) = cx.add_window_view(|_, cx| {
            let mut manager = FileManager::new(Path::new("/tmp"), ThemeAppearance::Dark, cx);
            manager.reduced_motion = true;
            manager
        });
        cx.simulate_resize(size(px(800.0), px(600.0)));
        cx.run_until_parked();
        manager.update(cx, |manager, cx| {
            manager.toggle_command_bar_menu(CommandBarMenuKind::Sort, cx);
        });
        cx.run_until_parked();

        let trigger = cx
            .debug_bounds("command-bar-menu-trigger-Sort")
            .expect("Sort trigger should be rendered");
        let panel = cx
            .debug_bounds("command-bar-menu-panel-Sort")
            .expect("Sort menu should be rendered");
        let first_label = cx
            .debug_bounds("command-bar-menu-label-Sort-0")
            .expect("first Sort menu label should be rendered");

        assert_eq!(
            panel.top(),
            trigger.bottom(),
            "trigger={trigger:?}, panel={panel:?}, first_label={first_label:?}"
        );
        assert_eq!(panel.left() + px(5.0), trigger.left());
        assert_eq!(first_label.left(), trigger.left() + px(8.0));
    }
}
