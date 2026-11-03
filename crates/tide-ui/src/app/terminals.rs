//! Terminal commands that act on a whole terminal: clearing, scrolling, killing and closing.
use super::*;

impl App {
    /// The terminal a terminal command acts on: the focused one, else the active bottom
    /// terminal, so the palette reaches it from any region.
    fn target_terminal(&mut self) -> Option<&mut TerminalTab> {
        if self.focus_in_terminal() {
            self.active_terminal_mut()
        } else {
            self.dock.active_mut()
        }
    }

    pub(super) fn clear_terminal(&mut self) {
        let Some(terminal) = self.target_terminal() else {
            self.set_status("Terminal: no terminal to clear");
            return;
        };
        if !terminal.clear() {
            self.set_status("Terminal: a full-screen program owns the screen; nothing was cleared");
        }
    }

    pub(super) fn scroll_terminal(&mut self, top: bool) {
        let Some(terminal) = self.target_terminal() else {
            self.set_status("Terminal: no terminal to scroll");
            return;
        };
        if top {
            terminal.scroll_to_top();
        } else {
            terminal.scroll_to_bottom();
        }
    }

    pub(super) fn confirm_kill_terminal(&mut self) {
        let Some(terminal) = self.target_terminal() else {
            self.set_status("Terminal: no terminal to kill");
            return;
        };
        if terminal.exited.is_some() {
            let message = format!("Terminal: {} has already exited", terminal.title);
            self.set_status(message);
            return;
        }
        let serial = terminal.serial;
        if terminal.is_busy() {
            let message = format!(
                "{} is still running a program. Kill every process in this terminal?",
                terminal.title
            );
            self.overlay = Some(modal::confirm(
                ConfirmAction::KillTerminal(serial),
                "Kill Terminal",
                &message,
            ));
        } else {
            self.kill_terminal(serial);
        }
    }

    /// Kills the terminal's process groups; the tab stays, marked exited like any terminal
    /// whose shell has ended, until it is closed.
    pub(super) fn kill_terminal(&mut self, serial: u64) {
        let terminal = self
            .dock
            .tabs
            .iter_mut()
            .chain(self.workbench.tabs_mut().filter_map(|tab| match tab {
                Tab::Terminal(terminal) => Some(terminal),
                _ => None,
            }))
            .find(|terminal| terminal.serial == serial);
        let Some(terminal) = terminal else {
            self.set_status("Terminal: that terminal has closed");
            return;
        };
        let title = terminal.title.clone();
        let message = match terminal.terminate() {
            Ok(()) if terminal.exited.is_some() => format!("Terminal: killed {title}"),
            Ok(()) => format!("Terminal: {title} has not started; nothing to kill"),
            Err(error) => format!("Terminal: kill failed: {error}"),
        };
        self.set_status(message);
        self.mark_session_dirty();
    }

    /// Titles of the terminals in the dock and the main panes, agent sessions included,
    /// that are running a program quitting would stop.
    pub(super) fn busy_terminal_titles(&self) -> Vec<String> {
        self.dock
            .tabs
            .iter()
            .chain(self.workbench.tabs().filter_map(|tab| match tab {
                Tab::Terminal(terminal) => Some(terminal),
                _ => None,
            }))
            .filter(|terminal| terminal.is_busy())
            .map(|terminal| terminal.title.clone())
            .collect()
    }

    /// Shell terminals in the bottom dock and the main panes. Agent sessions are managed
    /// from the Agents sidebar and stay open.
    fn shell_terminals(&self) -> impl Iterator<Item = &TerminalTab> {
        self.dock.tabs.iter().chain(
            self.workbench
                .tabs()
                .filter_map(|tab| match tab {
                    Tab::Terminal(terminal) => Some(terminal),
                    _ => None,
                })
                .filter(|terminal| terminal.ai_provider.is_none()),
        )
    }

    pub(super) fn confirm_close_all_terminals(&mut self) {
        let count = self.shell_terminals().count();
        if count == 0 {
            self.set_status("Terminal: no terminals are open");
            return;
        }
        let busy = self
            .shell_terminals()
            .filter(|terminal| terminal.is_busy())
            .map(|terminal| terminal.title.clone())
            .collect::<Vec<_>>();
        if busy.is_empty() {
            self.close_all_terminals();
            return;
        }
        self.overlay = Some(modal::confirm(
            ConfirmAction::CloseAllTerminals,
            "Close All Terminals",
            &format!(
                "Close {count} terminal(s)? Still running a program: {}. Their processes are stopped.",
                busy.join(", ")
            ),
        ));
    }

    pub(super) fn close_all_terminals(&mut self) {
        let mut closed = self.dock.tabs.len();
        while !self.dock.tabs.is_empty() {
            self.dock.close(self.dock.tabs.len() - 1);
        }
        for pane in self.workbench.pane_ids() {
            let indices = self.workbench.pane(pane).map_or(Vec::new(), |pane| {
                pane.tabs
                    .iter()
                    .enumerate()
                    .filter(|(_, tab)| {
                        matches!(tab, Tab::Terminal(terminal) if terminal.ai_provider.is_none())
                    })
                    .map(|(index, _)| index)
                    .collect()
            });
            for index in indices.into_iter().rev() {
                self.force_close_tab(pane, index);
                closed += 1;
            }
        }
        if self.focus == Focus::Dock {
            self.focus = Focus::Main;
        }
        self.mark_session_dirty();
        self.set_status(format!("Terminal: closed {closed} terminal(s)"));
    }
}
