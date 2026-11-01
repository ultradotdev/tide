//! Stateful document parsing on one bounded worker, never on the UI thread.
use crate::syntax::HighlightSpan;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{SyncSender, sync_channel},
    },
};
use syntect::{
    easy::HighlightLines,
    highlighting::{HighlightState, Theme},
    parsing::{ParseState, SyntaxSet},
};
use tide_core::TextSnapshot;
#[derive(Clone, PartialEq)]
struct Wanted {
    revision: u64,
    epoch: u64,
    lines: Vec<usize>,
}
struct Job {
    id: u64,
    ticket: u64,
    wanted: Wanted,
    changed: usize,
    text: TextSnapshot,
    extension: String,
    syntax: fn() -> Arc<SyntaxSet>,
    theme: Arc<Theme>,
}
struct Output {
    wanted: Wanted,
    lines: HashMap<usize, Vec<HighlightSpan>>,
    bytes: usize,
}
#[derive(Default)]
struct State {
    ready: HashMap<u64, Output>,
    pending: HashMap<u64, (u64, Wanted)>,
    next: u64,
}
struct Parsed {
    epoch: u64,
    revision: u64,
    extension: String,
    checkpoints: BTreeMap<usize, (HighlightState, ParseState)>,
}
pub struct DocumentHighlighter {
    sender: Mutex<Option<SyncSender<Job>>>,
    state: Arc<Mutex<State>>,
    epoch: AtomicU64,
    changed: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}
impl Default for DocumentHighlighter {
    fn default() -> Self {
        Self {
            sender: Mutex::new(None),
            state: Arc::default(),
            epoch: AtomicU64::new(0),
            changed: Arc::default(),
            stop: Arc::default(),
        }
    }
}
impl DocumentHighlighter {
    pub fn reset(&self) {
        self.epoch.fetch_add(1, Ordering::AcqRel);
        let mut state = self.state.lock().unwrap();
        state.ready.clear();
        state.pending.clear();
        self.changed.store(true, Ordering::Release);
    }
    pub fn take_changed(&self) -> bool {
        self.changed.swap(false, Ordering::AcqRel)
    }
    pub fn revision(&self, id: u64) -> u64 {
        self.state
            .lock()
            .unwrap()
            .ready
            .get(&id)
            .map_or(0, |o| o.wanted.revision)
    }
    pub fn line(&self, id: u64, revision: u64, line: usize) -> Option<Vec<HighlightSpan>> {
        self.state
            .lock()
            .unwrap()
            .ready
            .get(&id)
            .filter(|o| {
                o.wanted.revision == revision
                    && o.wanted.epoch == self.epoch.load(Ordering::Acquire)
            })
            .and_then(|o| o.lines.get(&line))
            .cloned()
    }
    #[allow(clippy::too_many_arguments)]
    pub fn request(
        &self,
        id: u64,
        revision: u64,
        changed: usize,
        text: TextSnapshot,
        extension: &str,
        mut lines: Vec<usize>,
        syntax: fn() -> Arc<SyntaxSet>,
        theme: Arc<Theme>,
    ) {
        lines.sort_unstable();
        lines.dedup();
        lines.truncate(512);
        if lines.is_empty() {
            return;
        }
        let wanted = Wanted {
            revision,
            epoch: self.epoch.load(Ordering::Acquire),
            lines,
        };
        let ticket = {
            let mut state = self.state.lock().unwrap();
            if state.ready.get(&id).is_some_and(|o| o.wanted == wanted)
                || state.pending.get(&id).is_some_and(|(_, w)| *w == wanted)
            {
                return;
            }
            state.next = state.next.wrapping_add(1);
            let ticket = state.next;
            state.pending.insert(id, (ticket, wanted.clone()));
            ticket
        };
        let mut sender = self.sender.lock().unwrap();
        if sender.is_none() {
            let (tx, rx) = sync_channel::<Job>(16);
            *sender = Some(tx);
            let state = self.state.clone();
            let changed = self.changed.clone();
            let stop = self.stop.clone();
            std::thread::spawn(move || {
                let mut parsed = HashMap::<u64, Parsed>::new();
                while let Ok(job) = rx.recv() {
                    if stop.load(Ordering::Acquire) {
                        break;
                    }
                    parse_job(job, &mut parsed, &state, &stop);
                    changed.store(true, Ordering::Release);
                }
            });
        }
        if sender
            .as_ref()
            .unwrap()
            .try_send(Job {
                id,
                ticket,
                wanted,
                changed,
                text,
                extension: extension.to_owned(),
                syntax,
                theme,
            })
            .is_err()
        {
            self.state.lock().unwrap().pending.remove(&id);
        }
    }
}
impl Drop for DocumentHighlighter {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.sender.get_mut().unwrap().take();
    }
}
fn parse_job(
    job: Job,
    parsed: &mut HashMap<u64, Parsed>,
    shared: &Mutex<State>,
    stop: &AtomicBool,
) {
    let current = || {
        !stop.load(Ordering::Acquire)
            && shared
                .lock()
                .unwrap()
                .pending
                .get(&job.id)
                .is_some_and(|(ticket, _)| *ticket == job.ticket)
    };
    if !current() {
        return;
    }
    // Decompression and first-use grammar initialization belong to the worker,
    // so opening the first editor can paint plain text immediately.
    let syntax_set = (job.syntax)();
    if !current() {
        return;
    }
    if parsed.len() >= 8 && !parsed.contains_key(&job.id) {
        parsed.clear();
    }
    let cached = parsed.entry(job.id).or_insert_with(|| Parsed {
        epoch: job.wanted.epoch,
        revision: job.wanted.revision,
        extension: job.extension.clone(),
        checkpoints: BTreeMap::new(),
    });
    if cached.epoch != job.wanted.epoch || cached.extension != job.extension {
        cached.epoch = job.wanted.epoch;
        cached.extension = job.extension.clone();
        cached.checkpoints.clear();
    } else if cached.revision != job.wanted.revision {
        cached.checkpoints.split_off(&job.changed.saturating_add(1));
    }
    cached.revision = job.wanted.revision;
    let first = job.wanted.lines[0];
    let last = *job.wanted.lines.last().unwrap();
    let syntax = syntax_set
        .find_syntax_by_extension(&job.extension)
        .unwrap_or_else(|| syntax_set.find_syntax_plain_text());
    let (start, mut highlighter) = cached.checkpoints.range(..=first).next_back().map_or_else(
        || (0, HighlightLines::new(syntax, &job.theme)),
        |(line, (h, p))| {
            (
                *line,
                HighlightLines::from_state(&job.theme, h.clone(), p.clone()),
            )
        },
    );
    let wanted = job.wanted.lines.iter().copied().collect::<HashSet<_>>();
    let mut lines = HashMap::new();
    let mut bytes = 0;
    let mut plain = false;
    for index in start..=last.min(job.text.len_lines().saturating_sub(1)) {
        if !current() {
            return;
        }
        let raw = job.text.line_with_ending(index).unwrap_or_default();
        // Oversized lines receive conservative plain text, with no unbounded
        // grammar work or false state checkpoints after the skipped line.
        plain |= raw.len() > 16 * 1024;
        let spans = if plain {
            crate::syntax::plain_span(&raw)
        } else {
            highlighter.highlight_line(&raw, &syntax_set).map_or_else(
                |_| crate::syntax::plain_span(&raw),
                |spans| spans.into_iter().map(crate::syntax::convert_span).collect(),
            )
        };
        if wanted.contains(&index) {
            bytes += spans.iter().map(|s| s.text.len() + 96).sum::<usize>();
            if bytes <= 2 * 1024 * 1024 {
                lines.insert(index, spans);
            }
        }
        if !plain && (index + 1) % 256 == 0 {
            let (h, p) = highlighter.state();
            cached.checkpoints.insert(index + 1, (h.clone(), p.clone()));
            highlighter = HighlightLines::from_state(&job.theme, h, p);
            if cached.checkpoints.len() > 128 {
                let key = *cached.checkpoints.keys().next().unwrap();
                cached.checkpoints.remove(&key);
            }
        }
    }
    let mut state = shared.lock().unwrap();
    if state
        .pending
        .get(&job.id)
        .is_some_and(|(ticket, _)| *ticket == job.ticket)
    {
        if state.ready.len() >= 8
            || state.ready.values().map(|o| o.bytes).sum::<usize>() + bytes > 8 * 1024 * 1024
        {
            state.ready.clear();
        }
        state.ready.insert(
            job.id,
            Output {
                wanted: job.wanted,
                lines,
                bytes,
            },
        );
        state.pending.remove(&job.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    static LOADER_THREAD: Mutex<Option<std::thread::ThreadId>> = Mutex::new(None);
    fn tracking_loader() -> Arc<SyntaxSet> {
        *LOADER_THREAD.lock().unwrap() = Some(std::thread::current().id());
        crate::syntax::shared_grammars()
    }
    #[test]
    fn grammar_initialization_runs_on_the_worker_and_notifies_ready_content() {
        let caller = std::thread::current().id();
        let highlighter = DocumentHighlighter::default();
        let document = tide_core::Document::new("let value = 1;\n");
        highlighter.request(
            42,
            1,
            0,
            document.text_snapshot(),
            "rs",
            vec![0],
            tracking_loader,
            Arc::new(Theme::default()),
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while highlighter.line(42, 1, 0).is_none() || !highlighter.take_changed() {
            assert!(Instant::now() < deadline, "worker never published syntax");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_ne!(LOADER_THREAD.lock().unwrap().unwrap(), caller);
        let line = highlighter
            .line(42, 1, 0)
            .unwrap()
            .into_iter()
            .map(|span| span.text)
            .collect::<String>();
        assert_eq!(line, "let value = 1;\n");
    }
}
