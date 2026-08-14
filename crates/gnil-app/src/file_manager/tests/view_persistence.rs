#[gpui::test]
fn view_preferences_are_restored_from_the_saved_config(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let initial_path = temp.path().to_path_buf();
    let config_paths = test_config_paths(temp.path());
    let saved_paths = config_paths.clone();
    let (manager, cx) = cx.add_window_view(move |_, cx| {
        let mut manager = FileManager::new(&initial_path, ThemeAppearance::Dark, cx);
        manager.config_paths = config_paths;
        manager.settings.file_layout = FileLayout::Details;
        manager.preview_visible = true;
        manager.settings.preview_enabled = true;
        manager.tab.show_hidden = false;
        manager.settings.show_hidden = false;
        manager
    });

    cx.update(|window, cx| {
        manager.update(cx, |manager, cx| {
            manager.set_file_layout(FileLayout::Grid, cx);
            manager.toggle_preview(&TogglePreview, window, cx);
            manager.toggle_hidden(&ToggleHidden, window, cx);
        });
    });

    let restored = saved_paths.load_settings().unwrap();
    assert_eq!(restored.file_layout, FileLayout::Grid);
    assert!(!restored.preview_enabled);
    assert!(restored.show_hidden);
}
