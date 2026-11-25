//! `workspace_store` — the on-disk workspace format (PRD §6.1, ADR-0001).
//!
//! A workspace is a folder of plain files, one file per request, folder, collection and
//! environment, with the file extension encoding the item type (REQ-WSP-01). [`open`]
//! reads such a folder into a [`WorkspaceSnapshot`]; [`save_item`] writes one item back as
//! deterministic JSON: pretty-printed with a two-space indent, struct fields in a stable
//! order, maps sorted, and a trailing newline, so saving an unchanged item is a zero-byte
//! diff (REQ-WSP-02).
//!
//! Writes are atomic — a temp file in the destination directory, fsynced, then renamed
//! (ADR-0001) — and a file that cannot become an item is reported as an [`ItemProblem`]
//! in the snapshot rather than failing the whole open, so a merge conflict, a half-written
//! file or the second half of a rename/rename duplicate is visible instead of silently
//! dropped (REQ-COLLAB-02).
//!
//! **Workspace files hold no secrets and no local state** (REQ-WSP-06): a secret variable
//! is written as `{key, kind, description}` and never carries a value, local or shared
//! (REQ-VAR-SHARE-02).
//!
//! This is a domain crate: no IO on the foreground thread is implied, no GPUI, no UI.

use api_model::{Collection, Environment, Folder, ItemId, Request, Workspace};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

/// The `format_version` this build writes into `pustman.json` (REQ-WSP-04).
pub const FORMAT_VERSION: &str = "1.0";

/// The major version of [`FORMAT_VERSION`]; a workspace declaring a greater one opens
/// read-only.
const FORMAT_VERSION_MAJOR: u32 = 1;

/// How many spellings of a slug are tried before a name is declared unplaceable.
const MAX_NAME_ATTEMPTS: u32 = 999;

const WORKSPACE_FILE: &str = "pustman.json";
const COLLECTION_FILE: &str = "collection.json";
const FOLDER_FILE: &str = "folder.json";
const COLLECTIONS_DIR: &str = "collections";
const ENVIRONMENTS_DIR: &str = "environments";
const REQUEST_SUFFIX: &str = ".request.json";
const ENVIRONMENT_SUFFIX: &str = ".environment.json";

/// A merge-conflict marker at the start of a line: `^(<{7}|>{7})( |$)` (REQ-COLLAB-02).
const CONFLICT_OURS: &str = "<<<<<<<";
const CONFLICT_THEIRS: &str = ">>>>>>>";

/// The `kind` discriminant `api_model::VariableKind::Secret` carries in a file.
const SECRET_KIND: &str = "secret";

/// One workspace item: its data and where its file lives in the workspace tree.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub body: ItemBody,
    /// The directory holding this item's slug, relative to the workspace root: the parent
    /// of a collection's or folder's own `<slug>` directory, or the directory holding a
    /// request's or environment's `<slug>.<kind>.json` file. Empty for [`ItemBody::Workspace`].
    /// It names somewhere inside the root; [`save_item`] refuses anything else.
    pub dir: PathBuf,
    /// The file this item occupies today, relative to the workspace root, or `None` for an
    /// item that has never been written. A rename leaves it behind.
    pub path: Option<PathBuf>,
}

/// What an [`Item`] holds (PRD §4).
// `Request` is the largest variant because a request carries its tables and settings
// inline (REQ-HTTP-03's `disabled_auto_headers` included); model values are cloned per
// edit rather than per frame, so boxing would cost more in call sites than it saves —
// the same reason `Child` in `collection.rs` and `CollectionItem` in `request.rs` allow
// this lint.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
pub enum ItemBody {
    Workspace(Workspace),
    Collection(Collection),
    Folder(Folder),
    Request(Request),
    Environment(Environment),
}

impl Item {
    /// The stable identity stored inside the item's file, when the file carries one.
    pub fn id(&self) -> Option<ItemId> {
        body_id(&self.body)
    }

    /// The display name, which is what [`save_item`] derives the file name from
    /// (REQ-WSP-03).
    pub fn name(&self) -> String {
        match &self.body {
            ItemBody::Workspace(workspace) => workspace.name.clone(),
            ItemBody::Collection(collection) => collection.name.clone(),
            ItemBody::Folder(folder) => folder.name.clone(),
            ItemBody::Environment(environment) => environment.name.clone(),
            ItemBody::Request(Request::Http(request)) => request.name.clone(),
            ItemBody::Request(Request::Unsupported(raw)) => raw
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
        }
    }

    /// Rename the item in memory. The file follows on the next [`save_item`], and the
    /// `id` inside it does not change (REQ-WSP-03).
    pub fn set_name(&mut self, name: &str) {
        match &mut self.body {
            ItemBody::Workspace(workspace) => workspace.name = name.to_string(),
            ItemBody::Collection(collection) => collection.name = name.to_string(),
            ItemBody::Folder(folder) => folder.name = name.to_string(),
            ItemBody::Environment(environment) => environment.name = name.to_string(),
            ItemBody::Request(Request::Http(request)) => request.name = name.to_string(),
            ItemBody::Request(Request::Unsupported(raw)) => {
                if let Some(object) = raw.as_object_mut() {
                    object.insert("name".to_string(), serde_json::Value::from(name));
                }
            }
        }
    }

    /// The file name stem: a slug of the name (REQ-WSP-03), falling back to the id for an
    /// item whose name carries no ASCII letters or digits.
    fn slug(&self) -> String {
        let slug = slugify(&self.name());
        if slug.is_empty() {
            self.id()
                .map_or_else(|| "item".to_string(), |id| slugify(&id.to_string()))
        } else {
            slug
        }
    }

    /// The deterministic file contents: pretty JSON with a trailing newline.
    fn to_bytes(&self) -> Result<Vec<u8>, SaveError> {
        let mut value = match &self.body {
            ItemBody::Workspace(workspace) => serde_json::to_value(workspace)?,
            ItemBody::Collection(collection) => serde_json::to_value(collection)?,
            ItemBody::Folder(folder) => serde_json::to_value(folder)?,
            ItemBody::Request(request) => serde_json::to_value(request)?,
            ItemBody::Environment(environment) => serde_json::to_value(environment)?,
        };
        sanitize_secret_variables(&mut value);
        let mut text = serde_json::to_string_pretty(&value)?;
        text.push('\n');
        Ok(text.into_bytes())
    }

    /// The file this item is written to, relative to the workspace root, with the slug
    /// suffixed `-2`, `-3`, … while another item already owns it (REQ-WSP-03). `bytes` is
    /// what will be written there: it tells this item's own interrupted save apart from
    /// another file (see [`Item::target_occupied`]).
    fn place(&self, root: &Path, bytes: &[u8]) -> Result<PathBuf, SaveError> {
        if let ItemBody::Workspace(_) = self.body {
            return Ok(PathBuf::from(WORKSPACE_FILE));
        }
        let slug = self.slug();
        for attempt in 1..=MAX_NAME_ATTEMPTS {
            let stem = numbered_slug(&slug, attempt);
            let relative = match shape(&self.body) {
                Shape::Workspace => PathBuf::from(WORKSPACE_FILE),
                Shape::Container(file) => self.dir.join(&stem).join(file),
                Shape::Leaf(suffix) => self.dir.join(format!("{stem}{suffix}")),
            };
            if !self.target_occupied(root, &relative, bytes)? {
                return Ok(relative);
            }
        }
        Err(SaveError::NameCollision(self.name()))
    }

    /// Whether `relative` is already taken by another file — never by this item's own, so
    /// saving under an unchanged name does not chase itself into a suffix.
    ///
    /// At [`Item::path`] the file is this item's own only while it is absent or still
    /// carries this item's `id`: an external edit (git checkout, another clone) can leave a
    /// different item's file there, and a merge conflict leaves one with no readable id at
    /// all. Both are problem files, not this item, so they keep their name and their bytes
    /// and this item is written under a suffixed name (REQ-COLLAB-02).
    ///
    /// Any other path counts as taken, with one exception: a file carrying this item's `id`
    /// whose bytes are exactly `bytes` is this item's own interrupted save — the new file
    /// was persisted and the old one not yet removed — so the name is kept, the write is
    /// the byte-identical no-op below and the cleanup removes the old file. Any other file
    /// at another path is never overwritten, even one carrying this item's `id` with
    /// different bytes: a rename/rename merge leaves two files sharing one id and this item
    /// describes only one of them (REQ-COLLAB-02).
    ///
    /// A container's `<slug>` directory is the one adoptable place: free when it holds no
    /// container file with a different id, because the item then takes the directory
    /// together with the children already in it — they are addressed by `order` ids
    /// (REQ-WSP-01) — and saving a child before its container must not split them across
    /// two directories (PRD §6.1).
    fn target_occupied(
        &self,
        root: &Path,
        relative: &Path,
        bytes: &[u8],
    ) -> Result<bool, SaveError> {
        let full = root.join(relative);
        if self.path.as_deref() == Some(relative) {
            return Ok(match file_identity(&full) {
                Some(theirs) => Some(theirs) != self.id(),
                None => full.exists(),
            });
        }
        if full.exists() {
            let ours = self.id();
            let own_interrupted_save =
                ours.is_some() && file_identity(&full) == ours && matches_bytes(&full, bytes)?;
            return Ok(!own_interrupted_save);
        }
        Ok(matches!(shape(&self.body), Shape::Container(_))
            && full.parent().is_some_and(|dir| {
                let dir = root.join(dir);
                match std::fs::symlink_metadata(&dir) {
                    Ok(metadata) if metadata.is_dir() => {
                        [COLLECTION_FILE, FOLDER_FILE].iter().any(|file| {
                            let candidate = dir.join(file);
                            candidate.exists() && file_identity(&candidate) != self.id()
                        })
                    }
                    // Something else owns the slug — a plain file, or a link out of the
                    // root — so the item cannot have it.
                    Ok(_) => true,
                    Err(_) => false,
                }
            }))
    }

    /// This item with [`Item::dir`] and [`Item::path`] re-pathed to the one file that still
    /// carries its `id`, when a container rename moved the recorded path away.
    ///
    /// A folder or collection rename moves its `<slug>` directory and everything in it, so
    /// a child read before the move still names the old place: saving it there would
    /// rebuild the old directory and leave the moved copy behind as a second file carrying
    /// one id, which [`open`] then reports as a duplicate. Exactly one other file carrying
    /// the id is where the item moved to, and the save follows it there. Several are a
    /// rename/rename merge's leftovers beside the move, and writing would pick a winner
    /// among them, so that save is refused with everything left as it is (REQ-COLLAB-02);
    /// none means the file is simply gone and the save stands where the item thinks it is.
    ///
    /// A file is that home only when its kind matches this body's (collection ↔
    /// `collection.json`, request ↔ `*.request.json`, …): an `Environment` carrying a
    /// request's id is another item squatting on it, and following it there would delete
    /// that file and write this one where [`classify`] never reads it (REQ-COLLAB-02).
    fn repathed(&self, root: &Path) -> Result<Option<Item>, SaveError> {
        let Some(previous) = self.path.as_deref() else {
            return Ok(None);
        };
        let Some(parent) = previous.parent() else {
            return Ok(None);
        };
        if root.join(parent).is_dir() {
            return Ok(None);
        }
        let Some(ours) = self.id() else {
            return Ok(None);
        };
        let mut files = Vec::new();
        collect_files(root, Path::new(""), &mut files)?;
        let mut homes = Vec::new();
        for relative in files {
            let Some(file) = classify(&relative) else {
                continue;
            };
            if file != body_file(&self.body) {
                continue;
            }
            if file_identity(&root.join(&relative)) == Some(ours) {
                homes.push((relative, file));
            }
        }
        match homes.len() {
            0 => Ok(None),
            1 => Ok(homes.pop().map(|(path, file)| Item {
                dir: placement_dir(&path, file),
                body: self.body.clone(),
                path: Some(path),
            })),
            _ => Err(SaveError::AmbiguousMove {
                id: ours,
                path: previous.to_path_buf(),
            }),
        }
    }
}

/// The stable identity inside an item's data, when it carries one. A request this build
/// does not model keeps its `id` in the raw JSON it was read from.
fn body_id(body: &ItemBody) -> Option<ItemId> {
    match body {
        ItemBody::Workspace(workspace) => Some(workspace.id),
        ItemBody::Collection(collection) => Some(collection.id),
        ItemBody::Folder(folder) => Some(folder.id),
        ItemBody::Environment(environment) => Some(environment.id),
        ItemBody::Request(Request::Http(request)) => Some(request.id),
        ItemBody::Request(Request::Unsupported(raw)) => raw
            .get("id")
            .and_then(serde_json::Value::as_str)
            .and_then(|id| id.parse().ok()),
    }
}

/// The `id` a file carries, whether or not its name and place make it one [`open`] loads:
/// what guards a write is the id in the bytes, and a file this build does not recognise
/// still has one (REQ-COLLAB-02). An absent file, invalid JSON or JSON without an `id` has
/// no id to compare.
fn file_identity(path: &Path) -> Option<ItemId> {
    let bytes = std::fs::read(path).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    value
        .get("id")
        .and_then(serde_json::Value::as_str)
        .and_then(|id| id.parse().ok())
}

/// Whether a container's `<slug>` directory can move to a new slug wholesale. A
/// `collection.json`/`folder.json` inside it that does not carry `ours` is another item's
/// file, or a problem file with no readable id at all (REQ-COLLAB-02): the move would take
/// it to the new slug and the write there would overwrite it, so that directory stays put
/// and the item is written beside it instead.
fn movable_container_dir(source: &Path, ours: Option<ItemId>) -> bool {
    let Ok(metadata) = std::fs::symlink_metadata(source) else {
        return false;
    };
    if !metadata.is_dir() {
        return false;
    }
    [COLLECTION_FILE, FOLDER_FILE].iter().all(|file| {
        let candidate = source.join(file);
        match file_identity(&candidate) {
            Some(theirs) => Some(theirs) == ours,
            None => !candidate.exists(),
        }
    })
}

/// One thing wrong with one workspace file: the file is in the snapshot as a problem, not
/// a reason to give up on the workspace (REQ-COLLAB-02).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemProblem {
    /// The file, relative to the workspace root.
    pub path: PathBuf,
    pub kind: ItemProblemKind,
}

/// Why a workspace file could not become an [`Item`].
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ItemProblemKind {
    /// The file is not valid JSON at this position, or is valid JSON but not a workspace
    /// item (then `line` and `col` point at the start of the file). Both are 1-based and
    /// are `serde_json::Error::line()`/`column()` verbatim, which is what serde_json's own
    /// message (`at line 5 column 3`) prints — nothing is added to them.
    #[error("invalid JSON at line {line}, column {col}")]
    InvalidJson {
        /// 1-based line.
        line: usize,
        /// 1-based column.
        col: usize,
    },
    /// The file carries merge-conflict markers and is not JSON at all.
    #[error("merge conflict markers")]
    ConflictMarkers,
    /// Two files carry the same `id`, which a rename/rename merge leaves behind. The file
    /// named in [`ItemProblem::path`] is the one held out of [`WorkspaceSnapshot::items`];
    /// `kept` names the file holding the item, so neither half of the merge is hidden
    /// (REQ-COLLAB-02).
    #[error("duplicate id {id}: {kept} already carries it")]
    DuplicateId {
        id: ItemId,
        /// The file that kept the item in [`WorkspaceSnapshot::items`].
        kept: PathBuf,
    },
}

/// Everything that stops [`open`] from describing a workspace at all.
#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// The folder holds no `pustman.json`, so it is not a workspace.
    #[error("{0} is not a Pustman workspace: pustman.json is missing")]
    NotAWorkspace(PathBuf),
    /// `pustman.json` itself is unreadable as a workspace file. Item files are reported as
    /// [`ItemProblem`]s instead; the workspace identity is not optional.
    #[error("{path}: {kind}")]
    WorkspaceFile {
        path: PathBuf,
        kind: ItemProblemKind,
    },
}

/// Everything that stops [`save_item`] from writing one item.
#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    /// Every spelling of the slug is already taken by another item.
    #[error("no free file name for {0}")]
    NameCollision(String),
    /// The item's file left [`Item::path`] behind — its container directory moved — and
    /// several files now carry its `id`: a rename/rename merge's leftovers beside the move
    /// (REQ-COLLAB-02). Which one the item moved to cannot be told apart, and writing would
    /// pick a winner among them, so the save is refused with every file left as it is.
    #[error("several files carry id {id}, so which one {path} moved to is ambiguous")]
    AmbiguousMove { id: ItemId, path: PathBuf },
    /// The workspace declares a newer major `format_version`, so this build must not write
    /// it: its files are shaped for a reader this build is not (REQ-WSP-04).
    #[error("read-only: pustman.json declares format_version {0}")]
    ReadOnly(String),
    /// The path would take the write outside the workspace root: it climbs out of the
    /// root, anchors outside it, or a directory on the way is a symlink — which is how an
    /// untrusted workspace points a write at a folder of its own choosing.
    #[error("{0} would write outside the workspace root")]
    OutsideRoot(PathBuf),
}

/// One workspace folder as Pustman sees it: every item it holds, where each file is, and
/// what could not be read (PRD §6.1).
#[derive(Debug)]
pub struct WorkspaceSnapshot {
    pub root: PathBuf,
    /// Every item by its [`ItemId`], the workspace itself included.
    pub items: BTreeMap<ItemId, Item>,
    /// Files that are not items: unreadable, conflicted, or a second file carrying an
    /// [`ItemId`] already in `items`. They are never overwritten (REQ-COLLAB-02).
    pub problems: Vec<ItemProblem>,
    /// The workspace declares a newer major `format_version`, so nothing may be written
    /// back (REQ-WSP-04).
    pub read_only: bool,
}

impl WorkspaceSnapshot {
    /// The path map entry for one item, relative to the workspace root.
    pub fn path(&self, id: &ItemId) -> Option<&Path> {
        self.items.get(id).and_then(|item| item.path.as_deref())
    }
}

/// Read a workspace folder into a snapshot.
///
/// A file that fails to parse becomes an [`ItemProblem`] in [`WorkspaceSnapshot::problems`]
/// rather than an [`OpenError`]; only a missing or unreadable `pustman.json` is fatal.
pub fn open(root: &Path) -> Result<WorkspaceSnapshot, OpenError> {
    let workspace_path = root.join(WORKSPACE_FILE);
    let workspace_bytes = match std::fs::read(&workspace_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(OpenError::NotAWorkspace(root.to_path_buf()));
        }
        Err(error) => return Err(OpenError::Io(error)),
    };
    let workspace = match parse_item(ItemFile::Workspace, &workspace_bytes) {
        Ok(ItemBody::Workspace(workspace)) => workspace,
        Ok(_) => {
            return Err(OpenError::WorkspaceFile {
                path: PathBuf::from(WORKSPACE_FILE),
                kind: ItemProblemKind::InvalidJson { line: 1, col: 1 },
            });
        }
        Err(error) => {
            return Err(OpenError::WorkspaceFile {
                path: PathBuf::from(WORKSPACE_FILE),
                kind: invalid_json(&error),
            });
        }
    };

    let mut items = BTreeMap::new();
    items.insert(
        workspace.id,
        Item {
            body: ItemBody::Workspace(workspace.clone()),
            dir: PathBuf::new(),
            path: Some(PathBuf::from(WORKSPACE_FILE)),
        },
    );

    let mut problems = Vec::new();
    let mut files = Vec::new();
    collect_files(root, Path::new(""), &mut files)?;
    files.sort();
    for relative in files {
        let Some(file) = classify(&relative) else {
            continue;
        };
        if file == ItemFile::Workspace {
            continue;
        }
        let bytes = std::fs::read(root.join(&relative))?;
        if has_conflict_markers(&String::from_utf8_lossy(&bytes)) {
            problems.push(ItemProblem {
                path: relative,
                kind: ItemProblemKind::ConflictMarkers,
            });
            continue;
        }
        let body = match parse_item(file, &bytes) {
            Ok(body) => body,
            Err(error) => {
                problems.push(ItemProblem {
                    path: relative,
                    kind: invalid_json(&error),
                });
                continue;
            }
        };
        let Some(id) = body_id(&body) else {
            problems.push(ItemProblem {
                path: relative,
                kind: ItemProblemKind::InvalidJson { line: 1, col: 1 },
            });
            continue;
        };
        // A second file carrying an id that is already in the snapshot is the other half
        // of a rename/rename merge, not a second spelling of the same item: it is held out
        // of `items` and reported, so its content stays visible and untouched.
        if let Some(kept) = items.get(&id) {
            problems.push(ItemProblem {
                path: relative,
                kind: ItemProblemKind::DuplicateId {
                    id,
                    kept: kept.path.clone().unwrap_or_default(),
                },
            });
            continue;
        }
        items.insert(
            id,
            Item {
                dir: placement_dir(&relative, file),
                body,
                path: Some(relative),
            },
        );
    }

    Ok(WorkspaceSnapshot {
        read_only: major_version(&workspace.format_version) > FORMAT_VERSION_MAJOR,
        root: root.to_path_buf(),
        items,
        problems,
    })
}

/// Write one item to the workspace, returning the file it landed in (relative to `root`).
///
/// The JSON is deterministic and the write is atomic (a temp file in the destination
/// directory, then a rename). A rename keeps the item's `id` and moves its file, and the
/// write is skipped entirely when the file already holds these exact bytes (REQ-WSP-02/03).
/// An item whose folder or collection moved under it — [`Item::path`] still names the old
/// directory — is saved where its file went instead (see `Item::repathed`).
///
/// Nothing is written to a workspace that declares a newer major `format_version`
/// (REQ-WSP-04), and every path the write touches is confined to `root`: [`Item::dir`] and
/// [`Item::path`] are caller-supplied, and a directory symlink in an untrusted workspace
/// is followed by an ordinary join, so both are checked before the first write.
pub fn save_item(root: &Path, item: &Item) -> Result<PathBuf, SaveError> {
    ensure_writable(root)?;
    confined(root, &item.dir)?;
    if let Some(previous) = &item.path {
        confined(root, previous)?;
    }

    let placed = match item.repathed(root)? {
        Some(placed) => placed,
        None => item.clone(),
    };

    let bytes = placed.to_bytes()?;
    let target = placed.place(root, &bytes)?;
    confined(root, &target)?;
    let target_dir = target.parent().unwrap_or_else(|| Path::new(""));

    if matches!(shape(&placed.body), Shape::Container(_)) {
        // A container's children live inside its `<slug>` directory, so a rename moves the
        // directory and everything in it before the item's own file is rewritten. The move
        // is taken only where it cannot take another item's file along: a source holding a
        // `collection.json`/`folder.json` that is not this item's would land under the new
        // slug and be overwritten there (REQ-COLLAB-02), and a destination directory that
        // is already on disk is one [`Item::target_occupied`] called free to adopt, which
        // `rename(2)` refuses to replace while it holds anything (PRD §6.1). Either way the
        // source is left where it is and the write goes into the destination directory.
        let destination = root.join(target_dir);
        let source = placed
            .path
            .as_deref()
            .and_then(Path::parent)
            .map(|previous| root.join(previous))
            .filter(|source| *source != destination);
        match source {
            Some(source)
                if !destination.exists() && movable_container_dir(&source, placed.id()) =>
            {
                std::fs::rename(source, &destination)?;
            }
            _ => std::fs::create_dir_all(&destination)?,
        }
    } else {
        std::fs::create_dir_all(root.join(target_dir))?;
    }

    // With the destination directory in place, resolving it must land under the resolved
    // root: the last word on where the bytes go, however the workspace folder is reached.
    let resolved_root = std::fs::canonicalize(root)?;
    let resolved_dir = std::fs::canonicalize(root.join(target_dir))?;
    if !resolved_dir.starts_with(&resolved_root) {
        return Err(SaveError::OutsideRoot(target.clone()));
    }

    let full = root.join(&target);
    if !matches_bytes(&full, &bytes)? {
        atomic_write(&full, &bytes)?;
    }

    if let Some(previous) = &placed.path
        && previous != &target
    {
        let previous = root.join(previous);
        // A rename takes the item's own old file with it and nothing else: a different item
        // that now sits where it used to keeps its file (REQ-COLLAB-02).
        if previous.exists() && file_identity(&previous) == placed.id() {
            std::fs::remove_file(previous)?;
        }
    }
    Ok(target)
}

/// Refuse to write into a workspace that declares a newer major `format_version`: this
/// build shapes files for its own major version and would leave a newer reader a workspace
/// it never asked for (REQ-WSP-04). A folder with no `pustman.json` yet has no declared
/// version to honour.
fn ensure_writable(root: &Path) -> Result<(), SaveError> {
    let bytes = match std::fs::read(root.join(WORKSPACE_FILE)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(SaveError::Io(error)),
    };
    let workspace: Workspace = serde_json::from_slice(&bytes)?;
    if major_version(&workspace.format_version) > FORMAT_VERSION_MAJOR {
        return Err(SaveError::ReadOnly(workspace.format_version));
    }
    Ok(())
}

/// Refuse a workspace-relative path that would take a write outside the workspace root.
///
/// An untrusted workspace is a folder of files anyone can shape: [`Item::dir`] and
/// [`Item::path`] are caller-supplied, and a symlinked directory inside the folder is an
/// escape hatch an ordinary join follows. So the path must be plain names — no `..`, no
/// absolute anchor — and no directory on the way may be a symlink: `symlink_metadata`
/// reports a link as itself, so the walk refuses one instead of following it out.
fn confined(root: &Path, relative: &Path) -> Result<(), SaveError> {
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(SaveError::OutsideRoot(relative.to_path_buf()));
    }
    let mut walk = root.to_path_buf();
    let components = relative.components().collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate() {
        walk.push(component.as_os_str());
        let last = index + 1 == components.len();
        match std::fs::symlink_metadata(&walk) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || (!last && !metadata.is_dir()) {
                    return Err(SaveError::OutsideRoot(relative.to_path_buf()));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(SaveError::Io(error)),
        }
    }
    Ok(())
}

/// How a kind of item is named on disk (PRD §6.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    /// `pustman.json`: fixed, never slugged.
    Workspace,
    /// `<dir>/<slug>/collection.json` — the slug names a directory.
    Container(&'static str),
    /// `<dir>/<slug>.request.json` — the slug names the file.
    Leaf(&'static str),
}

fn shape(body: &ItemBody) -> Shape {
    match body {
        ItemBody::Workspace(_) => Shape::Workspace,
        ItemBody::Collection(_) => Shape::Container(COLLECTION_FILE),
        ItemBody::Folder(_) => Shape::Container(FOLDER_FILE),
        ItemBody::Request(_) => Shape::Leaf(REQUEST_SUFFIX),
        ItemBody::Environment(_) => Shape::Leaf(ENVIRONMENT_SUFFIX),
    }
}

/// The file kind one [`ItemBody`] is written as (REQ-WSP-01) — the other half of
/// [`classify`], so a file can be told to be this item's kind or another's.
fn body_file(body: &ItemBody) -> ItemFile {
    match body {
        ItemBody::Workspace(_) => ItemFile::Workspace,
        ItemBody::Collection(_) => ItemFile::Collection,
        ItemBody::Folder(_) => ItemFile::Folder,
        ItemBody::Request(_) => ItemFile::Request,
        ItemBody::Environment(_) => ItemFile::Environment,
    }
}

/// The item type one file name encodes (REQ-WSP-01).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ItemFile {
    Workspace,
    Collection,
    Folder,
    Request,
    Environment,
}

fn classify(relative: &Path) -> Option<ItemFile> {
    let name = relative.file_name()?.to_str()?;
    if relative.components().count() == 1 {
        return (name == WORKSPACE_FILE).then_some(ItemFile::Workspace);
    }
    let top = relative.components().next()?.as_os_str().to_str()?;
    match top {
        COLLECTIONS_DIR if name == COLLECTION_FILE && has_placement_dir(relative) => {
            Some(ItemFile::Collection)
        }
        COLLECTIONS_DIR if name == FOLDER_FILE && has_placement_dir(relative) => {
            Some(ItemFile::Folder)
        }
        COLLECTIONS_DIR if name.ends_with(REQUEST_SUFFIX) => Some(ItemFile::Request),
        ENVIRONMENTS_DIR if name.ends_with(ENVIRONMENT_SUFFIX) => Some(ItemFile::Environment),
        _ => None,
    }
}

/// A `collection.json` or `folder.json` sits inside its own `<slug>` directory, so its
/// placement directory is the directory above that — one level further up than a request's.
fn has_placement_dir(relative: &Path) -> bool {
    relative
        .parent()
        .and_then(Path::parent)
        .is_some_and(|dir| !dir.as_os_str().is_empty())
}

fn placement_dir(relative: &Path, file: ItemFile) -> PathBuf {
    let parent = relative.parent().unwrap_or_else(|| Path::new(""));
    match file {
        ItemFile::Collection | ItemFile::Folder => match parent.parent() {
            Some(dir) => dir.to_path_buf(),
            None => PathBuf::new(),
        },
        ItemFile::Workspace | ItemFile::Request | ItemFile::Environment => parent.to_path_buf(),
    }
}

fn parse_item(file: ItemFile, bytes: &[u8]) -> Result<ItemBody, serde_json::Error> {
    Ok(match file {
        ItemFile::Workspace => ItemBody::Workspace(serde_json::from_slice(bytes)?),
        ItemFile::Collection => ItemBody::Collection(serde_json::from_slice(bytes)?),
        ItemFile::Folder => ItemBody::Folder(serde_json::from_slice(bytes)?),
        ItemFile::Request => ItemBody::Request(serde_json::from_slice(bytes)?),
        ItemFile::Environment => ItemBody::Environment(serde_json::from_slice(bytes)?),
    })
}

/// Every plain file under `relative_dir`, skipping dot entries (`.git`, `.pustman`, …), so
/// the walk stays out of version-control and local-state directories.
///
/// `DirEntry::file_type` does not follow symlinks, so a symlink is neither read nor walked:
/// a symlinked directory would take the walk out of the workspace, and one pointing back
/// into it would take it round for ever.
fn collect_files(root: &Path, relative_dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(root.join(relative_dir))? {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        entries.push((name, entry.file_type()?));
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    for (name, file_type) in entries {
        let relative = relative_dir.join(&name);
        if file_type.is_dir() {
            collect_files(root, &relative, out)?;
        } else if file_type.is_file() {
            out.push(relative);
        }
    }
    Ok(())
}

/// Merge-conflict markers, exactly the pattern REQ-COLLAB-02 gives: `^(<{7}|>{7})( |$)`
/// on any line.
fn has_conflict_markers(text: &str) -> bool {
    text.lines().any(is_conflict_marker)
}

fn is_conflict_marker(line: &str) -> bool {
    [CONFLICT_OURS, CONFLICT_THEIRS].iter().any(|prefix| {
        line.strip_prefix(*prefix)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(' '))
    })
}

/// The problem for one `serde_json::Error`, keeping its `line()` and `column()` verbatim:
/// both are already 1-based and its own message (`at line 5 column 3`) prints those same
/// numbers, so adding to either would point at a character that is not on the screen.
fn invalid_json(error: &serde_json::Error) -> ItemProblemKind {
    ItemProblemKind::InvalidJson {
        line: error.line(),
        col: error.column(),
    }
}

/// A file name stem from an item name (REQ-WSP-03): lowercase, runs of anything else
/// collapsed to a single dash. Public so a folder named after an item (the New Workspace
/// form's `<location>/<slug>`) reads the way that item's own files do.
pub fn slugify(name: &str) -> String {
    let mut slug = String::new();
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    slug
}

fn numbered_slug(slug: &str, attempt: u32) -> String {
    if attempt <= 1 {
        slug.to_string()
    } else {
        format!("{slug}-{attempt}")
    }
}

/// A secret variable's file records only `{key, kind, description}` (REQ-VAR-SHARE-02):
/// no shared value and no local value, so a repository never carries one. Everything else
/// about it stays in the OS keychain.
///
/// Every nested object is a candidate, not only the entries of a `variables` array: the raw
/// JSON kept verbatim (REQ-MODEL-02) holds a secret under its own key — a Postman-style
/// `variable`, anything in an `extra` map — and the file is committed to Git. The item's own
/// root is left in shape: it is the item and not a variable, and dropping the `id` and `name`
/// it carries would write a file [`open`] cannot read back (REQ-WSP-01).
fn sanitize_secret_variables(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for nested in map.values_mut() {
                sanitize_secret_variables(nested);
                sanitize_secret_variable(nested);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                sanitize_secret_variables(item);
                sanitize_secret_variable(item);
            }
        }
        _ => {}
    }
}

fn sanitize_secret_variable(value: &mut serde_json::Value) {
    let Some(map) = value.as_object() else {
        return;
    };
    if map.get("kind").and_then(serde_json::Value::as_str) != Some(SECRET_KIND) {
        return;
    }
    let mut kept = serde_json::Map::new();
    for key in ["key", "kind", "description"] {
        if let Some(field) = map.get(key) {
            kept.insert(key.to_string(), field.clone());
        }
    }
    *value = serde_json::Value::Object(kept);
}

/// The major number of a `format_version` string (`"1.0"` → 1). A string without one counts
/// as the oldest version, so it opens writable and is rewritten on save (REQ-WSP-04).
fn major_version(format_version: &str) -> u32 {
    format_version
        .split('.')
        .next()
        .and_then(|part| part.parse().ok())
        .unwrap_or(0)
}

fn matches_bytes(path: &Path, bytes: &[u8]) -> Result<bool, SaveError> {
    match std::fs::read(path) {
        Ok(existing) => Ok(existing == bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(SaveError::Io(error)),
    }
}

/// Write through a temp file in the destination directory, fsync it, then rename it into
/// place (REQ-WSP-02, ADR-0001).
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), SaveError> {
    let Some(dir) = path.parent() else {
        return Err(SaveError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "a workspace file needs a parent directory",
        )));
    };
    std::fs::create_dir_all(dir)?;
    let mut file = tempfile::NamedTempFile::new_in(dir)?;
    std::io::Write::write_all(&mut file, bytes)?;
    file.as_file().sync_all()?;
    file.persist(path)
        .map_err(|error| SaveError::Io(error.error))?;
    Ok(())
}

mod watcher;
pub use watcher::{FsChange, WatchError, WorkspaceWatcher, watch};

#[cfg(test)]
#[path = "workspace_store_tests.rs"]
mod tests;
