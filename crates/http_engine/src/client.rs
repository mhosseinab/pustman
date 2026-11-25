//! The GPUI entity that owns an in-flight send (PRD REQ-HTTP-12).

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

use api_model::{Auth, GraphQlRequest, HttpRequest, Method, SecretValues};
use futures::channel::oneshot;
use gpui::{AppContext, Context, EntityId, EventEmitter, Task};
use secrets::{SecretKey, SecretStore};
use variables::{DynamicValues, ScopeKind, ScopeStack};

use crate::body::resolve_body_files;
use crate::error::TransportError;
use crate::graphql::{self, GraphQlError, GraphQlSendInput};
use crate::history::{HistoryDraft, HistoryRecorder};
use crate::pipeline::{PipelineError, Warning};
use crate::prepare::{Prepared, prepare, prepare_graphql};
use crate::redacted_url;
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
/// it, so the Console entry the event becomes names its own send, however entries land in
/// order (S33o: a replaced send's cancellation is emitted after its history row, and can
/// land after the new send's entry).
#[derive(Clone, PartialEq, Eq)]
pub struct SendIdentity {
    /// The request's method.
    pub method: Method,
    /// The request's URL as it may be named: the pre-resolution text where the send
    /// resolves `{{variables}}` itself ([`HttpClient::send_request`] — exactly what
    /// `record_error` records), and only the scheme where the URL is resolved text
    /// ([`HttpClient::send`]) — the resolved URL carries secret values, a webhook token
    /// lives in the path (PRD §7) — which is the `redacted_url` form [`fmt::Debug`]
    /// follows too.
    pub url: String,
}

impl fmt::Debug for SendIdentity {
    /// Print no URL: it may carry secret values (PRD §7) — the same reason
    /// [`ResolvedRequest`]'s `Debug` prints no values.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SendIdentity")
            .field("method", &self.method)
            .finish_non_exhaustive()
    }
}

/// What happened to a send [`HttpClient`] was driving, and which send it was.
#[derive(Clone, Debug)]
pub struct SendEvent {
    /// The send this is about. `None` only in [`SendEvent::Cancelled`], which names no
    /// send.
    pub send: Option<SendIdentity>,
    /// The protocol the send is listed under in History and the Console: `None` for
    /// HTTP, [`graphql::PROTOCOL`] for a [`HttpClient::send_graphql`] send. Not part of
    /// equality, like [`SendEvent::send`]'s absence.
    pub protocol: Option<&'static str>,
}

impl SendEvent {
    /// "A send was cancelled", naming no send: the token a subscriber compares against
    /// to ask what happened (`event == SendEvent::Cancelled`).
    // Lower case like the enum variant this replaces, so the token keeps reading as the
    // event's name.
    #[allow(non_upper_case_globals)]
    pub const Cancelled: Self = Self {
        send: None,
        protocol: None,
    };
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

/// Whose send it is: a new send replaces — and so cancels — only its own owner's previous
/// send, and a cancel drops only its own owner's (REQ-HTTP-12). Two request tabs sharing
/// the app's one client are two owners, so one tab's send never cancels another's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SendOwner {
    /// The owner of [`HttpClient::send`], [`HttpClient::send_request`] and
    /// [`HttpClient::cancel`]: callers that never name themselves share this one slot.
    Shared,
    /// A send owned by one entity: a request tab names itself by its entity id.
    Entity(EntityId),
}

impl From<EntityId> for SendOwner {
    fn from(entity: EntityId) -> Self {
        Self::Entity(entity)
    }
}

/// One owner's send in flight.
struct PendingSend {
    /// The send's work. One that already finished stays until its owner's next send or
    /// cancel replaces it; [`PendingSend::is_running`] tells whether it still runs.
    task: Task<()>,
    /// The history row the send leaves when it ends before its own work can record it
    /// (PRD REQ-HIST-01: every send is recorded). `None` for a send that records nothing.
    history: Option<Arc<HistoryDraft>>,
    /// The [`SendEvent`] emitted when the send is dropped (REQ-HTTP-12): which send it is
    /// and the protocol it is listed under.
    cancelled: SendEvent,
}

impl PendingSend {
    fn is_running(&self) -> bool {
        !self.task.is_ready()
    }
}

/// Owns the requests in flight, one per [`SendOwner`].
///
/// The tasks live here, so dropping one is a deliberate act and never an accident:
/// [`HttpClient::cancel_as`] drops its owner's — which aborts the connection — and a new
/// send from the same owner drops the one before it. Other owners' sends are untouched.
/// What a send produces comes back on the task the send method returns.
///
/// `Decision:` one client keeps one pending send per owner rather than one client entity
/// per tab: the app's single client already carries the history recorder, the workspace
/// root and the Console's `watch` subscription, and per-tab clients would each need all
/// three wired. The owner key keeps "a new send in the same tab replaces the previous
/// one" exact while tabs stay independent.
pub struct HttpClient {
    transport: Arc<dyn Transport>,
    /// The [`SendOwner::Shared`] send in flight.
    pending_send: Option<PendingSend>,
    /// Every [`SendOwner::Entity`] send, by owner. Finished entries are pruned on the
    /// next send, so a closed tab leaves nothing behind for long.
    owned_sends: HashMap<EntityId, PendingSend>,
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
            owned_sends: HashMap::new(),
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

    /// Whether any owner's send is still in flight.
    pub fn is_sending(&self) -> bool {
        self.pending_send
            .as_ref()
            .is_some_and(PendingSend::is_running)
            || self.owned_sends.values().any(PendingSend::is_running)
    }

    /// Whether `owner`'s send is still in flight.
    pub fn is_sending_as(&self, owner: SendOwner) -> bool {
        let pending = match owner {
            SendOwner::Shared => self.pending_send.as_ref(),
            SendOwner::Entity(entity) => self.owned_sends.get(&entity),
        };
        pending.is_some_and(PendingSend::is_running)
    }

    /// Take `owner`'s pending send out of its slot, leaving the slot empty.
    fn take_pending(&mut self, owner: SendOwner) -> Option<PendingSend> {
        match owner {
            SendOwner::Shared => self.pending_send.take(),
            SendOwner::Entity(entity) => self.owned_sends.remove(&entity),
        }
    }

    /// Put `pending` in `owner`'s slot.
    fn put_pending(&mut self, owner: SendOwner, pending: PendingSend) {
        match owner {
            SendOwner::Shared => self.pending_send = Some(pending),
            SendOwner::Entity(entity) => {
                self.owned_sends.insert(entity, pending);
            }
        }
    }

    /// Send `request` as [`SendOwner::Shared`], replacing — and so cancelling — that
    /// owner's send still in flight.
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
        // The request is resolved text by the time `send` sees it, and the resolved URL
        // carries secret values (PRD §7: a webhook token lives in the path), so the
        // identity keeps only the printable form: a cancelled send's Console entry names
        // the method and the scheme, never URL text the caller resolved.
        let identity = SendIdentity {
            method: request.method.clone(),
            url: redacted_url(&request.url),
        };
        let transport = self.transport.clone();
        self.drive_send(
            cx,
            SendOwner::Shared,
            SendEvent {
                send: Some(identity),
                protocol: None,
            },
            None,
            move || transport.send(request),
            Err(TransportError::Cancelled),
        )
    }

    /// [`HttpClient::send_request_as`] for [`SendOwner::Shared`].
    pub fn send_request(
        &mut self,
        input: SendInput,
        cx: &mut Context<HttpClient>,
    ) -> Task<Result<SendOutcome, PipelineError>> {
        self.send_request_as(SendOwner::Shared, input, cx)
    }

    /// Prepare `input` — secrets, `{{variables}}`, auth — and send it as `owner`,
    /// replacing — and so cancelling — `owner`'s send still in flight. Other owners'
    /// sends keep running.
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
    pub fn send_request_as(
        &mut self,
        owner: SendOwner,
        input: SendInput,
        cx: &mut Context<HttpClient>,
    ) -> Task<Result<SendOutcome, PipelineError>> {
        self.send_request_recorded_in(owner, input, None, cx)
    }

    /// [`HttpClient::send_request_as`] from the folder whose installation key is
    /// `installation_key` (`None`: no folder), which the history row keeps
    /// ([`HistoryRecorder::draft_recorded_in`]).
    pub fn send_request_recorded_in(
        &mut self,
        owner: SendOwner,
        input: SendInput,
        installation_key: Option<String>,
        cx: &mut Context<HttpClient>,
    ) -> Task<Result<SendOutcome, PipelineError>> {
        self.send_request_with_protocol(owner, input, installation_key, None, cx)
    }

    /// Send a GraphQL request as `owner` (REQ-GQL-01): built by
    /// [`graphql::to_http_request`] into `POST application/json {query, variables,
    /// operationName}`, then sent exactly as [`HttpClient::send_request_recorded_in`]
    /// sends — `{{variables}}`, secrets, auth, cancellation — with its history row
    /// naming the protocol [`graphql::PROTOCOL`]. The Console entry is the caller's to
    /// record, as for every send: `console::record_outcome_as(graphql::PROTOCOL, …)`.
    ///
    /// # Errors
    ///
    /// [`GraphQlError`] when the request cannot be built — variables that are not JSON —
    /// before anything is sent or recorded, so the editor can mark the spot.
    pub fn send_graphql(
        &mut self,
        owner: SendOwner,
        input: GraphQlSendInput,
        installation_key: Option<String>,
        cx: &mut Context<HttpClient>,
    ) -> Result<Task<Result<SendOutcome, PipelineError>>, GraphQlError> {
        let GraphQlSendInput {
            request,
            ancestors,
            scopes,
            secrets,
            secret_key,
        } = input;
        let input = SendInput {
            request: graphql::to_http_request(&request)?,
            ancestors,
            scopes,
            secrets,
            secret_key,
        };
        Ok(self.send_request_with_protocol(owner, input, installation_key, Some(request), cx))
    }

    /// The one prepare-and-send path behind [`HttpClient::send_request_recorded_in`] and
    /// [`HttpClient::send_graphql`]. With `graphql`, `input.request` is its
    /// [`graphql::to_http_request`], the body on the wire is built by
    /// [`crate::prepare::prepare_graphql`] and the history row names
    /// [`graphql::PROTOCOL`].
    fn send_request_with_protocol(
        &mut self,
        owner: SendOwner,
        input: SendInput,
        installation_key: Option<String>,
        graphql: Option<GraphQlRequest>,
        cx: &mut Context<HttpClient>,
    ) -> Task<Result<SendOutcome, PipelineError>> {
        let transport = self.transport.clone();
        let workspace_root = self.workspace_root.clone();
        let draft = self.history.as_ref().and_then(|recorder| {
            if graphql.is_some() {
                recorder.draft_for_protocol(&input.request, installation_key, graphql::PROTOCOL)
            } else {
                recorder.draft_recorded_in(&input.request, installation_key)
            }
        });
        // The send names itself in the [`SendEvent`] a cancellation becomes: the method
        // and URL as the caller sent it, placeholders and all (REQ-HTTP-12).
        let cancelled = SendEvent {
            send: Some(SendIdentity {
                method: input.request.method.clone(),
                url: input.request.url.clone(),
            }),
            protocol: graphql.as_ref().map(|_| graphql::PROTOCOL),
        };
        // The send's work and this entity share the one draft (an `Arc`): its gate keeps
        // the send to one row however the two writers race.
        let recorded = draft.clone();
        self.drive_send(
            cx,
            owner,
            cancelled,
            draft,
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
                let prepared = match &graphql {
                    Some(graphql) => prepare_graphql(
                        &request,
                        graphql,
                        &ancestors,
                        &scopes,
                        secrets.as_ref(),
                        secret_key.as_ref(),
                        &mut dynamic,
                    ),
                    None => prepare(
                        &request,
                        &ancestors,
                        &scopes,
                        secrets.as_ref(),
                        secret_key.as_ref(),
                        &mut dynamic,
                    ),
                };
                match prepared {
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
        )
    }

    /// The one cancel/replace/oneshot pipeline behind both send shapes: cancel `owner`'s
    /// send in flight and no one else's (emitting [`SendEvent::Cancelled`]), keep the new
    /// one in `owner`'s slot with its history row `history`, build the work through
    /// `make_work` —
    /// which lets [`HttpClient::send`] record its request only once the previous send is
    /// gone — run it on the background executor, and hand the caller a task that resolves
    /// with the outcome or `cancelled` once this entity drops the send.
    fn drive_send<T, Work, MakeWork>(
        &mut self,
        cx: &mut Context<HttpClient>,
        owner: SendOwner,
        cancelled_event: SendEvent,
        history: Option<Arc<HistoryDraft>>,
        make_work: MakeWork,
        cancelled: T,
    ) -> Task<T>
    where
        T: Send + 'static,
        Work: Future<Output = T> + Send + 'static,
        MakeWork: FnOnce() -> Work,
    {
        self.cancel_pending(owner, cx);
        // A finished send of a tab that has since closed would otherwise stay forever.
        self.owned_sends.retain(|_, pending| pending.is_running());
        let work = make_work();
        let (sender, receiver) = oneshot::channel();
        let task = cx.background_spawn(async move {
            let outcome = work.await;
            if sender.send(outcome).is_err() {
                // The caller dropped the task it was handed; nobody wants the outcome.
            }
        });
        self.put_pending(
            owner,
            PendingSend {
                task,
                history,
                cancelled: cancelled_event,
            },
        );
        cx.background_spawn(async move { receiver.await.unwrap_or(cancelled) })
    }

    /// [`HttpClient::cancel_as`] for [`SendOwner::Shared`].
    pub fn cancel(&mut self, cx: &mut Context<HttpClient>) {
        self.cancel_pending(SendOwner::Shared, cx);
    }

    /// Drop `owner`'s send in flight, which aborts its connection, and leave nothing
    /// pending for it (REQ-HTTP-12). Other owners' sends keep running. A send that
    /// already finished is not reported as cancelled.
    pub fn cancel_as(&mut self, owner: SendOwner, cx: &mut Context<HttpClient>) {
        self.cancel_pending(owner, cx);
    }

    /// Drop the in-flight task and tell subscribers it is gone, with the event naming
    /// the send it was (REQ-HTTP-12). With a history row to write, the [`SendEvent`] is
    /// emitted only after that row is stored: the Console entry it becomes and the
    /// History panel's re-read of the database run on this event (S33o), and neither may
    /// land before the row exists.
    fn cancel_pending(&mut self, owner: SendOwner, cx: &mut Context<HttpClient>) {
        let Some(PendingSend {
            task,
            history: draft,
            cancelled: event,
        }) = self.take_pending(owner)
        else {
            return;
        };
        let was_sending = !task.is_ready();
        // Dropping the task cancels the send, which aborts its connection.
        drop(task);
        // The dropped task can no longer record its own row, so the cancelled row is
        // written on a background task of its own (REQ-HIST-01 records every send) and
        // the event follows it — never blocking the foreground on SQLite.
        if was_sending && let Some(draft) = draft {
            cx.spawn(async move |this, cx| {
                cx.background_spawn(async move { draft.record_cancelled() })
                    .await;
                let emitted = this.update(cx, |_, cx| cx.emit(event));
                if emitted.is_err() {
                    // Handled: the client entity was dropped while the row was being
                    // written; there is nobody left to tell.
                }
            })
            .detach();
        } else if was_sending {
            // Nothing is written, so there is nothing to order the event after.
            cx.emit(event);
        }
    }
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "cancel_order_tests.rs"]
mod cancel_order_tests;

#[cfg(test)]
#[path = "send_identity_tests.rs"]
mod send_identity_tests;

#[cfg(test)]
#[path = "concurrent_send_tests.rs"]
mod concurrent_send_tests;
