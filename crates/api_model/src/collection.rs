//! Workspaces, collections and folders (PRD §4, §6.1).
//!
//! A workspace folder holds one file per item; a parent stores its children's order as an
//! array of ids (`order`) rather than embedding them, so concurrent edits touch one file
//! each (REQ-WSP-01). The `children` field is therefore the in-memory tree assembled from
//! those files (or from an import) and is not serialized.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::ItemId;
use crate::auth::Auth;
use crate::request::Request;
use crate::variable::Variable;

/// `pustman.json`: the identity of one workspace folder (PRD §6.1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    pub id: ItemId,
    pub name: String,
    /// Checked on open; a newer major version opens read-only (REQ-WSP-04).
    pub format_version: String,
    /// Unknown fields from imports, preserved verbatim (REQ-MODEL-02).
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// One collection: metadata, auth, variables and the order of its children.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Collection {
    pub id: ItemId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Auth::is_inherit")]
    pub auth: Auth,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variables: Vec<Variable>,
    /// Child ids in display order (REQ-WSP-01); the children themselves live in their own
    /// files.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub order: Vec<ItemId>,
    /// The in-memory child tree (folders and requests). Persistence is `order` plus one file
    /// per child, so this is never serialized.
    #[serde(skip)]
    pub children: Vec<Child>,
    /// Unknown fields from imports, preserved verbatim (REQ-MODEL-02).
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// One folder: metadata, auth and the order of its children (PRD §4).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Folder {
    pub id: ItemId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Auth::is_inherit")]
    pub auth: Auth,
    /// The in-memory child tree; persistence is `order` plus one file per child.
    #[serde(skip)]
    pub children: Vec<Child>,
    /// Child ids in display order (REQ-WSP-01).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub order: Vec<ItemId>,
    /// Unknown fields from imports, preserved verbatim (REQ-MODEL-02).
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// A child of a collection or folder (PRD §4: `children: [Folder|Request]`).
// `Request` is the largest variant because a request carries its tables and settings
// inline (REQ-HTTP-03's `disabled_auto_headers` included); model values are cloned per
// edit rather than per frame, so boxing would cost more in call sites than it saves —
// the same reason `CollectionItem` in `request.rs` allows this lint.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
pub enum Child {
    Folder(Folder),
    Request(Request),
}

impl From<Folder> for Child {
    fn from(folder: Folder) -> Self {
        Child::Folder(folder)
    }
}

impl From<Request> for Child {
    fn from(request: Request) -> Self {
        Child::Request(request)
    }
}

#[cfg(test)]
#[path = "collection_tests.rs"]
mod tests;
