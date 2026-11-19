//! The abstraction a send goes through (PRD §5.1).

use futures::future::BoxFuture;

use crate::error::TransportError;
use crate::request::ResolvedRequest;
use crate::response::Response;

/// How a [`ResolvedRequest`] reaches a server.
///
/// The `send` is a boxed future so one [`crate::HttpClient`] can hold any transport as a
/// trait object. Its body must not block: whatever reads files or waits on the network
/// happens inside the returned future, and dropping that future aborts the request.
pub trait Transport: Send + Sync + 'static {
    /// Send one request. 4xx and 5xx come back as [`Response`]s, not as errors.
    fn send(
        &self,
        request: ResolvedRequest,
    ) -> BoxFuture<'static, Result<Response, TransportError>>;
}
