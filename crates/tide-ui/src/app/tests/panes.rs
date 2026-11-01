use super::*;

#[test]
fn header_arrows_restore_regions_and_dropdown_is_always_available() {
    let (_home, mut app) = test_app();
    app.new_file();
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(160, 40)).unwrap();
    for left in [true, false] {
        app.sidebar_left = left;
        app.agents_left = !left;
        app.sidebar_visible = true;
        terminal.draw(|f| app.draw(f)).unwrap();
        let pane = app.workbench.focused();
        assert!(
            app.layout
                .hits
                .iter()
                .any(|(_, hit)| *hit == HitTarget::TabOverflow(pane))
        );
        let width = app.sidebar_width;
        click_hit(&mut app, |t| {
            *t == HitTarget::Command(Command::WorkbenchToggleSidebar)
        });
        terminal.draw(|f| app.draw(f)).unwrap();
        assert!(app.layout.sidebar.is_none());
        click_hit(&mut app, |t| {
            *t == HitTarget::Command(Command::WorkbenchToggleSidebar)
        });
        terminal.draw(|f| app.draw(f)).unwrap();
        assert!(app.layout.sidebar.is_some());
        assert_eq!(app.sidebar_width, width);
        assert_ne!(
            terminal.backend().buffer()[(app.workbench.focused_pane().rect.x, 0)].symbol(),
            "┌"
        );
    }
    app.dock.visible = true;
    let height = app.dock.height;
    terminal.draw(|f| app.draw(f)).unwrap();
    click_hit(&mut app, |t| {
        *t == HitTarget::Command(Command::WorkbenchToggleTerminal)
    });
    terminal.draw(|f| app.draw(f)).unwrap();
    assert!(app.layout.dock.is_none());
    click_hit(&mut app, |t| {
        *t == HitTarget::Command(Command::WorkbenchToggleTerminal)
    });
    terminal.draw(|f| app.draw(f)).unwrap();
    assert!(app.layout.dock.is_some());
    assert_eq!(app.dock.height, height);
    assert_eq!(app.focus, Focus::Dock);
}

#[test]
fn pane_controls_preserve_dirty_views_and_panel_state_round_trips() {
    let (_home, mut app) = test_app();
    app.new_file();
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("keep dirty");
    app.split(SplitDirection::Vertical);
    let target = app.workbench.focused();
    app.close_pane(target);
    assert!(matches!(
        app.overlay.as_ref().unwrap().kind(),
        ModalKind::Confirm(ConfirmAction::ClosePane(_))
    ));
    app.handle_modal_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.workbench.pane_ids().len(), 2);
    app.close_pane(target);
    app.handle_modal_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
    assert_eq!(app.workbench.pane_ids().len(), 1);
    assert_eq!(
        app.workbench.active_editor().unwrap().document.text(),
        "keep dirty"
    );
    app.execute_command(Command::PaneAddPanel);
    assert_eq!(*app.overlay.as_ref().unwrap().kind(), ModalKind::AddPanel);
    assert!(
        app.overlay
            .as_ref()
            .unwrap()
            .base()
            .items
            .iter()
            .any(|item| item.value == "database.newQuery")
    );
    app.overlay = None;
    while !app.accordion.sections.is_empty() {
        app.accordion.remove(0);
    }
    app.execute_command(Command::SidebarAddOutline);
    assert_eq!(app.accordion.sections.len(), 1);
    app.accordion.toggle(0);
    app.execute_command(Command::WorkbenchMoveSidebarRight);
    app.execute_command(Command::WorkbenchMoveAgentsSidebarLeft);
    let saved = app.build_session();
    app.restore_session(saved).unwrap();
    assert_eq!(app.accordion.sections.len(), 1);
    assert_eq!(app.accordion.sections[0].id, SectionId::Outline);
    assert!(!app.accordion.sections[0].expanded);
    assert!(!app.sidebar_left);
    assert!(app.agents_left);
}

#[test]
fn responsive_split_dock_geometry_drag_and_status_targets_remain_actionable() {
    let (_home, mut app) = test_app();
    app.new_file();
    app.split(SplitDirection::Vertical);
    app.split(SplitDirection::Horizontal);
    app.dock.visible = true;
    app.ai.agents_visible = true;
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(160, 40)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let dock = app.layout.dock.unwrap();
    for id in app.workbench.pane_ids() {
        assert!(app.workbench.pane(id).unwrap().rect.bottom() <= dock.y);
    }
    assert_eq!(app.layout.sidebar.unwrap().height, 39);
    assert_eq!(app.layout.agents.unwrap().height, 39);
    let (rect, hit) = app
        .layout
        .hits
        .iter()
        .find(|(_, target)| {
            matches!(
                target,
                HitTarget::Resize(ResizeTarget::PaneBoundary(
                    _,
                    _,
                    SplitDirection::Vertical,
                    _
                ))
            )
        })
        .copied()
        .unwrap();
    let before = format!("{:?}", app.workbench.root());
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: rect.x,
        row: rect.y,
        modifiers: KeyModifiers::NONE,
    });
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: rect.x + 3,
        row: rect.y,
        modifiers: KeyModifiers::NONE,
    });
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: rect.x + 3,
        row: rect.y,
        modifiers: KeyModifiers::NONE,
    });
    assert_ne!(format!("{:?}", app.workbench.root()), before, "{hit:?}");
    let message = "a long status message ".repeat(30);
    app.set_status(&message);
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let rect = app
        .layout
        .hits
        .iter()
        .find(|(_, hit)| *hit == HitTarget::Command(Command::WorkbenchStatusHistory))
        .unwrap()
        .0;
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: rect.x,
        row: rect.y,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(
        *app.overlay.as_ref().unwrap().kind(),
        ModalKind::StatusHistory
    );
    app.handle_modal_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(
        app.overlay.as_ref().unwrap().base().message.as_deref(),
        Some(message.as_str())
    );
}

#[test]
fn bottom_tabs_close_the_clicked_terminal_and_share_overflow_picker() {
    let (_home, mut app) = test_app();
    for _ in 0..6 {
        app.open_terminal(false);
    }
    for (index, tab) in app.dock.tabs.iter_mut().enumerate() {
        tab.title = format!("terminal-with-long-title-{index}");
    }
    app.dock.active = 0;
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 40)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let rect = app
        .layout
        .hits
        .iter()
        .find(|(_, hit)| *hit == HitTarget::TabClose(DOCK_PANE, 0))
        .unwrap()
        .0;
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: rect.x,
        row: rect.y,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.dock.tabs.len(), 5);
    assert!(
        !app.dock
            .tabs
            .iter()
            .any(|tab| tab.title == "terminal-with-long-title-0")
    );
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let rect = app
        .layout
        .hits
        .iter()
        .find(|(_, hit)| *hit == HitTarget::TabOverflow(DOCK_PANE))
        .unwrap()
        .0;
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: rect.x,
        row: rect.y,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(
        *app.overlay.as_ref().unwrap().kind(),
        ModalKind::Tabs(DOCK_PANE)
    );
    app.overlay.as_mut().unwrap().base_mut().selected = 4;
    app.handle_modal_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.dock.active, 4);
    assert_eq!(app.focus, Focus::Dock);
    app.scroll_tabs(DOCK_PANE, false);
    assert!(app.dock.strip.manual);
}

#[test]
fn split_views_and_tree_restore_independent_selection_folds_and_shared_content() {
    let (_home, mut app) = test_app();
    let path = app.workspace.join("independent.py");
    std::fs::write(&path, "if ready:\n    first\n    second\nafter").unwrap();
    app.open_file(&path);
    app.split(SplitDirection::Horizontal);
    assert_eq!(app.workbench.pane_ids().len(), 2);
    let ids = app.workbench.pane_ids();
    app.workbench.set_focused(ids[0]);
    let first = app.workbench.active_editor_mut().unwrap();
    first.document.set_selection(tide_core::Selection {
        anchor: Position::new(3, 1),
        active: Position::new(3, 4),
    });
    first.restore_folds(&[0]);
    app.workbench.set_focused(ids[1]);
    let second = app.workbench.active_editor_mut().unwrap();
    second.document.set_cursor(Position::new(0, 3));
    let state = app.build_session();
    app.restore_session(state).unwrap();
    app.workbench.set_focused(ids[0]);
    let first = app.workbench.active_editor().unwrap();
    assert_eq!(first.selected_text().as_deref(), Some("fte"));
    assert_eq!(first.folded_ranges(), &[(0, 2)]);
    app.workbench.set_focused(ids[1]);
    let second = app.workbench.active_editor_mut().unwrap();
    assert_eq!(second.document.cursor().position, Position::new(0, 3));
    second.insert_text("!");
    app.workbench.set_focused(ids[0]);
    assert!(
        app.workbench
            .active_editor()
            .unwrap()
            .document
            .text()
            .starts_with("if !ready")
    );
}

#[test]
fn long_file_scrolls_beyond_the_first_viewport() {
    let (_home, mut app) = test_app();
    let path = app.workspace.join("long.rs");
    let content = (1..=200)
        .map(|line| format!("// line {line}\n"))
        .collect::<String>();
    std::fs::write(&path, content).unwrap();
    app.open_file(&path);
    let backend = ratatui::backend::TestBackend::new(100, 40);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let first = crate::test_support::screen_text(&terminal);
    assert!(first.contains("// line 1"));
    app.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let after_page = crate::test_support::screen_text(&terminal);
    assert!(
        after_page.contains("// line 21") || after_page.contains("// line 22"),
        "PageDown did not move the viewport; scroll_top={:?}",
        app.workbench
            .active_editor()
            .map(|editor| editor.scroll_top)
    );
}

#[test]
fn keyboard_navigates_between_main_panes() {
    let (_home, mut app) = test_app();
    app.execute_command(Command::ViewSplitVertical);
    let panes = app.workbench.pane_ids();
    assert_eq!(panes.len(), 2);
    let first_focus = app.workbench.focused();
    app.handle_key(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::CONTROL));
    let second_focus = app.workbench.focused();
    assert_ne!(
        first_focus, second_focus,
        "Ctrl+] did not focus the next pane"
    );
    app.handle_key(KeyEvent::new(KeyCode::Char('['), KeyModifiers::CONTROL));
    assert_eq!(
        app.workbench.focused(),
        first_focus,
        "Ctrl+[ did not focus the previous pane"
    );
    app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::CONTROL));
    assert_eq!(
        app.workbench.focused(),
        panes[1],
        "Ctrl+2 did not focus pane 2"
    );
    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT));
    assert_eq!(
        app.workbench.focused(),
        panes[0],
        "Alt+Right did not cycle panes"
    );
}

#[test]
fn outline_selection_jumps_to_the_symbol_line() {
    let (_home, mut app) = test_app();
    let path = app.workspace.join("outline.ts");
    std::fs::write(&path, "function alpha() {}\nfunction beta() {}\n").unwrap();
    app.open_file(&path);
    app.focus_sidebar_section(2, true);
    assert_eq!(app.accordion.focused_section(), SectionId::Outline);
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let cursor = app
        .workbench
        .active_editor()
        .unwrap()
        .document
        .cursor()
        .position;
    assert_eq!(cursor.line, 1);
}

#[test]
fn direct_sidebar_section_shortcuts_navigate_the_accordion() {
    let (_home, mut app) = test_app();
    app.handle_key(KeyEvent::new(
        KeyCode::Char('e'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    ));
    assert_eq!(app.focus, Focus::Sidebar);
    assert_eq!(app.accordion.focused_section(), SectionId::Explorer);
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(app.accordion.focused_section(), SectionId::SourceControl);
    app.handle_key(KeyEvent::new(
        KeyCode::Char('u'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    ));
    assert_eq!(app.accordion.focused_section(), SectionId::Outline);
    app.handle_key(KeyEvent::new(
        KeyCode::Char('l'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    ));
    assert_eq!(app.accordion.focused_section(), SectionId::Timeline);
}

#[test]
fn directional_navigation_crosses_regions_not_just_panes() {
    let (_home, mut app) = test_app();
    app.dock.visible = true;
    let backend = ratatui::backend::TestBackend::new(100, 40);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();

    app.focus = Focus::Sidebar;
    terminal.draw(|frame| app.draw(frame)).unwrap();
    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT));
    assert_eq!(
        app.focus,
        Focus::Main,
        "Alt+Right did not leave the sidebar"
    );
    app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::ALT));
    assert_eq!(
        app.focus,
        Focus::Sidebar,
        "Alt+Left did not enter the sidebar"
    );

    app.focus = Focus::Main;
    terminal.draw(|frame| app.draw(frame)).unwrap();
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT));
    assert_eq!(app.focus, Focus::Dock, "Alt+Down did not focus the dock");
    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT));
    assert_eq!(app.focus, Focus::Main, "Alt+Up did not return to main");
}

#[test]
fn keyboard_navigates_stacked_panes_directionally() {
    let (_home, mut app) = test_app();
    app.execute_command(Command::ViewSplitHorizontal);
    let panes = app.workbench.pane_ids();
    assert_eq!(panes.len(), 2);
    let backend = ratatui::backend::TestBackend::new(100, 40);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let bottom = app.workbench.focused();

    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT));
    assert_eq!(app.workbench.focused(), panes[0], "Alt+Up did not move up");
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT));
    assert_eq!(
        app.workbench.focused(),
        bottom,
        "Alt+Down did not move down"
    );
    // Ctrl+Alt+Up is the standard add-cursor binding in editors, so pane
    // navigation uses Alt+Up; multi-cursor is covered by editor tests.
}

#[test]
fn explorer_mouse_opens_expands_and_scrolls_the_rendered_rows() {
    let (_home, mut app) = test_app();
    let folder = app.workspace.join("src");
    std::fs::create_dir(&folder).unwrap();
    for index in 0..80 {
        std::fs::write(
            folder.join(format!("file{index:02}.rs")),
            "fn example() {}\n",
        )
        .unwrap();
    }
    app.file_tree.refresh();
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let content = app
        .layout
        .hits
        .iter()
        .find_map(|(rect, target)| {
            matches!(target, HitTarget::AccordionContent(0)).then_some(*rect)
        })
        .unwrap();
    let mouse = |kind, row| MouseEvent {
        kind,
        column: content.x + 2,
        row,
        modifiers: KeyModifiers::NONE,
    };
    app.handle_event(Event::Mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        content.y,
    )));
    assert_eq!(app.file_tree.entries.len(), 81);
    terminal.draw(|frame| app.draw(frame)).unwrap();
    for _ in 0..10 {
        app.handle_event(Event::Mouse(mouse(
            MouseEventKind::ScrollDown,
            content.y + 1,
        )));
        terminal.draw(|frame| app.draw(frame)).unwrap();
    }
    let screen = crate::test_support::screen_text(&terminal);
    let row = content.y + 1;
    let visible = screen.lines().nth(row as usize).unwrap();
    let name = visible
        .split_whitespace()
        .find(|text| text.starts_with("file") && text.ends_with(".rs"))
        .unwrap();
    let expected = folder.join(name);
    app.handle_event(Event::Mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        row,
    )));
    assert_eq!(
        app.workbench.active_editor().unwrap().path(),
        Some(expected.as_path())
    );
    assert_eq!(app.workbench.active_editor().unwrap().scroll_top, 0);
    // A wheel event over the tree must not scroll the editor.
    app.handle_event(Event::Mouse(mouse(
        MouseEventKind::ScrollDown,
        content.y + 1,
    )));
    assert_eq!(app.workbench.active_editor().unwrap().scroll_top, 0);
}

#[test]
fn cached_outline_follows_edits_and_equal_revision_document_switches() {
    let (_home, mut app) = test_app();
    let first = app.workspace.join("first.rs");
    let second = app.workspace.join("second.rs");
    std::fs::write(&first, "fn first() {}\n").unwrap();
    std::fs::write(&second, "fn second() {}\n").unwrap();
    app.open_file(first.clone());
    assert_eq!(app.outline_for_active()[0].1, "fn first() {}");
    app.workbench.active_editor_mut().unwrap().move_right(false);
    assert_eq!(app.outline_for_active()[0].1, "fn first() {}");
    app.open_file(second);
    assert_eq!(app.outline_for_active()[0].1, "fn second() {}");
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("// ");
    assert!(app.outline_for_active().is_empty());
    app.workbench.focus_editor(&first);
    assert_eq!(app.outline_for_active()[0].1, "fn first() {}");
}

#[test]
fn mouse_wheel_scrolls_without_snapping_back_to_the_cursor() {
    let (_home, mut app) = test_app();
    let path = app.workspace.join("long.rs");
    let content = (1..=200)
        .map(|line| format!("// line {line}\n"))
        .collect::<String>();
    std::fs::write(&path, content).unwrap();
    app.open_file(&path);
    let backend = ratatui::backend::TestBackend::new(100, 40);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 50,
        row: 15,
        modifiers: KeyModifiers::NONE,
    });
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let text = crate::test_support::screen_text(&terminal);
    assert!(
        !text.contains("// line 1\n"),
        "wheel scroll snapped back to cursor"
    );
}

#[test]
fn opens_and_closes_a_dock_terminal() {
    let (_home, mut app) = test_app();
    app.open_terminal(false);
    assert!(app.dock.visible);
    assert_eq!(app.dock.tabs.len(), 1);
    app.close_terminal();
    assert!(!app.dock.visible);
}

#[test]
fn tab_overflow_scrolls_and_picker_reveals_any_selected_tab() {
    let (_home, mut app) = test_app();
    let files = (0..12)
        .map(|index| app.workspace.join(format!("file-{index:02}-😀.txt")))
        .collect::<Vec<_>>();
    for file in &files {
        std::fs::write(file, "text").unwrap();
        app.open_file(file);
    }
    let pane = app.workbench.focused();
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 25)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let active = app.workbench.focused_pane().active;
    assert!(
        app.layout
            .hits
            .iter()
            .any(|(_, target)| *target == HitTarget::Tab(pane, active))
    );
    click_hit(&mut app, |target| {
        *target == HitTarget::TabScroll(pane, false)
    });
    terminal.draw(|frame| app.draw(frame)).unwrap();
    assert!(app.workbench.focused_pane().tab_scroll < active);
    click_hit(&mut app, |target| *target == HitTarget::TabOverflow(pane));
    let overlay = app.overlay.as_mut().unwrap();
    assert_eq!(overlay.base().items.len(), files.len() + 1); // Includes the Welcome tab.
    let first_index = overlay
        .base()
        .items
        .iter()
        .position(|item| item.label == files[0].file_name().unwrap().to_string_lossy())
        .unwrap();
    for ch in files[0].file_name().unwrap().to_string_lossy().chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    assert_eq!(app.overlay.as_ref().unwrap().base().filtered.len(), 1);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(
        app.workbench.active_editor().unwrap().path(),
        Some(files[0].as_path())
    );
    terminal.draw(|frame| app.draw(frame)).unwrap();
    assert!(
        app.layout
            .hits
            .iter()
            .any(|(_, target)| *target == HitTarget::Tab(pane, first_index))
    );
    let rect = app.workbench.focused_pane().rect;
    assert!(
        app.layout
            .hits
            .iter()
            .filter(|(_, target)| matches!(
                target,
                HitTarget::Tab(_, _) | HitTarget::TabClose(_, _)
            ))
            .all(|(target, _)| target.x >= rect.x && target.right() <= rect.right())
    );
}
