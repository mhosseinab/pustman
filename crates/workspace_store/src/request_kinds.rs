//! Which file suffix each request kind is saved under (PRD §6.1, REQ-WSP-01).
//!
//! [`KINDS`] is the one table: saving picks a request's suffix from its kind and opening
//! picks a file's kind from its suffix, so the two directions cannot drift apart.

use api_model::Request;

/// A request kind as its file name encodes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestKind {
    /// `.request.json`: an HTTP request, and also the home of a kind this build does not
    /// model yet (MQTT, …), which round-trips verbatim inside it.
    Http,
    GraphQl,
    WebSocket,
    SocketIo,
    Grpc,
}

/// Each kind with the `type` tag its JSON carries and the suffix its file is named with.
const KINDS: [(RequestKind, &str, &str); 5] = [
    (RequestKind::Http, "http", ".request.json"),
    (RequestKind::GraphQl, "graphql", ".graphql.json"),
    (RequestKind::WebSocket, "websocket", ".websocket.json"),
    (RequestKind::SocketIo, "socketio", ".socketio.json"),
    (RequestKind::Grpc, "grpc", ".grpc.json"),
];

/// The suffix of the generic request file, which holds HTTP and any unmodelled kind.
const REQUEST_SUFFIX: &str = ".request.json";

/// The file suffix `request` is saved under.
///
/// The lookup goes by the `type` tag rather than the variant, so a known kind that failed
/// to decode (kept as `Request::Unsupported`) stays in the file it was read from instead
/// of being moved to `.request.json` on its next save.
pub fn suffix_for(request: &Request) -> &'static str {
    let tag = request.type_tag();
    KINDS
        .iter()
        .find(|(_, kind_tag, _)| *kind_tag == tag)
        .map_or(REQUEST_SUFFIX, |(_, _, suffix)| suffix)
}

/// The request kind a file name encodes, or `None` when it is not a request file. The
/// suffix must follow a non-empty stem: `graphql.json` alone is not a request.
pub fn kind_for_file_name(name: &str) -> Option<RequestKind> {
    KINDS.iter().find_map(|(kind, _, suffix)| {
        name.strip_suffix(suffix)
            .is_some_and(|stem| !stem.is_empty())
            .then_some(*kind)
    })
}

#[cfg(test)]
#[path = "request_kinds_tests.rs"]
mod tests;
