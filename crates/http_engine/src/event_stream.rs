//! A `text/event-stream` response (REQ-SSE-01) read as it arrives: its events reach the
//! caller as [`SessionEvent`]s, one at a time, until the server closes the stream or the
//! user stops it.
//!
//! The transport hands the response body to [`EventStream::open`], which returns the
//! receiver the response carries ([`crate::ResponseBody::EventStream`]) and the pump that
//! feeds it. The pump owns the body: when it returns, the body is dropped, which closes
//! the connection. It returns when the server ends the body, when the body fails, when
//! [`EventStream::stop`] is called, and when every clone of the [`EventStream`] is gone —
//! so dropping the stream cancels it, as dropping a task does.
//!
//! The parser's size bounds drop or ignore oversized server input; every time they do,
//! the user is told through a [`SessionEvent::Error`] rather than the loss being silent.
//! There is no reconnect yet, so the server's `retry` field is not acted on.

use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use api_model::{CloseReason, SessionError, SessionEvent};
use futures::channel::{mpsc, oneshot};
use futures::future::{self, Either};
use futures::{SinkExt, Stream, StreamExt};

use crate::event_source::{EventSourceParser, MAX_EVENT_BYTES, MAX_FIELD_BYTES, MAX_ID_BYTES};
use crate::response::ResponseBody;

/// How many events wait for the reader before the pump stops reading the body. The
/// limit is backpressure: a server faster than the UI waits in its own socket buffers,
/// not in this process's memory.
const EVENT_BUFFER: usize = 64;

/// Whether a `Content-Type` value names an event stream: its media type, parameters and
/// case aside, is `text/event-stream`.
pub(crate) fn is_event_stream(content_type: &str) -> bool {
    content_type
        .split(';')
        .next()
        .is_some_and(|media_type| media_type.trim().eq_ignore_ascii_case("text/event-stream"))
}

/// The receiving end of an event-stream response.
///
/// Clones share one stream: an event is read by whichever clone asks first, and a stop
/// from any clone stops it for all. Two `EventStream`s are equal when they are the same
/// stream.
#[derive(Clone)]
pub struct EventStream {
    shared: Arc<Shared>,
}

struct Shared {
    receiving: futures::lock::Mutex<Receiving>,
    /// Dropping the sender is the stop signal. `None` once the stream was stopped.
    stop: Mutex<Option<oneshot::Sender<()>>>,
}

struct Receiving {
    events: mpsc::Receiver<SessionEvent>,
    /// A [`SessionEvent::Closed`] was handed out: the stream has nothing more to say.
    closed: bool,
}

impl EventStream {
    /// The receiver for `body`, a response body's chunks, and the pump that parses them
    /// into it. The caller runs the pump on the runtime the body belongs to; it ends on
    /// its own (module docs). `describe` turns a body failure into the sentence the user
    /// reads.
    pub(crate) fn open<S, B, E, D>(
        body: S,
        describe: D,
    ) -> (Self, impl Future<Output = ()> + Send + 'static)
    where
        S: Stream<Item = Result<B, E>> + Send + 'static,
        B: AsRef<[u8]> + Send + 'static,
        E: Send + 'static,
        D: Fn(E) -> String + Send + 'static,
    {
        let (sender, events) = mpsc::channel(EVENT_BUFFER);
        let (stop_sender, stop) = oneshot::channel();
        let stream = Self {
            shared: Arc::new(Shared {
                receiving: futures::lock::Mutex::new(Receiving {
                    events,
                    closed: false,
                }),
                stop: Mutex::new(Some(stop_sender)),
            }),
        };
        (stream, pump(body, sender, stop, describe))
    }

    /// The next event, waiting for it to arrive. The last event is always a
    /// [`SessionEvent::Closed`]; after it, `None`.
    pub async fn next(&self) -> Option<SessionEvent> {
        let mut receiving = self.shared.receiving.lock().await;
        if receiving.closed {
            return None;
        }
        let event = match receiving.events.next().await {
            Some(event) => event,
            // The pump ended without saying why: it was stopped, or it went away.
            None => SessionEvent::Closed {
                reason: if self.is_stopped() {
                    CloseReason::Client
                } else {
                    CloseReason::Lost
                },
            },
        };
        if matches!(event, SessionEvent::Closed { .. }) {
            receiving.closed = true;
        }
        Some(event)
    }

    /// Stop the stream: the pump drops the body, which closes the connection. Events
    /// already received are still returned by [`EventStream::next`], then a
    /// [`SessionEvent::Closed`] with [`CloseReason::Client`]. Stopping twice does
    /// nothing more.
    pub fn stop(&self) {
        drop(lock(&self.shared.stop).take());
    }

    /// Whether [`EventStream::stop`] was called.
    pub fn is_stopped(&self) -> bool {
        lock(&self.shared.stop).is_none()
    }
}

impl PartialEq for EventStream {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.shared, &other.shared)
    }
}

impl Eq for EventStream {}

// Events are server payloads, never logged: Debug says only whether it was stopped.
impl fmt::Debug for EventStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventStream")
            .field("stopped", &self.is_stopped())
            .finish_non_exhaustive()
    }
}

/// Read `body` into `sender` until it ends, fails or is stopped. Returning drops `body`,
/// which closes the connection.
async fn pump<S, B, E, D>(
    body: S,
    mut sender: mpsc::Sender<SessionEvent>,
    mut stop: oneshot::Receiver<()>,
    describe: D,
) where
    S: Stream<Item = Result<B, E>>,
    B: AsRef<[u8]>,
    D: Fn(E) -> String,
{
    let mut body = std::pin::pin!(body);
    let mut parser = EventSourceParser::new();
    let mut reported = Dropped::default();
    loop {
        let chunk = match future::select(&mut stop, body.next()).await {
            // Stopped, or every `EventStream` is gone: the stream itself reports the
            // close, since nobody may be left to receive it from here.
            Either::Left(_) => return,
            Either::Right((chunk, _)) => chunk,
        };
        let mut events: Vec<SessionEvent> = match chunk {
            Some(Ok(bytes)) => parser
                .feed(bytes.as_ref())
                .into_iter()
                .map(|event| SessionEvent::Received(event.into_message(now_millis())))
                .collect(),
            Some(Err(error)) => {
                parser.finish();
                vec![
                    SessionEvent::Error(SessionError {
                        message: describe(error),
                    }),
                    SessionEvent::Closed {
                        reason: CloseReason::Lost,
                    },
                ]
            }
            None => {
                // An event without its closing blank line is never dispatched.
                parser.finish();
                vec![SessionEvent::Closed {
                    reason: CloseReason::Server {
                        code: None,
                        reason: String::new(),
                    },
                }]
            }
        };
        let ended = matches!(events.last(), Some(SessionEvent::Closed { .. }));
        // The loss is reported before the close, so the user reads it in the log.
        let losses = reported.update(&parser);
        let insert_at = if ended {
            events.len().saturating_sub(1)
        } else {
            events.len()
        };
        events.splice(insert_at..insert_at, losses);
        for event in events {
            let delivered = match future::select(&mut stop, sender.send(event)).await {
                Either::Left(_) => false,
                Either::Right((sent, _)) => sent.is_ok(),
            };
            if !delivered {
                return;
            }
        }
        if ended {
            return;
        }
    }
}

/// What the parser's bounds have dropped so far, and what the user was told of.
#[derive(Default)]
struct Dropped {
    events: u64,
    fields: u64,
}

impl Dropped {
    /// The errors that tell the user about what was dropped since the last call.
    fn update(&mut self, parser: &EventSourceParser) -> Vec<SessionEvent> {
        let mut errors = Vec::new();
        let events = parser.oversized_events_dropped();
        if events > self.events {
            let new = events - self.events;
            errors.push(SessionEvent::Error(SessionError {
                message: format!(
                    "{new} {} larger than {} MiB {} dropped. Send smaller events to see them.",
                    plural(new, "event", "events"),
                    MAX_EVENT_BYTES / (1024 * 1024),
                    plural(new, "was", "were"),
                ),
            }));
            self.events = events;
        }
        let fields = parser.oversized_fields_ignored();
        if fields > self.fields {
            let new = fields - self.fields;
            errors.push(SessionEvent::Error(SessionError {
                message: format!(
                    "{new} oversized `id` or `event` {} ignored (limits: {} KiB for `id`, \
                     {} KiB for `event`).",
                    plural(new, "field was", "fields were"),
                    MAX_ID_BYTES / 1024,
                    MAX_FIELD_BYTES / 1024,
                ),
            }));
            self.fields = fields;
        }
        errors
    }
}

fn plural(count: u64, one: &'static str, many: &'static str) -> &'static str {
    if count == 1 { one } else { many }
}

/// Now, in milliseconds since the Unix epoch, as [`api_model::Message::at`] counts. A
/// clock before the epoch reads as the epoch.
fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or_default()
}

/// The stop signal's lock. A poisoned lock still holds usable state.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The event stream a send left open, held where a cancel can reach it: the
/// [`crate::HttpClient`] stops it when its owner cancels or sends again (REQ-HTTP-12).
#[derive(Clone, Default)]
pub(crate) struct OpenStream {
    state: Arc<Mutex<OpenStreamState>>,
}

#[derive(Default)]
struct OpenStreamState {
    stream: Option<EventStream>,
    /// Cancelled before the send handed its stream over: one arriving later is stopped
    /// on arrival, so a cancel racing the response never leaves a stream running.
    cancelled: bool,
}

impl OpenStream {
    /// Keep `body`'s stream, when it is one, for a later [`OpenStream::stop`].
    pub(crate) fn hold(&self, body: &ResponseBody) {
        let ResponseBody::EventStream(stream) = body else {
            return;
        };
        let mut state = lock(&self.state);
        if state.cancelled {
            stream.stop();
        } else {
            state.stream = Some(stream.clone());
        }
    }

    /// Stop the held stream, and any stream held from now on.
    pub(crate) fn stop(&self) {
        let mut state = lock(&self.state);
        state.cancelled = true;
        if let Some(stream) = state.stream.take() {
            stream.stop();
        }
    }
}

#[cfg(test)]
#[path = "event_stream_tests.rs"]
mod tests;
