//! The GPUI entity that owns an in-flight send (PRD REQ-HTTP-12).

use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

use api_model::{Auth, HttpRequest, Method, SecretValues};
use futures::channel::oneshot;
use gpui::{AppContext, Context, EventEmitter, Task};
use secrets::{SecretKey, SecretStore};
use variables::{DynamicValues, ScopeKind, ScopeStack};

use crate::body::resolve_body_files;
use crate::error::TransportError;
use crate::history::{HistoryDraft, HistoryRecorder};
use crate::pipeline::{PipelineError, Warning};
use crate::prepare::{Prepared, prepare};
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
/// `Debug` prints no values: the request's `Debug` prints no header or body values, the
/// response's `Debug` redacts, and `secrets_used` prints counts only, so an outcome can
/// be logged as it is.
#[derive(Debug)]
pub struct SendOutcome {
    /// The request as it went on the wire, every `{{variable}}` resolved — what the
    /// Console's raw view shows (REQ-CON-03). The redaction downstream uses
    /// `secrets_used` on it, exactly as `history` does.
    pub request: ResolvedRequest,
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

/// Which send a [`SendEvent`] is about: the request's method and URL as the caller sent
/// it — `{{variables}}` unresolved where the send resolves them — so the Console entry
/// the event becomes names its own send, however entries land in order (S33o: a replaced
/// send's cancellation is emitted after its history row, and can land after the new
/// send's entry).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SendIdentity {
    /// The request's method.
    pub method: Method,
    /// The request's URL, as the caller sent it.
    pub url: String,
}

/// What happened to a send [`HttpClient`] was driving, and which send it was.
#[derive(Clone, Debug)]
pub struct SendEvent {
    /// The send this is about. `None` only in [`SendEvent::Cancelled`], which names no
    /// send.
    pub send: Option<SendIdentity>,
}

impl SendEvent {
    /// "A send was cancelled", naming no send: the token a subscriber compares against
    /// to ask what happened (`event == SendEvent::Cancelled`).
    // Lower case like the enum variant this replaces, so the token keeps reading as the
    // event's name.
    #[allow(non_upper_case_globals)]
    pub const Cancelled: Self = Self { send: None };
}

impl PartialEq for SendEvent {
    /// Events compare by what happened, never by which send it happened to: there is one
    /// kind of occurrence — a cancellation — so [`SendEvent::Cancelled`] is equal to
    /// every cancellation whatever it names, and two events naming different sends are
    /// unequal. `Eq` is deliberately not implemented: the name-less token is equal to
    /// every cancellation, which is not transitive. Which send is read from
    /// [`SendEvent::send`].
    fn eq(&self, other: &Self) -> bool {
        match (&self.send, &other.send) {
            (Some(this), Some(other)) => this == other,
            _ => true,
        }
    }
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
    /// The history row the in-flight [`HttpClient::send_request`] leaves when it ends
    /// before its own work can record it (PRD REQ-HIST-01: every send is recorded).
    /// `None` for a send that records nothing.
    pending_history: Option<Arc<HistoryDraft>>,
    /// Which send is in flight: what the [`SendEvent`] naming it carries when the send
    /// is dropped (REQ-HTTP-12). `None` when no send is pending.
    pending_identity: Option<SendIdentity>,
    /// The root [`send_request`] resolves body file references against (REQ-HTTP-05).
    /// `None` keeps them as they are, which is what [`HttpClient::send`] does too.
    workspace_root: Option<PathBuf>,
    /// Where [`HttpClient::send_request`] records history. `None` records nothing, which
    /// is what [`HttpClient::send`] does too: it carries no pre-resolution request.
    history: Option<HistoryRecorder>,
}

impl HttpClient {
    /// A client that sends through `transport`.
    pub fn new(transport: Arc<dyn Transport>) -> Self {
        Self {
            transport,
            pending_send: None,
            pending_history: None,
            pending_identity: None,
            workspace_root: None,
            history: None,
        }
    }

    /// Resolve body file references against `root` from the next send on (REQ-HTTP-05):
    /// a relative one stays inside it and an absolute one is kept as it is.
    pub fn set_workspace_root(&mut self, root: PathBuf) {
        self.workspace_root = Some(root);
    }

    /// Record every [`HttpClient::send_request`] through `recorder` from the next send on
    /// (PRD §8.5). [`HttpClient::send`] records nothing: it has no pre-resolution
    /// request to store.
    pub fn set_history(&mut self, recorder: HistoryRecorder) {
        self.history = Some(recorder);
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
        let identity = SendIdentity {
            method: request.method.clone(),
            url: request.url.as_str().to_owned(),
        };
        let transport = self.transport.clone();
        self.drive_send(
            cx,
            identity,
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
        let draft = self
            .history
            .as_ref()
            .and_then(|recorder| recorder.draft(&input.request));
        // The send names itself in the [`SendEvent`] a cancellation becomes: the method
        // and URL as the caller sent it, placeholders and all (REQ-HTTP-12).
        let identity = SendIdentity {
            method: input.request.method.clone(),
            url: input.request.url.clone(),
        };
        // The send's work and this entity share the one draft (an `Arc`): its gate keeps
        // the send to one row however the two writers race.
        let recorded = draft.clone();
        let task = self.drive_send(
            cx,
            identity,
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
                match prepare(
                    &request,
                    &ancestors,
                    &scopes,
                    secrets.as_ref(),
                    secret_key.as_ref(),
                    &mut dynamic,
                ) {
                    Err(error) => {
                        // No secret was substituted before preparation failed, so the
                        // empty set redacts the pre-resolution request (history.rs).
                        if let Some(draft) = &recorded {
                            draft.record_error(&request, &error, &SecretValues::new());
                        }
                        Err(error)
                    }
                    Ok(prepared) => {
                        let Prepared {
                            mut resolved,
                            warnings,
                            auth,
                            secrets_used,
                        } = prepared;
                        if let Some(root) = &workspace_root {
                            resolved.body = match resolve_body_files(resolved.body, root) {
                                Ok(body) => body,
                                Err(error) => {
                                    let error = PipelineError::Transport(error);
                                    if let Some(draft) = &recorded {
                                        draft.record_error(&request, &error, &secrets_used);
                                    }
                                    return Err(error);
                                }
                            };
                        }
                        // The history row is written before the outcome reaches the
                        // caller: the caller's task resolves only once it is stored.
                        // The resolved request is kept for the outcome: the Console
                        // shows it as it went on the wire (REQ-CON-03).
                        let sent = resolved.clone();
                        match transport.send(resolved).await {
                            Ok(response) => {
                                if let Some(draft) = &recorded {
                                    draft.record_response(&request, &response, &secrets_used);
                                }
                                Ok(SendOutcome {
                                    request: sent,
                                    response,
                                    warnings,
                                    auth,
                                    secrets_used,
                                })
                            }
                            Err(error) => {
                                let error = PipelineError::Transport(error);
                                if let Some(draft) = &recorded {
                                    draft.record_error(&request, &error, &secrets_used);
                                }
                                Err(error)
                            }
                        }
                    }
                }
            },
            Err(PipelineError::Transport(TransportError::Cancelled)),
        );
        // Set alongside the new pending task, after `drive_send` recorded the send it
        // replaced (a replaced send is a cancellation).
        self.pending_history = draft;
        task
    }

    /// The one cancel/replace/oneshot pipeline behind both send shapes: cancel the send in
    /// flight (emitting [`SendEvent::Cancelled`]), build the work through `make_work` —
    /// which lets [`HttpClient::send`] record its request only once the previous send is
    /// gone — run it on the background executor, and hand the caller a task that resolves
    /// with the outcome or `cancelled` once this entity drops the send.
    fn drive_send<T, Work, MakeWork>(
        &mut self,
        cx: &mut Context<HttpClient>,
        identity: SendIdentity,
        make_work: MakeWork,
        cancelled: T,
    ) -> Task<T>
    where
        T: Send + 'static,
        Work: Future<Output = T> + Send + 'static,
        MakeWork: FnOnce() -> Work,
    {
        self.cancel_pending(cx);
        self.pending_identity = Some(identity);
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

    /// Drop the in-flight task and tell subscribers it is gone, with the event naming
    /// the send it was (REQ-HTTP-12). With a history row to write, the [`SendEvent`] is
    /// emitted only after that row is stored: the Console entry it becomes and the
    /// History panel's re-read of the database run on this event (S33o), and neither may
    /// land before the row exists.
    fn cancel_pending(&mut self, cx: &mut Context<HttpClient>) {
        let was_sending = self.is_sending();
        self.pending_send = None;
        let draft = self.pending_history.take();
        let send = self.pending_identity.take();
        // The dropped task can no longer record its own row, so the cancelled row is
        // written on a background task of its own (REQ-HIST-01 records every send) and
        // the event follows it — never blocking the foreground on SQLite.
        if was_sending && let Some(draft) = draft {
            cx.spawn(async move |this, cx| {
                cx.background_spawn(async move { draft.record_cancelled() })
                    .await;
                let emitted = this.update(cx, |_, cx| cx.emit(SendEvent { send }));
                if emitted.is_err() {
                    // Handled: the client entity was dropped while the row was being
                    // written; there is nobody left to tell.
                }
            })
            .detach();
        } else if was_sending {
            // Nothing is written, so there is nothing to order the event after.
            cx.emit(SendEvent { send });
        }
    }
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "cancel_order_tests.rs"]
mod cancel_order_tests;
