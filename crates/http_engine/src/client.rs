//! The GPUI entity that owns an in-flight send (PRD REQ-HTTP-12).

use std::sync::Arc;

use futures::channel::oneshot;
use gpui::{AppContext, Context, EventEmitter, Task};

use crate::error::TransportError;
use crate::request::ResolvedRequest;
use crate::response::Response;
use crate::transport::Transport;

/// What happened to a send [`HttpClient`] was driving.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SendEvent {
    /// The in-flight send was dropped before it finished: [`HttpClient::cancel`], or a
    /// newer [`HttpClient::send`] replaced it. It is never retried, and the Console
    /// records "Cancelled" (REQ-HTTP-12).
    Cancelled,
}

impl EventEmitter<SendEvent> for HttpClient {}

/// Owns the request in flight.
///
/// The task lives here, so dropping it is a deliberate act and never an accident:
/// [`HttpClient::cancel`] drops it — which aborts the connection — and a new
/// [`HttpClient::send`] drops the one before it. What a send produces comes back on the
/// task [`HttpClient::send`] returns.
pub struct HttpClient {
    transport: Arc<dyn Transport>,
    /// The send in flight. One that already finished leaves its task here until the next
    /// `send` or `cancel` replaces it; [`HttpClient::is_sending`] reports whether it is
    /// still running.
    pending_send: Option<Task<()>>,
}

impl HttpClient {
    /// A client that sends through `transport`.
    pub fn new(transport: Arc<dyn Transport>) -> Self {
        Self {
            transport,
            pending_send: None,
        }
    }

    /// Whether a send is still in flight.
    pub fn is_sending(&self) -> bool {
        self.pending_send
            .as_ref()
            .is_some_and(|task| !task.is_ready())
    }

    /// Send `request`, replacing — and so cancelling — any send still in flight.
    ///
    /// Dropping the returned task does not cancel the request: the request belongs to
    /// this entity until it finishes or [`HttpClient::cancel`] drops it. The task resolves
    /// with the outcome, or with [`TransportError::Cancelled`] when the send was cancelled
    /// or replaced. Nothing is ever retried.
    pub fn send(
        &mut self,
        request: ResolvedRequest,
        cx: &mut Context<HttpClient>,
    ) -> Task<Result<Response, TransportError>> {
        self.cancel_pending(cx);
        let in_flight = self.transport.send(request);
        let (sender, receiver) = oneshot::channel();
        self.pending_send = Some(cx.background_spawn(async move {
            let outcome = in_flight.await;
            if sender.send(outcome).is_err() {
                // The caller dropped the task it was handed; nobody wants the outcome.
            }
        }));
        cx.background_spawn(async move { receiver.await.unwrap_or(Err(TransportError::Cancelled)) })
    }

    /// Drop the in-flight send, which aborts its connection, and leave nothing pending
    /// (REQ-HTTP-12). A send that already finished is not reported as cancelled.
    pub fn cancel(&mut self, cx: &mut Context<HttpClient>) {
        self.cancel_pending(cx);
    }

    /// Drop the in-flight task and tell subscribers it is gone.
    fn cancel_pending(&mut self, cx: &mut Context<HttpClient>) {
        let was_sending = self.is_sending();
        self.pending_send = None;
        if was_sending {
            cx.emit(SendEvent::Cancelled);
        }
    }
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
