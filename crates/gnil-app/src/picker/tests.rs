use std::{cell::RefCell, collections::HashMap, path::PathBuf, rc::Rc, sync::Arc};

use gnil_core::{DirectoryLoadPhase, DirectorySnapshot, EntryMetadata, FileEntry, FileKind};
use gpui::{Entity, TestAppContext, VisualTestContext};

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
