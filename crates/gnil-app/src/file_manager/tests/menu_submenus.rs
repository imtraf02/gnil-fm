#[gpui::test]
fn empty_space_submenu_aligns_with_its_parent_item(cx: &mut TestAppContext) {
    let (manager, cx) = cx.add_window_view(|_, cx| {
        FileManager::new(Path::new("/tmp"), ThemeAppearance::Dark, cx)
    });
    manager.update(cx, |manager, cx| {
        manager.reduced_motion = true;
        let mut menu = EmptySpaceMenuState::new(
            point(px(120.0), px(120.0)),
            EmptySpaceMenuContext {
                capabilities: EmptySpaceMenuCapabilities {
                    clipboard_valid: false,
                    operation_running: false,
                    has_entries: true,
                },
                sort: SortSpec::default(),
                view: EmptySpaceViewState { show_hidden: false },
            },
            1,
        );
        menu.open_submenu(EmptySpaceSubmenu::SortBy);
        manager.empty_space_menu = Some(menu);
        cx.notify();
    });
    cx.run_until_parked();

    let trigger = cx
        .debug_bounds("empty-space-submenu-trigger-SortBy")
        .expect("Sort by trigger should be rendered");
    let submenu = cx
        .debug_bounds("empty-space-submenu-panel")
        .expect("empty-space submenu should be rendered");
    assert_eq!(submenu.top(), trigger.top());
}

#[gpui::test]
fn action_submenu_aligns_with_open_with_item(cx: &mut TestAppContext) {
    let (manager, cx) = cx.add_window_view(|_, cx| {
        FileManager::new(Path::new("/tmp"), ThemeAppearance::Dark, cx)
    });
    manager.update(cx, |manager, cx| {
        manager.reduced_motion = true;
        let mut menu = ActionMenuState::new(
            ActionMenuPlacement::Cursor(point(px(120.0), px(120.0))),
            MenuContext {
                selected_count: 1,
                open_with_eligible: true,
                ..MenuContext::default()
            },
            1,
        );
        menu.open_open_with_submenu(PathBuf::from("/tmp/file.txt"), "text/plain".into());
        manager.action_menu = Some(menu);
        cx.notify();
    });
    cx.run_until_parked();

    let trigger = cx
        .debug_bounds("file-submenu-trigger-OpenWith")
        .expect("Open with trigger should be rendered");
    let submenu = cx
        .debug_bounds("open-with-submenu-panel")
        .expect("Open with submenu should be rendered");
    assert_eq!(submenu.top(), trigger.top());
}

#[gpui::test]
fn empty_space_submenu_flips_at_viewport_edges_without_moving_parent(cx: &mut TestAppContext) {
    let (manager, cx) = cx.add_window_view(|_, cx| {
        FileManager::new(Path::new("/tmp"), ThemeAppearance::Dark, cx)
    });
    cx.simulate_resize(size(px(640.0), px(480.0)));
    cx.run_until_parked();
    manager.update(cx, |manager, cx| {
        manager.reduced_motion = true;
        manager.empty_space_menu = Some(EmptySpaceMenuState::new(
            point(px(630.0), px(470.0)),
            EmptySpaceMenuContext {
                capabilities: EmptySpaceMenuCapabilities {
                    clipboard_valid: false,
                    operation_running: false,
                    has_entries: true,
                },
                sort: SortSpec::default(),
                view: EmptySpaceViewState { show_hidden: false },
            },
            1,
        ));
        cx.notify();
    });
    cx.run_until_parked();
    let root_before = cx
        .debug_bounds("empty-space-menu-panel")
        .expect("root menu should be rendered");

    manager.update(cx, |manager, cx| {
        manager
            .empty_space_menu
            .as_mut()
            .expect("root menu")
            .open_submenu(EmptySpaceSubmenu::SortBy);
        cx.notify();
    });
    cx.run_until_parked();

    let root_after = cx
        .debug_bounds("empty-space-menu-panel")
        .expect("root menu should remain rendered");
    let submenu = cx
        .debug_bounds("empty-space-submenu-panel")
        .expect("submenu should be rendered");
    assert_eq!(root_after, root_before);
    assert!(submenu.right() <= root_after.left());
    assert!(submenu.left() >= px(4.0));
    assert!(submenu.top() >= px(4.0));
    assert!(submenu.right() <= px(636.0));
    assert!(submenu.bottom() <= px(476.0));
}

#[gpui::test]
fn open_with_async_content_keeps_both_menu_origins_stable(cx: &mut TestAppContext) {
    let (manager, cx) = cx.add_window_view(|_, cx| {
        FileManager::new(Path::new("/tmp"), ThemeAppearance::Dark, cx)
    });
    cx.simulate_resize(size(px(640.0), px(480.0)));
    cx.run_until_parked();
    manager.update(cx, |manager, cx| {
        manager.reduced_motion = true;
        let mut menu = ActionMenuState::new(
            ActionMenuPlacement::Cursor(point(px(630.0), px(470.0))),
            MenuContext {
                selected_count: 1,
                open_with_eligible: true,
                ..MenuContext::default()
            },
            1,
        );
        menu.open_open_with_submenu(PathBuf::from("/tmp/file.txt"), "text/plain".into());
        manager.action_menu = Some(menu);
        cx.notify();
    });
    cx.run_until_parked();
    let root_before = cx
        .debug_bounds("file-action-menu-panel")
        .expect("action menu should be rendered");
    let submenu_before = cx
        .debug_bounds("open-with-submenu-panel")
        .expect("loading submenu should be rendered");

    manager.update(cx, |manager, cx| {
        let submenu = manager
            .action_menu
            .as_mut()
            .and_then(|menu| menu.open_with_submenu.as_mut())
            .expect("Open with submenu");
        submenu.loading = false;
        submenu.applications = (0..5)
            .map(|index| DesktopApplication {
                desktop_id: format!("editor-{index}.desktop"),
                name: format!("Editor {index}"),
                generic_name: Some("Text Editor".into()),
                desktop_file: PathBuf::from(format!("/tmp/editor-{index}.desktop")),
                is_default: index == 0,
                compatible: true,
                declared_compatible: true,
            })
            .collect();
        cx.notify();
    });
    cx.run_until_parked();

    let root_after = cx
        .debug_bounds("file-action-menu-panel")
        .expect("action menu should remain rendered");
    let submenu_after = cx
        .debug_bounds("open-with-submenu-panel")
        .expect("loaded submenu should be rendered");
    assert_eq!(root_after, root_before);
    assert_eq!(submenu_after.origin, submenu_before.origin);
    assert!(submenu_after.right() <= root_after.left());
    assert!(submenu_after.left() >= px(4.0));
    assert!(submenu_after.top() >= px(4.0));
    assert!(submenu_after.right() <= px(636.0));
    assert!(submenu_after.bottom() <= px(476.0));
}
