//! What the economy reads and does through the world: where each player's
//! scored alleles live (by host lineage), and reinforcements (kin joining a
//! small lineage, which the server sells for renown).

use std::collections::BTreeMap;

use rand::Rng;
use rayon::prelude::*;

use super::World;
use crate::genome::PlayerId;
use crate::math::Vec2;
use crate::organism::{OrgId, Organism, Sex};
use crate::rng::{rng_for, stream};

/// How close a rendezvous brings the pair (inside the server's meeting
/// distance, 1.5 tiles, so the agreement runs).
pub const RENDEZVOUS_NEAR: f32 = 1.0;
/// Within this of the meeting point an animal counts as there.
pub const MEET_AT: f32 = 2.0;
/// How far from the midpoint a dry meeting point is searched for.
pub const MEET_SEARCH: i32 = 12;

/// Alleles in breeding adults by (host lineage, origin player), counted as
/// the legacy score counts them (`LegacySnapshot`: the same adults, loci and
/// morphology alleles; cohort members of abstract regions weighted alike).
/// Summed over hosts, an origin's alleles are its legacy tally.
pub type HostTally = BTreeMap<(u32, PlayerId), f64>;

impl World {
    /// [`HostTally`] of the world now: whose alleles every lineage's
    /// breeding adults carry. Whole numbers (exactly, for agents), added in
    /// a fixed order, so it is the same at any thread count.
    pub fn legacy_hosts(&self) -> HostTally {
        let morph = self.config.balance.morph_in_legacy();
        type Tally = BTreeMap<(u32, PlayerId), u64>;
        let tally: Tally = self
            .organisms
            .par_iter()
            .filter(|o| o.is_breeding_adult())
            .fold(
                || (Tally::new(), Vec::new()),
                |(mut t, mut counts): (Tally, Vec<(PlayerId, u32)>), o| {
                    o.genome.count_origins_with(&mut counts, morph);
                    for &(p, n) in &counts {
                        *t.entry((o.lineage, p)).or_default() += n as u64;
                    }
                    (t, counts)
                },
            )
            .map(|(t, _)| t)
            .reduce(Tally::new, |mut a, b| {
                for (k, n) in b {
                    *a.entry(k).or_default() += n;
                }
                a
            });
        let mut out: HostTally = tally.into_iter().map(|(k, n)| (k, n as f64)).collect();
        if self.config.lod.enabled {
            let today = self.today_f64();
            let mut per = BTreeMap::new();
            for (_, c) in self.cohorts() {
                if c.pool.is_empty() {
                    continue;
                }
                let a = c.adults(today);
                let adults = (a[0] + a[1]) as f64;
                if adults == 0.0 {
                    continue;
                }
                let w = adults / c.pool.len() as f64;
                for e in &c.pool {
                    per.clear();
                    e.genome.tally_origins_with(w, &mut per, morph);
                    for (&p, &n) in &per {
                        *out.entry((c.lineage, p)).or_default() += n;
                    }
                }
            }
        }
        out
    }

    /// Reinforcements: up to `n` adults join `lineage` on the best-fed
    /// ground near its living adults (agents only), each a copy of a living adult's genome,
    /// so every allele and its owner tag is the lineage's own (a hybrid's
    /// foreign alleles included): kin arriving from elsewhere, not new
    /// founders. Sexes alternate; ages are adult and mixed, as founders'.
    /// They join the anchor's group. Returns the new organisms' ids (none
    /// if the lineage has no living adult agent, or its hard cap is full).
    /// The lineage's adults within this many tiles of an adult make its
    /// group for placing reinforcements (`Balance::founding_safety`).
    pub const REINFORCE_CLUSTER: f32 = 8.0;

    pub fn reinforce_lineage(&mut self, lineage: u32, n: u32) -> Vec<OrgId> {
        self.reinforce_lineage_report(lineage, n).0
    }

    /// Tiles searched outward from the group for clear ground when it stands
    /// inside a rival territory (`Balance::reinforce_clear`).
    pub const REINFORCE_REACH: f32 = 60.0;

    /// [`World::reinforce_lineage`], and where the group stood when the
    /// reinforcements had to land away from it (`Balance::reinforce_clear`:
    /// it stands in or beside another player's territory), and how far.
    pub fn reinforce_lineage_report(&mut self, lineage: u32, n: u32) -> (Vec<OrgId>, Option<(Vec2, f32)>) {
        // Tiles around the group searched when nothing nearer is clear of
        // rivals (`Balance::rival_clear`).
        const WIDE_REINFORCE: f32 = 12.0;
        let adults: Vec<usize> = (0..self.organisms.len())
            .filter(|&i| {
                let o = &self.organisms[i];
                o.alive && o.matured && o.lineage == lineage
            })
            .collect();
        let n = (n as usize).min(self.cap_room(lineage));
        if adults.is_empty() || n == 0 {
            return (Vec::new(), None);
        }
        let mut rng = rng_for(self.config.seed, stream::FOUNDING, (1 << 40) | lineage as u64, self.next_id);
        // Land on good ground near the lineage: from up to 8 of its adults
        // (the best fed first), the spot within 6 tiles with the most food
        // the copies can reach (playtest 2: they landed on a starving
        // member's grazed-out tile).
        let mut picks: Vec<usize> = (0..adults.len().min(8)).map(|_| adults[rng.gen_range(0..adults.len())]).collect();
        picks.sort_by(|&a, &b| self.organisms[a].needs.hunger.total_cmp(&self.organisms[b].needs.hunger));
        picks.dedup();
        // With `Balance::founding_safety`: beside the lineage's largest group
        // (the adult with the most of the lineage's adults within
        // `REINFORCE_CLUSTER` tiles), not a random member: playtest 4's
        // reinforcements landed among predators, 8 of 10 eaten in 5 days.
        let (mut range, mut tries): (f32, usize) = (6.0, 12);
        if self.config.balance.founding_safety > 0.0 {
            let step = (adults.len() / 64).max(1);
            let anchor = (0..adults.len())
                .step_by(step)
                .map(|k| {
                    let p = self.organisms[adults[k]].pos;
                    let near =
                        adults.iter().filter(|&&j| self.organisms[j].pos.dist(p) < Self::REINFORCE_CLUSTER).count();
                    (near, k)
                })
                .max_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)))
                .map(|(_, k)| adults[k]);
            if let Some(a) = anchor {
                picks = vec![a];
                (range, tries) = (3.0, 24);
            }
        }
        let probe = &self.organisms[picks[0]];
        let (group, owner) = (probe.group, probe.owner);
        let body = probe.traits.body;
        let canopy = self.config.balance.canopy_share;
        let food = |w: &World, p: Vec2| -> f32 {
            let mut f = 0.0;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let q = w.map.clamp(p + Vec2::new(dx as f32, dy as f32));
                    f += w.map.tile_at(q).reachable_veg(w.map.ground_at(q), &body, canopy);
                }
            }
            f
        };
        // With `Balance::rival_clear`, the founding rules (playtest 9:
        // reinforcements landed inside rival territories, beside packs and on
        // the map edge): never in or at the edge of another player's
        // territory, nor at the map's edge; a rival predator near costs
        // more than any food is worth. A wider search when none is near.
        let clear = self.config.balance.rival_clear > 0.0;
        let anchor = probe.pos;
        let (packs, lands): (Vec<Vec2>, Vec<super::groups::Territory>) = if clear {
            tries = tries.max(40);
            (
                self.organisms
                    .iter()
                    .filter(|o| {
                        o.alive
                            && o.owner != owner
                            && o.owner != crate::WILD
                            && o.traits.carnivory > 0.6
                            && o.pos.dist(anchor) < super::PACK_CLEAR + WIDE_REINFORCE + 6.0
                    })
                    .map(|o| o.pos)
                    .collect(),
                self.territories.iter().filter(|(&(p, _), _)| p != owner).map(|(_, &t)| t).collect(),
            )
        } else {
            (Vec::new(), Vec::new())
        };
        // Counting how far they scatter from where they land (3 tiles) with
        // `Balance::reinforce_clear` (playtest 10: 2 tiles from an edge).
        let margin = super::RIVAL_TERRITORY_MARGIN + if self.config.balance.reinforce_clear > 0.0 { 3.0 } else { 0.0 };
        let mut home = probe.pos;
        let mut best = f32::MIN;
        let passes: &[f32] = if clear { &[range, WIDE_REINFORCE] } else { &[range] };
        for &range in passes {
            // Clear ground found (no rival predator near): done.
            if best > -500.0 {
                break;
            }
            for &a in &picks {
                let base = self.organisms[a].pos;
                for k in 0..tries {
                    let cand = if k == 0 {
                        base
                    } else {
                        self.map.clamp(base + Vec2::new(rng.gen_range(-range..range), rng.gen_range(-range..range)))
                    };
                    if !self.passable_for(probe, cand) {
                        continue;
                    }
                    let mut score = food(self, cand);
                    if clear {
                        let edge =
                            cand.x.min(cand.y).min(self.map.width as f32 - cand.x).min(self.map.height as f32 - cand.y);
                        if edge < super::EDGE_KEEP || lands.iter().any(|t| t.center.dist(cand) < t.radius + margin) {
                            continue;
                        }
                        let packs_near = packs.iter().filter(|q| q.dist(cand) < super::PACK_CLEAR).count();
                        score -= 1000.0 * packs_near as f32;
                    }
                    if score > best {
                        best = score;
                        home = cand;
                    }
                }
            }
        }

        // Nothing near is clear of rival territory (the group stands in one):
        // the nearest clear ground, outward ring by ring
        // (`Balance::reinforce_clear`; before, they landed in it).
        let mut moved = None;
        if clear && self.config.balance.reinforce_clear > 0.0 && best == f32::MIN {
            let mut r = WIDE_REINFORCE + 2.0;
            'rings: while r <= Self::REINFORCE_REACH {
                let steps = (r * 1.5).ceil() as usize;
                let mut ring_best: Option<(f32, Vec2)> = None;
                for s in 0..steps {
                    let a = std::f32::consts::TAU * s as f32 / steps as f32;
                    let cand = self.map.clamp(anchor + Vec2::new(a.cos(), a.sin()) * r);
                    let edge =
                        cand.x.min(cand.y).min(self.map.width as f32 - cand.x).min(self.map.height as f32 - cand.y);
                    if edge < super::EDGE_KEEP
                        || !self.passable_for(probe, cand)
                        || lands.iter().any(|t| t.center.dist(cand) < t.radius + margin)
                    {
                        continue;
                    }
                    let packs_near = packs.iter().filter(|q| q.dist(cand) < super::PACK_CLEAR).count();
                    let score = food(self, cand) - 1000.0 * packs_near as f32;
                    if ring_best.is_none_or(|(b, _)| score > b) {
                        ring_best = Some((score, cand));
                    }
                }
                if let Some((_, cand)) = ring_best {
                    home = cand;
                    moved = Some((anchor, anchor.dist(cand)));
                    break 'rings;
                }
                r += 2.0;
            }
        }
        let tpd = self.config.ticks_per_day as f32;
        let mut ids = Vec::with_capacity(n);
        for k in 0..n {
            let src = &self.organisms[adults[rng.gen_range(0..adults.len())]];
            let genome = src.genome.clone();
            // A generation after the adult it copies (`Balance::brood_care`):
            // a copy of a founder isn't a founder (no founders' free winter).
            let generation = src.generation + u32::from(self.config.balance.brood_care > 0.0);
            let sex = if k % 2 == 0 { Sex::Female } else { Sex::Male };
            let mut o = Organism::new(0, owner, genome, sex, home);
            for _ in 0..20 {
                let off = Vec2::new(rng.gen_range(-3.0..3.0), rng.gen_range(-3.0..3.0));
                let cand = self.map.clamp(home + off);
                if self.passable_for(&o, cand) {
                    o.pos = cand;
                    break;
                }
            }
            o.lineage = lineage;
            o.group = group;
            o.generation = generation;
            o.growth = 1.0;
            o.matured = true;
            // Young adults (playtest 3: copies arrived as old as founders
            // and died soon).
            o.age_ticks = ((o.traits.maturity_days + rng.gen_range(0.0..o.traits.lifespan_days * 0.05)) * tpd) as u32;
            o.needs.breeding = rng.gen_range(0.0..40.0);
            ids.push(self.insert(o));
        }
        (ids, moved)
    }

    /// Retire `player`'s animals ([`World::retire_player`]), but first hand
    /// each agent whose alleles are at least `min_share` another player's
    /// (the most of any other player, who has a living lineage in `homes`)
    /// to that player: it joins their lineage (`homes[p].0`) and main group,
    /// at most `homes[p].1` of them (the receiver's room under its cap; the
    /// rest leave with the retiree). Every player's alleles in a transferred
    /// animal become the wild's: a transfer credits no legacy to anyone
    /// (playtest 8: a feeder account bred ~470 animals off the leader's studs
    /// and retired, taking the leader to the hard cap). Cohort members and
    /// eggs are removed as before. Returns (removed, transferred).
    pub fn retire_player_sparing(
        &mut self,
        player: PlayerId,
        homes: &BTreeMap<PlayerId, (u32, usize)>,
        min_share: f64,
    ) -> (usize, usize) {
        let mut room: BTreeMap<PlayerId, usize> = homes.iter().map(|(&p, &(_, r))| (p, r)).collect();
        let morph = self.config.balance.morph_in_legacy();
        let mut counts = Vec::new();
        let mut moves: Vec<(usize, PlayerId, u32)> = Vec::new();
        for (k, o) in self.organisms.iter().enumerate() {
            if !o.alive || o.owner != player {
                continue;
            }
            o.genome.count_origins_with(&mut counts, morph);
            let total: u32 = counts.iter().map(|c| c.1).sum();
            let best = counts
                .iter()
                .filter(|&&(p, _)| p != player && p != crate::WILD && homes.contains_key(&p))
                .max_by_key(|&&(p, n)| (n, std::cmp::Reverse(p)));
            // The rival must hold more of it than the retiree: an animal
            // three quarters the retiree's no longer goes to a rival.
            let own = counts.iter().find(|&&(p, _)| p == player).map_or(0, |c| c.1);
            if let Some(&(p, n)) = best {
                let left = room.get_mut(&p).expect("a home");
                if total > 0 && n as f64 >= min_share * total as f64 && n > own && *left > 0 {
                    *left -= 1;
                    moves.push((k, p, homes[&p].0));
                }
            }
        }
        for &(k, p, lineage) in &moves {
            let o = &mut self.organisms[k];
            let old = o.lineage;
            o.owner = p;
            o.lineage = lineage;
            // The retiree's alleles in it become nobody's (the wild's): no
            // royalty or legacy for the retiree from it, and none gifted to
            // the receiver either (playtest 4: re-tagged as the receiver's,
            // colluding players handed each other legacy share). The
            // receiver's own alleles stay theirs (playtest 9: re-tagging them
            // wild took the receiver's own legacy away).
            let g = std::sync::Arc::make_mut(&mut o.genome);
            g.retag_origin(player, crate::WILD);
            o.group = 0;
            o.possessed_by = None;
            o.order = None;
            if let Some(c) = self.owner_counts.get_mut(&player) {
                *c = c.saturating_sub(1);
            }
            *self.owner_counts.entry(p).or_default() += 1;
            self.count_lineage(old, -1);
            self.count_lineage(lineage, 1);
        }
        (self.retire_player(player), moves.len())
    }

    /// An agreed mating's rendezvous: `a` and `b` walk to `meet` (a
    /// meeting point, [`World::meeting_point`]) and, once both are there,
    /// to each other, breaking off for a threat or an urgent thirst or
    /// hunger as a possessed animal does (`Balance::possess_danger`,
    /// `possess_urgency`). They keep off ice that could break, as migrants
    /// do. While either is possessed the other goes only to the meeting
    /// point and waits there: it never follows (a buyer can't lure a
    /// rival's animal anywhere). Replaces either's earlier rendezvous.
    pub fn set_rendezvous(&mut self, a: OrgId, b: OrgId, meet: Vec2) {
        for (x, y) in [(a, b), (b, a)] {
            if let Some((old, _)) = self.rendezvous.insert(x, (y, meet)) {
                if old != y && self.rendezvous.get(&old).is_some_and(|r| r.0 == x) {
                    self.rendezvous.remove(&old);
                }
            }
        }
    }

    /// The animals walking to an accepted market agreement's meeting: other
    /// players' animals spare them (`World::grace_spares`).
    pub fn set_market_shield(&mut self, ids: impl IntoIterator<Item = OrgId>) {
        self.market_shield = ids.into_iter().collect();
    }

    /// Whether `id` is shielded by a market agreement now.
    pub fn market_shielded(&self, id: OrgId) -> bool {
        self.market_shield.contains(&id)
    }

    /// Bar `id` from natural mating with another player's animals until
    /// `until` (a tick); expired bars are dropped as new ones are set.
    pub fn bar_cross_mating(&mut self, id: OrgId, until: u64) {
        let tick = self.tick;
        self.cross_mate_bar.retain(|_, &mut t| t > tick);
        self.cross_mate_bar.insert(id, until);
    }

    /// Whether `id` may not mate naturally with another player's animal now.
    pub fn cross_mating_barred(&self, id: OrgId) -> bool {
        self.cross_mate_bar.get(&id).is_some_and(|&t| t > self.tick)
    }

    /// End `a`'s rendezvous (and its partner's).
    pub fn clear_rendezvous(&mut self, a: OrgId) {
        if let Some((b, _)) = self.rendezvous.remove(&a) {
            if self.rendezvous.get(&b).is_some_and(|r| r.0 == a) {
                self.rendezvous.remove(&b);
            }
        }
    }

    /// `a`'s rendezvous partner, if any.
    pub fn rendezvous_of(&self, a: OrgId) -> Option<OrgId> {
        self.rendezvous.get(&a).map(|r| r.0)
    }

    /// Where `a` and `b` should meet: the dry tile nearest the midpoint
    /// between them (within [`MEET_SEARCH`] tiles; the midpoint itself if
    /// there is none) that `a`'s body can stand on, so the walk doesn't end
    /// in a lake.
    pub fn meeting_point(&self, a: OrgId, b: OrgId) -> Option<Vec2> {
        self.meeting_point_toward(a, b, 0.5)
    }

    /// [`World::meeting_point`] at fraction `t` of the way from `a` to `b`
    /// (0.5: the midpoint) instead of halfway.
    pub fn meeting_point_toward(&self, a: OrgId, b: OrgId, t: f32) -> Option<Vec2> {
        let (x, y) = (self.organism(a)?, self.organism(b)?);
        let mid = self.map.clamp(x.pos + (y.pos - x.pos) * t.clamp(0.0, 1.0));
        let dry =
            |p: Vec2| self.map.tile_at(p).water_depth <= 0.0 && self.passable_for(x, p) && self.passable_for(y, p);
        if dry(mid) {
            return Some(mid);
        }
        let mut best: Option<(f32, Vec2)> = None;
        for r in 1..=MEET_SEARCH {
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs() != r && dy.abs() != r {
                        continue;
                    }
                    let p = self.map.clamp(mid + Vec2::new(dx as f32, dy as f32));
                    if dry(p) {
                        let d = p.dist(mid);
                        if best.is_none_or(|(bd, _)| d < bd) {
                            best = Some((d, p));
                        }
                    }
                }
            }
            if best.is_some() {
                break;
            }
        }
        Some(best.map_or(mid, |b| b.1))
    }

    /// Whether `o` walks for a rendezvous (and so keeps off breakable ice).
    pub(super) fn in_rendezvous(&self, o: &Organism) -> bool {
        !self.rendezvous.is_empty() && self.rendezvous.contains_key(&o.id)
    }

    /// Where `o` walks for its rendezvous now, if it has one and nothing
    /// comes first (`decide`): the meeting point until it is there; then its
    /// partner, once the partner is there too and not possessed. None while
    /// they are within [`RENDEZVOUS_NEAR`] (the agreement runs).
    pub(super) fn rendezvous_target(&self, o: &Organism, threat: f32) -> Option<Vec2> {
        if self.rendezvous.is_empty() || o.possessed_by.is_some() || !o.matured {
            return None;
        }
        let &(partner, meet) = self.rendezvous.get(&o.id)?;
        let q = self.index.get(&partner).map(|&k| &self.organisms[k]).filter(|q| q.alive)?;
        if o.pos.dist(q.pos) <= RENDEZVOUS_NEAR || self.order_override(o, threat).is_some() {
            return None;
        }
        if o.pos.dist(meet) > MEET_AT {
            return Some(meet);
        }
        // At the meeting point: close in only on a partner that is there too
        // and walking on its own; otherwise wait here.
        if q.possessed_by.is_none() && q.pos.dist(meet) <= 2.0 * MEET_AT || q.pos.dist(o.pos) <= 2.0 * MEET_AT {
            Some(q.pos)
        } else {
            Some(meet)
        }
    }

    fn today_f64(&self) -> f64 {
        self.tick as f64 / self.config.ticks_per_day as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presets;
    use crate::WorldConfig;

    /// Summed over hosts, the tally is the legacy snapshot's.
    #[test]
    fn host_tally_sums_to_the_legacy_tally() {
        let mut w = presets::standard_world(WorldConfig { seed: 5, width: 64, height: 64, ..Default::default() }, 1);
        w.run_days(3);
        let hosts = w.legacy_hosts();
        let snap = w.latest_legacy().unwrap().clone();
        // The snapshot is from the day's start: compare against a fresh one.
        let morph = w.config.balance.morph_in_legacy();
        let fresh = crate::legacy::LegacySnapshot::compute(
            0,
            w.organisms.iter().filter(|o| o.is_breeding_adult()).map(|o| (&*o.genome, 0)),
            morph,
        );
        let mut by_origin: BTreeMap<PlayerId, f64> = BTreeMap::new();
        for (&(_, p), &n) in &hosts {
            *by_origin.entry(p).or_default() += n;
        }
        assert_eq!(by_origin, fresh.alleles);
        assert!(snap.total_alleles > 0.0);
        // Every host is a lineage, and a player's alleles live mostly at home.
        assert!(hosts.keys().all(|&(l, _)| (l as usize) < w.lineages.len()));
    }

    /// Retiring hands animals mostly of a rival's genes to that rival.
    #[test]
    fn retiring_spares_rivals_alleles() {
        let mut w = presets::standard_world(WorldConfig { seed: 6, width: 64, height: 64, ..Default::default() }, 1);
        let l1 = w.lineages.iter().position(|l| l.owner == 1).unwrap() as u32;
        let l2 = w.lineages.iter().position(|l| l.owner == 2).unwrap() as u32;
        let rival = w.organisms.iter().find(|o| o.owner == 2).unwrap().genome.clone();
        let ids: Vec<u64> = w.organisms.iter().filter(|o| o.alive && o.owner == 1).map(|o| o.id).take(3).collect();
        for &id in &ids {
            w.organisms.iter_mut().find(|o| o.id == id).unwrap().genome = rival.clone();
        }
        let n1 = w.organisms.iter().filter(|o| o.alive && o.owner == 1).count();
        let n2 = w.owner_count(2);
        let (removed, moved) = w.retire_player_sparing(1, &BTreeMap::from([(2, (l2, usize::MAX))]), 0.25);
        assert_eq!((removed, moved), (n1 - 3, 3));
        assert_eq!(w.owner_count(1), 0);
        assert_eq!(w.owner_count(2), n2 + 3);
        for id in ids {
            let o = w.organism(id).unwrap();
            assert_eq!((o.owner, o.lineage), (2, l2));
        }
        assert!(w.organisms.iter().all(|o| !o.alive || o.lineage != l1));
    }

    /// Playtest 3: an animal three quarters the retiree's went to a rival
    /// holding a quarter, then paid the retiree royalties. Now it goes only
    /// to a rival holding more of it than the retiree, with the retiree's
    /// alleles re-tagged as the receiver's.
    #[test]
    fn a_retirement_hands_over_only_animals_mostly_a_rivals() {
        use crate::genome::{Genome, MutationParams};
        let mut w = presets::standard_world(WorldConfig { seed: 6, width: 64, height: 64, ..Default::default() }, 1);
        let l2 = w.lineages.iter().position(|l| l.owner == 2).unwrap() as u32;
        let rival = w.organisms.iter().find(|o| o.owner == 2).unwrap().genome.clone();
        let own = w.organisms.iter().find(|o| o.owner == 1).unwrap().genome.clone();
        let none = MutationParams { rate: 0.0, sd: 0.0 };
        let mut rng = crate::rng::rng_for(1, 2, 3, 4);
        let half = Genome::inherit(&own, &rival, 1, none, 0.0, &mut rng);
        let mostly_rival = Genome::inherit(&half, &rival, 1, none, 0.0, &mut rng);
        let mostly_own = Genome::inherit(&half, &own, 1, none, 0.0, &mut rng);
        let share = |g: &Genome, p| {
            let mut c = Vec::new();
            g.count_origins(&mut c);
            c.iter().find(|x| x.0 == p).map_or(0, |x| x.1) as f64 / c.iter().map(|x| x.1).sum::<u32>() as f64
        };
        assert!(share(&mostly_rival, 2) > 0.6 && share(&mostly_own, 2) >= 0.25 && share(&mostly_own, 1) > 0.6);
        let ids: Vec<u64> = w.organisms.iter().filter(|o| o.alive && o.owner == 1).map(|o| o.id).take(2).collect();
        for (&id, g) in ids.iter().zip([&mostly_rival, &mostly_own]) {
            w.organisms.iter_mut().find(|o| o.id == id).unwrap().genome = std::sync::Arc::new(g.clone());
        }
        let (_, moved) = w.retire_player_sparing(1, &BTreeMap::from([(2, (l2, usize::MAX))]), 0.25);
        assert_eq!(moved, 1, "only the animal mostly the rival's");
        let o = w.organism(ids[0]).unwrap();
        assert_eq!(o.owner, 2);
        assert_eq!(share(&o.genome, 1), 0.0, "none of the retiree's alleles left in it");
        // The retiree's went to the wild, not to the receiver (no legacy
        // gifted); the receiver's own stay theirs (playtest 9).
        let before = share(&mostly_rival, 2);
        assert!((share(&o.genome, 2) - before).abs() < 1e-9, "the receiver's own alleles kept");
        assert!((share(&o.genome, crate::WILD) - (1.0 - before)).abs() < 1e-9);
        assert!(w.organism(ids[1]).is_none());
    }

    /// A transfer fills only the receiver's room under its cap; the rest
    /// leave with the retiree (playtest 8: a feeder account's retirement took
    /// the leader to the hard cap).
    #[test]
    fn a_retirement_hands_over_no_more_than_the_receivers_room() {
        let mut w = presets::standard_world(WorldConfig { seed: 6, width: 64, height: 64, ..Default::default() }, 1);
        let l2 = w.lineages.iter().position(|l| l.owner == 2).unwrap() as u32;
        let rival = w.organisms.iter().find(|o| o.owner == 2).unwrap().genome.clone();
        let ids: Vec<u64> = w.organisms.iter().filter(|o| o.alive && o.owner == 1).map(|o| o.id).take(3).collect();
        for &id in &ids {
            w.organisms.iter_mut().find(|o| o.id == id).unwrap().genome = rival.clone();
        }
        let n2 = w.owner_count(2);
        let (_, moved) = w.retire_player_sparing(1, &BTreeMap::from([(2, (l2, 1))]), 0.25);
        assert_eq!(moved, 1);
        assert_eq!(w.owner_count(2), n2 + 1);
        assert_eq!(w.owner_count(1), 0);
        let (_, none) = {
            let mut w2 =
                presets::standard_world(WorldConfig { seed: 6, width: 64, height: 64, ..Default::default() }, 1);
            let l2 = w2.lineages.iter().position(|l| l.owner == 2).unwrap() as u32;
            w2.retire_player_sparing(1, &BTreeMap::from([(2, (l2, 0))]), 0.25)
        };
        assert_eq!(none, 0, "a receiver at its cap gets none");
    }

    /// A rendezvous brings two animals far apart together on their own.
    #[test]
    fn a_rendezvous_brings_the_pair_together() {
        let mut w = presets::standard_world(WorldConfig { seed: 8, width: 64, height: 64, ..Default::default() }, 1);
        let a = w.organisms.iter().find(|o| o.owner == 1 && o.matured).unwrap().id;
        let b = w.organisms.iter().find(|o| o.owner == 2 && o.matured).unwrap().id;
        // Put b where some animal stands farthest from a (passable ground).
        let pa = w.organism(a).unwrap().pos;
        let far = w
            .organisms
            .iter()
            .filter(|o| o.alive)
            .map(|o| o.pos)
            .max_by(|x, y| x.dist(pa).total_cmp(&y.dist(pa)))
            .unwrap();
        let ib = w.organism_index(b).unwrap();
        w.organisms[ib].pos = far;
        let start = w.organism(a).unwrap().pos.dist(w.organism(b).unwrap().pos);
        assert!(start > 10.0, "{start}");
        let meet = w.meeting_point(a, b).unwrap();
        assert_eq!(w.map.tile_at(meet).water_depth, 0.0, "a dry meeting point");
        w.set_rendezvous(a, b, meet);
        assert_eq!((w.rendezvous_of(a), w.rendezvous_of(b)), (Some(b), Some(a)));
        let mut met = None;
        for t in 0..20 * w.config.ticks_per_day {
            w.step();
            let (Some(x), Some(y)) = (w.organism(a), w.organism(b)) else { break };
            if x.pos.dist(y.pos) <= 1.5 {
                met = Some(t);
                break;
            }
        }
        assert!(met.is_some(), "they never met (started {start:.0} apart)");
        w.clear_rendezvous(b);
        assert_eq!((w.rendezvous_of(a), w.rendezvous_of(b)), (None, None));
    }

    /// A possessed partner isn't followed: the other walks only to the
    /// meeting point and waits there.
    #[test]
    fn a_possessed_partner_is_not_followed() {
        let mut w = presets::standard_world(WorldConfig { seed: 8, width: 64, height: 64, ..Default::default() }, 1);
        let a = w.organisms.iter().find(|o| o.owner == 1 && o.matured).unwrap().id;
        let b = w.organisms.iter().find(|o| o.owner == 2 && o.matured).unwrap().id;
        let meet = w.meeting_point(a, b).unwrap();
        w.set_rendezvous(a, b, meet);
        w.possess(2, b).unwrap();
        // Lead b far away: a must stay near the meeting point.
        let ib = w.organism_index(b).unwrap();
        let away = w.map.clamp(meet + Vec2::new(25.0, 25.0));
        for _ in 0..6 * w.config.ticks_per_day {
            w.organisms[ib].pos = away;
            w.step();
        }
        let pa = w.organism(a).unwrap().pos;
        assert!(pa.dist(meet) < pa.dist(away), "a followed the possessed partner: at {pa:?}, meet {meet:?}");
        // Near it (breaks to eat or drink allowed), never at the possessed one.
        assert!(pa.dist(meet) <= 10.0, "a waits near the meeting point: {:.1} away", pa.dist(meet));
        assert!(pa.dist(away) > 20.0, "a followed: {:.1} from the possessed partner", pa.dist(away));
    }

    /// Animals in open market offers are spared by other players'
    /// animals (not by the wild's) until the server lifts the shield.
    #[test]
    fn the_market_shield_spares_offered_animals_from_rivals() {
        let mut w = presets::standard_world(WorldConfig { seed: 8, width: 64, height: 64, ..Default::default() }, 1);
        w.config.balance.attack_grace = 0.0;
        let a = w.organisms.iter().position(|o| o.owner == 1).unwrap();
        let b = w.organisms.iter().position(|o| o.owner == 2).unwrap();
        assert!(!w.grace_spares(&w.organisms[a], &w.organisms[b]));
        let id = w.organisms[b].id;
        w.set_market_shield([id]);
        assert!(w.grace_spares(&w.organisms[a], &w.organisms[b]), "a rival spares it");
        w.set_market_shield([]);
        assert!(!w.grace_spares(&w.organisms[a], &w.organisms[b]));
    }

    /// A possessed male (or one barred for a while after) sires no young
    /// in another player's herd outside an agreement (playtest 8's stud
    /// raid); barred, the same male still may not.
    #[test]
    fn a_possessed_male_does_not_sire_a_rivals_young() {
        let mut w = presets::standard_world(WorldConfig { seed: 8, width: 64, height: 64, ..Default::default() }, 1);
        let m = w.organisms.iter().find(|o| o.owner == 1 && o.matured && o.sex == Sex::Male).unwrap().id;
        let females: Vec<OrgId> =
            w.organisms.iter().filter(|o| o.owner == 2 && o.matured && o.sex == Sex::Female).map(|o| o.id).collect();
        w.possess(1, m).unwrap();
        let im = w.organism_index(m).unwrap();
        for _ in 0..4 * w.config.ticks_per_day {
            // Keep him among her females.
            let at = w.organism(females[0]).map(|f| f.pos);
            if let Some(p) = at {
                w.organisms[im].pos = p;
            }
            w.step();
        }
        let sired =
            w.organisms.iter().filter(|o| o.owner == 2).any(|o| o.pregnancy.as_ref().is_some_and(|p| p.father_id == m));
        assert!(!sired, "the possessed male sired a rival's young");
        w.release(1, m).ok();
        w.bar_cross_mating(m, w.tick() + 1_000);
        assert!(w.cross_mating_barred(m));
    }

    #[test]
    fn reinforcements_copy_living_adults_near_them() {
        let mut w = presets::standard_world(WorldConfig { seed: 6, width: 64, height: 64, ..Default::default() }, 1);
        w.run_days(1);
        let l = w.lineages.iter().position(|l| l.owner == 2).unwrap() as u32;
        let before = w.organisms.iter().filter(|o| o.alive && o.lineage == l).count();
        let ids = w.reinforce_lineage(l, 6);
        assert_eq!(ids.len(), 6);
        let after = w.organisms.iter().filter(|o| o.alive && o.lineage == l).count();
        assert_eq!(after, before + 6);
        for id in ids {
            let o = w.organism(id).unwrap();
            assert!(o.matured && o.owner == 2);
            let kin = w.organisms.iter().any(|k| k.id != id && k.lineage == l && *k.genome == *o.genome);
            assert!(kin, "a copy of a living adult");
        }
        // A lineage without adults gets none.
        assert!(w.reinforce_lineage(999, 3).is_empty());
    }

    /// Playtest 4: reinforcements landed beside a lone member among
    /// predators and 8 of 10 were eaten. They land beside the lineage's
    /// largest group (`Balance::founding_safety`).
    /// Playtest 5: clones kept generation 0 (a founder's free first winter).
    /// They are young adults a generation after the adult they copy.
    #[test]
    fn reinforcements_are_young_adults_of_the_next_generation() {
        let mut w = presets::standard_world(WorldConfig { seed: 6, width: 64, height: 64, ..Default::default() }, 1);
        w.run_days(1);
        let l = w.lineages.iter().position(|l| l.owner == 2).unwrap() as u32;
        let tpd = w.config.ticks_per_day as f32;
        for brood_care in [1.0, 0.0] {
            let mut c = w.clone();
            c.config.balance.brood_care = brood_care;
            let ids = c.reinforce_lineage(l, 4);
            assert_eq!(ids.len(), 4);
            for id in ids {
                let o = c.organism(id).unwrap();
                assert_eq!(o.generation, u32::from(brood_care > 0.0), "founders' copies");
                let age = o.age_ticks as f32 / tpd;
                assert!(o.matured && age <= o.traits.maturity_days + 0.06 * o.traits.lifespan_days, "{age}");
            }
        }
    }

    #[test]
    fn reinforcements_land_beside_the_largest_group() {
        let mut w = presets::standard_world(WorldConfig { seed: 6, width: 64, height: 64, ..Default::default() }, 1);
        w.run_days(1);
        let l = w.lineages.iter().position(|l| l.owner == 2).unwrap() as u32;
        let members: Vec<usize> =
            (0..w.organisms.len()).filter(|&i| w.organisms[i].alive && w.organisms[i].lineage == l).collect();
        assert!(members.len() >= 6);
        // All but a few stand at one spot; the rest are spread far off.
        let herd = w.organisms[members[0]].pos;
        let walkable = |w: &World, p: Vec2| w.passable_for(&w.organisms[members[0]], p);
        for (k, &i) in members.iter().enumerate() {
            let p = if k < members.len() - 3 {
                herd + Vec2::new((k % 3) as f32 * 0.3, (k / 3) as f32 * 0.3)
            } else {
                Vec2::new((10 + 20 * (k % 3)) as f32, 60.0)
            };
            if walkable(&w, p) {
                w.organisms[i].pos = w.map.clamp(p);
            }
        }
        for _ in 0..4 {
            let mut c = w.clone();
            let ids = c.reinforce_lineage(l, 6);
            assert_eq!(ids.len(), 6);
            for id in ids {
                let d = c.organism(id).unwrap().pos.dist(herd);
                assert!(d < 8.0, "{d:.1} tiles from the herd");
            }
            w.next_id += 7;
        }
    }

    /// Playtest 9: reinforcements landed inside rival territories, beside
    /// packs and on the map edge. With `Balance::rival_clear` they land
    /// outside another player's territory (and its margin) even when the
    /// group stands at its edge.
    #[test]
    fn reinforcements_keep_out_of_rival_territory() {
        let mut w = presets::standard_world(WorldConfig { seed: 6, width: 64, height: 64, ..Default::default() }, 1);
        w.run_days(1);
        let l = w.lineages.iter().position(|l| l.owner == 2).unwrap() as u32;
        let members: Vec<usize> =
            (0..w.organisms.len()).filter(|&i| w.organisms[i].alive && w.organisms[i].lineage == l).collect();
        let herd = w.organisms[members[0]].pos;
        for &i in &members {
            w.organisms[i].pos = herd;
        }
        // A rival's territory right over the herd.
        let t = super::super::groups::Territory { center: herd, radius: 5.0, claimed_tick: 0 };
        w.territories.insert((77, 0), t);
        let keep = t.radius + super::super::RIVAL_TERRITORY_MARGIN;
        let mut c = w.clone();
        let ids = c.reinforce_lineage(l, 6);
        assert_eq!(ids.len(), 6);
        for id in ids {
            let d = c.organism(id).unwrap().pos.dist(herd);
            // Placed within 3 tiles of a home outside the territory's margin.
            assert!(d > keep - 3.0, "{d:.1} tiles from the rival territory's centre");
        }
        // The old rule: beside the herd, inside it.
        let mut old = w.clone();
        old.config.balance.rival_clear = 0.0;
        let ids = old.reinforce_lineage(l, 6);
        assert!(ids.iter().any(|&id| old.organism(id).unwrap().pos.dist(herd) < t.radius));
    }

    /// Playtest 10: reinforcements landed inside a rival territory where the
    /// pack lived (nothing near it was clear), and 2 tiles from an edge.
    /// With `Balance::reinforce_clear` they land on the nearest clear ground
    /// outside it, margin and scatter included, and the move is reported.
    #[test]
    fn reinforcements_leave_a_rival_territory_the_group_lives_in() {
        let mut w = presets::standard_world(WorldConfig { seed: 6, width: 96, height: 96, ..Default::default() }, 1);
        w.run_days(1);
        let l = w.lineages.iter().position(|l| l.owner == 2).unwrap() as u32;
        let members: Vec<usize> =
            (0..w.organisms.len()).filter(|&i| w.organisms[i].alive && w.organisms[i].lineage == l).collect();
        let herd = w.map.clamp(Vec2::new(48.0, 48.0));
        for &i in &members {
            w.organisms[i].pos = herd;
        }
        let t = super::super::groups::Territory { center: herd, radius: 18.0, claimed_tick: 0 };
        w.territories.insert((77, 0), t);
        let mut c = w.clone();
        let (ids, moved) = c.reinforce_lineage_report(l, 6);
        assert_eq!(ids.len(), 6);
        let (from, dist) = moved.expect("moved out of the territory");
        assert_eq!(from, herd);
        assert!(dist > t.radius + super::super::RIVAL_TERRITORY_MARGIN, "{dist:.1}");
        for id in ids {
            let d = c.organism(id).unwrap().pos.dist(herd);
            assert!(d > t.radius + super::super::RIVAL_TERRITORY_MARGIN, "{d:.1} tiles from the territory's centre");
        }
        // The old rule: landed in it.
        let mut old = w.clone();
        old.config.balance.reinforce_clear = 0.0;
        let (ids, moved) = old.reinforce_lineage_report(l, 6);
        assert!(moved.is_none());
        assert!(ids.iter().any(|&id| old.organism(id).unwrap().pos.dist(herd) < t.radius));
    }
}
