use std::{cell::RefCell, collections::HashMap, fmt::Write as _, path::PathBuf, rc::Rc, sync::Arc};

use gnil_core::{DirectoryLoadPhase, DirectorySnapshot, EntryMetadata, FileEntry, FileKind};
use gpui::{Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext, point, px};
use tempfile::tempdir;

use super::*;
use crate::portal_protocol::{CommonOptions, FilterRule, OpenFileOptions, SaveFileOptions};

fn common() -> CommonOptions {
    CommonOptions {
        accept_label: None,
        modal: true,
        choices: Vec::new(),
        current_folder: Some(PathBuf::from("/tmp")),
    }
}

fn request(kind: PickerRequestKind) -> PickerRequest {
    PickerRequest {
        handle: "/org/freedesktop/portal/desktop/request/test/picker".into(),
        app_id: "org.example.Test".into(),
        parent_window: String::new(),
        title: "Choose a file".into(),
        kind,
    }
}

fn add_picker(
    cx: &mut TestAppContext,
    kind: PickerRequestKind,
) -> (Entity<Picker>, &mut VisualTestContext) {
    let (response, _outcomes) = async_channel::bounded(1);
    let windows = Rc::new(RefCell::new(HashMap::new()));
    cx.add_window_view(|window, cx| Picker::new(request(kind), response, windows, window, cx))
}

#[gpui::test]
fn open_mode_requires_a_selection(cx: &mut TestAppContext) {
    let (open, cx) = add_picker(
        cx,
        PickerRequestKind::Open(OpenFileOptions {
            common: common(),
            multiple: false,
            directory: false,
            filters: Vec::new(),
            current_filter: None,
        }),
    );
    assert!(!cx.read(|cx| open.read(cx).can_accept(cx)));
    open.update(cx, |picker, cx| {
        picker.selected.insert(PathBuf::from("/tmp/file.txt"));
        cx.notify();
    });
    assert!(cx.read(|cx| open.read(cx).can_accept(cx)));
}

#[gpui::test]
fn directory_mode_accepts_the_current_folder(cx: &mut TestAppContext) {
    let (directory, cx) = add_picker(
        cx,
        PickerRequestKind::Open(OpenFileOptions {
            common: common(),
            multiple: false,
            directory: true,
            filters: Vec::new(),
            current_filter: None,
        }),
    );
    assert!(cx.read(|cx| directory.read(cx).can_accept(cx)));
}

#[gpui::test]
fn save_mode_accepts_a_valid_prefilled_name(cx: &mut TestAppContext) {
    let (save, cx) = add_picker(
        cx,
        PickerRequestKind::Save(SaveFileOptions {
            common: common(),
            current_name: Some("report.txt".into()),
            current_file: None,
            filters: Vec::new(),
            current_filter: None,
        }),
    );
    assert!(cx.read(|cx| save.read(cx).can_accept(cx)));
}

#[gpui::test]
fn active_filter_keeps_folders_and_matching_files(cx: &mut TestAppContext) {
    let filter = PortalFilter {
        label: "Text".into(),
        rules: vec![FilterRule::Glob("*.txt".into())],
    };
    let (picker, cx) = add_picker(
        cx,
        PickerRequestKind::Open(OpenFileOptions {
            common: common(),
            multiple: true,
            directory: false,
            filters: vec![filter.clone()],
            current_filter: Some(filter),
        }),
    );
    let visible = picker.update(cx, |picker, cx| {
        picker.snapshot = Arc::new(DirectorySnapshot {
            generation: 1,
            path: PathBuf::from("/tmp"),
            entries: vec![
                FileEntry {
                    path: PathBuf::from("/tmp/nonexistent-folder"),
                    name: "nonexistent-folder".into(),
                    kind: FileKind::Directory,
                    hidden: false,
                    metadata: EntryMetadata::Pending,
                    git_status: None,
                },
                FileEntry {
                    path: PathBuf::from("/tmp/notes.txt"),
                    name: "notes.txt".into(),
                    kind: FileKind::File,
                    hidden: false,
                    metadata: EntryMetadata::Pending,
                    git_status: None,
                },
                FileEntry {
                    path: PathBuf::from("/tmp/photo.png"),
                    name: "photo.png".into(),
                    kind: FileKind::File,
                    hidden: false,
                    metadata: EntryMetadata::Pending,
                    git_status: None,
                },
            ],
            unreadable_entries: 0,
            phase: DirectoryLoadPhase::Complete,
        });
        picker.visible_entries_key = None;
        picker.visible_indices(cx).to_vec()
    });

    assert_eq!(visible, vec![0, 1]);
}

#[gpui::test]
fn picker_list_has_height_and_renders_scanned_entries(cx: &mut TestAppContext) {
    let (picker, cx) = add_picker(
        cx,
        PickerRequestKind::Open(OpenFileOptions {
            common: common(),
            multiple: false,
            directory: false,
            filters: Vec::new(),
            current_filter: None,
        }),
    );
    picker.update(cx, |picker, cx| {
        picker.file_layout = FileLayout::Details;
        picker.snapshot = Arc::new(DirectorySnapshot {
            generation: 1,
            path: PathBuf::from("/tmp"),
            entries: vec![FileEntry {
                path: PathBuf::from("/tmp/visible.txt"),
                name: "visible.txt".into(),
                kind: FileKind::File,
                hidden: false,
                metadata: EntryMetadata::Pending,
                git_status: None,
            }],
            unreadable_entries: 0,
            phase: DirectoryLoadPhase::Complete,
        });
        picker.visible_entries_key = None;
        cx.notify();
    });
    cx.run_until_parked();

    let row = cx
        .debug_bounds("picker-entry-0")
        .expect("the first scanned entry should be rendered");
    assert!(row.size.height > gpui::px(0.0));
}

#[gpui::test]
fn picker_layout_selector_persists_the_grid_choice(cx: &mut TestAppContext) {
    let config_root = tempdir().unwrap();
    let config_paths = ConfigPaths {
        config: config_root.path().join("config.toml"),
        keymap: config_root.path().join("keymap.toml"),
        session: config_root.path().join("session.json"),
        cache: config_root.path().join("cache"),
        journal: config_root.path().join("jobs.jsonl"),
    };
    let saved_paths = config_paths.clone();
    let (picker, cx) = add_picker(
        cx,
        PickerRequestKind::Open(OpenFileOptions {
            common: common(),
            multiple: false,
            directory: false,
            filters: Vec::new(),
            current_filter: None,
        }),
    );
    picker.update(cx, |picker, cx| {
        picker.config_paths = config_paths;
        picker.file_layout = FileLayout::Details;
        cx.notify();
    });
    cx.run_until_parked();

    let grid = cx
        .debug_bounds("picker-layout-grid")
        .expect("picker grid layout option should be rendered");
    cx.simulate_click(
        point(
            grid.left() + grid.size.width / 2.0,
            grid.top() + grid.size.height / 2.0,
        ),
        Modifiers::none(),
    );

    assert_eq!(cx.read(|cx| picker.read(cx).file_layout), FileLayout::Grid);
    assert_eq!(
        saved_paths.load_settings().unwrap().file_layout,
        FileLayout::Grid
    );
}

#[gpui::test]
fn picker_sidebar_renders_configured_favorites(cx: &mut TestAppContext) {
    let favorite_root = tempdir().unwrap();
    let favorite = favorite_root.path().join("favorite");
    std::fs::create_dir(&favorite).unwrap();
    let (picker, cx) = add_picker(
        cx,
        PickerRequestKind::Open(OpenFileOptions {
            common: common(),
            multiple: false,
            directory: false,
            filters: Vec::new(),
            current_filter: None,
        }),
    );
    picker.update(cx, |picker, cx| {
        picker.favorites = vec![favorite];
        cx.notify();
    });
    cx.run_until_parked();

    assert!(
        cx.debug_bounds("picker-favorite-0").is_some(),
        "configured favorites should be visible in the picker sidebar"
    );
}

#[test]
fn recent_xbel_reader_caps_input_size() {
    let temporary = tempdir().unwrap();
    let path = temporary.path().join("recently-used.xbel");
    std::fs::write(
        &path,
        vec![b'x'; usize::try_from(MAX_RECENT_XBEL_BYTES).unwrap() + 1024],
    )
    .unwrap();

    let source = read_recent_source(&path).unwrap();

    assert_eq!(
        source.len(),
        usize::try_from(MAX_RECENT_XBEL_BYTES).unwrap()
    );
}

#[test]
fn recent_xbel_handles_a_bookmark_with_many_attributes() {
    let mut source = String::from("<xbel><bookmark");
    for index in 0..512 {
        write!(source, " data-{index}=\"value\"").unwrap();
    }
    source.push_str(" href=\"file:///tmp/report.txt\"></bookmark></xbel>");

    assert_eq!(
        recent_paths_from_source(source.as_bytes()),
        [PathBuf::from("/tmp/report.txt")]
    );
}

#[gpui::test]
fn open_dropdown_does_not_block_clicks_outside(cx: &mut TestAppContext) {
    let favorite_root = tempdir().unwrap();
    let favorite = favorite_root.path().join("favorite");
    std::fs::create_dir(&favorite).unwrap();
    let filter = PortalFilter {
        label: "Text".into(),
        rules: vec![FilterRule::Glob("*.txt".into())],
    };
    let (picker, cx) = add_picker(
        cx,
        PickerRequestKind::Open(OpenFileOptions {
            common: common(),
            multiple: false,
            directory: false,
            filters: vec![filter.clone()],
            current_filter: Some(filter),
        }),
    );
    picker.update(cx, |picker, cx| {
        picker.favorites = vec![favorite.clone()];
        picker.filter_open = true;
        picker.reduced_motion = true;
        cx.notify();
    });
    cx.run_until_parked();

    let favorite_row = cx
        .debug_bounds("picker-favorite-0")
        .expect("favorite row should remain visible behind the dropdown");
    let position = point(
        favorite_row.left() + px(10.0),
        favorite_row.top() + px(10.0),
    );
    cx.simulate_mouse_down(position, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(position, MouseButton::Left, Modifiers::none());

    cx.read(|cx| {
        let picker = picker.read(cx);
        assert!(!picker.filter_open);
        assert_eq!(picker.current_dir, favorite);
    });
}
