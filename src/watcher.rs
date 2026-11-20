use crate::vault::{is_ignored_dir, is_ignored_dir_for_root, is_unity_project};
use anyhow::Result;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use walkdir::WalkDir;

pub struct WorkspaceWatcher {
    changed: Arc<AtomicBool>,
    _watcher: RecommendedWatcher,
}

impl WorkspaceWatcher {
    pub fn start(root: &Path) -> Result<Self> {
        let changed = Arc::new(AtomicBool::new(false));
        let changed_bg = Arc::clone(&changed);
        let (tx, rx) = mpsc::channel::<()>();
        let is_unity = is_unity_project(root);
        let root_owned = root.to_path_buf();

        let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            let Ok(event) = res else {
                return;
            };
            if event
                .paths
                .iter()
                .any(|p| !should_ignore_event_path(p, &root_owned, is_unity))
            {
                let _ = tx.send(());
            }
        })?;

        let mut watched = 0usize;
        let walker = WalkDir::new(root).into_iter().filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            if !e.file_type().is_dir() {
                return false;
            }
            if e.depth() == 0 {
                return true;
            }
            name != ".obsidian" && !is_ignored_dir_for_root(&name, e.depth(), is_unity, false)
        });
        for entry in walker.flatten() {
            if entry.file_type().is_dir() {
                let _ = watcher.watch(entry.path(), RecursiveMode::NonRecursive);
                watched += 1;
                if watched >= 2048 {
                    break;
                }
            }
        }

        thread::Builder::new()
            .name("vkarma-fs-watcher".to_string())
            .spawn(move || {
                while rx.recv().is_ok() {
                    let deadline = Instant::now() + Duration::from_millis(400);
                    loop {
                        let remaining = deadline.saturating_duration_since(Instant::now());
                        if remaining.is_zero() || rx.recv_timeout(remaining).is_err() {
                            break;
                        }
                    }
                    changed_bg.store(true, Ordering::Release);
                }
            })?;

        Ok(Self {
            changed,
            _watcher: watcher,
        })
    }

    pub fn poll_changed(&self) -> bool {
        self.changed.swap(false, Ordering::AcqRel)
    }
}

fn should_ignore_event_path(path: &Path, root: &Path, is_unity: bool) -> bool {
    if let Some(name) = path.file_name().and_then(|n| n.to_str())
        && name.ends_with(".vkarma-tmp")
    {
        return true;
    }
    if let Ok(rel) = path.strip_prefix(root) {
        for (depth, comp) in rel.components().enumerate() {
            let name = comp.as_os_str().to_string_lossy();
            if name == ".obsidian"
                || is_ignored_dir_for_root(&name, depth + 1, is_unity, true)
            {
                return true;
            }
        }
        return false;
    }
    for comp in path.components() {
        let name = comp.as_os_str().to_string_lossy();
        if is_ignored_dir(&name, true) || name == ".obsidian" {
            return true;
        }
    }
    false
}
