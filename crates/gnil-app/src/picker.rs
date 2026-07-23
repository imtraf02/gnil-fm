use std::{
    borrow::Cow,
    cell::RefCell,
    collections::{HashMap, HashSet},
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

use gnil_core::{
    ConfigPaths, DirectorySnapshot, FileEntry, FileKind, FileMetadata, SortSpec,
    ThemeAppearance, ThemeCatalog, ThemeMode,
};
use gnil_fs::{ScanOptions, fuzzy_match_score, mount_device, scan_devices, scan_directory};
use gpui::{
    AnyElement, AnyWindowHandle, App, AssetSource, ClickEvent, Context, Entity, FocusHandle,
    Focusable as _, KeyBinding, Render, SharedString, Subscription, Window, actions, div, img,
    prelude::*, px, rgb, uniform_list,
};
use quick_xml::{Reader, events::Event};
use url::Url;

use crate::{
    path_input::{PathTarget, completion_candidates, resolve_path_input, validate_path},
    portal_protocol::{
        PickerOutcome, PickerRequest, PickerRequestKind, PortalChoice, PortalFilter, valid_save_name,
    },
    text_input::{self, TextInput, TextInputEvent},
    theme_runtime::{
        accent, accent_background, background, border, border_focused, danger, set_active, surface,
        surface_elevated, text, text_emphasized, text_muted,
    },
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

pub struct PickerAssets;

impl AssetSource for PickerAssets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        let bytes: Option<&'static [u8]> = match path {
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
            "icons/folder-downloads.svg" => {
                Some(include_bytes!("../../../assets/icons/folder-downloads.svg"))
            }
            "icons/folder-pictures.svg" => {
                Some(include_bytes!("../../../assets/icons/folder-pictures.svg"))
            }
            "icons/folder-documents.svg" => {
                Some(include_bytes!("../../../assets/icons/folder-documents.svg"))
            }
            "icons/folder-videos.svg" => {
                Some(include_bytes!("../../../assets/icons/folder-videos.svg"))
            }
            "icons/folder-music.svg" => {
                Some(include_bytes!("../../../assets/icons/folder-music.svg"))
            }
            "icons/folder-desktop.svg" => {
                Some(include_bytes!("../../../assets/icons/folder-desktop.svg"))
            }
            "icons/file-generic.svg" => {
                Some(include_bytes!("../../../assets/icons/file-generic.svg"))
            }
            "icons/file-code.svg" => Some(include_bytes!("../../../assets/icons/file-code.svg")),
            "icons/file-text.svg" => Some(include_bytes!("../../../assets/icons/file-text.svg")),
            "icons/file-image.svg" => {
                Some(include_bytes!("../../../assets/icons/file-image.svg"))
            }
            "icons/file-document.svg" => {
                Some(include_bytes!("../../../assets/icons/file-document.svg"))
            }
            "icons/file-archive.svg" => {
                Some(include_bytes!("../../../assets/icons/file-archive.svg"))
            }
            "icons/file-media.svg" => {
                Some(include_bytes!("../../../assets/icons/file-media.svg"))
            }
            "icons/device-usb.svg" => {
                Some(include_bytes!("../../../assets/icons/device-usb.svg"))
            }
            "icons/device-drive.svg" => {
                Some(include_bytes!("../../../assets/icons/device-drive.svg"))
            }
            "icons/empty-state.svg" => {
                Some(include_bytes!("../../../assets/icons/empty-state.svg"))
            }
            _ => None,
        };
        Ok(bytes.map(Cow::Borrowed))
    }

    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        match path {
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
                "device-usb.svg".into(),
                "device-drive.svg".into(),
                "empty-state.svg".into(),
            ]),
            _ => Ok(Vec::new()),
        }
    }
}

#[derive(Clone)]
pub enum PickerUiCommand {
    Open {
        request: PickerRequest,
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

pub fn run_command_loop(
    receiver: async_channel::Receiver<PickerUiCommand>,
    cx: &mut App,
) {
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

fn open_picker_window(
    request: PickerRequest,
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
    let external_parent = crate::portal_protocol::parent_handle(&request.parent_window)
        .map(str::to_owned)
        .map(gpui::ExternalWindowParent::Wayland);
    let opened = cx.open_window(
        gpui::WindowOptions {
            window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
            window_min_size: Some(gpui::size(px(720.0), px(480.0))),
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
            external_parent,
            ..Default::default()
        },
        |window, cx| {
            cx.new(|cx| {
                let mut picker =
                    Picker::new(request, response.clone(), windows.clone(), window, cx);
                picker.load_directory(cx);
                picker.refresh_devices(cx);
                picker
            })
        },
    );
    match opened {
        Ok(window) => {
            windows
                .borrow_mut()
                .insert(handle_key, window.clone().into());
            let _ = window.update(cx, |picker, window, cx| {
                window.focus(&picker.focus_handle(cx));
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

struct Picker {
    focus: FocusHandle,
    request: PickerRequest,
    response: Option<async_channel::Sender<PickerOutcome>>,
    windows: OpenWindows,
    current_dir: PathBuf,
    back: Vec<PathBuf>,
    forward: Vec<PathBuf>,
    location: PickerLocation,
    snapshot: DirectorySnapshot,
    selected: HashSet<PathBuf>,
    anchor: Option<usize>,
    loading: bool,
    error: Option<String>,
    status: Option<String>,
    generation: u64,
    show_hidden: bool,
    path_editing: bool,
    path_input: Entity<TextInput>,
    search_input: Entity<TextInput>,
    name_input: Option<Entity<TextInput>>,
    initial_name_display: Option<String>,
    initial_raw_name: Option<OsString>,
    _subscriptions: Vec<Subscription>,
    devices: Vec<gnil_fs::DeviceEntry>,
    filters: Vec<PortalFilter>,
    filter_index: Option<usize>,
    filter_open: bool,
    choices: Vec<PortalChoice>,
    choice_open: Option<usize>,
}

impl Picker {
    fn new(
        request: PickerRequest,
        response: async_channel::Sender<PickerOutcome>,
        windows: OpenWindows,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        load_picker_theme(window);
        let settings = ConfigPaths::discover().load_settings().unwrap_or_default();
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        let (suggested_dir, suggested_name, filters, active_filter, choices) =
            request_initial_state(&request, &home);
        let (current_dir, status) = accessible_directory(&suggested_dir).map_or_else(
            |message| (home.clone(), Some(message)),
            |path| (path, None),
        );
        let path_input = cx.new(|cx| {
            TextInput::new("Enter a path", current_dir.display().to_string(), cx)
                .with_key_context("PathInput")
        });
        let search_input = cx.new(|cx| TextInput::new("Search this folder", "", cx));
        let initial_name_display = suggested_name.as_ref().map(|name| name.display.clone());
        let initial_raw_name = suggested_name.as_ref().and_then(|name| name.raw.clone());
        let name_input = suggested_name.map(|name| {
            cx.new(|cx| TextInput::new("File name", name.display, cx).with_key_context("TextInput"))
        });
        let mut subscriptions = Vec::new();
        for input in [&path_input, &search_input]
            .into_iter()
            .chain(name_input.iter())
        {
            subscriptions.push(cx.subscribe(input, |_, _, _: &TextInputEvent, cx| cx.notify()));
        }
        let filter_index = active_filter
            .as_ref()
            .and_then(|active| filters.iter().position(|filter| filter == active));
        Self {
            focus: cx.focus_handle(),
            request,
            response: Some(response),
            windows,
            current_dir: current_dir.clone(),
            back: Vec::new(),
            forward: Vec::new(),
            location: PickerLocation::Directory,
            snapshot: DirectorySnapshot {
                generation: 0,
                path: current_dir,
                entries: Vec::new(),
                unreadable_entries: 0,
            },
            selected: HashSet::new(),
            anchor: None,
            loading: false,
            error: None,
            status,
            generation: 0,
            show_hidden: settings.show_hidden,
            path_editing: false,
            path_input,
            search_input,
            name_input,
            initial_name_display,
            initial_raw_name,
            _subscriptions: subscriptions,
            devices: Vec::new(),
            filters,
            filter_index,
            filter_open: false,
            choices,
            choice_open: None,
        }
    }

    fn active_filter(&self) -> Option<&PortalFilter> {
        self.filter_index.and_then(|index| self.filters.get(index))
    }

    fn load_directory(&mut self, cx: &mut Context<Self>) {
        self.location = PickerLocation::Directory;
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        let path = self.current_dir.clone();
        let show_hidden = self.show_hidden;
        self.loading = true;
        self.error = None;
        self.selected.clear();
        let task = cx.background_executor().spawn(async move {
            scan_directory(
                &path,
                ScanOptions {
                    generation,
                    show_hidden,
                    sort: SortSpec::default(),
                },
            )
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                this.loading = false;
                match result {
                    Ok(snapshot) => this.snapshot = snapshot,
                    Err(error) => {
                        this.snapshot.entries.clear();
                        this.error = Some(permission_message(&this.current_dir, &error));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn refresh_devices(&mut self, cx: &mut Context<Self>) {
        let task = cx.background_executor().spawn(async { scan_devices() });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if let Ok(devices) = result {
                    this.devices = devices;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn open_device(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(device) = self.devices.get(index).cloned() else {
            return;
        };
        if let Some(path) = device.mount_path {
            self.navigate(path, cx);
            return;
        }
        self.status = Some(format!("Mounting {}…", device.label));
        let task = cx
            .background_executor()
            .spawn(async move { mount_device(&device.id) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(path) => this.navigate(path, cx),
                Err(error) => {
                    this.status = None;
                    this.error = Some(error.to_string());
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn navigate(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if path != self.current_dir {
            self.back.push(self.current_dir.clone());
            self.forward.clear();
            self.current_dir = path;
        }
        self.status = None;
        self.load_directory(cx);
    }

    fn go_back(&mut self, _: &GoBack, _: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.back.pop() else { return };
        self.forward.push(self.current_dir.clone());
        self.current_dir = path;
        self.load_directory(cx);
    }

    fn go_forward(&mut self, _: &GoForward, _: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.forward.pop() else { return };
        self.back.push(self.current_dir.clone());
        self.current_dir = path;
        self.load_directory(cx);
    }

    fn go_up(&mut self, _: &GoUp, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(parent) = self.current_dir.parent().map(Path::to_path_buf) {
            self.navigate(parent, cx);
        }
    }

    fn activate_path(&mut self, _: &ActivatePath, window: &mut Window, cx: &mut Context<Self>) {
        self.path_editing = true;
        let value = self.current_dir.display().to_string();
        self.path_input.update(cx, |input, cx| {
            input.set_text(value, cx);
            input.set_invalid(false, cx);
            input.select_all(cx);
            window.focus(&input.focus_handle(cx));
        });
        cx.notify();
    }

    fn complete_path(&mut self, _: &CompletePath, window: &mut Window, cx: &mut Context<Self>) {
        if !self.path_editing {
            return;
        }
        let text = self.path_input.read(cx).text().to_owned();
        match completion_candidates(
            &text,
            &self.current_dir,
            dirs::home_dir().as_deref(),
            self.show_hidden,
        ) {
            Ok(candidates) if !candidates.is_empty() => {
                let replacement = candidates[0].input.clone();
                self.path_input.update(cx, |input, cx| {
                    input.set_text(replacement, cx);
                    window.focus(&input.focus_handle(cx));
                });
            }
            Ok(_) => self.status = Some("No matching folder".into()),
            Err(error) => self.error = Some(error),
        }
        cx.notify();
    }

    fn submit_path(&mut self, cx: &mut Context<Self>) {
        let text = self.path_input.read(cx).text().trim().to_owned();
        let result = resolve_path_input(&text, &self.current_dir, dirs::home_dir().as_deref())
            .and_then(validate_path);
        match result {
            Ok(PathTarget::Directory(path)) => {
                self.path_editing = false;
                self.navigate(path, cx);
            }
            Ok(PathTarget::File { path, parent }) => {
                match &self.request.kind {
                    PickerRequestKind::Open(options) if options.directory => {
                        self.path_input
                            .update(cx, |input, cx| input.set_invalid(true, cx));
                        self.error = Some("Select a folder, not a regular file".into());
                        cx.notify();
                    }
                    PickerRequestKind::Open(_) if self
                        .active_filter()
                        .is_some_and(|filter| !filter.matches(&path)) =>
                    {
                        self.path_input
                            .update(cx, |input, cx| input.set_invalid(true, cx));
                        self.error = Some("This file does not match the active filter".into());
                        cx.notify();
                    }
                    PickerRequestKind::Open(_) => {
                        self.path_editing = false;
                        self.navigate(parent, cx);
                        self.selected.insert(path);
                    }
                    PickerRequestKind::Save(_) => {
                        let raw_name = path.file_name().map(OsStr::to_owned);
                        let display_name = raw_name
                            .as_deref()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        self.initial_name_display = Some(display_name.clone());
                        self.initial_raw_name = raw_name;
                        if let Some(input) = &self.name_input {
                            input.update(cx, |input, cx| input.set_text(display_name, cx));
                        }
                        self.path_editing = false;
                        self.navigate(parent, cx);
                    }
                    PickerRequestKind::SaveMany(_) => {
                        self.path_input
                            .update(cx, |input, cx| input.set_invalid(true, cx));
                        self.error = Some("Choose a destination folder".into());
                        cx.notify();
                    }
                }
            }
            Err(error) => {
                self.path_input
                    .update(cx, |input, cx| input.set_invalid(true, cx));
                self.error = Some(error);
                cx.notify();
            }
        }
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        if !self.allows_multiple() {
            return;
        }
        self.selected = self
            .visible_entries(cx)
            .into_iter()
            .filter(|entry| self.entry_selectable(entry))
            .map(|entry| entry.path.clone())
            .collect();
        cx.notify();
    }

    fn click_entry(
        &mut self,
        index: usize,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let visible = self.visible_entries(cx);
        let Some(entry) = visible.get(index).cloned() else {
            return;
        };
        if !self.entry_selectable(&entry) {
            if entry_is_directory(&entry) && event.click_count() >= 2 {
                self.navigate(entry.path, cx);
            }
            return;
        }
        let modifiers = event.modifiers();
        if self.allows_multiple() && modifiers.shift {
            let start = self.anchor.unwrap_or(index).min(index);
            let end = self.anchor.unwrap_or(index).max(index);
            if !modifiers.control && !modifiers.platform {
                self.selected.clear();
            }
            for item in &visible[start..=end] {
                if self.entry_selectable(item) {
                    self.selected.insert(item.path.clone());
                }
            }
        } else if self.allows_multiple() && (modifiers.control || modifiers.platform) {
            if !self.selected.remove(&entry.path) {
                self.selected.insert(entry.path.clone());
            }
            self.anchor = Some(index);
        } else {
            self.selected.clear();
            self.selected.insert(entry.path.clone());
            self.anchor = Some(index);
        }

        if matches!(self.request.kind, PickerRequestKind::Save(_)) && !entry_is_directory(&entry) {
            if let Some(input) = &self.name_input {
                input.update(cx, |input, cx| input.set_text(entry.name.clone(), cx));
            }
        }
        if event.click_count() >= 2 {
            if entry_is_directory(&entry) {
                self.navigate(entry.path, cx);
            } else if matches!(self.request.kind, PickerRequestKind::Open(_)) {
                if self.finish_accept(cx) {
                    window.remove_window();
                }
            }
        } else {
            cx.notify();
        }
    }

    fn entry_selectable(&self, entry: &FileEntry) -> bool {
        match &self.request.kind {
            PickerRequestKind::Open(options) if options.directory => entry_is_directory(entry),
            PickerRequestKind::Open(_) => !entry_is_directory(entry),
            PickerRequestKind::Save(_) => true,
            PickerRequestKind::SaveMany(_) => entry_is_directory(entry),
        }
    }

    fn allows_multiple(&self) -> bool {
        matches!(&self.request.kind, PickerRequestKind::Open(options) if options.multiple)
    }

    fn accept(&mut self, _: &Accept, window: &mut Window, cx: &mut Context<Self>) {
        if self.path_editing {
            self.submit_path(cx);
            window.focus(&self.focus_handle(cx));
            return;
        }
        if self.finish_accept(cx) {
            window.remove_window();
        }
    }

    fn finish_accept(&mut self, cx: &mut Context<Self>) -> bool {
        let paths = match &self.request.kind {
            PickerRequestKind::Open(options) if options.directory => {
                let mut paths: Vec<_> = self.selected.iter().cloned().collect();
                if paths.is_empty() {
                    paths.push(self.current_dir.clone());
                }
                paths.sort();
                paths
            }
            PickerRequestKind::Open(options) => {
                let mut paths: Vec<_> = self.selected.iter().cloned().collect();
                paths.sort();
                if !options.multiple {
                    paths.truncate(1);
                }
                if paths.is_empty() {
                    self.status = Some("Select a file to continue".into());
                    cx.notify();
                    return false;
                }
                paths
            }
            PickerRequestKind::Save(_) => {
                let Some(input) = &self.name_input else { return false };
                let name = input.read(cx).text().to_owned();
                if !valid_save_name(&name) {
                    input.update(cx, |input, cx| input.set_invalid(true, cx));
                    self.status = Some("Enter a valid file name without /".into());
                    cx.notify();
                    return false;
                }
                let name = self.save_name(cx).unwrap_or_else(|| OsString::from(name));
                vec![self.current_dir.join(name)]
            }
            PickerRequestKind::SaveMany(options) => {
                let directory = self
                    .selected
                    .iter()
                    .find(|path| path.is_dir())
                    .cloned()
                    .unwrap_or_else(|| self.current_dir.clone());
                options.files.iter().map(|name| directory.join(name)).collect()
            }
        };
        self.complete(PickerOutcome::Accepted {
            paths,
            choices: self
                .choices
                .iter()
                .map(|choice| (choice.id.clone(), choice.selected.clone()))
                .collect(),
            current_filter: self.active_filter().cloned(),
        });
        true
    }

    fn cancel(&mut self, _: &Cancel, window: &mut Window, cx: &mut Context<Self>) {
        if self.filter_open || self.choice_open.is_some() {
            self.filter_open = false;
            self.choice_open = None;
            cx.notify();
            return;
        }
        if self.path_editing {
            self.path_editing = false;
            self.error = None;
            window.focus(&self.focus_handle(cx));
            cx.notify();
            return;
        }
        self.complete(PickerOutcome::Cancelled);
        window.remove_window();
    }

    fn complete(&mut self, outcome: PickerOutcome) {
        self.windows.borrow_mut().remove(&self.request.handle);
        if let Some(sender) = self.response.take() {
            let _ = sender.try_send(outcome);
        }
    }

    fn show_recent(&mut self, cx: &mut Context<Self>) {
        self.location = PickerLocation::Recent;
        self.generation = self.generation.wrapping_add(1);
        self.loading = true;
        self.error = None;
        self.selected.clear();
        let generation = self.generation;
        let task = cx.background_executor().spawn(async move { recent_entries() });
        cx.spawn(async move |this, cx| {
            let entries = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.generation == generation {
                    this.loading = false;
                    this.snapshot = DirectorySnapshot {
                        generation,
                        path: PathBuf::from("recent:///"),
                        entries,
                        unreadable_entries: 0,
                    };
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn visible_entries(&self, cx: &App) -> Vec<FileEntry> {
        let query = self.search_input.read(cx).text().trim().to_owned();
        let active_filter = self.active_filter();
        let mut entries: Vec<_> = self
            .snapshot
            .entries
            .iter()
            .filter(|entry| active_filter.is_none_or(|filter| filter.matches(&entry.path)))
            .filter(|entry| query.is_empty() || fuzzy_match_score(&entry.name, &query).is_some())
            .cloned()
            .collect();
        if !query.is_empty() {
            entries.sort_by_key(|entry| {
                std::cmp::Reverse(fuzzy_match_score(&entry.name, &query).unwrap_or_default())
            });
        }
        entries
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let places = picker_places();
        let current = self.current_dir.clone();
        let mut sidebar = div()
            .w(px(190.0))
            .flex_none()
            .border_r_1()
            .border_color(rgb(border()))
            .p_3()
            .flex()
            .flex_col()
            .gap_1()
            .child(section_label("PLACES"));
        for (index, (label, path)) in places.into_iter().enumerate() {
            let active = self.location == PickerLocation::Directory && current == path;
            let icon = match label.as_str() {
                "Home" => "icons/folder-favorite.svg",
                "Downloads" => "icons/folder-downloads.svg",
                "Pictures" => "icons/folder-pictures.svg",
                "Documents" => "icons/folder-documents.svg",
                "Videos" => "icons/folder-videos.svg",
                "Music" => "icons/folder-music.svg",
                "Desktop" => "icons/folder-desktop.svg",
                _ => "icons/folder-closed.svg",
            };
            sidebar = sidebar.child(
                sidebar_item_with_icon(label.clone(), icon, active)
                    .id(("picker-place", index))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.navigate(path.clone(), cx);
                    })),
            );
        }
        if matches!(self.request.kind, PickerRequestKind::Open(_)) {
            sidebar = sidebar.child(
                sidebar_item("Recent", self.location == PickerLocation::Recent)
                    .id("picker-recent")
                    .on_click(cx.listener(|this, _, _, cx| this.show_recent(cx))),
            );
        }
        sidebar = sidebar.child(section_label("DEVICES").mt_4());
        for (index, device) in self.devices.iter().enumerate() {
            let icon = if device.removable {
                "icons/device-usb.svg"
            } else {
                "icons/device-drive.svg"
            };
            sidebar = sidebar.child(
                sidebar_item_with_icon(device.label.clone(), icon, false)
                    .id(("picker-device", index))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_device(index, cx);
                    })),
            );
        }
        sidebar.into_any_element()
    }

    fn render_toolbar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let path = self.current_dir.display().to_string();
        let path_field: AnyElement = if self.path_editing {
            div()
                .h_9()
                .flex_1()
                .min_w_0()
                .child(self.path_input.clone())
                .into_any_element()
        } else {
            div()
                .id("picker-breadcrumb")
                .h_9()
                .flex_1()
                .min_w_0()
                .px_3()
                .rounded_md()
                .border_1()
                .border_color(rgb(border()))
                .bg(rgb(surface_elevated()))
                .flex()
                .items_center()
                .cursor_pointer()
                .hover(|style| style.border_color(rgb(border_focused())))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.activate_path(&ActivatePath, window, cx);
                }))
                .child(div().flex_1().min_w_0().truncate().child(path))
                .child(div().text_xs().text_color(rgb(text_muted())).child("Ctrl+L"))
                .into_any_element()
        };
        div()
            .h(px(58.0))
            .px_4()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(rgb(border()))
            .child(
                nav_button("‹", !self.back.is_empty())
                    .id("picker-nav-back")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.go_back(&GoBack, window, cx);
                    })),
            )
            .child(
                nav_button("›", !self.forward.is_empty())
                    .id("picker-nav-forward")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.go_forward(&GoForward, window, cx);
                    })),
            )
            .child(
                nav_button("↑", self.current_dir.parent().is_some())
                    .id("picker-nav-up")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.go_up(&GoUp, window, cx);
                    })),
            )
            .child(path_field)
            .child(div().w(px(220.0)).h_9().child(self.search_input.clone()))
            .into_any_element()
    }

    fn render_file_list(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let entries = Arc::new(self.visible_entries(cx));
        let selected = self.selected.clone();
        let directory_only = matches!(&self.request.kind, PickerRequestKind::Open(options) if options.directory);
        if entries.is_empty() {
            let title = if self.loading {
                "Opening folder…"
            } else if self.error.is_some() {
                "This location is unavailable"
            } else {
                "Nothing matches here"
            };
            return div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .mt_6()
                .gap_3()
                .child(img("icons/empty-state.svg").size(px(220.0)))
                .child(div().text_sm().text_color(rgb(text())).child(title))
                .into_any_element();
        }
        uniform_list(
            "picker-file-list",
            entries.len(),
            cx.processor(move |_this, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|index| {
                        let entry = entries[index].clone();
                        let highlighted = selected.contains(&entry.path);
                        let disabled = directory_only && !entry_is_directory(&entry);
                        let is_directory = entry_is_directory(&entry);
                        let icon = file_icon_asset(&entry);
                        let name = entry.name.clone();
                        let detail = if is_directory {
                            "Folder".into()
                        } else {
                            size_label(entry.metadata.len)
                        };
                        div()
                            .id(("picker-entry", index))
                            .h(px(34.0))
                            .w_full()
                            .px_3()
                            .rounded_md()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_sm()
                            .when(highlighted, |row| row.bg(rgb(accent_background())))
                            .when(!highlighted && !disabled, |row| {
                                row.hover(|style| style.bg(rgb(surface_elevated())))
                            })
                            .text_color(if disabled {
                                rgb(text_muted())
                            } else {
                                rgb(text())
                            })
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                                this.click_entry(index, event, window, cx);
                            }))
                            .child(img(icon).size_5())
                            .child(div().flex_1().min_w_0().truncate().child(name))
                            .child(
                                div()
                                    .w(px(88.0))
                                    .text_right()
                                    .text_xs()
                                    .text_color(rgb(text_muted()))
                                    .child(detail),
                            )
                    })
                    .collect()
            }),
        )
        .flex_1()
        .into_any_element()
    }

    fn render_filter(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.filters.is_empty() {
            return None;
        }
        let label = self
            .active_filter()
            .map_or("All files", |filter| filter.label.as_str())
            .to_owned();
        Some(
            div()
                .relative()
                .child(
                    secondary_button(label)
                        .id("picker-filter-trigger")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.filter_open = !this.filter_open;
                            this.choice_open = None;
                            cx.notify();
                        })),
                )
                .when(self.filter_open, |root| {
                    root.child(self.render_filter_menu(cx))
                })
                .into_any_element(),
        )
    }

    fn render_filter_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        self.filters
            .iter()
            .enumerate()
            .fold(
                div()
                    .absolute()
                    .bottom(px(38.0))
                    .left_0()
                    .w(px(220.0))
                    .p_1()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(border_focused()))
                    .bg(rgb(surface()))
                    .shadow_lg()
                    .occlude(),
                |menu, (index, filter)| {
                    let label = filter.label.clone();
                    menu.child(
                        div()
                            .id(("picker-filter", index))
                            .h_8()
                            .px_2()
                            .rounded_md()
                            .flex()
                            .items_center()
                            .text_xs()
                            .when(self.filter_index == Some(index), |row| {
                                row.bg(rgb(accent_background()))
                            })
                            .hover(|row| row.bg(rgb(surface_elevated())))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.filter_index = Some(index);
                                this.filter_open = false;
                                this.selected.clear();
                                cx.notify();
                            }))
                            .child(label),
                    )
                },
            )
            .into_any_element()
    }

    fn render_choices(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let mut row = div().flex().items_center().gap_2();
        for index in 0..self.choices.len() {
            let choice = self.choices[index].clone();
            if choice.is_boolean() {
                let enabled = choice.selected == "true";
                row = row.child(
                    secondary_button(format!(
                        "{}: {}",
                        choice.label,
                        if enabled { "On" } else { "Off" }
                    ))
                        .id(("picker-boolean-choice", index))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.choices[index].selected = if enabled { "false" } else { "true" }.into();
                            cx.notify();
                        })),
                );
            } else {
                let selected_label = choice
                    .options
                    .iter()
                    .find(|(id, _)| *id == choice.selected)
                    .map_or(choice.selected.clone(), |(_, label)| label.clone());
                row = row.child(
                    div()
                        .relative()
                        .child(
                            secondary_button(format!("{}: {}", choice.label, selected_label))
                                .id(("picker-choice-trigger", index))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.choice_open = (this.choice_open != Some(index)).then_some(index);
                                    this.filter_open = false;
                                    cx.notify();
                                })),
                        )
                        .when(self.choice_open == Some(index), |root| {
                            root.child(self.render_choice_menu(index, cx))
                        }),
                );
            }
        }
        row.into_any_element()
    }

    fn render_choice_menu(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        self.choices[index]
            .options
            .iter()
            .cloned()
            .enumerate()
            .fold(
                div()
                    .absolute()
                    .bottom(px(38.0))
                    .left_0()
                    .w(px(220.0))
                    .p_1()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(border_focused()))
                    .bg(rgb(surface()))
                    .shadow_lg()
                    .occlude(),
                |menu, (option_index, (id, label))| {
                    let selected = self.choices[index].selected == id;
                    menu.child(
                        div()
                            .id(("picker-choice", option_index))
                            .h_8()
                            .px_2()
                            .rounded_md()
                            .flex()
                            .items_center()
                            .text_xs()
                            .when(selected, |row| row.bg(rgb(accent_background())))
                            .hover(|row| row.bg(rgb(surface_elevated())))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.choices[index].selected = id.clone();
                                this.choice_open = None;
                                cx.notify();
                            }))
                            .child(label),
                    )
                },
            )
            .into_any_element()
    }

    fn accept_label(&self) -> String {
        let common = match &self.request.kind {
            PickerRequestKind::Open(options) => &options.common,
            PickerRequestKind::Save(options) => &options.common,
            PickerRequestKind::SaveMany(options) => &options.common,
        };
        let default = match &self.request.kind {
            PickerRequestKind::Open(options) if options.directory => "Select Folder",
            PickerRequestKind::Open(_) => "Open",
            PickerRequestKind::Save(_) => "Save",
            PickerRequestKind::SaveMany(_) => "Select Folder",
        };
        let mut label = common
            .accept_label
            .as_deref()
            .unwrap_or(default)
            .replace('_', "");
        if self.allows_multiple() && self.selected.len() > 1 {
            label.push_str(&format!(" ({})", self.selected.len()));
        }
        label
    }

    fn can_accept(&self, cx: &App) -> bool {
        match &self.request.kind {
            PickerRequestKind::Open(options) if options.directory => true,
            PickerRequestKind::Open(_) => !self.selected.is_empty(),
            PickerRequestKind::Save(_) => self
                .name_input
                .as_ref()
                .is_some_and(|input| valid_save_name(input.read(cx).text())),
            PickerRequestKind::SaveMany(_) => true,
        }
    }

    fn save_name(&self, cx: &App) -> Option<OsString> {
        let display = self.name_input.as_ref()?.read(cx).text().to_owned();
        if self.initial_name_display.as_deref() == Some(display.as_str()) {
            self.initial_raw_name
                .clone()
                .or_else(|| Some(OsString::from(display)))
        } else {
            Some(OsString::from(display))
        }
    }

    fn conflict_warning(&self, cx: &App) -> Option<String> {
        match &self.request.kind {
            PickerRequestKind::Save(_) => {
                let name = self.name_input.as_ref()?.read(cx).text().to_owned();
                valid_save_name(&name)
                    .then(|| self.current_dir.join(self.save_name(cx).unwrap_or_else(|| OsString::from(name))))
                    .filter(|path| path.exists())
                    .map(|_| "A file with this name already exists; the app will decide whether to replace it.".into())
            }
            PickerRequestKind::SaveMany(options) => {
                let directory = self
                    .selected
                    .iter()
                    .find(|path| path.is_dir())
                    .unwrap_or(&self.current_dir);
                let count = options
                    .files
                    .iter()
                    .filter(|name| directory.join(name).exists())
                    .count();
                (count > 0).then(|| format!("{count} destination name(s) already exist."))
            }
            PickerRequestKind::Open(_) => None,
        }
    }
}

impl gpui::Focusable for Picker {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Drop for Picker {
    fn drop(&mut self) {
        self.windows.borrow_mut().remove(&self.request.handle);
        if let Some(sender) = self.response.take() {
            let _ = sender.try_send(PickerOutcome::Cancelled);
        }
    }
}

impl Render for Picker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let error = self.error.clone();
        let status = self.status.clone();
        let warning = self.conflict_warning(cx);
        let save_many_summary = match &self.request.kind {
            PickerRequestKind::SaveMany(options) => {
                Some(format!("Choose a destination for {} file(s).", options.files.len()))
            }
            _ => None,
        };
        let accept_enabled = self.can_accept(cx);
        let name_input = self.name_input.clone();
        let filter = self.render_filter(cx);
        let choices = (!self.choices.is_empty()).then(|| self.render_choices(cx));
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
            .font_family("Noto Sans")
            .child(self.render_toolbar(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.render_sidebar(cx))
                    .child(div().flex_1().min_w_0().p_3().child(self.render_file_list(cx))),
            )
            .child(
                div()
                    .min_h(px(72.0))
                    .px_4()
                    .py_3()
                    .border_t_1()
                    .border_color(rgb(border()))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .when_some(error.or(status).or(warning).or(save_many_summary), |row, message| {
                        row.child(
                            div()
                                .text_xs()
                                .text_color(rgb(if self.error.is_some() { danger() } else { text_muted() }))
                                .child(message),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .when_some(name_input, |row, input| {
                                row.child(div().w(px(280.0)).h_9().child(input))
                            })
                            .when_some(choices, |row, choices| row.child(choices))
                            .child(div().flex_1())
                            .when_some(filter, |row, filter| row.child(filter))
                            .child(
                                secondary_button("Cancel")
                                    .id("picker-cancel")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.cancel(&Cancel, window, cx);
                                    })),
                            )
                            .child(
                                primary_button(self.accept_label(), accept_enabled)
                                    .id("picker-accept")
                                    .when(accept_enabled, |button| {
                                        button.on_click(cx.listener(|this, _, window, cx| {
                                            this.accept(&Accept, window, cx);
                                        }))
                                    }),
                            ),
                    ),
            )
    }
}

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
                .and_then(|path| path.file_name().map(|name| SuggestedName {
                    display: name.to_string_lossy().into_owned(),
                    raw: Some(name.to_owned()),
                }))
                .or_else(|| options.current_name.clone().map(|display| SuggestedName {
                    display,
                    raw: None,
                }))
                .or_else(|| Some(SuggestedName {
                    display: String::new(),
                    raw: None,
                }));
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
        metadata: FileMetadata {
            len: metadata.len(),
            modified_unix_ms: None,
            mode,
            readonly: metadata.permissions().readonly(),
            symlink_target: file_type.is_symlink().then(|| fs::read_link(path).ok()).flatten(),
            mime: mime_guess::from_path(path).first_raw().map(str::to_owned),
        },
        git_status: None,
    })
}

fn entry_is_directory(entry: &FileEntry) -> bool {
    entry.kind == FileKind::Directory || (entry.kind == FileKind::Symlink && entry.path.is_dir())
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
        .when(!active, |row| row.hover(|style| style.bg(rgb(surface_elevated()))))
        .child(label.into())
}

fn sidebar_item_with_icon(
    label: impl Into<SharedString>,
    icon: &'static str,
    active: bool,
) -> gpui::Div {
    sidebar_item("", active)
        .gap_2()
        .child(img(icon).size_5())
        .child(label.into())
}

fn nav_button(label: &'static str, enabled: bool) -> gpui::Div {
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
        .child(label)
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
        FileKind::Directory if entry.metadata.readonly => "icons/folder-readonly.svg",
        FileKind::Directory => match entry.name.to_ascii_lowercase().as_str() {
            "downloads" => "icons/folder-downloads.svg",
            "pictures" | "images" | "photos" => "icons/folder-pictures.svg",
            "documents" | "docs" => "icons/folder-documents.svg",
            "videos" | "movies" => "icons/folder-videos.svg",
            "music" | "audio" => "icons/folder-music.svg",
            "desktop" => "icons/folder-desktop.svg",
            _ => "icons/folder-closed.svg",
        },
        FileKind::Symlink if entry.path.is_dir() => "icons/folder-symlink.svg",
        FileKind::File | FileKind::Symlink => match entry
            .path
            .extension()
            .and_then(OsStr::to_str)
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str()
        {
            "rs" | "js" | "jsx" | "ts" | "tsx" | "py" | "go" | "c" | "cpp" | "h"
            | "java" | "kt" | "lua" | "sh" | "nix" | "toml" | "json" | "yaml" => {
                "icons/file-code.svg"
            }
            "txt" | "md" | "rst" | "log" | "csv" => "icons/file-text.svg",
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "avif" | "svg" | "bmp" => {
                "icons/file-image.svg"
            }
            "pdf" | "doc" | "docx" | "odt" | "rtf" | "epub" => {
                "icons/file-document.svg"
            }
            "zip" | "tar" | "gz" | "bz2" | "xz" | "zst" | "7z" | "rar" => {
                "icons/file-archive.svg"
            }
            "mp3" | "flac" | "wav" | "ogg" | "mp4" | "mkv" | "webm" | "mov" => {
                "icons/file-media.svg"
            }
            _ => "icons/file-generic.svg",
        },
        FileKind::Other => "icons/file-generic.svg",
    }
}

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
