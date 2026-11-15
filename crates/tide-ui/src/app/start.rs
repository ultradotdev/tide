//! The start page shown in a pane with nothing else open, and the release notes
//! opened once after an upgrade.
use super::*;

/// How many recently opened files the start page lists.
const RECENT_FILES: usize = 6;
/// How many saved sessions the start page lists.
const SESSIONS: usize = 4;
/// Panes at least this wide show the start page in two columns.
const TWO_COLUMNS: u16 = 74;
const LEFT_COLUMN: u16 = 42;

/// Something on the start page that Enter or a click runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StartItem {
    Command(Command, &'static str),
    File(PathBuf),
    Session {
        name: String,
        title: String,
        detail: String,
    },
    Learn(Command, String),
}

const START_COMMANDS: &[(Command, &str)] = &[
    (Command::WorkbenchQuickOpen, "Open a file…"),
    (Command::FileNew, "New file"),
    (Command::TerminalNewInPane, "New terminal tab"),
    (Command::AgentsNewSession, "Start an agent…"),
    (Command::GitFocusPanel, "Source Control"),
    (Command::WorkbenchCommandPalette, "Command palette"),
];

impl App {
    /// Everything the start page can run, in the order the arrow keys visit it.
    pub(crate) fn start_items(&self) -> Vec<StartItem> {
        let mut items = START_COMMANDS
            .iter()
            .map(|&(command, label)| StartItem::Command(command, label))
            .collect::<Vec<_>>();
        items.extend(
            self.recent_files
                .iter()
                .take(RECENT_FILES)
                .cloned()
                .map(StartItem::File),
        );
        let now = timestamp_millis();
        items.extend(self.start_sessions.iter().take(SESSIONS).map(|session| {
            let files = match session.documents {
                1 => "1 file".to_owned(),
                count => format!("{count} files"),
            };
            let detail = match session.saved_millis {
                Some(saved) => format!(
                    "{files} · {}",
                    super::session::age(now.saturating_sub(saved))
                ),
                None => files,
            };
            StartItem::Session {
                name: session.name.clone(),
                title: session.title.clone(),
                detail,
            }
        }));
        items.extend([
            StartItem::Learn(Command::HelpWelcome, "Welcome guide".to_owned()),
            StartItem::Learn(
                Command::HelpReleaseNotes,
                format!("What's new in {}", env!("CARGO_PKG_VERSION")),
            ),
            StartItem::Learn(
                Command::HelpKeyboardShortcuts,
                "Keyboard shortcuts".to_owned(),
            ),
            StartItem::Learn(Command::HelpThemes, "Themes".to_owned()),
            StartItem::Learn(Command::HelpAgents, "Agents".to_owned()),
        ]);
        items
    }

    /// Records `path` as the most recently opened file.
    pub(super) fn remember_recent_file(&mut self, path: &Path) {
        self.recent_files.retain(|recent| recent != path);
        self.recent_files.insert(0, path.to_path_buf());
        self.recent_files.truncate(RECENT_FILES);
    }

    /// Seeds the recent files from a session's documents, most recent first.
    pub(super) fn seed_recent_files(&mut self, session: &tide_config::SessionState) {
        for document in session.documents.iter().rev() {
            let path = PathBuf::from(&document.file_path);
            let path = if path.is_absolute() {
                path
            } else {
                self.workspace.join(path)
            };
            if path.is_file() && !self.recent_files.contains(&path) {
                self.recent_files.push(path);
            }
        }
        self.recent_files.truncate(RECENT_FILES);
    }

    /// Re-reads the named sessions saved for this folder.
    pub(super) fn refresh_start_sessions(&mut self) {
        self.start_sessions = self.session_store.workspace_sessions();
    }

    /// Shows the start page: switches to it where it is already open, or opens
    /// it as a new tab in the focused pane.
    pub(super) fn open_start_page(&mut self) {
        self.refresh_start_sessions();
        self.start_selected = 0;
        self.focus = Focus::Main;
        let open = self.workbench.pane_ids().into_iter().find_map(|id| {
            let index = self
                .workbench
                .pane(id)?
                .tabs
                .iter()
                .position(|tab| matches!(tab, Tab::Welcome))?;
            Some((id, index))
        });
        match open {
            Some((id, index)) => {
                self.workbench.set_focused(id);
                if let Some(pane) = self.workbench.pane_mut(id) {
                    pane.active = index;
                }
            }
            None => self.workbench.insert_tab_focused(Tab::Welcome),
        }
        self.mark_session_dirty();
    }

    pub(super) fn handle_start_key(&mut self, key: KeyEvent) {
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
        {
            return;
        }
        let last = self.start_items().len().saturating_sub(1);
        let selected = self.start_selected.min(last);
        self.start_selected = match key.code {
            KeyCode::Up | KeyCode::Left | KeyCode::BackTab => selected.saturating_sub(1),
            KeyCode::Down | KeyCode::Right | KeyCode::Tab => (selected + 1).min(last),
            KeyCode::Home => 0,
            KeyCode::End => last,
            KeyCode::Enter | KeyCode::Char(' ') => {
                self.activate_start_item(selected);
                return;
            }
            _ => return,
        };
    }

    pub(super) fn activate_start_item(&mut self, index: usize) {
        let Some(item) = self.start_items().into_iter().nth(index) else {
            return;
        };
        self.start_selected = index;
        match item {
            StartItem::Command(command, _) | StartItem::Learn(command, _) => {
                self.execute_command(command);
            }
            StartItem::File(path) => self.open_file(path),
            StartItem::Session { name, .. } => self.open_named_session(&name),
        }
    }

    /// Opens this version's release notes after an upgrade, unless turned off.
    pub(super) fn show_release_notes(&mut self) {
        if self.setting_bool("update.showReleaseNotes", true)
            && let Some(page) = crate::help::for_command(Command::HelpReleaseNotes)
        {
            self.open_help(page);
            self.set_status(format!(
                "Tide was updated to {}; these are its release notes",
                env!("CARGO_PKG_VERSION")
            ));
        }
    }

    pub(super) fn render_start(
        &self,
        frame: &mut Frame,
        area: Rect,
        pane: PaneId,
        out: &mut Painted,
    ) {
        let palette = self.palette;
        let area = Rect {
            x: area.x.saturating_add(2).min(area.right()),
            y: area.y.saturating_add(1).min(area.bottom()),
            width: area.width.saturating_sub(4),
            height: area.height.saturating_sub(1),
        };
        if area.is_empty() {
            return;
        }
        let items = self.start_items();
        let active = self.focus == Focus::Main && self.workbench.focused() == pane;
        let selected = active.then(|| self.start_selected.min(items.len().saturating_sub(1)));
        let text = Style::default().fg(palette.foreground);
        let muted = Style::default().fg(palette.muted);
        let heading = Style::default()
            .fg(palette.foreground)
            .add_modifier(Modifier::BOLD);
        let highlight = Style::default()
            .fg(palette.selection_fg)
            .bg(palette.selection_bg);

        let mut header = vec![Line::from(vec![
            Span::styled(
                "Tide",
                Style::default()
                    .fg(palette.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {}", env!("CARGO_PKG_VERSION")), muted),
        ])];
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let mut about = vec![
            "A terminal-native code editor".to_owned(),
            self.workspace.file_name().map_or_else(
                || self.workspace.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            ),
        ];
        if let Some(branch) = self
            .git
            .state
            .snapshot
            .as_ref()
            .and_then(|git| git.branch.clone())
        {
            about.push(branch);
        }
        header.push(Line::from(Span::styled(
            truncate_to_width(&about.join("  ·  "), area.width as usize).to_owned(),
            muted,
        )));
        header.push(Line::raw(""));
        let header_height = saturating_u16(header.len());
        frame.render_widget(Paragraph::new(header), area);
        let body = Rect {
            y: area.y + header_height.min(area.height),
            height: area.height.saturating_sub(header_height),
            ..area
        };

        let two_columns = body.width >= TWO_COLUMNS;
        let (left, right) = if two_columns {
            let [left, right] =
                Layout::horizontal([Constraint::Length(LEFT_COLUMN), Constraint::Min(1)])
                    .areas(body);
            (left, right)
        } else {
            (body, body)
        };

        // Each column is a list of rows; an item row records its index for highlight and clicks.
        let mut columns: [Vec<(Line<'static>, Option<usize>)>; 2] = [Vec::new(), Vec::new()];
        let mut index = 0;
        let row_width = left.width.min(LEFT_COLUMN - 2) as usize;
        columns[0].push((Line::from(Span::styled("Start", heading)), None));
        for &(command, label) in START_COMMANDS {
            let key = self.key_hint(command).unwrap_or_default();
            let style = if selected == Some(index) {
                highlight
            } else {
                text
            };
            let label_width = row_width.saturating_sub(display_width(&key) + 3).max(1);
            let label = truncate_to_width(label, label_width).to_owned();
            let gap = row_width.saturating_sub(2 + display_width(&label) + display_width(&key));
            columns[0].push((
                Line::from(vec![
                    Span::styled(format!("  {label}{}", " ".repeat(gap)), style),
                    Span::styled(
                        key,
                        if selected == Some(index) {
                            highlight
                        } else {
                            muted
                        },
                    ),
                ]),
                Some(index),
            ));
            index += 1;
        }

        let side = usize::from(two_columns);
        if !two_columns {
            columns[side].push((Line::raw(""), None));
        }
        let side_width = if two_columns { right.width } else { left.width } as usize;
        if !self.recent_files.is_empty() {
            columns[side].push((Line::from(Span::styled("Recent files", heading)), None));
            for path in self.recent_files.iter().take(RECENT_FILES) {
                let shown = path.strip_prefix(&self.workspace).map_or_else(
                    |_| crate::help::tilde(path, home.as_deref()),
                    |relative| relative.display().to_string(),
                );
                let style = if selected == Some(index) {
                    highlight
                } else {
                    text
                };
                columns[side].push((
                    Line::from(Span::styled(
                        format!("  {}", truncate_start(&shown, side_width.saturating_sub(2))),
                        style,
                    )),
                    Some(index),
                ));
                index += 1;
            }
            columns[side].push((Line::raw(""), None));
        }
        if !self.start_sessions.is_empty() {
            columns[side].push((Line::from(Span::styled("Saved sessions", heading)), None));
            for item in &items[index..] {
                let StartItem::Session { title, detail, .. } = item else {
                    break;
                };
                let row = selected == Some(index);
                let title =
                    truncate_to_width(title, side_width.saturating_sub(4).max(1)).to_owned();
                let detail = truncate_to_width(
                    &format!("   {detail}"),
                    side_width.saturating_sub(2 + display_width(&title)),
                )
                .to_owned();
                columns[side].push((
                    Line::from(vec![
                        Span::styled(format!("  {title}"), if row { highlight } else { text }),
                        Span::styled(detail, if row { highlight } else { muted }),
                    ]),
                    Some(index),
                ));
                index += 1;
            }
        }

        let [mut first, second] = columns;
        let pairs = if two_columns {
            vec![(first, left), (second, right)]
        } else {
            first.extend(second);
            vec![(first, left)]
        };
        let mut bottom = left.y;
        for (column, rect) in pairs {
            let mut y = rect.y;
            for (line, item) in column {
                if y >= rect.bottom() {
                    break;
                }
                let row = Rect {
                    y,
                    height: 1,
                    ..rect
                };
                frame.render_widget(Paragraph::new(line), row);
                if let Some(item) = item {
                    out.hits.push((row, HitTarget::StartItem(pane, item)));
                }
                y += 1;
            }
            bottom = bottom.max(y);
        }

        // Learn: one wrapped row of help pages below both columns.
        let mut y = bottom.saturating_add(1);
        if y >= body.bottom() {
            return;
        }
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled("Learn", heading))),
            Rect {
                y,
                height: 1,
                ..body
            },
        );
        y += 1;
        let mut x = body.x + 2;
        for item in &items[index..] {
            let StartItem::Learn(command, label) = item else {
                continue;
            };
            let label = match (command, self.key_hint(*command)) {
                (Command::HelpWelcome, Some(key)) => format!("{label} ({key})"),
                _ => label.clone(),
            };
            let width = saturating_u16(display_width(&label));
            if x > body.x + 2 && x.saturating_add(width) > body.right() {
                x = body.x + 2;
                y += 1;
            }
            if y >= body.bottom() {
                return;
            }
            let rect = Rect {
                x,
                y,
                width: width.min(body.right().saturating_sub(x)),
                height: 1,
            };
            let style = if selected == Some(index) {
                highlight
            } else {
                Style::default()
                    .fg(palette.accent)
                    .add_modifier(Modifier::UNDERLINED)
            };
            frame.render_widget(Paragraph::new(Span::styled(label, style)), rect);
            out.hits.push((rect, HitTarget::StartItem(pane, index)));
            x = x.saturating_add(width + 4);
            index += 1;
        }
    }
}

/// `text` cut from the left to fit `width` columns, so a path keeps its file name.
fn truncate_start(text: &str, width: usize) -> String {
    if display_width(text) <= width {
        return text.to_owned();
    }
    let mut kept = String::new();
    let mut used = 1;
    for character in text.chars().rev() {
        let character_width = display_width(character.encode_utf8(&mut [0; 4]));
        if used + character_width > width {
            break;
        }
        used += character_width;
        kept.insert(0, character);
    }
    format!("…{kept}")
}
