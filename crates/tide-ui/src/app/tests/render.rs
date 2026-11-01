use super::*;

#[test]
fn transient_write_feedback_keeps_space_when_document_metadata_is_long() {
    let (_home, mut app) = test_app();
    let file = app
        .workspace
        .join("an-extremely-long-file-name-that-must-not-hide-the-save-confirmation.rs");
    std::fs::write(&file, "content").unwrap();
    app.open_file(&file);
    app.set_status("Edited hunk staged; working file preserved");
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 40)).unwrap();
    terminal.draw(|f| app.draw(f)).unwrap();
    let row = (0..120)
        .map(|x| terminal.backend().buffer()[(x, 39)].symbol())
        .collect::<String>();
    assert!(
        row.contains("Edited hunk staged") && row.contains("Ln 1, Col 1"),
        "{row}"
    );
}

#[test]
fn status_uses_active_document_selection_eol_and_settings_with_real_click_targets() {
    let (_home, mut app) = test_app();
    let file = app.workspace.join("status.rs");
    std::fs::write(&file, "hello\r\nworld\r\n").unwrap();
    app.open_file(&file);
    app.settings.set("editor.tabSize", serde_json::json!(8));
    app.settings
        .set("editor.insertSpaces", serde_json::json!(false));
    app.workbench
        .active_editor_mut()
        .unwrap()
        .document
        .set_selection(tide_core::Selection {
            anchor: Position::new(0, 0),
            active: Position::new(0, 3),
        });
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(160, 40)).unwrap();
    terminal.draw(|f| app.draw(f)).unwrap();
    let row = (0..160)
        .map(|x| terminal.backend().buffer()[(x, 39)].symbol())
        .collect::<String>();
    assert!(
        row.contains("Ln 1, Col 4")
            && row.contains("3 selected")
            && row.contains("Rust")
            && row.contains("Tab Size: 8")
            && row.contains("CRLF"),
        "{row}"
    );
    assert!(!row.contains("Files") && !row.contains("Agents"));
    click_hit(&mut app, |t| {
        *t == HitTarget::Command(Command::EditorGotoLine)
    });
    assert!(matches!(
        app.overlay.as_ref().unwrap().kind(),
        ModalKind::Prompt(PromptAction::GotoLine)
    ));
    app.overlay = None;
    app.new_file();
    terminal.draw(|f| app.draw(f)).unwrap();
    let row = (0..160)
        .map(|x| terminal.backend().buffer()[(x, 39)].symbol())
        .collect::<String>();
    assert!(
        row.contains("Untitled") && !row.contains("CRLF") && !row.contains("3 selected"),
        "{row}"
    );
}

#[test]
fn constrained_workbench_frames_and_overlays_never_escape_the_viewport() {
    let (_home, mut app) = test_app();
    app.new_file();
    app.split(SplitDirection::Vertical);
    app.ai.agents_visible = true;
    app.dock.visible = true;
    for (width, height) in [(1, 1), (10, 4), (20, 7), (40, 12), (82, 26)] {
        for focus in [
            Focus::Main,
            Focus::Sidebar,
            Focus::Agents,
            Focus::Dock,
            Focus::Status,
        ] {
            app.focus = focus;
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| app.draw(frame)).unwrap();
            let bounds = Rect::new(0, 0, width, height);
            for (rect, hit) in &app.layout.hits {
                if !rect.is_empty() {
                    assert_eq!(rect.intersection(bounds), *rect, "{width}x{height} {hit:?}");
                }
            }
        }
    }
}

#[test]
fn word_wrap_renders_continuations_without_changing_content() {
    let (_home, mut app) = test_app();
    app.new_file();
    app.sidebar_visible = false;
    let content = format!("{}END", "x".repeat(100));
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text(&content);
    app.settings
        .set("editor.wordWrap", serde_json::Value::String("off".into()));
    app.execute_command(Command::WorkbenchToggleWordWrap);
    let backend = ratatui::backend::TestBackend::new(50, 12);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let screen = (0..12)
        .map(|y| {
            (0..50)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    // A continuation may split the sentinel after a width change.
    assert!(
        screen
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .contains("END"),
        "{screen}"
    );
    assert_eq!(
        app.workbench.active_editor().unwrap().document.text(),
        content
    );
    assert!(app.workbench.active_editor().unwrap().word_wrap_enabled());
    assert!(!command_supported("workbench.toggleMinimap"));
}

#[test]
fn renders_the_real_app() {
    let (_home, mut app) = test_app();
    app.new_file();
    app.workbench
        .active_editor_mut()
        .unwrap()
        .insert_text("fn main() {}\n");
    let backend = ratatui::backend::TestBackend::new(100, 30);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let text = crate::test_support::screen_text(&terminal);
    let rows = text.lines().collect::<Vec<_>>();
    let cells = |y: u16, x: u16, width: u16| {
        rows[y as usize]
            .chars()
            .skip(x as usize)
            .take(width as usize)
            .collect::<String>()
    };
    let sidebar = app.layout.sidebar.unwrap();
    assert!(
        cells(sidebar.y, sidebar.x, sidebar.width).starts_with(" ▼ Explorer"),
        "{text}"
    );
    let pane = first_pane(&app);
    let title = app.workbench.active_tab().unwrap().title();
    assert!(
        cells(pane.tab_bar.y, pane.tab_bar.x, pane.tab_bar.width).contains(&format!(" {title} ")),
        "{text}"
    );
    let first = cells(pane.content.y, pane.content.x, pane.content.width);
    let second = cells(pane.content.y + 1, pane.content.x, pane.content.width);
    let (number, code) = first.trim_start().split_once(' ').unwrap();
    assert_eq!(number, "1", "{first:?}");
    assert!(code.trim_start().starts_with("fn main() {}"), "{first:?}");
    assert_eq!(second.split_whitespace().next(), Some("2"), "{second:?}");
    assert!(!second.contains("fn"), "{second:?}");
    let cursor = app.workbench.active_editor().unwrap().cursor_screen();
    let (x, y) = cursor.expect("the focused editor shows its cursor");
    assert_eq!(y, pane.content.y + 1);
    assert_eq!(
        terminal.get_cursor_position().unwrap(),
        ratatui::layout::Position { x, y }
    );
}

fn first_pane(app: &App) -> PaneLayout {
    app.layout
        .main_items
        .iter()
        .find_map(|item| match item {
            MainItem::Pane(pane) => Some(*pane),
            MainItem::Divider(_) => None,
        })
        .expect("the workbench has a pane")
}

/// Key cells of the workbench, with the theme colors they are drawn in.
#[test]
fn workbench_regions_draw_in_theme_colors() {
    let (_home, app, terminal) = pinned_workbench(120, 40);
    let buffer = terminal.backend().buffer();
    let palette = app.palette;
    let sidebar = app.layout.sidebar.unwrap();
    let status = app.layout.status.unwrap();
    let pane = first_pane(&app);
    let find = |area: Rect, symbol: &str| {
        (area.x..area.right())
            .find(|x| buffer[(*x, area.y)].symbol() == symbol)
            .map(|x| (x, area.y))
            .unwrap_or_else(|| panic!("no {symbol:?} in {area:?}"))
    };
    let probes = [
        ("explorer header", find(sidebar, "E")),
        ("sidebar body", (sidebar.x + 1, sidebar.y + 6)),
        ("active tab", find(pane.tab_bar, "m")),
        ("line number", find(pane.content, "1")),
        ("status bar", find(status, ">")),
    ];
    let mut summary = String::new();
    for (name, (x, y)) in probes {
        let cell = &buffer[(x, y)];
        summary.push_str(&format!(
            "{name}: {:?} fg={:?} bg={:?} {:?}\n",
            cell.symbol(),
            cell.fg,
            cell.bg,
            cell.modifier
        ));
    }
    let cell = |(x, y): (u16, u16)| &buffer[(x, y)];
    assert!(app.layout.sections[0].focused);
    let header = cell(find(sidebar, "E"));
    assert_eq!(
        (header.fg, header.bg),
        (palette.section_focused_fg, palette.section_focused_bg)
    );
    assert!(header.modifier.contains(Modifier::BOLD));
    assert_eq!(cell((sidebar.x + 1, sidebar.y + 6)).bg, palette.sidebar);
    // The focused pane draws its tab strip in the host palette.
    assert_eq!(app.focus, Focus::Main);
    let host = palette.host(true, false);
    let tab = cell(find(pane.tab_bar, "m"));
    assert_eq!((tab.fg, tab.bg), (host.tab_active_fg, host.tab_active));
    assert!(tab.modifier.contains(Modifier::BOLD));
    assert_ne!(
        cell(find(pane.content, "1")).fg,
        cell(find(pane.content, "=")).fg,
        "the gutter is not drawn like code"
    );
    // Word wrap is on by default; the cursor line still gets its background.
    assert_eq!(
        app.workbench
            .active_editor()
            .unwrap()
            .document
            .cursor()
            .position
            .line,
        0
    );
    assert_eq!(cell(find(pane.content, "1")).bg, palette.line_highlight);
    let item = cell(find(status, ">"));
    assert_eq!(
        (item.fg, item.bg),
        (palette.status_foreground, palette.status)
    );
    assert_ne!(palette.sidebar, palette.background);
    assert_ne!(palette.status, palette.background);
    insta::assert_snapshot!("workbench_styles_120x40", summary);
}

#[test]
fn snapshot_workbench_120x40() {
    insta::assert_snapshot!("app_120x40", snapshot_app(120, 40));
}

#[test]
fn snapshot_workbench_82x26() {
    insta::assert_snapshot!("app_82x26", snapshot_app(82, 26));
}

#[test]
fn snapshot_workbench_160x50() {
    insta::assert_snapshot!("app_160x50", snapshot_app(160, 50));
}

#[test]
fn snapshot_command_palette() {
    insta::assert_snapshot!(
        "modal_command_palette",
        snapshot_modal(120, 40, |app| app.open_command_palette())
    );
}

#[test]
fn snapshot_quick_open() {
    insta::assert_snapshot!(
        "modal_quick_open",
        snapshot_modal(120, 40, |app| {
            app.open_quick_open();
            app.settle();
        })
    );
}

#[test]
fn snapshot_goto_prompt() {
    insta::assert_snapshot!(
        "modal_goto_line",
        snapshot_modal(120, 40, |app| app.open_goto_prompt())
    );
}

#[test]
fn snapshot_save_as_browser() {
    insta::assert_snapshot!(
        "modal_save_as",
        snapshot_modal(120, 40, |app| {
            std::fs::write(app.workspace.join("README.md"), "# Tide").unwrap();
            app.open_save_as_prompt();
        })
    );
}

#[test]
fn snapshot_confirm() {
    insta::assert_snapshot!(
        "modal_confirm",
        snapshot_modal(120, 40, |app| {
            app.overlay = Some(modal::confirm(
                ConfirmAction::CloseDirty,
                "Unsaved Changes",
                "Discard changes in this tab?",
            ));
        })
    );
}
