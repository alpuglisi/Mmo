//! Offline protection, the game side: who is logged out and since when.
//!
//! The rules themselves are in the simulation (`evo_sim::world::offline`):
//! a protected lineage at or below its floor (12 living members) is hunted
//! with a quarter of the usual kill chance and takes half the starvation and
//! thirst damage. This module decides which lineages are protected:
//!
//! - A player's lineage is protected from the moment their last connection
//!   to this world closes (or the server restarts) until they connect again
//!   or [`GameConfig::offline_protection_ticks`] ticks have passed (12 hours
//!   at the default 1 s tick), whichever comes first. A spell's clock is
//!   saved, so a restart doesn't give a fresh one.
//! - Organisms in an open directed-mating agreement (either side, pending or
//!   accepted), listed on the marketplace, or offered as a bidder's organism
//!   on an auction are exempt: the trip and the trade carry risk by design.
//! - On login, a player whose lineage was actually shielded (it was small
//!   enough for protection to apply) gets an alert saying for how long.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use evo_sim::PlayerId;
use serde::{Deserialize, Serialize};

use super::{AlertKind, Game, OrgId, Outbox};

/// Default length of a protection spell in hours (`--offline-protection-hours`).
pub const DEFAULT_PROTECTION_HOURS: f64 = 12.0;
/// [`DEFAULT_PROTECTION_HOURS`] in ticks at 1 tick per second (the
/// `GameConfig` default; the server binary converts the hours at its clock).
pub const DEFAULT_PROTECTION_TICKS: u64 = 12 * 3600;

/// One logged-out stretch of a player's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spell {
    /// Tick the player went offline.
    pub start: u64,
    /// Ticks the protection actually applied (the lineage was at or below
    /// the floor).
    pub engaged: u64,
    /// The cap ran out; kept only to report on the next login.
    pub over: bool,
}

/// Offline players' protection spells. Saved with the game.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct OfflineTracker {
    spells: BTreeMap<PlayerId, Spell>,
    /// What the world was last given, to skip unchanged updates.
    #[serde(skip)]
    sent: Option<(BTreeSet<u32>, BTreeSet<OrgId>)>,
}

impl OfflineTracker {
    /// The spells, running or run out, by player.
    pub fn spells(&self) -> &BTreeMap<PlayerId, Spell> {
        &self.spells
    }
}

impl Game {
    /// `player`'s last connection closed: start their spell (an existing
    /// one, from before a restart, keeps its clock).
    pub(super) fn offline_logout(&mut self, player: PlayerId) {
        if self.lineage_of.contains_key(&player) && self.cfg.offline_protection_ticks > 0 {
            let start = self.world.tick();
            self.offline.spells.entry(player).or_insert(Spell { start, ..Spell::default() });
        }
        self.sync_offline_protection();
    }

    /// `player` connected while having no other connection: end their spell
    /// and say what it did.
    pub(super) fn offline_login(&mut self, player: PlayerId, out: &mut Outbox) {
        let Some(spell) = self.offline.spells.remove(&player) else { return };
        self.sync_offline_protection();
        if spell.engaged == 0 {
            return;
        }
        let days = spell.engaged.div_ceil(self.world.config.ticks_per_day.max(1) as u64);
        let text = format!(
            "While you were away, offline protection shielded your small lineage for {days} simulated day{} \
             (fewer kills by hunters, half the starvation and thirst damage).",
            if days == 1 { "" } else { "s" }
        );
        self.alert(player, AlertKind::Milestone, text, None, out);
    }

    /// Players with a lineage and no connection (all of them after a
    /// restart) get a spell if they had none.
    pub(super) fn offline_resume(&mut self, saved: Option<OfflineTracker>) {
        if let Some(t) = saved {
            self.offline.spells = t.spells;
        }
        let online: BTreeSet<PlayerId> = self.conns.values().map(|c| c.player).collect();
        let players: Vec<PlayerId> = self.lineage_of.keys().copied().filter(|p| !online.contains(p)).collect();
        for p in players {
            self.offline_logout(p);
        }
    }

    /// Once per tick: count ticks the protection applied, end spells that
    /// ran out, and give the world the protected lineages and the exempt
    /// organisms if they changed.
    pub(super) fn offline_after_tick(&mut self) {
        let (tick, cap) = (self.world.tick(), self.cfg.offline_protection_ticks);
        for (p, s) in self.offline.spells.iter_mut().filter(|(_, s)| !s.over) {
            if tick.saturating_sub(s.start) >= cap {
                s.over = true;
            } else if self.lineage_of.get(p).is_some_and(|&l| self.world.offline_protection().engaged(l)) {
                s.engaged += 1;
            }
        }
        self.sync_offline_protection();
    }

    /// Hand the world the lineages of running spells and the organisms in
    /// open agreements or on the market, when either changed.
    fn sync_offline_protection(&mut self) {
        let lineages: BTreeSet<u32> = (self.offline.spells.iter())
            .filter(|(_, s)| !s.over)
            .filter_map(|(p, _)| self.lineage_of.get(p).copied())
            .collect();
        let exempt: BTreeSet<OrgId> = if lineages.is_empty() {
            BTreeSet::new()
        } else {
            let offers = self.offers.values().filter(|o| o.status.is_open()).flat_map(|o| [o.mine, o.partner]);
            let listed = self.economy.listings.values().flat_map(|l| {
                std::iter::once(l.organism).chain(l.auction.as_ref().and_then(|a| a.high.as_ref()).map(|b| b.mine))
            });
            offers.chain(listed).collect()
        };
        let next = (lineages, exempt);
        if self.offline.sent.as_ref() == Some(&next) {
            return;
        }
        let (lineages, exempt) = next.clone();
        self.world.set_offline_protection(lineages, exempt.into_iter().collect::<HashSet<_>>());
        self.offline.sent = Some(next);
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use evo_sim::{BirthType, Thermo};

    /// Two players with six founders each (below the protection floor),
    /// both connected, on a small world with no wild ecosystem.
    fn game(cap: u64) -> (Game, Vec<OrgId>, Vec<OrgId>) {
        let world = build_world(&WorldSettings { seed: 7, width: 64, height: 64, wild: false, ..Default::default() });
        let mut g = Game::new(world, GameConfig { offline_protection_ticks: cap, ..GameConfig::default() });
        let design = |n: &str| FounderDesign::new(n, Thermo::Warm, BirthType::Live, 2).founders(6);
        let a = g.found(1, &design("A")).unwrap().founders;
        let b = g.found(2, &design("B")).unwrap().founders;
        g.connect(10, 1, "ann");
        g.connect(20, 2, "bob");
        (g, a, b)
    }

    fn tick(g: &mut Game) -> Outbox {
        g.world.step();
        g.after_tick()
    }

    fn protected(g: &Game) -> Vec<u32> {
        g.world.offline_protection().lineages().iter().copied().collect()
    }

    fn told(out: &Outbox) -> bool {
        out.iter().any(|(_, m)| matches!(m, ServerMsg::Alert { alert } if alert.text.contains("offline protection")))
    }

    #[test]
    fn protection_runs_from_logout_to_login() {
        let (mut g, _, _) = game(1000);
        let (la, lb) = (g.lineage_of(1).unwrap(), g.lineage_of(2).unwrap());
        assert!(protected(&g).is_empty(), "both online");
        // A second connection closing doesn't log Ann out.
        g.connect(11, 1, "ann");
        g.disconnect(11);
        assert!(protected(&g).is_empty());
        g.disconnect(10);
        assert_eq!(protected(&g), vec![la]);
        assert!(g.world.offline_protection().engaged(la), "six members, below the floor");
        assert!(!g.world.offline_protection().engaged(lb));
        for _ in 0..5 {
            tick(&mut g);
        }
        assert_eq!(g.offline.spells()[&1].engaged, 5);
        // Back online: protection ends and Ann hears it shielded her lineage.
        let out = g.connect(12, 1, "ann");
        assert!(protected(&g).is_empty());
        assert!(g.offline.spells().is_empty());
        assert!(told(&out));
    }

    #[test]
    fn protection_ends_after_the_cap() {
        let (mut g, _, _) = game(10);
        g.disconnect(20);
        assert_eq!(protected(&g), vec![g.lineage_of(2).unwrap()]);
        for _ in 0..10 {
            tick(&mut g);
        }
        assert!(protected(&g).is_empty(), "the cap ran out");
        assert!(g.offline.spells()[&2].over);
        // Still reported at the next login.
        assert!(told(&g.connect(21, 2, "bob")));
        // Turned off: no spell at all.
        let (mut g, _, _) = game(0);
        g.disconnect(20);
        assert!(protected(&g).is_empty());
        assert!(!told(&g.connect(21, 2, "bob")));
    }

    #[test]
    fn organisms_in_an_agreement_are_exempt() {
        let (mut g, a, b) = game(1000);
        let sex = |g: &Game, id: OrgId| g.world.organism(id).unwrap().sex;
        let fa = *a.iter().find(|&&i| sex(&g, i) == evo_sim::organism::Sex::Female).unwrap();
        let mb = *b.iter().find(|&&i| sex(&g, i) == evo_sim::organism::Sex::Male).unwrap();
        g.command(10, 1, None, ClientMsg::RequestMating { mine: fa, partner: mb, price: None });
        g.disconnect(10);
        g.disconnect(20);
        let p = g.world.offline_protection();
        assert!(p.is_exempt(fa) && p.is_exempt(mb));
        assert!(!p.is_exempt(a.iter().copied().find(|&i| i != fa).unwrap()));
    }

    #[test]
    fn a_restart_protects_everyone_and_keeps_running_clocks() {
        let (mut g, _, _) = game(1000);
        g.disconnect(10);
        for _ in 0..3 {
            tick(&mut g);
        }
        let started = g.offline.spells()[&1].start;
        let (saved, world) = (g.saved(), g.world.clone());
        let cfg = GameConfig { offline_protection_ticks: 1000, ..GameConfig::default() };
        let r = Game::resume(world, cfg, saved, Vec::new());
        assert_eq!(r.offline.spells()[&1].start, started, "Ann's spell keeps its clock");
        assert_eq!(r.offline.spells()[&2].start, r.world.tick(), "Bob was online: his spell starts now");
        assert_eq!(protected(&r).len(), 2);
    }
}
