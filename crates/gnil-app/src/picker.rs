use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    ffi::{OsStr, OsString},
    fs,
    hash::{DefaultHasher, Hash as _, Hasher as _},
    io::{self, Read as _},
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use gnil_core::{
    ConfigPaths, DirectoryLoadPhase, DirectorySnapshot, EntryMetadata, FileEntry, FileKind,
    FileLayout, FileMetadata, SortSpec, ThemeAppearance, ThemeCatalog, ThemeMode,
};
use gnil_fs::{
    DirectoryWatcher, ScanOptions, WatchEvent, fuzzy_match_score, mount_device, scan_devices,
    scan_directory_progressive,
};
use gpui::{
    AnchoredPositionMode, AnyElement, AnyWindowHandle, App, ClickEvent, Context, Corner, Entity,
    FocusHandle, Focusable as _, KeyBinding, MouseButton, MouseDownEvent, Render, SharedString,
    Subscription, Window, actions, anchored, deferred, div, img, point, prelude::*, px, rgb,
    uniform_list,
};
use quick_xml::{Reader, XmlVersion, events::Event};
use url::Url;

use crate::{
    device_refresh::{DEVICE_MONITOR_INTERVAL, DeviceRefresh},
    path_input::{PathTarget, completion_candidates, resolve_path_input, validate_path},
    portal_protocol::{
        PickerOutcome, PickerRequest, PickerRequestKind, PortalChoice, PortalFilter,
        valid_save_name,
    },
    text_input::{self, TextInput, TextInputEvent},
    theme_runtime::{
        accent, accent_background, background, border, border_focused, danger, set_active, surface,
        surface_elevated, text, text_emphasized, text_muted, warning as warning_color,
    },
    ui::control::FocusableControl,
    ui::icon::{IconSize, IconTone, ui_icon},
    ui::overlay::{MENU_PRIORITY, OverlayMotionState, animate_overlay},
};

actions!(
    gnil_picker,
    [
        Accept,
        Cancel,
        ActivatePath,
        CompletePath,
        GoBack,
        GoForward,
        GoUp,
        SelectAll
    ]
);

pub use crate::assets::Assets as PickerAssets;

#[derive(Clone)]
pub enum PickerUiCommand {
    Open {
        request: Box<PickerRequest>,
        response: async_channel::Sender<PickerOutcome>,
        started: async_channel::Sender<Result<(), String>>,
    },
    Close {
        handle: String,
    },
}

type OpenWindows = Rc<RefCell<HashMap<String, AnyWindowHandle>>>;

pub fn bind_keys(cx: &mut App) {
    text_input::bind_keys(cx);
    cx.bind_keys([
        KeyBinding::new("enter", Accept, Some("Picker")),
        KeyBinding::new("escape", Cancel, Some("Picker")),
        KeyBinding::new("ctrl-l", ActivatePath, Some("Picker")),
        KeyBinding::new("tab", CompletePath, Some("PathInput")),
        KeyBinding::new("alt-left", GoBack, Some("Picker")),
        KeyBinding::new("alt-right", GoForward, Some("Picker")),
        KeyBinding::new("alt-up", GoUp, Some("Picker")),
        KeyBinding::new("ctrl-a", SelectAll, Some("Picker")),
    ]);
}

pub fn run_command_loop(receiver: async_channel::Receiver<PickerUiCommand>, cx: &mut App) {
    let windows: OpenWindows = Rc::new(RefCell::new(HashMap::new()));
    cx.spawn(async move |cx| {
        while let Ok(command) = receiver.recv().await {
            let windows = windows.clone();
            let _ = cx.update(|cx| match command {
                PickerUiCommand::Open {
                    request,
                    response,
                    started,
                } => {
                    open_picker_window(request, response, started, windows, cx);
                }
                PickerUiCommand::Close { handle } => {
                    if let Some(window) = windows.borrow_mut().remove(&handle) {
                        let _ = window.update(cx, |_, window, _| window.remove_window());
                    }
                }
            });
        }
    })
    .detach();
}

#[allow(clippy::needless_pass_by_value)]
fn open_picker_window(
    request: Box<PickerRequest>,
    response: async_channel::Sender<PickerOutcome>,
    started: async_channel::Sender<Result<(), String>>,
    windows: OpenWindows,
    cx: &mut App,
) {
    let bounds = gpui::Bounds::centered(None, gpui::size(px(920.0), px(640.0)), cx);
    let handle_key = request.handle.clone();
    let title = request.title.clone();
    let modal = match &request.kind {
        PickerRequestKind::Open(options) => options.common.modal,
        PickerRequestKind::Save(options) => options.common.modal,
        PickerRequestKind::SaveMany(options) => options.common.modal,
    };
    let wayland_parent = crate::portal_protocol::parent_handle(&request.parent_window)
        .map(str::to_owned)
        .map(gpui::WaylandParentHandle::new);
    let opened = cx.open_window(
        gpui::WindowOptions {
            window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
            window_min_size: Some(gpui::size(px(720.0), px(480.0))),
            window_background: gpui::WindowBackgroundAppearance::Opaque,
            titlebar: Some(gpui::TitlebarOptions {
                title: Some(title.into()),
                ..Default::default()
            }),
            app_id: Some("gnil-fm-portal".into()),
            kind: if modal {
                gpui::WindowKind::Floating
            } else {
                gpui::WindowKind::Normal
            },
            wayland_parent,
            ..Default::default()
        },
        |window, cx| {
            cx.new(|cx| {
                let mut picker =
                    Picker::new(*request, response.clone(), windows.clone(), window, cx);
                picker.load_directory(cx);
                picker.refresh_devices(cx);
                Picker::schedule_device_monitor(cx);
                picker
            })
        },
    );
    match opened {
        Ok(window) => {
            windows.borrow_mut().insert(handle_key, window.into());
            let _ = window.update(cx, |picker, window, cx| {
                window.focus(&picker.initial_focus_handle(cx));
            });
            cx.activate(true);
            let _ = started.try_send(Ok(()));
        }
        Err(error) => {
            let message = error.to_string();
            let _ = started.try_send(Err(message.clone()));
            let _ = response.try_send(PickerOutcome::Failed(message));
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PickerLocation {
    Directory,
    Recent,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct VisibleEntriesKey {
    generation: u64,
    phase: DirectoryLoadPhase,
    query: String,
    filter_index: Option<usize>,
}

#[allow(clippy::struct_excessive_bools)]
struct Picker {
    focus: FocusHandle,
    request: PickerRequest,
    response: Option<async_channel::Sender<PickerOutcome>>,
    windows: OpenWindows,
    current_dir: PathBuf,
    back: Vec<PathBuf>,
    forward: Vec<PathBuf>,
    location: PickerLocation,
    snapshot: Arc<DirectorySnapshot>,
    selected: HashSet<PathBuf>,
    anchor: Option<usize>,
    loading: bool,
    error: Option<String>,
    status: Option<String>,
    generation: u64,
    file_layout: FileLayout,
    config_paths: ConfigPaths,
    show_hidden: bool,
    path_editing: bool,
    path_input: Entity<TextInput>,
    search_input: Entity<TextInput>,
    name_input: Option<Entity<TextInput>>,
    initial_name_display: Option<String>,
    initial_raw_name: Option<OsString>,
    _subscriptions: Vec<Subscription>,
    devices: Vec<gnil_fs::DeviceEntry>,
    device_refresh: DeviceRefresh,
    device_scan_error: Option<String>,
    favorites: Vec<PathBuf>,
    filters: Vec<PortalFilter>,
    filter_index: Option<usize>,
    filter_open: bool,
    choices: Vec<PortalChoice>,
    choice_open: Option<usize>,
    menu_closing: bool,
    reduced_motion: bool,
    respect_gitignore: bool,
    metadata_workers: usize,
    scan_cancel: Option<Arc<AtomicBool>>,
    visible_entries_key: Option<VisibleEntriesKey>,
    visible_indices: Arc<[usize]>,
    directory_watcher: Option<DirectoryWatcher>,
    watched_path: Option<PathBuf>,
}

include!("picker/loading.rs");

include!("picker/actions.rs");

include!("picker/view.rs");

include!("picker/footer.rs");

impl gpui::Focusable for Picker {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Picker {
    fn initial_focus_handle(&self, cx: &App) -> FocusHandle {
        self.name_input.as_ref().map_or_else(
            || self.focus.clone(),
            |input| input.read(cx).focus_handle(cx),
        )
    }
}

#[cfg(test)]
mod tests;

impl Drop for Picker {
    fn drop(&mut self) {
        self.windows.borrow_mut().remove(&self.request.handle);
        if let Some(sender) = self.response.take() {
            let _ = sender.try_send(PickerOutcome::Cancelled);
        }
    }
}

impl Render for Picker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("Picker")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::accept))
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(Self::activate_path))
            .on_action(cx.listener(Self::complete_path))
            .on_action(cx.listener(Self::go_back))
            .on_action(cx.listener(Self::go_forward))
            .on_action(cx.listener(Self::go_up))
            .on_action(cx.listener(Self::select_all))
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(background()))
            .text_color(rgb(text_emphasized()))
            .child(self.render_toolbar(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.render_sidebar(cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .flex()
                            .flex_col()
                            .overflow_hidden()
                            .p_3()
                            .child(self.render_file_list(window, cx)),
                    ),
            )
            .child(self.render_footer(cx))
    }
}

include!("picker/helpers.rs");
