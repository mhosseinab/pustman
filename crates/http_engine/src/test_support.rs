//! A scripted `Transport` for tests: responses, errors and delays, no network.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use futures::FutureExt;
use futures::future::BoxFuture;
use gpui::BackgroundExecutor;

use crate::error::TransportError;
use crate::request::ResolvedRequest;
use crate::response::{Response, ResponseBody};
use crate::transport::Transport;

/// One scripted outcome of a send, delivered after `delay`.
struct Scripted {
    delay: Duration,
    outcome: Result<Response, TransportError>,
}

/// What [`FakeTransport`] records across the sends it serves.
#[derive(Default)]
struct FakeState {
    script: VecDeque<Scripted>,
    requests: Vec<ResolvedRequest>,
    dropped: usize,
}

/// The state lock. A poisoned lock still holds usable state for a test.
fn lock(state: &Mutex<FakeState>) -> MutexGuard<'_, FakeState> {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Counts send futures dropped before they could complete: a cancelled or replaced send.
struct DropCounter {
    state: Arc<Mutex<FakeState>>,
    armed: bool,
}

impl DropCounter {
    fn new(state: Arc<Mutex<FakeState>>) -> Self {
        Self { state, armed: true }
    }

    /// The future ran to completion; dropping it is not a cancellation.
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for DropCounter {
    fn drop(&mut self) {
        if self.armed {
            lock(&self.state).dropped += 1;
        }
    }
}

/// A [`Transport`] that replays a script instead of touching the network.
///
/// Every `send` takes the next scripted outcome and records the request it received.
/// Delays run on GPUI's executor timer, so the test's clock drives them. Futures dropped
/// before completing — a send that was cancelled or replaced — are counted in
/// [`FakeTransport::dropped`].
#[derive(Clone)]
pub struct FakeTransport {
    executor: BackgroundExecutor,
    state: Arc<Mutex<FakeState>>,
}

impl FakeTransport {
    /// A transport with an empty script: every send fails, so a test that forgot to script
    /// an outcome fails loudly instead of hanging.
    pub fn new(executor: BackgroundExecutor) -> Self {
        Self {
            executor,
            state: Arc::new(Mutex::new(FakeState::default())),
        }
    }

    /// A transport whose first send answers `status` and `body`, without a delay.
    pub fn with_response(
        executor: BackgroundExecutor,
        status: u16,
        body: impl Into<Vec<u8>>,
    ) -> Self {
        let transport = Self::new(executor);
        transport.push_response(Duration::ZERO, status, body);
        transport
    }

    /// Script one response, delivered after `delay`.
    pub fn push_response(&self, delay: Duration, status: u16, body: impl Into<Vec<u8>>) -> &Self {
        let response = Response::new(status, "", ResponseBody::Bytes(body.into()));
        self.push_scripted(delay, Ok(response))
    }

    /// Script one failure, delivered after `delay`.
    pub fn push_error(&self, delay: Duration, error: TransportError) -> &Self {
        self.push_scripted(delay, Err(error))
    }

    /// The requests the transport received, in order.
    pub fn requests(&self) -> Vec<ResolvedRequest> {
        lock(&self.state).requests.clone()
    }

    /// How many send futures were dropped before completing: a cancelled or replaced send.
    pub fn dropped(&self) -> usize {
        lock(&self.state).dropped
    }

    fn push_scripted(&self, delay: Duration, outcome: Result<Response, TransportError>) -> &Self {
        lock(&self.state)
            .script
            .push_back(Scripted { delay, outcome });
        self
    }
}

impl Transport for FakeTransport {
    fn send(
        &self,
        request: ResolvedRequest,
    ) -> BoxFuture<'static, Result<Response, TransportError>> {
        let scripted = {
            let mut state = lock(&self.state);
            state.requests.push(request);
            state.script.pop_front()
        };
        let mut dropped = DropCounter::new(self.state.clone());
        let executor = self.executor.clone();
        async move {
            let outcome = match scripted {
                Some(scripted) => {
                    if !scripted.delay.is_zero() {
                        executor.timer(scripted.delay).await;
                    }
                    scripted.outcome
                }
                None => Err(TransportError::Io {
                    detail: "FakeTransport: no scripted outcome left for this send".to_string(),
                }),
            };
            dropped.disarm();
            outcome
        }
        .boxed()
    }
}
