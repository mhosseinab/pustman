//! External change watcher for the on-disk workspace (REQ-COLLAB-01/02).
//!
//! [`watch`] starts a [`notify`] watcher on a workspace root and returns a receiver of
//! [`FsChange`] events. Events are debounced by [`DEBOUNCE`] so a single write that the
//! OS reports as several notifications becomes one change. Writes that
//! [`crate::save_item`] itself made are recognised by content hash and ignored
//! ([`WorkspaceWatcher::record_own_write`]), and `.git/` is never reported.

use crate::{
    ItemFile, ItemProblem, ItemProblemKind, body_id, classify, collect_files, has_conflict_markers,
    invalid_json, parse_item,
};
use api_model::ItemId;
use notify::{RecursiveMode, Watcher};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long raw [`notify`] events are collected before one pass over them, so a single
/// write that arrives as create-plus-modify (or several modifies) becomes one [`FsChange`]
/// and a just-completed [`crate::save_item`] has time to record itself.
const DEBOUNCE: Duration = Duration::from_millis(100);

/// A change to the workspace on disk that Pustman did not make (REQ-COLLAB-01).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FsChange {
    /// A file appeared that carries a readable item id.
    ItemAdded { id: ItemId, path: PathBuf },
    /// A file we already knew changed on disk.
    ItemChanged { id: ItemId, path: PathBuf },
    /// A file we knew disappeared; `id` is the last id it carried.
    ItemRemoved { id: ItemId, path: PathBuf },
    /// A file is present but cannot be read as an item (REQ-COLLAB-02).
    Problem(ItemProblem),
    /// Something changed that we cannot attribute to one item; the caller should re-open.
    Rescan,
}

/// Everything that stops [`watch`] from starting.
#[derive(Debug, thiserror::Error)]
pub enum WatchError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("notify: {0}")]
    Notify(#[from] notify::Error),
}

/// Content hashes of the bytes [`crate::save_item`] last wrote per path, so the resulting
/// file event is not reported back as an external change.
/// Recorded own writes and own deletes: `Some(hash)` is bytes we wrote, `None` is a
/// path we deleted (the old leg of a rename). Both are one-shot and consumed on match.
type OwnWrites = Arc<Mutex<BTreeMap<PathBuf, Option<u64>>>>;

/// Last known item id per path, so a remove event can carry the id the file had.
type KnownItems = Arc<Mutex<BTreeMap<PathBuf, ItemId>>>;

/// Keeps the [`notify`] watcher alive and records the writes Pustman itself makes.
///
/// Dropping it stops the watcher; the receiver then returns an error once the background
/// thread has drained.
pub struct WorkspaceWatcher {
    _watcher: notify::RecommendedWatcher,
    own_writes: OwnWrites,
}

impl WorkspaceWatcher {
    /// Record that [`crate::save_item`] wrote `relative` under `root`, so the file event
    /// the write triggers is not reported back as an external change (REQ-COLLAB-01).
    ///
    /// Call this immediately after a successful [`crate::save_item`]; the debounce window
    /// is long enough for the hash to be in place before the batch is processed.
    /// When `relative` is already gone from disk this records an OWN DELETE instead, which is
    /// what the old leg of a rename is. See [`Self::record_own_rename`].
    pub fn record_own_write(&self, root: &Path, relative: &Path) {
        let full = root.join(relative);
        let recorded = match std::fs::read(&full) {
            Ok(bytes) => Some(content_hash(&bytes)),
            // The file is gone: `save_item` moved or removed it. A content hash can never
            // match a deleted file, so recording the path alone is what suppresses the
            // `ItemRemoved` that would otherwise tell the caller the item it just renamed
            // is deleted.
            Err(_) => None,
        };
        self.own_writes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(relative.to_path_buf(), recorded);
    }

    /// Record a [`crate::save_item`] that MOVED an item: `previous` is the path it left
    /// behind and `saved` is where it landed. Both legs of the rename are ours, so neither
    /// the `ItemRemoved{previous}` nor the event at `saved` may be reported as an external
    /// change (REQ-COLLAB-01).
    ///
    /// A rename of a LEAF item produces no directory event, so the `Rescan` that covers a
    /// container rename does not reach it: both paths must be recorded explicitly.
    pub fn record_own_rename(&self, root: &Path, previous: &Path, saved: &Path) {
        self.record_own_write(root, previous);
        self.record_own_write(root, saved);
    }
}

/// Start watching `root` for external workspace changes (REQ-COLLAB-01).
///
/// Returns a [`WorkspaceWatcher`] — drop it to stop watching — and a receiver of
/// [`FsChange`] events.  Events are debounced by [`DEBOUNCE`]; writes made through
/// [`crate::save_item`] are ignored when recorded with
/// [`WorkspaceWatcher::record_own_write`]; `.git/` is never reported.
pub fn watch(root: &Path) -> Result<(WorkspaceWatcher, Receiver<FsChange>), WatchError> {
    let (notify_tx, notify_rx) = mpsc::channel::<Result<notify::Event, notify::Error>>();
    let (fs_tx, fs_rx) = mpsc::channel::<FsChange>();

    let own_writes: OwnWrites = Arc::new(Mutex::new(BTreeMap::new()));
    let known_items: KnownItems = Arc::new(Mutex::new(BTreeMap::new()));

    // Populate `known_items` before the watcher starts so the first event for a file
    // that already exists is `ItemChanged`, not `ItemAdded`.
    scan_known_items(root, &known_items);

    let mut watcher = notify::recommended_watcher(notify_tx)?;
    watcher.watch(root, RecursiveMode::Recursive)?;

    let root_buf = root.to_path_buf();
    let own_writes_thread = Arc::clone(&own_writes);
    let known_items_thread = Arc::clone(&known_items);
    std::thread::spawn(move || {
        event_loop(
            notify_rx,
            fs_tx,
            root_buf,
            own_writes_thread,
            known_items_thread,
        );
    });

    Ok((
        WorkspaceWatcher {
            _watcher: watcher,
            own_writes,
        },
        fs_rx,
    ))
}

/// Fill `known_items` with every readable item already on disk, so the first change to a
/// pre-existing file is reported as [`FsChange::ItemChanged`] rather than
/// [`FsChange::ItemAdded`].
fn scan_known_items(root: &Path, known_items: &KnownItems) {
    let mut files = Vec::new();
    if collect_files(root, Path::new(""), &mut files).is_err() {
        return;
    }
    files.sort();
    let mut known = known_items
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for relative in &files {
        let Some(file) = classify(relative) else {
            continue;
        };
        let Ok(bytes) = std::fs::read(root.join(relative)) else {
            continue;
        };
        if has_conflict_markers(&String::from_utf8_lossy(&bytes)) {
            continue;
        }
        let Ok(body) = parse_item(file, &bytes) else {
            continue;
        };
        if let Some(id) = body_id(&body) {
            known.insert(relative.clone(), id);
        }
    }
}

/// Collect raw events for [`DEBOUNCE`], then hand the batch to [`process_events`].
/// Exits when the notify channel closes (the watcher was dropped).
fn event_loop(
    notify_rx: Receiver<Result<notify::Event, notify::Error>>,
    fs_tx: Sender<FsChange>,
    root: PathBuf,
    own_writes: OwnWrites,
    known_items: KnownItems,
) {
    loop {
        let first = match notify_rx.recv() {
            Ok(event) => event,
            Err(_) => return,
        };
        let mut events = vec![first];
        let deadline = Instant::now() + DEBOUNCE;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            match notify_rx.recv_timeout(remaining) {
                Ok(event) => events.push(event),
                Err(_) => break,
            }
        }
        if !process_events(&events, &fs_tx, &root, &own_writes, &known_items) {
            return;
        }
    }
}

/// Turn one batch of raw [`notify`] events into [`FsChange`]s.  Returns `false` when the
/// receiver is gone and there is nobody left to tell.
fn process_events(
    events: &[Result<notify::Event, notify::Error>],
    fs_tx: &Sender<FsChange>,
    root: &Path,
    own_writes: &OwnWrites,
    known_items: &KnownItems,
) -> bool {
    let mut paths: Vec<PathBuf> = Vec::new();
    for event in events {
        match event {
            Ok(event) => {
                // Access events do not change bytes and metadata-only events (chmod,
                // ownership) do not touch content: skip both so an unchanged file is not
                // re-reported.
                if matches!(
                    event.kind,
                    notify::EventKind::Access(_)
                        | notify::EventKind::Modify(notify::event::ModifyKind::Metadata(_))
                ) {
                    continue;
                }
                for path in &event.paths {
                    if let Some(relative) = to_relative(root, path) {
                        paths.push(relative);
                    }
                }
            }
            Err(_) => {
                if !emit(fs_tx, FsChange::Rescan) {
                    return false;
                }
            }
        }
    }
    paths.sort();
    paths.dedup();
    // A rename reports both legs; within a batch the lexicographic order can put the
    // addition before the removal of the same id, and an ordered consumer then ends with
    // the item deleted although the file exists at the new path. Removals go first.
    let (gone, present): (Vec<PathBuf>, Vec<PathBuf>) = paths
        .into_iter()
        .partition(|relative| !root.join(relative).exists());
    for relative in gone.iter().chain(present.iter()) {
        if !process_path(relative, fs_tx, root, own_writes, known_items) {
            return false;
        }
    }
    true
}

/// Decide what happened to one workspace-relative path.  Returns `false` when the
/// receiver is gone.
fn process_path(
    relative: &Path,
    fs_tx: &Sender<FsChange>,
    root: &Path,
    own_writes: &OwnWrites,
    known_items: &KnownItems,
) -> bool {
    // Any dot component is workspace machinery (`.git`, `.pustman`, a dot-directory):
    // `collect_files` never lists them, so the watcher must not report them either.
    if relative
        .components()
        .any(|component| component.as_os_str().to_string_lossy().starts_with('.'))
    {
        return true;
    }
    if is_own_write(relative, root, own_writes, known_items) {
        // Our own write is not an external change, but the file is now KNOWN: without
        // registering it the next external edit is misclassified as `ItemAdded` and an
        // external delete yields `Rescan` instead of `ItemRemoved`.
        if let Some(file) = classify(relative) {
            register_known(relative, root, file, known_items);
        }
        return true;
    }
    let full = root.join(relative);
    // The root itself, or a directory: notify reports directory creates and renames here,
    // and a git-pull writes its children before their watch is armed. Dropping those loses
    // the files outright (REQ-COLLAB-01), so ask the caller to re-open.
    if relative.as_os_str().is_empty() || full.is_dir() {
        return emit(fs_tx, FsChange::Rescan);
    }
    let Some(file) = classify(relative) else {
        // A plain non-item file: nothing to report. A removed directory lands here too,
        // but its children already reported their own removals.
        return true;
    };
    if full.is_file() {
        process_existing_file(relative, &full, file, fs_tx, known_items)
    } else if !full.exists() {
        process_removed_file(relative, fs_tx, known_items)
    } else {
        true
    }
}

/// Record that `relative` carries `id`, so the next change to it is `ItemChanged` rather
/// than `ItemAdded`, and a later delete reports `ItemRemoved{id}` rather than `Rescan`.
fn register_known(relative: &Path, root: &Path, file: ItemFile, known_items: &KnownItems) {
    let Ok(bytes) = std::fs::read(root.join(relative)) else {
        return;
    };
    let Ok(body) = parse_item(file, &bytes) else {
        return;
    };
    if let Some(id) = body_id(&body) {
        known_items
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(relative.to_path_buf(), id);
    }
}

/// A file is present on disk: read it and report what it carries (REQ-COLLAB-02).
fn process_existing_file(
    relative: &Path,
    full: &Path,
    file: ItemFile,
    fs_tx: &Sender<FsChange>,
    known_items: &KnownItems,
) -> bool {
    let bytes = match std::fs::read(full) {
        Ok(bytes) => bytes,
        Err(_) => return emit(fs_tx, FsChange::Rescan),
    };
    let text = String::from_utf8_lossy(&bytes);
    if has_conflict_markers(&text) {
        return emit(
            fs_tx,
            FsChange::Problem(ItemProblem {
                path: relative.to_path_buf(),
                kind: ItemProblemKind::ConflictMarkers,
            }),
        );
    }
    let body = match parse_item(file, &bytes) {
        Ok(body) => body,
        Err(error) => {
            return emit(
                fs_tx,
                FsChange::Problem(ItemProblem {
                    path: relative.to_path_buf(),
                    kind: invalid_json(&error),
                }),
            );
        }
    };
    let Some(id) = body_id(&body) else {
        return emit(fs_tx, FsChange::Rescan);
    };
    let (is_new, replaced) = {
        let mut known = known_items
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = known.insert(relative.to_path_buf(), id);
        (previous.is_none(), previous.filter(|old| *old != id))
    };
    // A different item now occupies the path (a git checkout between two branches): the
    // old one is gone from here and an id-keyed model would leak it without this.
    if let Some(old) = replaced
        && !emit(
            fs_tx,
            FsChange::ItemRemoved {
                id: old,
                path: relative.to_path_buf(),
            },
        )
    {
        return false;
    }
    // A replacement id is UNSEEN even though the path is known: reporting it as
    // `ItemChanged` would drop it in a consumer that only inserts on `ItemAdded`.
    let change = if is_new || replaced.is_some() {
        FsChange::ItemAdded {
            id,
            path: relative.to_path_buf(),
        }
    } else {
        FsChange::ItemChanged {
            id,
            path: relative.to_path_buf(),
        }
    };
    emit(fs_tx, change)
}

/// A file disappeared: report the last id it carried, or ask for a rescan when we never
/// knew one.
fn process_removed_file(
    relative: &Path,
    fs_tx: &Sender<FsChange>,
    known_items: &KnownItems,
) -> bool {
    let id = {
        let mut known = known_items
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        known.remove(relative)
    };
    match id {
        Some(id) => emit(
            fs_tx,
            FsChange::ItemRemoved {
                id,
                path: relative.to_path_buf(),
            },
        ),
        None => emit(fs_tx, FsChange::Rescan),
    }
}

/// Whether the bytes at `relative` match what [`WorkspaceWatcher::record_own_write`] last
/// stored — the file event is ours and must not come back as an external change.
///
/// The entry is **consumed** on first match, so a later external write that happens to
/// restore the same bytes is reported: an ABA revert is a real change and swallowing it
/// leaves the caller's model stale with no recovery signal.
fn is_own_write(
    relative: &Path,
    root: &Path,
    own_writes: &OwnWrites,
    known_items: &KnownItems,
) -> bool {
    let mut own = own_writes
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(expected) = own.remove(relative) else {
        return false;
    };
    let Some(expected) = expected else {
        // Our own delete (the old leg of a rename): suppress the removal and forget the path
        // so the next external change there is classified afresh. The marker matches ONLY
        // while the path is genuinely absent — a stale or coalesced marker must never eat a
        // genuine external change at that path (REQ-COLLAB-01).
        if root.join(relative).exists() {
            return false;
        }
        known_items
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(relative);
        return true;
    };
    let full = root.join(relative);
    let Ok(bytes) = std::fs::read(&full) else {
        return false;
    };
    content_hash(&bytes) == expected
}

/// Send one change; `false` means the receiver is gone.
fn emit(fs_tx: &Sender<FsChange>, change: FsChange) -> bool {
    fs_tx.send(change).is_ok()
}

/// Strip the watched root from an absolute [`notify`] path.
fn to_relative(root: &Path, absolute: &Path) -> Option<PathBuf> {
    absolute
        .strip_prefix(root)
        .ok()
        .map(|relative| relative.to_path_buf())
}

fn content_hash(bytes: &[u8]) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}
