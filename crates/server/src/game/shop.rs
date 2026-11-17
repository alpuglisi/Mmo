//! The renown shop, the breeder's royalty, compatibility and the score
//! breakdown: what gives the mating market a reason to exist (playtest 1:
//! zero trades, renown piling up with nothing to buy).
//!
//! - **Shop** ([`Game::shop_view`]): an extra directed-mating slot for the
//!   rest of the weather season, and reinforcements for a small lineage
//!   (prices and limits in [`crate::economy`]). Renown spent is destroyed.
//! - **Royalty** ([`Game::pay_royalties`]): renown a day for a player's
//!   alleles living outside their lineage (studs that sired into rivals'
//!   dams, genes that spread into the wild).
//! - **Compatibility** ([`Game::compatibility`]): which lineages a player's
//!   can breed with at all (body plans 0.30 apart are sterile), shown per
//!   listing in the market too.
//! - **Score breakdown** ([`Game::score_breakdown`]): whose genes make up a
//!   legacy share, from the daily host tally (`World::legacy_hosts`).

use std::collections::BTreeMap;

use evo_sim::genome::Genome;
use evo_sim::mating::{body_plan_gap, fertility_from_gap, BODY_PLAN_GENES};
use evo_sim::{PlayerId, WILD};

use super::Game;
use crate::design::gene_key;
use crate::economy;
use crate::protocol::{
    CompatibilityRow, CompatibilityView, CrowdingView, ErrorInfo, GeneHost, GeneSource, LedgerKind, ListingView,
    ScoreBreakdown, ShopView,
};

/// Adults of each lineage compared in a compatibility check.
const SAMPLE: usize = 16;

/// Living adults' genomes of `lineage`, up to [`SAMPLE`], spread over the
/// organism list (the same sample for the same world).
fn sample_of(g: &Game, lineage: u32) -> Vec<&Genome> {
    let all: Vec<&Genome> =
        g.world.organisms.iter().filter(|o| o.alive && o.matured && o.lineage == lineage).map(|o| &*o.genome).collect();
    if all.len() <= SAMPLE {
        return all;
    }
    let step = all.len() as f64 / SAMPLE as f64;
    (0..SAMPLE).map(|k| all[(k as f64 * step) as usize]).collect()
}

/// Best body-plan fertility over the pairs of `a` × `b`, and that pair's
/// widest body-plan gene and gap.
fn best_pair(a: &[&Genome], b: &[&Genome]) -> Option<(f32, String, f32)> {
    let mut best: Option<(f32, &Genome, &Genome, f32)> = None;
    for &x in a {
        for &y in b {
            let gap = body_plan_gap(x, y);
            if best.is_none_or(|(_, _, _, g)| gap < g) {
                best = Some((fertility_from_gap(gap), x, y, gap));
            }
        }
    }
    let (fertility, x, y, gap) = best?;
    let (gene, gene_gap) = BODY_PLAN_GENES
        .iter()
        .map(|&g| (g, (x.expr(g) - y.expr(g)).abs()))
        .fold((BODY_PLAN_GENES[0], -1.0f32), |m, v| if v.1 > m.1 { v } else { m });
    let limiting = if gap > gene_gap + 1e-6 { "morphology".to_string() } else { gene_key(gene) };
    Some((fertility, limiting, gap))
}

impl Game {
    /// The shop as `player` sees it now.
    pub(super) fn shop_view(&self, player: PlayerId) -> ShopView {
        let bought = self.world.bonus_slots(player);
        let reinforced = self.reinforcements_this_season(player);
        let max_alive = self.reinforce_max_alive();
        ShopView {
            balance: self.balance(player),
            slots_left: self.world.mating_slots_left(player),
            slots_bought: bought,
            slot_price: economy::slot_price(bought),
            reinforce_size: economy::REINFORCE_SIZE,
            reinforce_price: economy::reinforce_price(reinforced),
            reinforce_max_alive: max_alive,
            reinforce_blocked: self.reinforce_blocked(player).err().map(|e| e.message),
            market_fee_percent: economy::MARKET_FEE_PERCENT,
            royalty_per_point: economy::ROYALTY_PER_POINT,
        }
    }

    /// Most living members a reinforced lineage may have.
    fn reinforce_max_alive(&self) -> usize {
        (self.world.config.lineage_cap as f64 * economy::REINFORCE_MAX_SHARE).floor() as usize
    }

    fn reinforcements_this_season(&self, player: PlayerId) -> u32 {
        match self.economy.reinforcements.get(&player) {
            Some(&(season, n)) if season == self.season.season => n,
            _ => 0,
        }
    }

    /// Why `player` can't buy reinforcements now (their lineage otherwise).
    fn reinforce_blocked(&self, player: PlayerId) -> Result<u32, ErrorInfo> {
        let Some(lineage) = self.lineage_of(player).filter(|_| !self.is_extinct(player)) else {
            return Err(ErrorInfo::new("no_lineage", "you have no living lineage to reinforce"));
        };
        let alive = self.owner_population(player);
        let max = self.reinforce_max_alive();
        if alive > max {
            return Err(ErrorInfo::new(
                "too_large",
                format!("reinforcements are for lineages of at most {max} (yours has {alive})"),
            ));
        }
        let adults = self.world.organisms.iter().any(|o| o.alive && o.matured && o.lineage == lineage);
        if !adults {
            return Err(ErrorInfo::new("no_adults", "reinforcements join a living adult; your lineage has none"));
        }
        Ok(lineage)
    }

    /// Spend `price` renown in the shop (destroyed).
    fn spend(&mut self, player: PlayerId, price: u64, text: String) -> Result<(), ErrorInfo> {
        let debit = i64::try_from(price).map_err(|_| ErrorInfo::new("bad_price", "price out of range"))?;
        self.post(player, -debit, LedgerKind::Purchase, text, None, None).map_err(|e| {
            ErrorInfo::new("insufficient_funds", format!("{} renown needed, you have {}", e.needed, e.balance))
                .with_detail(serde_json::json!({ "balance": e.balance, "needed": e.needed }))
        })?;
        Ok(())
    }

    /// `buy_mating_slot`.
    pub(super) fn buy_mating_slot(&mut self, player: PlayerId) -> Result<ShopView, ErrorInfo> {
        if self.lineage_of(player).is_none() {
            return Err(ErrorInfo::new("no_lineage", "found a lineage first"));
        }
        let bought = self.world.bonus_slots(player);
        let price = economy::slot_price(bought).ok_or_else(|| {
            ErrorInfo::new(
                "limit_reached",
                format!("at most {} extra slots a weather season", economy::MAX_BOUGHT_SLOTS),
            )
        })?;
        self.spend(player, price, format!("Extra mating slot ({} bought this season)", bought + 1))?;
        self.world.grant_mating_slots(player, 1);
        Ok(self.shop_view(player))
    }

    /// `reinforce`: the new members' ids and the shop after.
    pub(super) fn reinforce(&mut self, player: PlayerId) -> Result<(Vec<u64>, ShopView), ErrorInfo> {
        let lineage = self.reinforce_blocked(player)?;
        let n = self.reinforcements_this_season(player);
        let price = economy::reinforce_price(n);
        if self.balance(player) < price {
            let balance = self.balance(player);
            return Err(ErrorInfo::new("insufficient_funds", format!("{price} renown needed, you have {balance}"))
                .with_detail(serde_json::json!({ "balance": balance, "needed": price })));
        }
        let ids = self.world.reinforce_lineage(lineage, economy::REINFORCE_SIZE);
        if ids.is_empty() {
            return Err(ErrorInfo::new("lineage_full", "no room for reinforcements"));
        }
        self.spend(player, price, format!("Reinforcements: {} adults joined your lineage", ids.len()))?;
        self.economy.reinforcements.insert(player, (self.season.season, n + 1));
        // New organisms: the next state build indexes the world again.
        self.state_cache.invalidate();
        Ok((ids, self.shop_view(player)))
    }

    /// `compatibility`: every other lineage with living adults, most
    /// fertile first.
    pub(super) fn compatibility(&self, player: PlayerId) -> CompatibilityView {
        let Some(mine) = self.lineage_of(player).filter(|_| !self.is_extinct(player)) else {
            return CompatibilityView { lineage: None, rows: Vec::new() };
        };
        let own = sample_of(self, mine);
        if own.is_empty() {
            return CompatibilityView { lineage: None, rows: Vec::new() };
        }
        let mut rows: Vec<CompatibilityRow> = (0..self.world.lineages.len() as u32)
            .filter(|&l| l != mine)
            .filter_map(|l| {
                let theirs = sample_of(self, l);
                let (fertility, limiting, gap) = best_pair(&own, &theirs)?;
                let lin = &self.world.lineages[l as usize];
                Some(CompatibilityRow {
                    lineage: l,
                    lineage_name: lin.name.clone(),
                    owner: lin.owner,
                    owner_name: self.name(lin.owner),
                    fertility,
                    compatible: fertility > 0.0,
                    limiting,
                    gap,
                })
            })
            .collect();
        rows.sort_by(|a, b| b.fertility.total_cmp(&a.fertility).then(a.gap.total_cmp(&b.gap)));
        CompatibilityView { lineage: Some(mine), rows }
    }

    /// Fill each listing's `compatible` and `body_plan_fertility` for
    /// `player` (nothing when they have no living adult).
    pub(super) fn annotate_compatibility(&self, player: PlayerId, views: &mut [ListingView]) {
        let Some(mine) = self.lineage_of(player) else { return };
        let own = sample_of(self, mine);
        if own.is_empty() {
            return;
        }
        for v in views {
            let Some(o) = self.world.organism(v.organism) else { continue };
            if let Some((fertility, _, _)) = best_pair(&own, &[&*o.genome]) {
                v.compatible = Some(fertility > 0.0);
                v.body_plan_fertility = Some(fertility);
            }
        }
    }

    /// The host tally of the day's legacy snapshot (`World::legacy_hosts`),
    /// taken with the daily record.
    pub(super) fn refresh_hosts(&mut self) {
        self.hosts = self.world.legacy_hosts();
    }

    /// `player`'s alleles outside their lineage, over all alleles.
    fn exported_share(&self, player: PlayerId) -> f64 {
        let total: f64 = self.hosts.values().sum();
        if total <= 0.0 {
            return 0.0;
        }
        let home = self.lineage_of(player);
        let out: f64 = self.hosts.iter().filter(|(&(l, p), _)| p == player && Some(l) != home).map(|(_, &n)| n).sum();
        out / total
    }

    /// The daily breeder's royalty (`economy::royalty`).
    pub(super) fn pay_royalties(&mut self, day: u64) {
        let players: Vec<(PlayerId, u32)> = self.lineage_of.iter().map(|(&p, &l)| (p, l)).collect();
        for (p, _) in players {
            let exported = self.exported_share(p);
            let pay = economy::royalty(exported, !self.is_extinct(p));
            if pay > 0 {
                let text = format!(
                    "Day {} royalty: {:.2}% of all alleles are yours and live outside your lineage",
                    day + 1,
                    exported * 100.0
                );
                let _ = self.post(p, pay as i64, LedgerKind::Royalty, text, None, None);
            }
        }
    }

    /// Whose genes make up `player`'s legacy share (`lineage_stats.score`).
    pub(super) fn score_breakdown(&self, player: PlayerId) -> Option<ScoreBreakdown> {
        let total: f64 = self.hosts.values().sum();
        let day = self.world.latest_legacy().map_or(self.world.day(), |l| l.day);
        if total <= 0.0 {
            return None;
        }
        let home = self.lineage_of(player);
        let mut carried: BTreeMap<PlayerId, f64> = BTreeMap::new();
        let mut placed: BTreeMap<u32, f64> = BTreeMap::new();
        for (&(l, p), &n) in &self.hosts {
            if Some(l) == home {
                *carried.entry(p).or_default() += n;
            }
            if p == player {
                *placed.entry(l).or_default() += n;
            }
        }
        let lineage_alleles: f64 = carried.values().sum();
        let mine: f64 = placed.values().sum();
        let home_alleles = carried.get(&player).copied().unwrap_or(0.0);
        let exported = mine - home_alleles;
        let mut carried: Vec<GeneSource> = carried
            .into_iter()
            .map(|(p, n)| GeneSource {
                player: p,
                name: if p == WILD { "wild".into() } else { self.name(p) },
                share: n / lineage_alleles.max(1e-9),
            })
            .collect();
        carried.sort_by(|a, b| b.share.total_cmp(&a.share).then(a.player.cmp(&b.player)));
        let mut placed: Vec<GeneHost> = placed
            .into_iter()
            .map(|(l, n)| {
                let lin = self.world.lineages.get(l as usize);
                let owner = lin.map_or(WILD, |x| x.owner);
                GeneHost {
                    lineage: l,
                    lineage_name: lin.map(|x| x.name.clone()).unwrap_or_default(),
                    owner,
                    owner_name: if owner == WILD { "wild".into() } else { self.name(owner) },
                    share: n / mine.max(1e-9),
                }
            })
            .collect();
        placed.sort_by(|a, b| b.share.total_cmp(&a.share).then(a.lineage.cmp(&b.lineage)));
        let exported_share = exported / total;
        Some(ScoreBreakdown {
            day,
            share: mine / total,
            head_share: lineage_alleles / total,
            home_share: home_alleles / total,
            exported_share,
            foreign_share: (lineage_alleles - home_alleles) / total,
            carried,
            placed,
            royalty: economy::royalty(exported_share, home.is_some() && !self.is_extinct(player)),
        })
    }

    /// The soft lineage cap as it stands for `player` (`lineage_stats.crowding`).
    pub(super) fn crowding_view(&self, player: PlayerId) -> Option<CrowdingView> {
        self.lineage_of(player)?;
        let count = self.world.crowding_counts().get(&player).copied().unwrap_or(0.0);
        let [litter, hunger] = self.world.crowding_effect(count);
        Some(CrowdingView {
            soft_cap: self.world.config.lineage_cap,
            hard_cap: self.world.player_ceiling(),
            count,
            juvenile_weight: self.world.config.balance.crowding_juveniles,
            litter,
            hunger,
        })
    }
}
