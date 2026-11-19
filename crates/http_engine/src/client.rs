//! The GPUI entity that owns an in-flight send (PRD REQ-HTTP-12).

use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

use api_model::{Auth, HttpRequest, SecretValues};
use futures::channel::oneshot;
use gpui::{AppContext, Context, EventEmitter, Task};
use secrets::{SecretKey, SecretStore};
use variables::{DynamicValues, ScopeKind, ScopeStack};

use crate::body::resolve_body_files;
use crate::error::TransportError;
use crate::pipeline::{PipelineError, Warning};
use crate::prepare::prepare;
use crate::request::ResolvedRequest;
use crate::request_auth::AppliedAuth;
use crate::response::Response;
use crate::transport::Transport;

/// How a send finds the keychain key of a secret variable: the winning variable's scope
/// kind and name in, the key out — or `None` for "no key for this scope". Only the
/// caller knows the workspace and scope ids the key needs.
pub type SecretKeyLookup = Arc<dyn Fn(ScopeKind, &str) -> Option<SecretKey> + Send + Sync>;

/// Everything one [`HttpClient::send_request`] needs (PRD §7): owned outright, because
/// the send runs on the background executor and the caller moves on.
pub struct SendInput {
    /// The request to send, unresolved: [`prepare`] fills in `{{variables}}`, secrets and
    /// auth before the transport sees it.
    pub request: HttpRequest,
    /// The request's parents' auths, nearest first (folder → … → collection).
    pub ancestors: Vec<Auth>,
    /// The variables the request resolves against.
    pub scopes: ScopeStack,
    /// Where a referenced secret's value comes from.
    pub secrets: Arc<dyn SecretStore>,
    /// The keychain key of a secret variable, given the winning variable's scope kind and
    /// name: only the caller knows the workspace and scope ids. `None` means "no key for
    /// this scope", which is an unresolved secret.
    pub secret_key: SecretKeyLookup,
}

/// What one [`HttpClient::send_request`] produced.
///
/// `Debug` prints no values: the response's `Debug` redacts, and `secrets_used` prints
/// counts only, so an outcome can be logged as it is.
#[derive(Debug)]
pub struct SendOutcome {
    /// The server's answer. 4xx and 5xx are responses, not errors.
    pub response: Response,
    /// What to tell the user about the send: variables nothing resolved, TLS verification
    /// off (REQ-VAR-07, REQ-HTTP-10).
    pub warnings: Vec<Warning>,
    /// What the effective auth did. An `Unsupported` or `Incomplete` auth added no
    /// credential, and must reach the user — it is never sent silently (REQ-HTTP-06).
    pub auth: AppliedAuth,
    /// The secret values this send substituted, for redaction downstream
    /// (history, Console, exports — `api_model::redact_text`). Never log them; when
    /// [`SecretValues::refused_count`] is not zero the caller warns the user that those
    /// values cannot be masked and appear as they are.
    pub secrets_used: SecretValues,
}

/// What happened to a send [`HttpClient`] was driving.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SendEvent {
    /// The in-flight send was dropped before it finished: [`HttpClient::cancel`], or a
    /// newer [`HttpClient::send`] or [`HttpClient::send_request`] replaced it. It is never
    /// retried, and the Console records "Cancelled" (REQ-HTTP-12).
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
    /// The root [`send_request`] resolves body file references against (REQ-HTTP-05).
    /// `None` keeps them as they are, which is what [`HttpClient::send`] does too.
    workspace_root: Option<PathBuf>,
}

impl HttpClient {
    /// A client that sends through `transport`.
    pub fn new(transport: Arc<dyn Transport>) -> Self {
        Self {
            transport,
            pending_send: None,
            workspace_root: None,
        }
    }

    /// Resolve body file references against `root` from the next send on (REQ-HTTP-05):
    /// a relative one stays inside it and an absolute one is kept as it is.
    pub fn set_workspace_root(&mut self, root: PathBuf) {
        self.workspace_root = Some(root);
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
        let transport = self.transport.clone();
        self.drive_send(
            cx,
            move || transport.send(request),
            Err(TransportError::Cancelled),
        )
    }

    /// Prepare `input` — secrets, `{{variables}}`, auth — and send it, replacing — and so
    /// cancelling — any send still in flight.
    ///
    /// Preparation reads the OS keychain, which is blocking IO: it runs on the background
    /// executor, never on the foreground, and the transport is called only afterwards. A
    /// secret variable with no local value fails the send as
    /// [`TransportError::UnresolvedSecret`] before the transport is called at all — the
    /// wire never sees an empty string or a literal `{{name}}` for a secret
    /// (REQ-VAR-SHARE-02). The body's file references resolve against the workspace root
    /// the same way, in the same task (REQ-HTTP-05). Everything else — warnings, the auth
    /// outcome, the secret values for redaction — comes back in [`SendOutcome`].
    ///
    /// Dropping the returned task does not cancel the send and nothing is retried, exactly
    /// as for [`HttpClient::send`].
    pub fn send_request(
        &mut self,
        input: SendInput,
        cx: &mut Context<HttpClient>,
    ) -> Task<Result<SendOutcome, PipelineError>> {
        let transport = self.transport.clone();
        let workspace_root = self.workspace_root.clone();
        self.drive_send(
            cx,
            move || async move {
                let SendInput {
                    request,
                    ancestors,
                    scopes,
                    secrets,
                    secret_key,
                } = input;
                // One `DynamicValues` is one send: `{{$guid}}` carries one value through
                // every field and the auth (REQ-VAR-05).
                let mut dynamic = DynamicValues::new();
                let ancestors: Vec<&Auth> = ancestors.iter().collect();
                let mut prepared = prepare(
                    &request,
                    &ancestors,
                    &scopes,
                    secrets.as_ref(),
                    secret_key.as_ref(),
                    &mut dynamic,
                )?;
                if let Some(root) = &workspace_root {
                    prepared.resolved.body = resolve_body_files(prepared.resolved.body, root)?;
                }
                let response = transport.send(prepared.resolved).await?;
                Ok(SendOutcome {
                    response,
                    warnings: prepared.warnings,
                    auth: prepared.auth,
                    secrets_used: prepared.secrets_used,
                })
            },
            Err(PipelineError::Transport(TransportError::Cancelled)),
        )
    }

    /// The one cancel/replace/oneshot pipeline behind both send shapes: cancel the send in
    /// flight (emitting [`SendEvent::Cancelled`]), build the work through `make_work` —
    /// which lets [`HttpClient::send`] record its request only once the previous send is
    /// gone — run it on the background executor, and hand the caller a task that resolves
    /// with the outcome or `cancelled` once this entity drops the send.
    fn drive_send<T, Work, MakeWork>(
        &mut self,
        cx: &mut Context<HttpClient>,
        make_work: MakeWork,
        cancelled: T,
    ) -> Task<T>
    where
        T: Send + 'static,
        Work: Future<Output = T> + Send + 'static,
        MakeWork: FnOnce() -> Work,
    {
        self.cancel_pending(cx);
        let work = make_work();
        let (sender, receiver) = oneshot::channel();
        self.pending_send = Some(cx.background_spawn(async move {
            let outcome = work.await;
            if sender.send(outcome).is_err() {
                // The caller dropped the task it was handed; nobody wants the outcome.
            }
        }));
        cx.background_spawn(async move { receiver.await.unwrap_or(cancelled) })
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
