//! The dynamic variables of one send: `$guid` and friends (PRD §7 REQ-VAR-05).

use std::collections::BTreeMap;
use time::OffsetDateTime;
use uuid::Uuid;

/// Values for the dynamic variables of one send.
///
/// A dynamic variable is generated once and handed to every later occurrence of the same
/// name, so `{{$randomInt}}` twice in one request sends one number (REQ-VAR-05). Create
/// one per send: a new instance generates new values.
pub struct DynamicValues {
    values: BTreeMap<String, String>,
}

impl DynamicValues {
    /// Nothing generated yet.
    pub fn new() -> Self {
        Self {
            values: BTreeMap::new(),
        }
    }

    /// The value for dynamic variable `name`, generating it on first use.
    ///
    /// `None` for any other name, which the caller then resolves as a scoped variable.
    pub fn value(&mut self, name: &str) -> Option<&str> {
        if !self.values.contains_key(name) {
            self.values.insert(name.to_string(), generate(name)?);
        }
        self.values.get(name).map(String::as_str)
    }

    /// Whether `name` is one of the dynamic variables this crate generates. Such a name
    /// always resolves, so it is never "unresolved".
    pub fn is_dynamic_name(name: &str) -> bool {
        matches!(
            name,
            "$guid" | "$timestamp" | "$isoTimestamp" | "$randomInt" | "$randomUUID"
        )
    }
}

impl Default for DynamicValues {
    fn default() -> Self {
        Self::new()
    }
}

/// Generate one value for a dynamic variable; `None` for any other name. Kept in step
/// with [`DynamicValues::is_dynamic_name`], which names the same five variables.
fn generate(name: &str) -> Option<String> {
    match name {
        // Both are random UUID v4 strings, hyphenated like Postman's.
        "$guid" | "$randomUUID" => Some(Uuid::new_v4().to_string()),
        "$timestamp" => Some(OffsetDateTime::now_utc().unix_timestamp().to_string()),
        // ISO 8601 with milliseconds, the shape Postman's $isoTimestamp emits. Built
        // from the time components so it cannot fail, unlike fallible formatting.
        "$isoTimestamp" => {
            let now = OffsetDateTime::now_utc();
            Some(format!(
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
                now.year(),
                u8::from(now.month()),
                now.day(),
                now.hour(),
                now.minute(),
                now.second(),
                now.millisecond(),
            ))
        }
        // 0 to 1000 inclusive, as Postman documents it.
        "$randomInt" => {
            let value: i32 = rand::random_range(0..=1000);
            Some(value.to_string())
        }
        _ => None,
    }
}

#[cfg(test)]
#[path = "dynamic_values_tests.rs"]
mod tests;
