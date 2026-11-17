//! Offline protection, the game side: who is logged out and since when.
//!
//! The rules themselves are in the simulation (`evo_sim::world::offline`).
//! Two forms run side by side:
//!
//! - **Surviving core**: a protected lineage at or below its floor (12
//!   living members) is hunted with a quarter of the usual kill chance and
//!   takes half the starvation and thirst damage.
//! - **Home range**: the territory one of the player's groups claimed
//!   before they logged off shields the group's members inside it: a strike
//!   that would kill one succeeds with half its usual chance, the softening
//!   shared out once more than 20 are inside. Predation only.
//!
//! This module decides who is protected, and when:
//!
//! - A player's spell runs from the moment their last connection to this
//!   world closes (or the server restarts) until they connect again. A
//!   spell's clock is saved, so a restart doesn't give a fresh one.
//! - The surviving core applies for the first
//!   [`GameConfig::offline_protection_ticks`] ticks of a spell (12 hours at
//!   the default 1 s tick).
//! - The home range is picked at logout: the territory held by the
//!   player's group with the most members (ties to the lower group id), so
//!   one per player, claimed before logging off. It applies from
//!   [`GameConfig::home_range_onset_ticks`] after logout (10 minutes: logging
//!   off can't be used to slip out of a fight), at full strength until
//!   [`GameConfig::home_range_ticks`] (12 hours), then fades linearly to
//!   nothing over [`GameConfig::home_range_fade_ticks`] (24 hours more), in
//!   steps of 5%. It ends early if the territory is abandoned or replaced.
//! - Organisms in an accepted directed-mating agreement (either side; a
//!   pending request carries no consent), listed on the marketplace, or offered as a bidder's organism
//!   on an auction are exempt from both: the trip and the trade carry risk
//!   by design.
//! - On login, a player whose lineage was actually shielded (it was small
//!   enough for the surviving core to apply, or its home range had residents)
//!   gets an alert saying for how long.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use evo_sim::world::groups::GroupInfo;
use evo_sim::world::offline::HomeRange;
use evo_sim::PlayerId;
use serde::{Deserialize, Serialize};

use super::{AlertKind, Game, GroupView, OfferStatus, OrgId, Outbox};

/// Default length of a protection spell in hours (`--offline-protection-hours`).
pub const DEFAULT_PROTECTION_HOURS: f64 = 12.0;
/// [`DEFAULT_PROTECTION_HOURS`] in ticks at 1 tick per second (the
/// `GameConfig` default; the server binary converts the hours at its clock).
pub const DEFAULT_PROTECTION_TICKS: u64 = 12 * 3600;
/// Default hours a home range shields at full strength (`--home-range-hours`;
/// it then fades over twice as long).
pub const DEFAULT_HOME_RANGE_HOURS: f64 = 12.0;
/// Minutes offline before the home range applies.
pub const HOME_RANGE_ONSET_MINUTES: f64 = 10.0;
/// The home range's timings in ticks at 1 tick per second.
pub const DEFAULT_HOME_RANGE_TICKS: u64 = 12 * 3600;
pub const DEFAULT_HOME_RANGE_ONSET_TICKS: u64 = 10 * 60;
pub const DEFAULT_HOME_RANGE_FADE_TICKS: u64 = 24 * 3600;
/// Steps in which a fading home range loses strength (5% each), so the world
/// is told of a change at most this many times in a fade.
const FADE_STEPS: f32 = 20.0;

/// The territory a spell shields: the group holding it and when it was
/// claimed (a later claim is another territory, not shielded).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HomeClaim {
    pub group: u8,
    pub claimed_tick: u64,
}

/// One logged-out stretch of a player's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spell {
    /// Tick the player went offline.
    pub start: u64,
    /// Ticks the protection actually applied (the lineage was at or below
    /// the floor).
    pub engaged: u64,
    /// The surviving core's cap ran out; kept only to report on the next
    /// login (the home range may still apply).
    pub over: bool,
    /// The home range picked at logout (absent: none was held, or saved
    /// before home ranges).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home: Option<HomeClaim>,
    /// Ticks the home range applied with residents inside it.
    #[serde(default)]
    pub home_engaged: u64,
    /// Protection already used in the rolling window when the spell began
    /// (`Game::protection_used`): the spell's clock starts this far in, so
    /// reconnecting can't renew the protection (absent in older saves: 0).
    #[serde(default)]
    pub credit: u64,
}

impl Spell {
    /// How far into its protection the spell is at `tick`: the ticks since
    /// logout plus the credit it started with.
    pub fn elapsed(&self, tick: u64) -> u64 {
        tick.saturating_sub(self.start).saturating_add(self.credit)
    }
}

/// Offline players' protection spells. Saved with the game.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct OfflineTracker {
    spells: BTreeMap<PlayerId, Spell>,
    /// Ended spells within the rolling window, by player: (logout, login)
    /// ticks, oldest first (`Game::protection_used`).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    past: BTreeMap<PlayerId, Vec<(u64, u64)>>,
    /// What the world was last given, to skip unchanged updates.
    #[serde(skip)]
    sent: Option<(BTreeSet<u32>, BTreeSet<OrgId>)>,
    #[serde(skip)]
    sent_ranges: Option<Vec<HomeRange>>,
    /// Logins and logouts update the world at the next
    /// [`Game::flush_offline`] rather than at once ([`Game::defer_offline`]).
    #[serde(skip)]
    defer: bool,
    /// A login or logout since the world was last updated (when deferred).
    #[serde(skip)]
    dirty: bool,
}

impl OfflineTracker {
    /// The spells, running or run out, by player.
    pub fn spells(&self) -> &BTreeMap<PlayerId, Spell> {
        &self.spells
    }
}

/// The group whose territory would be shielded: of the groups holding a
/// territory, the one with the most members (ties to the lower id).
pub fn home_group(groups: &[GroupInfo]) -> Option<u8> {
    groups
        .iter()
        .filter(|g| g.territory.is_some() && g.members > 0)
        .max_by_key(|g| (g.members, std::cmp::Reverse(g.id)))
        .map(|g| g.id)
}

impl Game {
    /// A player's groups as sent in a state, the home range marked.
    pub(super) fn group_views(&self, groups: &[GroupInfo]) -> Vec<GroupView> {
        let home = if self.home_ranges_on() { home_group(groups) } else { None };
        (groups.iter()).map(|g| GroupView { home_range: home == Some(g.id), ..GroupView::from(g) }).collect()
    }

    /// [`Game::group_views`] for `player`, the home range carrying the
    /// protection the player has left in minutes.
    pub(super) fn group_views_for(&self, player: PlayerId, groups: &[GroupInfo]) -> Vec<GroupView> {
        let mut views = self.group_views(groups);
        let left = self.protection_left(player, self.world.tick());
        let minutes = (left as f64 * self.cfg.tick_ms / 60_000.0).floor() as u32;
        for v in views.iter_mut().filter(|v| v.home_range) {
            v.protection_left_min = Some(minutes);
        }
        views
    }

    /// Whether home ranges are on (`GameConfig::home_range_ticks`).
    pub(super) fn home_ranges_on(&self) -> bool {
        self.cfg.home_range_ticks > 0
    }

    /// The home range `player` would have if they logged off now.
    fn home_claim(&self, player: PlayerId) -> Option<HomeClaim> {
        if !self.home_ranges_on() {
            return None;
        }
        let group = home_group(&self.world.groups(player))?;
        let t = self.world.territory(player, group)?;
        Some(HomeClaim { group, claimed_tick: t.claimed_tick })
    }

    /// Strength of `spell`'s home range at `tick`: 0 before the onset, 1
    /// until `home_range_ticks`, then fading to 0 (in 5% steps).
    fn home_strength(&self, spell: &Spell, tick: u64) -> f32 {
        let c = &self.cfg;
        if spell.home.is_none() || c.home_range_ticks == 0 {
            return 0.0;
        }
        let t = spell.elapsed(tick);
        if t < c.home_range_onset_ticks {
            0.0
        } else if t < c.home_range_ticks {
            1.0
        } else if c.home_range_fade_ticks == 0 {
            0.0
        } else {
            let left = 1.0 - (t - c.home_range_ticks) as f64 / c.home_range_fade_ticks as f64;
            ((left.max(0.0) as f32) * FADE_STEPS).ceil() / FADE_STEPS
        }
    }

    /// `player`'s last connection closed: start their spell (an existing
    /// one, from before a restart, keeps its clock and home range).
    pub(super) fn offline_logout(&mut self, player: PlayerId) {
        if self.lineage_of.contains_key(&player) && (self.cfg.offline_protection_ticks > 0 || self.home_ranges_on()) {
            let start = self.world.tick();
            let home = self.home_claim(player);
            let credit = self.protection_used(player, start);
            self.offline.spells.entry(player).or_insert(Spell { start, home, credit, ..Spell::default() });
        }
        self.offline_changed();
    }

    /// The spells changed: tell the world now, or at the next flush.
    fn offline_changed(&mut self) {
        if self.offline.defer {
            self.offline.dirty = true;
        } else {
            self.sync_offline_protection();
        }
    }

    /// Update the world with logins and logouts from now on only at
    /// [`Game::flush_offline`]: the runner flushes before each world step
    /// (and save), the only time the world reads the protection. A burst of
    /// reconnections (a restart) then costs one update instead of one per
    /// player, each a pass over every spell, listing and open offer.
    pub fn defer_offline(&mut self) {
        self.offline.defer = true;
    }

    /// Give the world the protection logins and logouts changed, if any.
    pub fn flush_offline(&mut self) {
        if std::mem::take(&mut self.offline.dirty) {
            self.sync_offline_protection();
        }
    }

    /// The protection budget: at most this many ticks of protection in any
    /// [`Game::protection_window`] (the surviving core's cap, else the home
    /// range's full-strength span; 12 hours by default).
    pub(super) fn protection_budget(&self) -> u64 {
        if self.cfg.offline_protection_ticks > 0 {
            self.cfg.offline_protection_ticks
        } else {
            self.cfg.home_range_ticks
        }
    }

    /// The rolling window the budget counts over: twice the budget (24
    /// hours by default).
    pub(super) fn protection_window(&self) -> u64 {
        2 * self.protection_budget()
    }

    /// Ticks `player` spent in protection spells (ended ones and the running
    /// one) within the window before `tick`, up to the budget.
    pub(super) fn protection_used(&self, player: PlayerId, tick: u64) -> u64 {
        let from = tick.saturating_sub(self.protection_window());
        let overlap = |a: u64, b: u64| b.min(tick).saturating_sub(a.max(from));
        let past: u64 = self.offline.past.get(&player).map_or(0, |v| v.iter().map(|&(a, b)| overlap(a, b)).sum());
        let running = self.offline.spells.get(&player).map_or(0, |s| overlap(s.start, tick));
        (past + running).min(self.protection_budget())
    }

    /// Protection `player` would still have if they logged off at `tick`.
    pub(super) fn protection_left(&self, player: PlayerId, tick: u64) -> u64 {
        self.protection_budget() - self.protection_used(player, tick)
    }

    /// `player` connected while having no other connection: end their spell
    /// and say what it did.
    pub(super) fn offline_login(&mut self, player: PlayerId, out: &mut Outbox) {
        let Some(spell) = self.offline.spells.remove(&player) else { return };
        // Keep the spell's span for the rolling budget, dropping spans that
        // have left the window.
        let (tick, window) = (self.world.tick(), self.protection_window());
        let past = self.offline.past.entry(player).or_default();
        past.push((spell.start, tick));
        past.retain(|&(_, end)| end + window > tick);
        self.offline_changed();
        let tpd = self.world.config.ticks_per_day.max(1) as u64;
        let days = |ticks: u64| {
            let d = ticks.div_ceil(tpd);
            format!("{d} simulated day{}", if d == 1 { "" } else { "s" })
        };
        let mut parts = Vec::new();
        if spell.engaged > 0 {
            parts.push(format!(
                "offline protection shielded your small lineage for {} \
                 (fewer kills by hunters, half the starvation and thirst damage)",
                days(spell.engaged)
            ));
        }
        if let (Some(home), true) = (spell.home, spell.home_engaged > 0) {
            parts.push(format!(
                "your home range (group {}'s territory) kept hunters off the members inside it for {}",
                (b'A' + home.group) as char,
                days(spell.home_engaged)
            ));
        }
        if parts.is_empty() {
            return;
        }
        let text = format!("While you were away, {}.", parts.join("; and "));
        self.alert(player, AlertKind::Milestone, text, None, out);
    }

    /// Players with a lineage and no connection (all of them after a
    /// restart) get a spell if they had none.
    pub(super) fn offline_resume(&mut self, saved: Option<OfflineTracker>) {
        if let Some(t) = saved {
            self.offline.spells = t.spells;
            self.offline.past = t.past;
        }
        let online: BTreeSet<PlayerId> = self.conns.values().map(|c| c.player).collect();
        let players: Vec<PlayerId> = self.lineage_of.keys().copied().filter(|p| !online.contains(p)).collect();
        for p in players {
            self.offline_logout(p);
        }
    }

    /// Once per tick: count ticks the protection applied, end spells that
    /// ran out, and give the world the protected lineages, the exempt
    /// organisms and the home ranges if they changed.
    pub(super) fn offline_after_tick(&mut self) {
        let (tick, cap) = (self.world.tick(), self.cfg.offline_protection_ticks);
        let p = self.world.offline_protection();
        for (player, s) in self.offline.spells.iter_mut() {
            if !s.over {
                if s.elapsed(tick) >= cap {
                    s.over = true;
                } else if self.lineage_of.get(player).is_some_and(|&l| p.engaged(l)) {
                    s.engaged += 1;
                }
            }
            if s.home.is_some() && p.shielded(*player) > 0 {
                s.home_engaged += 1;
            }
        }
        self.offline.dirty = false;
        self.sync_offline_protection();
    }

    /// The home ranges of running spells: those past the onset and not yet
    /// faded, whose territory still stands as it was claimed.
    fn home_ranges(&self) -> Vec<HomeRange> {
        let tick = self.world.tick();
        (self.offline.spells.iter())
            .filter_map(|(&owner, s)| {
                let home = s.home?;
                let strength = self.home_strength(s, tick);
                let t = self.world.territory(owner, home.group).filter(|t| t.claimed_tick == home.claimed_tick)?;
                let &lineage = self.lineage_of.get(&owner)?;
                (strength > 0.0).then_some(HomeRange {
                    owner,
                    lineage,
                    group: home.group,
                    center: t.center,
                    radius: t.radius,
                    strength,
                })
            })
            .collect()
    }

    /// Hand the world the lineages of running spells, the organisms in open
    /// agreements or on the market, and the home ranges, when any changed.
    fn sync_offline_protection(&mut self) {
        let lineages: BTreeSet<u32> = if self.cfg.offline_protection_ticks == 0 {
            BTreeSet::new()
        } else {
            (self.offline.spells.iter())
                .filter(|(_, s)| !s.over)
                .filter_map(|(p, _)| self.lineage_of.get(p).copied())
                .collect()
        };
        let ranges = self.home_ranges();
        let exempt: BTreeSet<OrgId> = if lineages.is_empty() && ranges.is_empty() {
            BTreeSet::new()
        } else {
            // Only an accepted agreement: a pending request carries no
            // consent from the partner's owner, and exempting on it let a
            // rival strip a logged-out player's protection by asking.
            let offers =
                self.offers.values().filter(|o| o.status == OfferStatus::Accepted).flat_map(|o| [o.mine, o.partner]);
            let listed = self.economy.listings.values().flat_map(|l| {
                std::iter::once(l.organism).chain(l.auction.as_ref().and_then(|a| a.high.as_ref()).map(|b| b.mine))
            });
            offers.chain(listed).collect()
        };
        let next = (lineages, exempt);
        if self.offline.sent.as_ref() != Some(&next) {
            let (lineages, exempt) = next.clone();
            self.world.set_offline_protection(lineages, exempt.into_iter().collect::<HashSet<_>>());
            self.offline.sent = Some(next);
        }
        if self.offline.sent_ranges.as_ref() != Some(&ranges) {
            self.world.set_home_ranges(ranges.clone());
            self.offline.sent_ranges = Some(ranges);
        }
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

    /// Deferred updates (`Game::defer_offline`, as the runner runs games)
    /// leave the world as immediate ones: players log in and out at random
    /// between ticks on a world with hunters, the deferred game flushing
    /// before each step, and both stay the same organism for organism.
    #[test]
    fn deferred_protection_updates_change_nothing() {
        use rand::{Rng, SeedableRng};
        let make = || {
            let world =
                build_world(&WorldSettings { seed: 3, width: 96, height: 96, wild: true, ..Default::default() });
            let mut g = Game::new(world, GameConfig { offline_protection_ticks: 400, ..GameConfig::default() });
            for p in 1..=6u32 {
                let d = FounderDesign::new(format!("L{p}"), Thermo::Warm, BirthType::Live, 2).founders(4);
                g.found(p, &d).unwrap();
            }
            g
        };
        let (mut now, mut later) = (make(), make());
        later.defer_offline();
        let mut rng = rand::rngs::StdRng::seed_from_u64(9);
        let mut online = [false; 7];
        for t in 0..300 {
            for _ in 0..rng.gen_range(0..4) {
                let p = rng.gen_range(1..=6u32);
                let conn = p as u64 * 10;
                if online[p as usize] {
                    now.disconnect(conn);
                    later.disconnect(conn);
                } else {
                    let (a, b) = (now.connect(conn, p, "p"), later.connect(conn, p, "p"));
                    assert_eq!(format!("{a:?}"), format!("{b:?}"), "tick {t}");
                }
                online[p as usize] = !online[p as usize];
            }
            later.flush_offline();
            assert_eq!(protected(&now), protected(&later), "tick {t}");
            let (a, b) = (tick(&mut now), tick(&mut later));
            assert_eq!(format!("{a:?}"), format!("{b:?}"), "tick {t}");
            let state = |g: &Game| {
                g.world.organisms.iter().map(|o| (o.id, o.alive, o.pos.x, o.pos.y, o.health)).collect::<Vec<_>>()
            };
            assert_eq!(state(&now), state(&later), "tick {t}");
        }
        assert!(now.offline.spells().values().any(|s| s.engaged > 0), "protection did something");
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

    /// The protection is a rolling budget: at most the cap's worth of
    /// protection in any window of twice the cap, however often the player
    /// reconnects (before, every logout started a fresh spell, so a script
    /// reconnecting just before the cap kept a lineage protected for good).
    #[test]
    fn reconnecting_does_not_renew_protection() {
        let (mut g, _, _) = game(10);
        let lb = g.lineage_of(2).unwrap();
        assert_eq!(g.protection_left(2, g.world.tick()), 10);
        g.disconnect(20);
        for _ in 0..6 {
            tick(&mut g);
        }
        // A quick reconnect: the next spell starts six ticks in.
        g.connect(21, 2, "bob");
        assert_eq!(g.protection_left(2, g.world.tick()), 4);
        g.disconnect(21);
        assert_eq!(g.offline.spells()[&2].credit, 6);
        for _ in 0..3 {
            tick(&mut g);
        }
        assert_eq!(protected(&g), vec![lb], "budget left");
        tick(&mut g);
        assert!(protected(&g).is_empty(), "ten ticks of protection in the window, however split");
        // Online again, the budget comes back as the window rolls on.
        g.connect(22, 2, "bob");
        for _ in 0..20 {
            tick(&mut g);
        }
        assert_eq!(g.protection_left(2, g.world.tick()), 10);
        g.disconnect(22);
        assert_eq!(protected(&g), vec![lb]);
    }

    /// The budget's history survives a restart.
    #[test]
    fn the_protection_budget_survives_a_restart() {
        let (mut g, _, _) = game(10);
        g.disconnect(20);
        for _ in 0..6 {
            tick(&mut g);
        }
        g.connect(21, 2, "bob");
        let (saved, world) = (g.saved(), g.world.clone());
        let json = serde_json::to_string(&saved).unwrap();
        let saved: SavedGame = serde_json::from_str(&json).unwrap();
        let mut r = Game::resume(
            world,
            GameConfig { offline_protection_ticks: 10, ..GameConfig::default() },
            saved,
            Vec::new(),
        );
        r.connect(22, 2, "bob");
        assert_eq!(r.protection_left(2, r.world.tick()), 4);
    }

    /// The home range shows the protection left, in minutes.
    #[test]
    fn the_home_range_shows_the_protection_left() {
        let (mut g, a, _) = game(600);
        g.cfg.tick_ms = 60_000.0;
        let center = g.world.organism(a[0]).unwrap().pos;
        g.world.claim_territory(1, 0, center, 6.0).unwrap();
        let home = |g: &Game| {
            let views = g.group_views_for(1, &g.world.groups(1));
            views.iter().find(|v| v.home_range).and_then(|v| v.protection_left_min)
        };
        assert_eq!(home(&g), Some(600));
        g.disconnect(10);
        for _ in 0..90 {
            tick(&mut g);
        }
        g.connect(11, 1, "ann");
        assert_eq!(home(&g), Some(510));
        // Groups without a home range carry nothing.
        let views = g.group_views_for(2, &g.world.groups(2));
        assert!(views.iter().all(|v| v.protection_left_min.is_none()));
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
        // A pending request strips nobody's protection: Bob never consented
        // (and a rival could otherwise target a logged-out player's last
        // animals just by asking).
        let p = g.world.offline_protection();
        assert!(!p.is_exempt(fa) && !p.is_exempt(mb), "a pending request exempted its organisms");
        // Accepted, the trip carries risk for both.
        let id = *g.offers.keys().next().unwrap();
        g.connect(21, 2, "bob");
        g.command(21, 2, None, ClientMsg::RespondMating { request: id, accept: true });
        g.disconnect(21);
        assert_eq!(g.offers[&id].status, OfferStatus::Accepted, "apart, the agreement waits");
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

    /// Ann's game with home ranges from 5 ticks after logout, full until 20,
    /// faded out at 40, and the surviving core off (so only the home range
    /// acts); her main group holds a territory around its founders.
    fn home_game() -> (Game, u32) {
        let (mut g, a, _) = game(0);
        g.cfg.home_range_onset_ticks = 5;
        g.cfg.home_range_ticks = 20;
        g.cfg.home_range_fade_ticks = 20;
        let center = g.world.organism(a[0]).unwrap().pos;
        g.world.claim_territory(1, 0, center, 6.0).unwrap();
        let l = g.lineage_of(1).unwrap();
        (g, l)
    }

    fn ranges(g: &Game) -> Vec<evo_sim::world::offline::HomeRange> {
        g.world.offline_protection().home_ranges().to_vec()
    }

    fn told_home(out: &Outbox) -> bool {
        out.iter().any(|(_, m)| matches!(m, ServerMsg::Alert { alert } if alert.text.contains("home range")))
    }

    #[test]
    fn the_home_range_applies_after_the_onset_and_fades() {
        let (mut g, l) = home_game();
        let t = g.world.territory(1, 0).unwrap();
        // Online, the territory is marked as the home range on the group card.
        let s = g.state_for(10).unwrap();
        assert!(s.groups.iter().any(|x| x.id == 0 && x.home_range));
        g.disconnect(10);
        let spell = g.offline.spells()[&1];
        assert_eq!(spell.home, Some(HomeClaim { group: 0, claimed_tick: t.claimed_tick }));
        assert!(protected(&g).is_empty(), "the surviving core is off");
        assert!(ranges(&g).is_empty(), "not before the onset");
        for _ in 0..5 {
            tick(&mut g);
        }
        let r = ranges(&g);
        assert_eq!(r.len(), 1);
        assert_eq!((r[0].owner, r[0].lineage, r[0].group, r[0].strength), (1, l, 0, 1.0));
        assert_eq!((r[0].center, r[0].radius), (t.center, t.radius));
        assert!(g.world.offline_protection().shielded(1) > 0, "founders inside their territory");
        for _ in 0..25 {
            tick(&mut g);
        }
        // Ten ticks into a twenty-tick fade.
        assert_eq!(ranges(&g)[0].strength, 0.5);
        for _ in 0..10 {
            tick(&mut g);
        }
        assert!(ranges(&g).is_empty(), "faded out");
        let engaged = g.offline.spells()[&1].home_engaged;
        assert!((30..=35).contains(&engaged), "{engaged} ticks shielded");
        // Back online: Ann hears what it did.
        let out = g.connect(12, 1, "ann");
        assert!(told_home(&out));
        assert!(ranges(&g).is_empty());
    }

    #[test]
    fn only_a_territory_held_at_logout_is_shielded() {
        // No territory at logout: none later.
        let (mut g, a, _) = game(0);
        g.cfg.home_range_onset_ticks = 0;
        g.disconnect(10);
        assert_eq!(g.offline.spells()[&1].home, None);
        let center = g.world.organism(a[0]).unwrap().pos;
        g.world.claim_territory(1, 0, center, 6.0).unwrap();
        tick(&mut g);
        assert!(ranges(&g).is_empty(), "claimed after logout");
        // Abandoned (or replaced) after logout: the shield ends.
        let (mut g, _) = home_game();
        g.cfg.home_range_onset_ticks = 0;
        g.disconnect(10);
        tick(&mut g);
        assert_eq!(ranges(&g).len(), 1);
        g.world.abandon_territory(1, 0).unwrap();
        tick(&mut g);
        assert!(ranges(&g).is_empty(), "abandoned");
        assert_eq!(g.world.offline_protection().shielded(1), 0);
    }

    #[test]
    fn the_largest_group_holding_a_territory_is_the_home_range() {
        let info = |id: u8, members: usize, held: bool| evo_sim::world::groups::GroupInfo {
            id,
            members,
            adults: members,
            migration: None,
            territory: held.then_some(evo_sim::world::groups::Territory {
                center: Vec2::ZERO,
                radius: 4.0,
                claimed_tick: 0,
            }),
            rally: None,
            max_radius: 8.0,
            claim_ready_tick: 0,
            obedience: 0.5,
        };
        assert_eq!(home_group(&[info(0, 9, false), info(1, 3, true), info(2, 5, true)]), Some(2));
        assert_eq!(home_group(&[info(0, 5, true), info(1, 5, true)]), Some(0), "ties to the lower id");
        assert_eq!(home_group(&[info(0, 5, false)]), None);
        assert_eq!(home_group(&[info(0, 0, true)]), None, "no members");
    }

    #[test]
    fn home_ranges_turn_off_and_survive_a_restart() {
        // Off: no mark on the card, no claim in the spell.
        let (mut g, _) = home_game();
        g.cfg.home_range_ticks = 0;
        assert!(g.state_for(10).unwrap().groups.iter().all(|x| !x.home_range));
        g.disconnect(10);
        assert!(!g.offline.spells().contains_key(&1), "both forms off: no spell");
        // A restart keeps the claim and the clock.
        let (mut g, _) = home_game();
        g.disconnect(10);
        for _ in 0..6 {
            tick(&mut g);
        }
        let home = g.offline.spells()[&1].home;
        assert!(home.is_some());
        let (saved, world, cfg) = (g.saved(), g.world.clone(), g.cfg.clone());
        let r = Game::resume(world, cfg, saved, Vec::new());
        assert_eq!(r.offline.spells()[&1].home, home);
        assert_eq!(ranges(&r).len(), 1, "shielding again at once after the restart");
    }
}
