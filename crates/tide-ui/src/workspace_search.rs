//! Bounded, cooperatively cancellable workspace search.
use crate::modal::ModalItem;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[cfg(unix)]
fn open_beneath(root: &std::fs::File, relative: &Path) -> Option<std::fs::File> {
    use nix::fcntl::{OFlag, openat};
    use nix::sys::stat::Mode;
    let mut directory = root.try_clone().ok()?;
    let components: Vec<_> = relative.components().collect();
    for (index, component) in components.iter().enumerate() {
        let std::path::Component::Normal(name) = component else {
            return None;
        };
        let last = index + 1 == components.len();
        let flags = OFlag::O_RDONLY
            | OFlag::O_CLOEXEC
            | OFlag::O_NOFOLLOW
            | OFlag::O_NONBLOCK
            | if last {
                OFlag::empty()
            } else {
                OFlag::O_DIRECTORY
            };
        directory =
            std::fs::File::from(openat(&directory, Path::new(name), flags, Mode::empty()).ok()?);
    }
    Some(directory)
}
fn read_beneath(
    root: &std::fs::File,
    root_path: &Path,
    relative: &Path,
    cache: &mut HashMap<PathBuf, std::fs::File>,
) -> Option<String> {
    #[cfg(unix)]
    let mut file = {
        use nix::fcntl::{OFlag, openat};
        use nix::sys::stat::Mode;
        let parent = relative.parent()?;
        let directory = if parent.as_os_str().is_empty() {
            root
        } else {
            if !cache.contains_key(parent) {
                let directory = open_beneath(root, parent)?;
                if !directory.metadata().ok()?.is_dir() {
                    return None;
                }
                if cache.len() >= 2 {
                    cache.clear();
                }
                cache.insert(parent.to_owned(), directory);
            }
            cache.get(parent)?
        };
        let _ = root_path;
        std::fs::File::from(
            openat(
                directory,
                Path::new(relative.file_name()?),
                OFlag::O_RDONLY | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK,
                Mode::empty(),
            )
            .ok()?,
        )
    };
    #[cfg(not(unix))]
    let mut file = {
        let _ = (root, cache);
        let path = root_path.join(relative).canonicalize().ok()?;
        if !path.starts_with(root_path.canonicalize().ok()?) {
            return None;
        }
        std::fs::File::open(path).ok()?
    };
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > 1_000_000 {
        return None;
    }
    // Bound the read to the opening size. Small regular files need no second
    // EOF read, and concurrent appends cannot expand the admitted byte budget.
    let mut bytes = vec![0; usize::try_from(metadata.len()).ok()?];
    file.read_exact(&mut bytes).ok()?;
    String::from_utf8(bytes).ok()
}

enum SearchTask {
    Directory(PathBuf),
    File(PathBuf),
}
struct QueueState {
    pending: std::collections::VecDeque<SearchTask>,
    active: usize,
}
struct TaskQueue {
    state: std::sync::Mutex<QueueState>,
    wake: std::sync::Condvar,
    active: AtomicUsize,
}
impl TaskQueue {
    fn pop(&self, cancel: &AtomicBool, done: &AtomicBool) -> Option<SearchTask> {
        let mut state = crate::lock(&self.state);
        loop {
            if cancel.load(Ordering::Acquire) || done.load(Ordering::Acquire) {
                return None;
            }
            if let Some(task) = state.pending.pop_front() {
                state.active += 1;
                self.active.fetch_add(1, Ordering::Release);
                return Some(task);
            }
            if state.active == 0 {
                return None;
            }
            state = self
                .wake
                .wait_timeout(state, std::time::Duration::from_millis(5))
                .unwrap()
                .0;
        }
    }
    fn push(&self, task: SearchTask) -> Result<(), SearchTask> {
        let mut state = crate::lock(&self.state);
        if state.pending.len() >= 64 {
            return Err(task);
        }
        state.pending.push_back(task);
        self.wake.notify_one();
        Ok(())
    }
    fn complete(&self) {
        let mut state = crate::lock(&self.state);
        state.active -= 1;
        self.active.fetch_sub(1, Ordering::Release);
        self.wake.notify_all();
    }
}
struct SearchContext<'a> {
    root: &'a Path,
    handle: std::fs::File,
    cancel: &'a AtomicBool,
    done: AtomicBool,
    files: AtomicUsize,
    entries: AtomicUsize,
    workers: usize,
    queue: TaskQueue,
    items: std::sync::Mutex<Vec<ModalItem>>,
}
impl SearchContext<'_> {
    fn stopped(&self) -> bool {
        self.cancel.load(Ordering::Acquire) || self.done.load(Ordering::Acquire)
    }
    fn file(
        &self,
        relative: &Path,
        regex: &regex::Regex,
        directories: &mut HashMap<PathBuf, std::fs::File>,
    ) {
        if self.stopped() {
            return;
        }
        if self.files.fetch_add(1, Ordering::Relaxed) >= 100_000 {
            self.done.store(true, Ordering::Release);
            return;
        }
        let Some(content) = read_beneath(&self.handle, self.root, relative, directories) else {
            return;
        };
        if content.contains('\0') {
            return;
        }
        for (index, line) in content.lines().enumerate() {
            if self.stopped() {
                return;
            }
            if regex.is_match(line) {
                let trimmed = line.trim();
                let mut preview: String = trimmed.chars().take(240).collect();
                if preview.len() < trimmed.len() {
                    preview.push('…');
                }
                let mut items = crate::lock(&self.items);
                if items.len() >= 500 {
                    self.done.store(true, Ordering::Release);
                    return;
                }
                items.push(ModalItem::new(
                    format!("{}:{}: {}", relative.display(), index + 1, preview),
                    String::new(),
                    format!("{}\t{}", self.root.join(relative).display(), index + 1),
                ));
                if items.len() == 500 {
                    self.done.store(true, Ordering::Release);
                    return;
                }
            }
        }
    }
    fn directory(
        &self,
        relative: &Path,
        regex: &regex::Regex,
        directories: &mut HashMap<PathBuf, std::fs::File>,
        distribute: bool,
    ) {
        #[cfg(unix)]
        if !relative.as_os_str().is_empty()
            && !open_beneath(&self.handle, relative)
                .is_some_and(|file| file.metadata().is_ok_and(|m| m.is_dir()))
        {
            return;
        }
        let path = self.root.join(relative);
        let mut builder = ignore::WalkBuilder::new(&path);
        builder
            .hidden(false)
            .git_ignore(true)
            .git_global(false)
            .git_exclude(true);
        if distribute {
            builder.max_depth(Some(1));
        }
        for entry in builder.build() {
            if self.stopped() {
                break;
            }
            let Ok(entry) = entry else {
                continue;
            };
            if entry.depth() == 0 {
                continue;
            }
            if self.entries.fetch_add(1, Ordering::Relaxed) >= 200_000 {
                self.done.store(true, Ordering::Release);
                break;
            }
            let Some(kind) = entry.file_type() else {
                continue;
            };
            let Ok(relative) = entry.path().strip_prefix(self.root) else {
                continue;
            };
            if kind.is_file() {
                // Busy directory workers read inline; spare workers can help
                // with flat directories without filling an unbounded frontier.
                if distribute
                    && self.queue.active.load(Ordering::Acquire) < self.workers
                    && self
                        .queue
                        .push(SearchTask::File(relative.to_owned()))
                        .is_ok()
                {
                    continue;
                }
                self.file(relative, regex, directories);
            } else if kind.is_dir()
                && distribute
                && let Err(SearchTask::Directory(path)) =
                    self.queue.push(SearchTask::Directory(relative.to_owned()))
            {
                // Never block every worker on a full queue. Traverse this
                // subtree inline, using the same inherited ignore rules.
                self.directory(&path, regex, directories, false);
            }
        }
    }
    fn work(&self, regex: regex::Regex) {
        let mut directories = HashMap::new();
        while let Some(task) = self.queue.pop(self.cancel, &self.done) {
            match task {
                SearchTask::Directory(relative) => {
                    self.directory(&relative, &regex, &mut directories, true)
                }
                SearchTask::File(relative) => self.file(&relative, &regex, &mut directories),
            }
            self.queue.complete();
        }
    }
}

pub fn search(root: &Path, query: &str, cancel: &AtomicBool) -> Vec<ModalItem> {
    search_with_parallelism(root, query, cancel, 4)
}

/// Search with a caller-supplied worker budget, clamped to one through four.
/// The acceptance benchmark uses this to measure filesystem contention.
#[doc(hidden)]
pub fn search_with_parallelism(
    root: &Path,
    query: &str,
    cancel: &AtomicBool,
    parallelism: usize,
) -> Vec<ModalItem> {
    if query.len() > 4096 || cancel.load(Ordering::Acquire) {
        return Vec::new();
    }
    let regex = regex::RegexBuilder::new(query)
        .case_insensitive(true)
        .build()
        .unwrap_or_else(|_| {
            regex::RegexBuilder::new(&regex::escape(query))
                .case_insensitive(true)
                .build()
                .expect("escaped regex")
        });
    let Ok(handle) = std::fs::File::open(root) else {
        return Vec::new();
    };
    // Bound queued paths, file reads, directory descriptors and total traversal.
    let workers = parallelism.clamp(1, 4);
    let context = SearchContext {
        root,
        handle,
        cancel,
        workers,
        done: AtomicBool::new(false),
        files: AtomicUsize::new(0),
        entries: AtomicUsize::new(0),
        queue: TaskQueue {
            state: std::sync::Mutex::new(QueueState {
                pending: [SearchTask::Directory(PathBuf::new())].into(),
                active: 0,
            }),
            wake: std::sync::Condvar::new(),
            active: AtomicUsize::new(0),
        },
        items: std::sync::Mutex::new(Vec::new()),
    };
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let context = &context;
            let regex = regex.clone();
            scope.spawn(move || context.work(regex));
        }
    });
    let mut items = context.items.into_inner().unwrap();
    items.sort_by(|a, b| {
        let (a_path, a_line) = a.value.rsplit_once('\t').unwrap();
        let (b_path, b_line) = b.value.rsplit_once('\t').unwrap();
        a_path.cmp(b_path).then_with(|| {
            a_line
                .parse::<usize>()
                .unwrap()
                .cmp(&b_line.parse::<usize>().unwrap())
        })
    });
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn replaced_directory_and_leaf_symlinks_cannot_escape_the_open_workspace() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("private.txt"), "outside secret").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("replaced")).unwrap();
        std::os::unix::fs::symlink(outside.path().join("private.txt"), root.path().join("leaf"))
            .unwrap();
        let handle = std::fs::File::open(root.path()).unwrap();
        let mut cache = HashMap::new();
        assert!(
            read_beneath(
                &handle,
                root.path(),
                Path::new("replaced/private.txt"),
                &mut cache
            )
            .is_none()
        );
        assert!(read_beneath(&handle, root.path(), Path::new("leaf"), &mut cache).is_none());
        assert!(search(root.path(), "secret", &AtomicBool::new(false)).is_empty());
    }
    #[test]
    fn parallel_search_caps_results_and_orders_lines_without_queue_deadlocks() {
        let root = tempfile::tempdir().unwrap();
        for index in 0..100 {
            let directory = root.path().join(format!("dir{index:03}"));
            std::fs::create_dir(&directory).unwrap();
            std::fs::write(directory.join("file.txt"), "needle\n".repeat(30)).unwrap();
        }
        for workers in [0, 1, 2, 3, 4, usize::MAX] {
            let results =
                search_with_parallelism(root.path(), "needle", &AtomicBool::new(false), workers);
            assert_eq!(results.len(), 500);
            for pair in results.windows(2) {
                let (a, an) = pair[0].value.rsplit_once('\t').unwrap();
                let (b, bn) = pair[1].value.rsplit_once('\t').unwrap();
                assert!(
                    a < b
                        || (a == b && an.parse::<usize>().unwrap() < bn.parse::<usize>().unwrap())
                );
            }
        }
    }

    #[test]
    fn distributed_directory_tasks_inherit_root_and_nested_ignore_rules() {
        let root = tempfile::tempdir().unwrap();
        for path in ["src/kept", "src/local", "ignored"] {
            std::fs::create_dir_all(root.path().join(path)).unwrap();
        }
        std::fs::write(root.path().join(".ignore"), "ignored/\n*.skip\n").unwrap();
        std::fs::write(root.path().join("src/.ignore"), "local/\n").unwrap();
        for path in [
            "src/kept/yes.txt",
            "src/kept/no.skip",
            "src/local/no.txt",
            "ignored/no.txt",
        ] {
            std::fs::write(root.path().join(path), "needle").unwrap();
        }
        let items = search(root.path(), "needle", &AtomicBool::new(false));
        assert_eq!(items.len(), 1);
        assert!(items[0].label.starts_with("src/kept/yes.txt:1:"));
    }

    #[test]
    fn reads_empty_and_exact_limit_files_but_skips_oversized_files() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("empty.txt"), "").unwrap();
        std::fs::write(
            root.path().join("exact.txt"),
            format!("needle{}", "x".repeat(999_994)),
        )
        .unwrap();
        std::fs::write(
            root.path().join("oversized.txt"),
            format!("needle{}", "x".repeat(999_995)),
        )
        .unwrap();
        let items = search(root.path(), "needle", &AtomicBool::new(false));
        assert_eq!(items.len(), 1);
        assert!(items[0].label.starts_with("exact.txt:1:"));
    }

    #[test]
    fn result_previews_and_queries_have_byte_bounds() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("long.txt"),
            format!("needle{}", "界".repeat(100_000)),
        )
        .unwrap();
        let cancel = AtomicBool::new(false);
        let results = search(root.path(), "needle", &cancel);
        assert_eq!(results.len(), 1);
        assert!(results[0].label.len() < 1100);
        assert!(results[0].label.ends_with('…'));
        assert!(search(root.path(), &"a".repeat(4097), &cancel).is_empty());
    }
    #[test]
    fn cancellation_prevents_results_and_search_is_workspace_bounded() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("file.txt"), "needle needle\nnext needle\n").unwrap();
        let cancel = AtomicBool::new(true);
        assert!(search(root.path(), "needle", &cancel).is_empty());
        cancel.store(false, Ordering::Release);
        assert_eq!(search(root.path(), "needle", &cancel).len(), 2);
    }
}
