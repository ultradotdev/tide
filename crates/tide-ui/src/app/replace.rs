//! Replace in Files: prompts for the search and replacement, previews the matches in the
//! background, then edits open buffers (so Undo reverts them) and writes the other files.
use super::*;
use crate::workspace_replace::{Plan, PlannedFile, Replace, Written};

impl App {
    pub(super) fn open_replace_in_files(&mut self) {
        let title = format!(
            "Replace in Files: Find{}",
            describe_options(self.search_options)
        );
        let initial = self.last_find_query.clone();
        self.open_prompt(PromptAction::ReplaceInFilesFind, &title, &initial);
    }

    pub(super) fn prompt_replacement_in_files(&mut self, query: String) {
        if query.is_empty() {
            self.set_status("Replace in files needs a search term");
            return;
        }
        self.last_find_query.clone_from(&query);
        let initial = self.last_replace_text.clone();
        self.open_prompt(
            PromptAction::ReplaceInFilesWith(query),
            "Replace in Files: Replace With",
            &initial,
        );
    }

    /// The text of every file open in an editor, by canonical path.
    fn open_buffers(&self) -> HashMap<PathBuf, String> {
        self.workbench
            .tabs()
            .filter_map(Tab::editor)
            .filter_map(|editor| {
                let path = tide_core::fs::canonical_target(editor.path()?);
                let handle = self.documents.get(&path)?;
                Some((path, handle.text()))
            })
            .collect()
    }

    pub(super) fn preview_replace_in_files(&mut self, query: String, replacement: String) {
        self.last_replace_text.clone_from(&replacement);
        let replace = Replace {
            query,
            replacement,
            options: crate::search::SearchOptions {
                selection: None,
                ..self.search_options
            },
        };
        self.replace_cancel
            .store(true, std::sync::atomic::Ordering::Release);
        self.replace_cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cancel = self.replace_cancel.clone();
        self.replace_generation = self.replace_generation.wrapping_add(1);
        let generation = self.replace_generation;
        self.pending_replace = None;
        let open = self.open_buffers();
        let root = self.workspace.clone();
        let sender = self.events_tx.clone();
        self.set_status("Replace in files: finding matches…");
        self.jobs.spawn_blocking(move || {
            let result = crate::workspace_replace::plan(&root, replace, &open, &cancel);
            let _ = sender.blocking_send(AppEvent::ReplacePlanned { generation, result });
        });
    }

    pub(super) fn show_replace_plan(&mut self, generation: u64, result: Result<Plan, String>) {
        if generation != self.replace_generation {
            return;
        }
        let plan = match result {
            Ok(plan) => plan,
            Err(error) => {
                self.set_status(format!("Replace in files: {error}"));
                return;
            }
        };
        if plan.files.is_empty() {
            self.set_status(format!(
                "Replace in files: no matches for {}",
                plan.replace.query
            ));
            return;
        }
        if self.overlay.is_some() {
            self.set_status("Replace in files: another dialog is open; run it again");
            return;
        }
        const SHOWN: usize = 30;
        let mut list = plan
            .files
            .iter()
            .take(SHOWN)
            .map(|file| format!("  {} ({})", file.relative, file.matches))
            .collect::<Vec<_>>()
            .join("\n");
        if plan.files.len() > SHOWN {
            list.push_str(&format!("\n  …and {} more", plan.files.len() - SHOWN));
        }
        let message = format!(
            "Replace {} match(es) in {} file(s)?\n“{}” → “{}”{}\n\n{list}\n\nOpen files change in their editors, where Undo reverts them; the others are written to disk.",
            plan.matches(),
            plan.files.len(),
            plan.replace.query,
            plan.replace.replacement,
            describe_options(plan.replace.options),
        );
        self.overlay = Some(modal::confirm(
            ConfirmAction::ReplaceInFiles(generation),
            "Replace in Files",
            &message,
        ));
        self.pending_replace = Some((generation, plan));
    }

    pub(super) fn apply_replace_in_files(&mut self, generation: u64) {
        let Some((planned, plan)) = self.pending_replace.take() else {
            return;
        };
        if planned != generation {
            return;
        }
        let Plan { replace, files } = plan;
        let mut to_write: Vec<PlannedFile> = Vec::new();
        let mut written = Written::default();
        let mut buffers = 0;
        for file in files {
            // A file opened since the preview is edited in its buffer, not behind it.
            let Some(handle) = self.documents.get(&file.path) else {
                if file.digest.is_some() {
                    to_write.push(file);
                } else {
                    written.skipped.push(file.relative);
                }
                continue;
            };
            let edits = handle.with(|document| {
                crate::search::matches(
                    document,
                    &replace.query,
                    replace.options,
                    Some(&replace.replacement),
                )
            });
            match edits {
                Ok(edits) if !edits.is_empty() => {
                    if handle.apply_edits_preserving_cursor(&edits).is_ok() {
                        written.files += 1;
                        written.matches += edits.len();
                        buffers += 1;
                    } else {
                        written.skipped.push(file.relative);
                    }
                }
                _ => written.skipped.push(file.relative),
            }
        }
        if buffers > 0 {
            self.mark_session_dirty();
        }
        let root = self.workspace.clone();
        let sender = self.events_tx.clone();
        self.set_status("Replace in files: writing…");
        self.jobs.spawn_blocking(move || {
            let mut result = crate::workspace_replace::write_files(&root, &replace, &to_write);
            result.files += written.files;
            result.matches += written.matches;
            result.skipped.extend(written.skipped);
            let _ = sender.blocking_send(AppEvent::ReplaceWritten {
                written: result,
                buffers,
            });
        });
    }

    pub(super) fn report_replace_in_files(&mut self, written: &Written, buffers: usize) {
        let mut message = format!(
            "Replaced {} match(es) in {} file(s)",
            written.matches, written.files
        );
        if buffers > 0 {
            message.push_str(&format!(
                "; {buffers} open file(s) changed in their editors and are unsaved"
            ));
        }
        if !written.skipped.is_empty() {
            message.push_str(&format!(
                "; skipped because they changed since the preview: {}",
                written.skipped.join(", ")
            ));
        }
        self.set_status(message);
    }
}

/// The find options a replacement uses, for prompts and the preview.
fn describe_options(options: crate::search::SearchOptions) -> String {
    let parts = [
        (options.regex, "regex"),
        (options.case_sensitive, "match case"),
        (options.whole_word, "whole word"),
    ]
    .into_iter()
    .filter_map(|(on, name)| on.then_some(name))
    .collect::<Vec<_>>();
    if parts.is_empty() {
        String::new()
    } else {
        format!(" ({})", parts.join(", "))
    }
}
