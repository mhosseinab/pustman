//! The method vocabulary of the method picker (REQ-HTTP-01): the standard verbs the
//! dropdown lists, and the rule a typed custom method must pass.
//!
//! The token rule has exactly one implementation — [`api_model::Method::from_token`],
//! which checks the RFC 9110 `token` (`tchar`) set — and this module calls it. A second
//! validator here could disagree with what the model accepts on import or export.

use api_model::{Method, MethodError};
use gpui::SharedString;

/// The standard verbs the dropdown lists, in menu order (REQ-HTTP-01).
pub const STANDARD_METHODS: [Method; 9] = [
    Method::Get,
    Method::Post,
    Method::Put,
    Method::Patch,
    Method::Delete,
    Method::Head,
    Method::Options,
    Method::Trace,
    Method::Connect,
];

/// The custom method the user typed: the model's token rule applied to the trimmed
/// text. A text field collects stray whitespace; a token never carries any.
pub fn custom_method(text: &str) -> Result<Method, MethodError> {
    Method::from_token(text.trim())
}

/// How a rejected token is explained where it was typed: what the user entered and what
/// a method may contain (RFC 9110 §5.6.2 `token`).
pub fn rejection_message(text: &str) -> SharedString {
    let typed = text.trim();
    format!(
        "{typed:?} is not a valid HTTP method — a method is a non-empty token of \
         letters, digits or ! # $ % & ' * + - . ^ _ ` | ~ (RFC 9110)"
    )
    .into()
}

#[cfg(test)]
#[path = "method_tests.rs"]
mod tests;
