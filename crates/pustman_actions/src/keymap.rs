//! The default keymaps (`assets/keymaps/`, PRD §3) and the loader that binds them.
//!
//! A keymap file is a JSON array of groups. Each group names one key context and maps
//! keystrokes to actions: a bare string is a unit action (`request::Send`), an array pairs
//! an action name with its JSON input (`["workspace::ActivateTab", 0]`). Duplicate
//! keystrokes inside one context survive parsing so they can be reported instead of one
//! binding silently replacing another.

use gpui::{App, KeyBinding, KeyBindingContextPredicate};
use serde::Deserialize;
use std::{fmt, rc::Rc};

/// The bundled default keymaps, one per platform (PRD §3: Cmd on macOS, Ctrl on Linux and
/// Windows).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeymapPlatform {
    Macos,
    Linux,
    Windows,
}

impl KeymapPlatform {
    /// Every bundled keymap, for the tests that must hold on all three platforms.
    #[cfg(test)]
    const ALL: [KeymapPlatform; 3] = [
        KeymapPlatform::Macos,
        KeymapPlatform::Linux,
        KeymapPlatform::Windows,
    ];

    fn current() -> Self {
        if cfg!(target_os = "macos") {
            KeymapPlatform::Macos
        } else if cfg!(target_os = "windows") {
            KeymapPlatform::Windows
        } else {
            KeymapPlatform::Linux
        }
    }

    fn json(self) -> &'static str {
        match self {
            KeymapPlatform::Macos => include_str!("../../../assets/keymaps/default-macos.json"),
            KeymapPlatform::Linux => include_str!("../../../assets/keymaps/default-linux.json"),
            KeymapPlatform::Windows => include_str!("../../../assets/keymaps/default-windows.json"),
        }
    }
}

/// One binding from a keymap file, in file order.
#[derive(Debug, PartialEq)]
struct KeymapEntry {
    context: String,
    keystroke: String,
    action: String,
    input: Option<serde_json::Value>,
}

impl KeymapEntry {
    /// Turns a problem with this entry into a message the user can act on.
    fn problem(&self, detail: String) -> String {
        format!(
            "keymap entry \"{}\" -> {} in context \"{}\": {}",
            self.keystroke, self.action, self.context, detail
        )
    }
}

/// Binds the default keymap for the current platform (PRD §3).
///
/// Returns one message per entry that could not be bound; the caller must surface these
/// to the user — a bad entry is never silently dropped.
pub fn load_default_keymap(cx: &mut App) -> Vec<String> {
    load_keymap(cx, KeymapPlatform::current().json())
}

/// Binds every entry of one keymap file, skipping and reporting the entries that do not bind.
fn load_keymap(cx: &mut App, json: &str) -> Vec<String> {
    let entries = match parse_keymap_entries(json) {
        Ok(entries) => entries,
        Err(problem) => return vec![problem],
    };

    let mut problems = Vec::new();
    let mut bindings = Vec::new();
    for entry in entries {
        match build_binding(cx, &entry) {
            Ok(binding) => bindings.push(binding),
            Err(problem) => problems.push(problem),
        }
    }
    cx.bind_keys(bindings);
    problems
}

/// Parses one keymap file into its entries, in file order.
fn parse_keymap_entries(json: &str) -> Result<Vec<KeymapEntry>, String> {
    let groups: Vec<RawGroup> = serde_json::from_slice(json.as_bytes())
        .map_err(|error| format!("invalid keymap JSON: {error}"))?;

    let mut entries = Vec::new();
    for group in groups {
        for (keystroke, spec) in group.bindings.0 {
            let (action, input) = parse_action_spec(&spec)?;
            entries.push(KeymapEntry {
                context: group.context.clone(),
                keystroke,
                action,
                input,
            });
        }
    }
    Ok(entries)
}

/// The action half of one binding: a bare action name, or a name paired with its input.
fn parse_action_spec(
    spec: &serde_json::Value,
) -> Result<(String, Option<serde_json::Value>), String> {
    match spec {
        serde_json::Value::String(name) => Ok((name.clone(), None)),
        serde_json::Value::Array(parts) => match parts.as_slice() {
            [serde_json::Value::String(name)] => Ok((name.clone(), None)),
            [serde_json::Value::String(name), input] => Ok((name.clone(), Some(input.clone()))),
            _ => Err(format!(
                "binding value must be an action name or [name, input], got {spec}"
            )),
        },
        _ => Err(format!(
            "binding value must be an action name or [name, input], got {spec}"
        )),
    }
}

/// Builds one key binding; the error message names the offending entry.
fn build_binding(cx: &App, entry: &KeymapEntry) -> Result<KeyBinding, String> {
    let action = cx
        .build_action(&entry.action, entry.input.clone())
        .map_err(|error| entry.problem(format!("cannot build action: {error}")))?;
    let context_predicate = KeyBindingContextPredicate::parse(&entry.context)
        .map_err(|error| entry.problem(format!("cannot parse context: {error}")))?;
    KeyBinding::load(
        &entry.keystroke,
        action,
        Some(Rc::new(context_predicate)),
        false,
        None,
        cx,
    )
    .map_err(|error| entry.problem(format!("cannot parse keystroke: {error}")))
}

#[derive(Deserialize)]
struct RawGroup {
    context: String,
    bindings: RawBindings,
}

/// The bindings map of one context group, kept as raw pairs so that a keystroke bound
/// twice in the same JSON object is not silently merged away by `serde_json::Value`.
struct RawBindings(Vec<(String, serde_json::Value)>);

impl<'de> Deserialize<'de> for RawBindings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct BindingsVisitor;

        impl<'de> serde::de::Visitor<'de> for BindingsVisitor {
            type Value = RawBindings;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a map of keystrokes to actions")
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: serde::de::MapAccess<'de>,
            {
                let mut bindings = Vec::new();
                while let Some((keystroke, spec)) = map.next_entry::<String, serde_json::Value>()? {
                    bindings.push((keystroke, spec));
                }
                Ok(RawBindings(bindings))
            }
        }

        deserializer.deserialize_map(BindingsVisitor)
    }
}

#[cfg(test)]
#[path = "keymap_tests.rs"]
mod tests;
