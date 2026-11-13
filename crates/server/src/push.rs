//! A connection's outgoing queue, with `state` messages coalesced.
//!
//! The world thread pushes to each connection without waiting: replies and
//! events (mating updates, alerts) must all arrive, in order, but a `state`
//! is a whole snapshot that the next one replaces. Possession sub-steps send
//! the possessing player a state several times a tick (in fast mode, 150 a
//! second), so a client that reads a little slower than that would fill a
//! plain queue with stale states and then lose the connection on the next
//! event that no longer fits.
//!
//! So a state never takes more than one queue slot: it is kept in a
//! per-connection slot, and the queue holds only a marker where it was
//! first pushed. A newer state replaces the one in the slot without queueing
//! another marker; the writer sends whatever the slot holds when it reaches
//! the marker. Order is kept (a state goes out no earlier than its first
//! push, and never older than what was pushed before the events after it),
//! and a slow reader gets only the latest state while every event still
//! arrives. A queue full of events is a client too far behind: the push
//! fails and the world thread drops the connection.

use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;

use crate::protocol::ServerMsg;

// (The same size as the `ServerMsg` a plain queue would hold.)
#[allow(clippy::large_enum_variant)]
enum Item {
    Msg(ServerMsg),
    /// Send the slot's latest state here.
    State,
}

type Slot = Arc<Mutex<Option<ServerMsg>>>;

/// The world thread's end of a connection.
#[derive(Clone)]
pub struct PushSender {
    queue: mpsc::Sender<Item>,
    state: Slot,
}

/// The socket writer's end of a connection.
pub struct PushReceiver {
    queue: mpsc::Receiver<Item>,
    state: Slot,
}

/// A handle that doesn't keep the connection open (for the socket reader's
/// own replies, e.g. a rate-limit error).
pub struct WeakPushSender {
    queue: mpsc::WeakSender<Item>,
}

/// What a successful push did.
#[derive(Debug, PartialEq, Eq)]
pub enum Pushed {
    Queued,
    /// The message was a state and replaced one not sent yet.
    Replaced,
}

/// Why a push failed.
#[derive(Debug, PartialEq, Eq)]
pub enum PushError {
    /// The queue is full of replies and events: the client is too far behind.
    Full,
    /// The connection is gone.
    Closed,
}

/// A connection queue holding up to `cap` replies and events (plus states,
/// coalesced into one slot).
pub fn channel(cap: usize) -> (PushSender, PushReceiver) {
    let (tx, rx) = mpsc::channel(cap);
    let state: Slot = Arc::default();
    (PushSender { queue: tx, state: state.clone() }, PushReceiver { queue: rx, state })
}

fn err<T>(e: TrySendError<T>) -> PushError {
    match e {
        TrySendError::Full(_) => PushError::Full,
        TrySendError::Closed(_) => PushError::Closed,
    }
}

impl PushSender {
    /// Queue a message without waiting. A `state` replaces one not sent yet.
    pub fn try_send(&self, msg: ServerMsg) -> Result<Pushed, PushError> {
        if !matches!(msg, ServerMsg::State(_)) {
            return self.queue.try_send(Item::Msg(msg)).map(|_| Pushed::Queued).map_err(err);
        }
        if self.queue.is_closed() {
            return Err(PushError::Closed);
        }
        let mut slot = self.state.lock().unwrap();
        // A marker is already queued for the state in the slot.
        if slot.replace(msg).is_some() {
            return Ok(Pushed::Replaced);
        }
        self.queue.try_send(Item::State).map(|_| Pushed::Queued).map_err(|e| {
            // No marker for it: don't leave it in the slot (the next state
            // would then wait for a marker that never comes).
            *slot = None;
            err(e)
        })
    }

    pub fn downgrade(&self) -> WeakPushSender {
        WeakPushSender { queue: self.queue.downgrade() }
    }
}

impl WeakPushSender {
    /// Queue a message if the connection is still open and has room
    /// (dropped otherwise).
    pub fn try_send(&self, msg: ServerMsg) {
        if let Some(q) = self.queue.upgrade() {
            let _ = q.try_send(Item::Msg(msg));
        }
    }
}

impl PushReceiver {
    /// The next message to write, or `None` once every [`PushSender`] is
    /// dropped and the queue is drained.
    pub async fn recv(&mut self) -> Option<ServerMsg> {
        loop {
            match self.queue.recv().await? {
                Item::Msg(m) => return Some(m),
                Item::State => {
                    if let Some(m) = self.state.lock().unwrap().take() {
                        return Some(m);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{ClockView, ErrorInfo, StateView, ViewWindow, WeatherView};

    fn state(tick: u64) -> ServerMsg {
        let clock =
            ClockView { tick, day: 0, year: 0, hour: 0, season: "spring".into(), daytime: true, era: 0, era_days: 1 };
        let weather = WeatherView { mean_wind: 0.0, storm: false, drought: 0.0, flood: 0.0, catastrophe: None };
        ServerMsg::State(Box::new(StateView {
            clock,
            weather,
            player: 1,
            lineage: None,
            slots_left: 0,
            population: 0,
            migration: None,
            groups: Vec::new(),
            view: ViewWindow { x: 0.0, y: 0.0, radius: 1.0 },
            organisms: Vec::new(),
            nearby: Vec::new(),
            leaderboard: Default::default(),
            offers: Vec::new(),
            balance: 0,
            escrow: 0,
        }))
    }

    fn event(n: u64) -> ServerMsg {
        ServerMsg::err(Some(n), "x", ErrorInfo::new("e", "e"))
    }

    fn tick_of(m: &ServerMsg) -> Option<u64> {
        match m {
            ServerMsg::State(s) => Some(s.clock.tick),
            _ => None,
        }
    }

    fn id_of(m: &ServerMsg) -> Option<u64> {
        match m {
            ServerMsg::Result { id, .. } => *id,
            _ => None,
        }
    }

    #[tokio::test]
    async fn states_coalesce_in_place_and_events_all_arrive_in_order() {
        let (tx, mut rx) = channel(4);
        // Far more states than the queue holds, between events.
        tx.try_send(event(1)).unwrap();
        assert_eq!(tx.try_send(state(0)), Ok(Pushed::Queued));
        for t in 1..100 {
            assert_eq!(tx.try_send(state(t)), Ok(Pushed::Replaced));
        }
        tx.try_send(event(2)).unwrap();
        for t in 100..200 {
            tx.try_send(state(t)).unwrap();
        }
        tx.try_send(event(3)).unwrap();
        // Event 1, then the latest state (pushed before event 2 first, so it
        // goes there), event 2 and event 3; nothing else.
        assert_eq!(id_of(&rx.recv().await.unwrap()), Some(1));
        assert_eq!(tick_of(&rx.recv().await.unwrap()), Some(199));
        assert_eq!(id_of(&rx.recv().await.unwrap()), Some(2));
        assert_eq!(id_of(&rx.recv().await.unwrap()), Some(3));
        // A state pushed after the slot was emptied gets a marker again.
        tx.try_send(state(200)).unwrap();
        assert_eq!(tick_of(&rx.recv().await.unwrap()), Some(200));
        drop(tx);
        assert!(rx.recv().await.is_none());
    }

    #[tokio::test]
    async fn a_queue_full_of_events_refuses_and_a_closed_one_says_so() {
        let (tx, mut rx) = channel(2);
        tx.try_send(event(1)).unwrap();
        tx.try_send(event(2)).unwrap();
        assert_eq!(tx.try_send(event(3)), Err(PushError::Full));
        // A state that found no room for its marker isn't kept.
        assert_eq!(tx.try_send(state(1)), Err(PushError::Full));
        assert_eq!(id_of(&rx.recv().await.unwrap()), Some(1));
        tx.try_send(state(2)).unwrap();
        assert_eq!(id_of(&rx.recv().await.unwrap()), Some(2));
        assert_eq!(tick_of(&rx.recv().await.unwrap()), Some(2));
        // The weak handle doesn't keep the connection open.
        let weak = tx.downgrade();
        weak.try_send(event(4));
        assert_eq!(id_of(&rx.recv().await.unwrap()), Some(4));
        drop(tx);
        weak.try_send(event(5));
        assert!(rx.recv().await.is_none());
        let (tx, rx) = channel(2);
        drop(rx);
        assert_eq!(tx.try_send(state(1)), Err(PushError::Closed));
        assert_eq!(tx.try_send(event(1)), Err(PushError::Closed));
    }
}
