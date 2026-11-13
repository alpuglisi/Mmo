//! Rate limiting and coalescing: a player gets at most a few pushes an
//! hour, however much happens.
//!
//! A notice opens a batch for its player that is released `window` later,
//! so a burst (an auction won and the mating it starts, a catastrophe and
//! the season ending the same day) goes out as one notification. A batch is
//! held longer when sending it would break the limits: `min_gap` since the
//! player's last push, and `per_hour` pushes in any rolling hour. Notices
//! arriving meanwhile join the held batch, which keeps the most important
//! few (`keep`) and counts the rest. Pure: the caller passes the time.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use evo_sim::PlayerId;

use super::Note;

const HOUR: Duration = Duration::from_secs(3600);

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// How long a batch collects notices after its first.
    pub window: Duration,
    /// Least time between two pushes to a player.
    pub min_gap: Duration,
    /// Most pushes to a player in any rolling hour (at least 1).
    pub per_hour: u32,
    /// Notices a batch keeps (the most important); the rest are counted.
    pub keep: usize,
}

/// What to send a player: the most important notices first.
#[derive(Clone, Debug, PartialEq)]
pub struct Batch {
    pub player: PlayerId,
    pub notes: Vec<Note>,
    /// Notices coalesced into it, kept or not.
    pub total: usize,
}

#[derive(Default)]
struct PlayerQueue {
    /// When the pushes of the last hour went out, oldest first.
    sent: VecDeque<Instant>,
    pending: Option<(Instant, Batch)>,
}

impl PlayerQueue {
    fn prune(&mut self, now: Instant) {
        while self.sent.front().is_some_and(|&t| now.saturating_duration_since(t) >= HOUR) {
            self.sent.pop_front();
        }
    }

    /// The earliest time the limits allow the next push.
    fn allowed_at(&self, limits: &Limits, now: Instant) -> Instant {
        let mut at = now;
        if let Some(&last) = self.sent.back() {
            at = at.max(last + limits.min_gap);
        }
        let cap = limits.per_hour.max(1) as usize;
        if self.sent.len() >= cap {
            at = at.max(self.sent[self.sent.len() - cap] + HOUR);
        }
        at
    }
}

pub struct Coalescer {
    limits: Limits,
    players: HashMap<PlayerId, PlayerQueue>,
}

impl Coalescer {
    pub fn new(limits: Limits) -> Self {
        Coalescer { limits, players: HashMap::new() }
    }

    /// Add a notice (one that the player's preferences let through).
    pub fn add(&mut self, note: Note, now: Instant) {
        let limits = self.limits;
        let q = self.players.entry(note.player).or_default();
        q.prune(now);
        match &mut q.pending {
            Some((_, batch)) => {
                batch.total += 1;
                batch.notes.push(note);
                // Most important first; the same kind in arrival order.
                batch.notes.sort_by_key(|n| n.kind);
                batch.notes.truncate(limits.keep.max(1));
            }
            None => {
                let due = (now + limits.window).max(q.allowed_at(&limits, now));
                q.pending = Some((due, Batch { player: note.player, notes: vec![note], total: 1 }));
            }
        }
    }

    /// When the next batch is due, if any is waiting.
    pub fn next_due(&self) -> Option<Instant> {
        self.players.values().filter_map(|q| q.pending.as_ref().map(|p| p.0)).min()
    }

    /// The batches due at `now`, each counted as a push to its player.
    pub fn take_due(&mut self, now: Instant) -> Vec<Batch> {
        let limits = self.limits;
        let mut out = Vec::new();
        for q in self.players.values_mut() {
            let Some((due, _)) = q.pending else { continue };
            if due > now {
                continue;
            }
            q.prune(now);
            let allowed = q.allowed_at(&limits, now);
            if allowed > now {
                q.pending.as_mut().expect("pending").0 = allowed;
                continue;
            }
            let (_, batch) = q.pending.take().expect("pending");
            q.sent.push_back(now);
            out.push(batch);
        }
        // Forget players with nothing waiting and no push in the last hour.
        self.players.retain(|_, q| {
            q.prune(now);
            q.pending.is_some() || !q.sent.is_empty()
        });
        out.sort_by_key(|b| b.player);
        out
    }

    /// Players tracked (with a batch waiting or a push in the last hour).
    pub fn tracked(&self) -> usize {
        self.players.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::PushKind;

    fn note(player: PlayerId, kind: PushKind, text: &str) -> Note {
        Note { player, world: "main".into(), world_name: "Main".into(), kind, text: text.into(), organism: None }
    }

    fn limits() -> Limits {
        Limits { window: Duration::from_secs(30), min_gap: Duration::from_secs(300), per_hour: 4, keep: 3 }
    }

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    #[test]
    fn a_burst_goes_out_as_one_batch_after_the_window() {
        let t0 = Instant::now();
        let mut c = Coalescer::new(limits());
        c.add(note(1, PushKind::Season, "season over"), t0);
        c.add(note(1, PushKind::Market, "outbid"), t0 + secs(5));
        c.add(note(2, PushKind::Catastrophe, "flood"), t0 + secs(10));
        c.add(note(1, PushKind::Mating, "offer"), t0 + secs(20));
        c.add(note(1, PushKind::Catastrophe, "flood"), t0 + secs(25));
        assert_eq!(c.next_due(), Some(t0 + secs(30)));
        assert!(c.take_due(t0 + secs(29)).is_empty());
        let out = c.take_due(t0 + secs(30));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].player, 1);
        assert_eq!(out[0].total, 4);
        // The most important three, by kind.
        let kinds: Vec<_> = out[0].notes.iter().map(|n| n.kind).collect();
        assert_eq!(kinds, vec![PushKind::Mating, PushKind::Market, PushKind::Season]);
        assert_eq!(c.next_due(), Some(t0 + secs(40)));
        let out = c.take_due(t0 + secs(40));
        assert_eq!((out[0].player, out[0].total), (2, 1));
        assert_eq!(c.next_due(), None);
    }

    #[test]
    fn pushes_keep_the_minimum_gap_and_the_hourly_cap() {
        let t0 = Instant::now();
        let mut c = Coalescer::new(limits());
        let mut sent = Vec::new();
        // A notice every 10 seconds for two hours.
        let mut t = t0;
        while t < t0 + secs(7200) {
            c.add(note(1, PushKind::Death, "x"), t);
            for b in c.take_due(t) {
                sent.push((t, b.total));
            }
            t += secs(10);
        }
        // Every notice is in some batch until the end.
        let pending: usize = 7200 / 10 - sent.iter().map(|s| s.1).sum::<usize>();
        assert!(pending > 0 && pending < 7200 / 10);
        for w in sent.windows(2) {
            assert!(w[1].0 - w[0].0 >= secs(300), "gap {:?}", w[1].0 - w[0].0);
        }
        for (i, &(at, _)) in sent.iter().enumerate() {
            let in_hour = sent[i..].iter().take_while(|s| s.0 < at + secs(3600)).count();
            assert!(in_hour <= 4, "{in_hour} pushes in the hour from {:?}", at - t0);
        }
        // The first goes out after the window; with the gap the four of an
        // hour come 5 minutes apart, then the cap holds the rest until the
        // first leaves the hour.
        let offsets: Vec<u64> = sent.iter().map(|s| (s.0 - t0).as_secs()).collect();
        assert_eq!(&offsets[..5], &[30, 330, 630, 930, 3630]);
    }

    #[test]
    fn a_quiet_player_is_forgotten_after_an_hour() {
        let t0 = Instant::now();
        let mut c = Coalescer::new(limits());
        c.add(note(1, PushKind::Death, "x"), t0);
        assert_eq!(c.take_due(t0 + secs(30)).len(), 1);
        assert_eq!(c.tracked(), 1);
        // A notice soon after waits for the gap.
        c.add(note(1, PushKind::Death, "y"), t0 + secs(60));
        assert_eq!(c.next_due(), Some(t0 + secs(330)));
        assert_eq!(c.take_due(t0 + secs(330)).len(), 1);
        assert!(c.take_due(t0 + secs(330) + HOUR).is_empty());
        assert_eq!(c.tracked(), 0);
    }
}
