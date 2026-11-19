//! The renown shop, the breeder's royalty, compatibility and the score
//! breakdown: what gives the mating market a reason to exist (playtest 1:
//! zero trades, renown piling up with nothing to buy).
//!
//! - **Shop** ([`Game::shop_view`]): an extra directed-mating slot for the
//!   rest of the weather season, and reinforcements for a small lineage
//!   (prices and limits in [`crate::economy`]). Renown spent is destroyed.
//! - **Royalty** ([`Game::pay_royalties`]): renown a day for a player's
//!   alleles living in other players' lineages (studs that sired into
//!   rivals' dams; alleles in the wild don't pay).
//! - **Compatibility** ([`Game::compatibility`]): which lineages a player's
//!   can breed with at all (body plans 0.30 apart are sterile), shown per
//!   listing in the market too.
//! - **Score breakdown** ([`Game::score_breakdown`]): whose genes make up a
//!   legacy share, from the daily host tally (`World::legacy_hosts`).

use std::collections::{BTreeMap, BTreeSet};

use evo_sim::genome::Genome;
use evo_sim::mating::{fertility_gap, plan_weights, Relief, BODY_PLAN_GENES, BODY_PLAN_STERILE};
use evo_sim::{PlayerId, WILD};

use super::Game;
use crate::design::gene_key;
use crate::economy;
use crate::names;
use crate::protocol::{
    CompatibilityRow, CompatibilityView, CrowdingView, DesignCompatView, ErrorInfo, GeneHost, GeneSource, LedgerKind,
    ListingView, ScoreBreakdown, ShopView,
};

/// Another player's lineage in a compatibility check: tenths and no
/// limiting gene. Varying one gene at a time against exact gaps solved a
/// rival's body plan (playtest 4).
fn coarsen_rivals(rows: &mut [CompatibilityRow], player: PlayerId) {
    for r in rows.iter_mut().filter(|r| r.owner != player && r.owner != WILD) {
        let tenth = |x: f32| (x * 10.0).round() / 10.0;
        (r.fertility, r.fertility_low, r.fertility_high) =
            (tenth(r.fertility), tenth(r.fertility_low), tenth(r.fertility_high));
        (r.best_fertility, r.fertile_share, r.gap) = (tenth(r.best_fertility), tenth(r.fertile_share), tenth(r.gap));
        r.limiting = "body_plan".into();
    }
}

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

/// Margin (in body-plan gap) above the sterile line under which drift can
/// make a sterile pairing fertile within a few generations: founders vary
/// by about 0.05 in each expressed gene (`design::FOUNDER_VARIATION`) and
/// selection moves lineages' means by as much (playtest 2: a "safe" lineage
/// ended 52% wild).
pub const DRIFT_HIGH: f32 = 0.05;
/// Margin under which the drift risk is low rather than none.
pub const DRIFT_LOW: f32 = 0.12;

/// Fertility over every pair of two samples of individuals: what a real
/// mating between a random member of each is likely to give, not the best
/// pair (playtest 2: a "compatible" badge from the best pair, 13 of 17 real
/// pairs sterile).
#[derive(Clone, Debug, PartialEq)]
pub struct Spread {
    /// Mean fertility over the pairs (body plan with `Balance::egg_hybrid`,
    /// and whole-genome distance: `mating::fertility_with`).
    pub mean: f32,
    /// The 10th and 90th percentile pair.
    pub low: f32,
    pub high: f32,
    /// The best pair.
    pub best: f32,
    /// Share of pairs with any fertility.
    pub fertile_share: f32,
    /// The closest pair's body-plan gap (for fertility) and its widest gene.
    pub gap: f32,
    pub limiting: String,
    /// `none`, `low` or `high`: how likely drift is to make the two
    /// (more) fertile ([`DRIFT_HIGH`], [`DRIFT_LOW`]); `high` once any pair
    /// is fertile, since genes then flow.
    pub drift_risk: &'static str,
}

/// [`DesignCompatView::egg_warning`]'s text: warm-blooded eggs left alone
/// need 24 °C or more (`world::environment::egg_range`).
pub const EGG_WARNING: &str = "Warm-blooded eggs need a parent's heat: left alone they chill whenever the air is \
     below 24 °C, so abandoned eggs almost never hatch. Choose incubated eggs \
     (a parent broods them) or live birth, or make the design cold-blooded.";

/// Drought share of the land from which the design check warns a design
/// with no drought trait (`DesignCompatView::drought_warning`).
pub const DROUGHT_DESIGN_WARN: f32 = 0.4;
/// Estivation from which a design counts as drought-ready: above the gene's
/// strategy threshold (`evo_sim::stats::STRATEGY_THRESHOLD`, below which it
/// does nothing) with room for founder variation. Playtest 7: the advice
/// said 0.3, and a design with 0.4 never estivated.
pub const DROUGHT_TRAIT: f32 = evo_sim::stats::STRATEGY_THRESHOLD + 0.1;

/// [`DesignCompatView::drought_warning`]'s text.
pub fn drought_design_text(drought: f32) -> String {
    format!(
        "Drought: {:.0}% of the land is dry. Founders far from water can die of thirst within days, even inside \
         their grace. Keeping them by lasting water (the Food layer) is the surest answer, or wait for the drought \
         to ease. Estivation ({DROUGHT_TRAIT:.1} or more) lets them sleep through a real dry spell where no water is \
         near, but it has costs: a sleeping lineage doesn't breed, so a dry spring can cost a season's young, and \
         it wakes needing food.",
        drought * 100.0
    )
}

/// [`DesignCompatView::carnivore_egg_warning`]'s text.
pub const CARNIVORE_EGG_WARNING: &str = "An egg-laying hunter leaves its clutch to hunt, and unattended eggs chill or \
     are raided: expect several eggs lost for each one that hatches. Live birth suits a roaming carnivore; an egg layer \
     does better with a smaller range and a partner left at the nest.";

/// [`Spread`] over the pairs of `a` × `b` (None when either is empty), the
/// body-plan gap counted `scale` times (`mating::wild_gap_scale`: a player's
/// design against a wild lineage), each pair's fertility times `litter`
/// (`mating::wild_fertility`: the same pair's share of a litter).
pub fn spread(a: &[&Genome], b: &[&Genome], relief: Relief, scale: f32, litter: f32) -> Option<Spread> {
    let mut fert: Vec<f32> = Vec::with_capacity(a.len() * b.len());
    let mut closest: Option<(f32, &Genome, &Genome)> = None;
    for &x in a {
        for &y in b {
            let gap = fertility_gap(x, y, relief) * scale;
            fert.push(evo_sim::mating::fertility_scaled(x, y, relief, scale) * litter);
            if closest.is_none_or(|(g, _, _)| gap < g) {
                closest = Some((gap, x, y));
            }
        }
    }
    let (gap, x, y) = closest?;
    fert.sort_by(f32::total_cmp);
    let n = fert.len();
    let at = |q: f64| fert[((q * (n - 1) as f64).round() as usize).min(n - 1)];
    let fertile = fert.iter().filter(|&&f| f > 0.0).count();
    let weights = plan_weights(relief);
    let (gene, gene_gap) = BODY_PLAN_GENES
        .iter()
        .zip(weights)
        .map(|(&g, w)| (g, (x.expr(g) - y.expr(g)).abs() * w * scale))
        .fold((BODY_PLAN_GENES[0], -1.0f32), |m, v| if v.1 > m.1 { v } else { m });
    let limiting = if gap > gene_gap + 1e-6 { "morphology".to_string() } else { gene_key(gene) };
    let margin = gap - BODY_PLAN_STERILE;
    let drift_risk = if fertile > 0 || margin < DRIFT_HIGH {
        "high"
    } else if margin < DRIFT_LOW {
        "low"
    } else {
        "none"
    };
    Some(Spread {
        mean: fert.iter().sum::<f32>() / n as f32,
        low: at(0.1),
        high: at(0.9),
        best: fert[n - 1],
        fertile_share: fertile as f32 / n as f32,
        gap,
        limiting,
        drift_risk,
    })
}

/// Whether a spread counts as compatible: at least half the pairs fertile.
pub fn compatible(s: &Spread) -> bool {
    s.fertile_share >= 0.5
}

impl Game {
    /// The shop as `player` sees it now.
    pub(super) fn shop_view(&self, player: PlayerId) -> ShopView {
        let bought = self.world.bonus_slots(player);
        let reinforced = self.reinforcements_this_season(player);
        let max_alive = self.reinforce_max_alive();
        ShopView {
            refound_debt: self.economy.refound_debt.get(&player).copied().filter(|&d| d > 0),
            balance: self.balance(player),
            slots_left: self.world.mating_slots_left(player),
            slots_bought: bought,
            slot_price: economy::slot_price(bought),
            reinforce_size: economy::REINFORCE_SIZE,
            reinforce_price: self.reinforce_price_quote(player, reinforced),
            reinforce_max_alive: max_alive,
            reinforce_blocked: self.reinforce_blocked(player).err().map(|e| e.message),
            market_fee_percent: economy::MARKET_FEE_PERCENT,
            royalty_per_point: economy::ROYALTY_PER_POINT,
            reinforcements_bought: reinforced,
            reinforcements_max: economy::MAX_REINFORCEMENTS_PER_SEASON,
            nameplate_price: if self.economy.nameplates.contains_key(&player) {
                economy::NAMEPLATE_CHANGE_PRICE
            } else {
                economy::NAMEPLATE_PRICE
            },
            nameplate_colors: economy::NAMEPLATE_COLORS.iter().map(|c| c.to_string()).collect(),
            nameplate: self.economy.nameplates.get(&player).cloned(),
            species_name_price: economy::SPECIES_NAME_PRICE,
            provision_price: self
                .provision_terms(player)
                .map_or(economy::PROVISION_PRICE, |(_, n, per)| per * n.len() as u64),
            provision_size: self.provision_terms(player).map_or(economy::PROVISION_SIZE, |(_, n, _)| n.len()),
            provisions_bought: self.recent_count(self.economy.provision_days.get(&player)),
            provisions_max: economy::MAX_PROVISIONS_PER_SEASON,
            provision_blocked: self.provision_terms(player).err().map(|e| e.message),
            survey_price: economy::survey_price(self.owner_population(player)),
            restore_price: economy::restore_price(self.owner_population(player), self.restores_bought(player)),
            restores_bought: self.restores_bought(player),
            animal_name_price: economy::ANIMAL_NAME_PRICE,
            owed: self.economy.refound_debt.get(&player).copied().unwrap_or(0),
            next_refound_fee: {
                let (today, window) = (self.world.day(), self.refound_window_days());
                let n = self
                    .economy
                    .refound_days
                    .get(&player)
                    .map_or(0, |d| d.iter().filter(|&&x| x + window > today).count());
                crate::game::refound_fee(n as u32)
            },
            reinforce_next_day: self.reinforce_next(player).map(|x| x.0),
            reinforce_next_minutes: self.reinforce_next(player).map(|x| x.1),
        }
    }

    /// The shop as `player` sees it, remembering its prices that move with
    /// the head count (survey, restore pasture) so a purchase within
    /// [`QUOTE_SECONDS`] pays no more than quoted (playtest 9: quoted 1,169,
    /// charged 1,147, or the other way).
    pub(super) fn quote_shop(&mut self, player: PlayerId) -> ShopView {
        let view = self.shop_view(player);
        self.shop_quotes.insert(
            player,
            ShopQuote { tick: self.world.tick(), survey: view.survey_price, restore: view.restore_price },
        );
        view
    }

    /// `now`, or the quoted price if lower and quoted within
    /// [`QUOTE_SECONDS`].
    fn quoted(&self, player: PlayerId, now: u64, which: impl Fn(&ShopQuote) -> u64) -> u64 {
        let ticks = (QUOTE_SECONDS * 1000.0 / self.cfg.tick_ms.max(1.0)).ceil() as u64;
        self.shop_quotes
            .get(&player)
            .filter(|q| self.world.tick().saturating_sub(q.tick) <= ticks)
            .map_or(now, |q| now.min(which(q)))
    }

    /// Purchases in `days` within the last ranked season's length.
    fn recent_count(&self, days: Option<&Vec<u64>>) -> u32 {
        let (today, window) = (self.world.day(), self.cfg.season_days.max(1));
        days.map_or(0, |d| d.iter().filter(|&&day| day + window > today).count() as u32)
    }

    /// The price of the next reinforcement: now, or with the window full,
    /// on the day it frees a slot (playtest 8: a blocked purchase quoted
    /// 600, the price with every recent purchase still counted, and the
    /// next one cost 150).
    fn reinforce_price_quote(&self, player: PlayerId, reinforced: u32) -> u64 {
        let Some((day, _)) = self.reinforce_next(player) else { return economy::reinforce_price(reinforced) };
        let window = self.cfg.season_days.max(1);
        let then = self
            .economy
            .reinforce_days
            .get(&player)
            .map_or(0, |d| d.iter().filter(|&&x| x + window > day).count() as u32);
        economy::reinforce_price(then)
    }

    /// When the reinforcement window frees a slot, if it is full: the day
    /// and about how many real minutes from now.
    fn reinforce_next(&self, player: PlayerId) -> Option<(u64, u64)> {
        let days = self.economy.reinforce_days.get(&player)?;
        if self.recent_count(Some(days)) < economy::MAX_REINFORCEMENTS_PER_SEASON {
            return None;
        }
        let (today, window) = (self.world.day(), self.cfg.season_days.max(1));
        let oldest = days.iter().copied().filter(|&d| d + window > today).min()?;
        let day = oldest + window;
        let minutes =
            ((day - today) as f64 * self.world.config.ticks_per_day as f64 * self.cfg.tick_ms / 60_000.0).ceil();
        Some((day, minutes as u64))
    }

    /// What a provision for `player` would be now: their lineage, the
    /// adults it would feed (the neediest first) and the renown per adult;
    /// or why not.
    fn provision_terms(&self, player: PlayerId) -> Result<(u32, Vec<usize>, u64), ErrorInfo> {
        let Some(lineage) = self.lineage_of(player).filter(|_| !self.is_extinct(player)) else {
            return Err(ErrorInfo::new("no_lineage", "you have no living lineage to provision"));
        };
        if self.recent_count(self.economy.provision_days.get(&player)) >= economy::MAX_PROVISIONS_PER_SEASON {
            return Err(ErrorInfo::new(
                "limit_reached",
                format!(
                    "at most {} provisions within a ranked season's length ({} days)",
                    economy::MAX_PROVISIONS_PER_SEASON,
                    self.cfg.season_days
                ),
            ));
        }
        let max = self.reinforce_max_alive().max(1);
        let alive = self.owner_population(player);
        let (size, per) = if alive <= max {
            (economy::PROVISION_SIZE, economy::PROVISION_PER_ANIMAL)
        } else {
            let blocks = alive.div_ceil(max);
            ((economy::PROVISION_SIZE * blocks).min(economy::PROVISION_MAX_SIZE), economy::PROVISION_PER_ANIMAL_LARGE)
        };
        let mut needy: Vec<usize> = (0..self.world.organisms.len())
            .filter(|&i| {
                let o = &self.world.organisms[i];
                o.alive && o.lineage == lineage && o.matured
            })
            .collect();
        if needy.is_empty() {
            return Err(ErrorInfo::new("no_adults", "your lineage has no adult to provision"));
        }
        let need = |i: usize| {
            let n = &self.world.organisms[i].needs;
            n.hunger.max(n.thirst)
        };
        needy.sort_by(|&a, &b| need(b).total_cmp(&need(a)).then(a.cmp(&b)));
        needy.truncate(size);
        Ok((lineage, needy, per))
    }

    /// `provision`: the lineage's hungriest or thirstiest adults are fed and
    /// watered a little, priced per adult fed. The number fed and the shop
    /// after.
    pub(super) fn provision(&mut self, player: PlayerId) -> Result<(usize, ShopView), ErrorInfo> {
        let (_, needy, per) = self.provision_terms(player)?;
        let price = per * needy.len() as u64;
        self.spend(player, price, format!("Provision: {} adults fed and watered ({per} renown each)", needy.len()))?;
        for &i in &needy {
            let n = &mut self.world.organisms[i].needs;
            n.hunger = (n.hunger - economy::PROVISION_AMOUNT).max(0.0);
            n.thirst = (n.thirst - economy::PROVISION_AMOUNT).max(0.0);
        }
        let (today, window) = (self.world.day(), self.cfg.season_days.max(1));
        let days = self.economy.provision_days.entry(player).or_default();
        days.retain(|&d| d + window > today);
        days.push(today);
        Ok((needy.len(), self.shop_view(player)))
    }

    /// Pasture restorations `player` bought within the last ranked
    /// season's length.
    fn restores_bought(&self, player: PlayerId) -> u32 {
        self.recent_count(self.economy.restore_days.get(&player))
    }

    /// `restore_pasture`: the plants within [`economy::RESTORE_RADIUS`] of
    /// one of the player's groups regrow to what the season allows
    /// (`World::restore_pasture`), for renown that scales with the largest
    /// lineage with animals in the circle (the buyer's or another's: no
    /// cheap purchase by a small ally on a leader's ground) and doubles with
    /// each restoration in the window. With `preview`, only what it would
    /// add and cost. Nothing to regrow (midwinter, a drought, already
    /// grown) is refused, not charged.
    pub(super) fn restore_pasture(
        &mut self,
        player: PlayerId,
        group: u8,
        preview: bool,
    ) -> Result<crate::protocol::RestoreView, ErrorInfo> {
        if self.lineage_of(player).is_none() || self.is_extinct(player) {
            return Err(ErrorInfo::new("no_lineage", "you have no living lineage to restore pasture for"));
        }
        let Some(at) = self.world.group_middle(player, group) else {
            return Err(ErrorInfo::new("no_such_group", format!("your group {group} has no animals")));
        };
        let r = economy::RESTORE_RADIUS;
        // Every player with animals in the circle benefits: the price follows
        // the largest of their lineages.
        let mut benefit: BTreeMap<PlayerId, usize> = BTreeMap::new();
        for o in self.world.organisms.iter().filter(|o| o.alive && o.owner != WILD && o.pos.dist(at) <= r) {
            *benefit.entry(o.owner).or_default() += 1;
        }
        let (priced_by, size) = benefit
            .keys()
            .map(|&p| (p, self.owner_population(p)))
            .chain(std::iter::once((player, self.owner_population(player))))
            .max_by_key(|&(p, n)| (n, p == player))
            .unwrap();
        let now = economy::restore_price(size, self.restores_bought(player));
        let price = self.quoted(player, now, |q| q.restore);
        let food = self.world.restore_pasture(at, r, false);
        let others: Vec<String> =
            benefit.iter().filter(|(&p, _)| p != player).map(|(&p, &n)| format!("{} ({n})", self.name(p))).collect();
        let mut view = crate::protocol::RestoreView {
            group,
            x: at.x.round(),
            y: at.y.round(),
            radius: r,
            restored: food.round(),
            unit: "food".into(),
            price,
            priced_by: (priced_by != player).then(|| self.name(priced_by)),
            also_feeds: others,
            preview,
            shop: None,
        };
        if preview {
            view.shop = Some(self.shop_view(player));
            return Ok(view);
        }
        if food < 1.0 {
            return Err(ErrorInfo::new(
                "nothing_to_restore",
                "nothing would regrow there now (midwinter, a drought, or the plants are already as high as the season allows)",
            ));
        }
        self.spend(
            player,
            price,
            format!("Pasture restored within {r} tiles of group {group}: about {:.0} food", food),
        )?;
        view.restored = self.world.restore_pasture(at, r, true).round();
        let (today, window) = (self.world.day(), self.cfg.season_days.max(1));
        let days = self.economy.restore_days.entry(player).or_default();
        days.retain(|&d| d + window > today);
        days.push(today);
        view.shop = Some(self.shop_view(player));
        Ok(view)
    }

    /// `survey`: rivals' groups and territories near the player's main
    /// group (or its biggest group when the main one is empty), coarsely, for renown scaled by their lineage's size.
    pub(super) fn survey(&mut self, player: PlayerId) -> Result<crate::protocol::SurveyView, ErrorInfo> {
        if self.lineage_of(player).is_none() || self.is_extinct(player) {
            return Err(ErrorInfo::new("no_lineage", "you have no living lineage to survey from"));
        }
        // From the main group, else the biggest group with animals
        // (playtest 8: refused with 88 alive because the main group was
        // empty).
        let from = self.world.group_middle(player, 0).or_else(|| {
            let groups = self.world.groups(player);
            let big = groups.iter().filter(|g| g.members > 0).max_by_key(|g| (g.members, std::cmp::Reverse(g.id)))?;
            self.world.group_middle(player, big.id)
        });
        let Some(from) = from else {
            return Err(ErrorInfo::new("no_lineage", "none of your groups has animals to survey from"));
        };
        let price = self.quoted(player, economy::survey_price(self.owner_population(player)), |q| q.survey);
        let coarse = |v: f32| (v / 8.0).floor() * 8.0 + 4.0;
        let mut groups = Vec::new();
        let rivals: Vec<PlayerId> = self.names.keys().copied().filter(|&p| p != player && p != WILD).collect();
        for p in rivals {
            for g in self.world.groups(p) {
                let Some(mid) = self.world.group_middle(p, g.id) else { continue };
                if mid.dist(from) > economy::SURVEY_RADIUS || g.members == 0 {
                    continue;
                }
                groups.push(crate::protocol::SurveyGroup {
                    player: p,
                    name: self.name(p),
                    x: coarse(mid.x),
                    y: coarse(mid.y),
                    members: g.members.div_ceil(10) * 10,
                    // Or the one it holds while away (playtest 8: left out).
                    territory: g
                        .territory
                        .or_else(|| self.world.territory_away(p, g.id))
                        .map(|t| [t.center.x.round(), t.center.y.round(), t.radius.round()]),
                });
            }
        }
        self.spend(
            player,
            price,
            format!(
                "Survey: {} rival group{} within {} tiles",
                groups.len(),
                if groups.len() == 1 { "" } else { "s" },
                economy::SURVEY_RADIUS
            ),
        )?;
        Ok(crate::protocol::SurveyView {
            price,
            x: coarse(from.x),
            y: coarse(from.y),
            radius: economy::SURVEY_RADIUS,
            groups,
            shop: self.shop_view(player),
        })
    }

    /// `name_animal`: a cosmetic name for one of the player's living
    /// animals (the species-name filter; not another player's name).
    pub(super) fn name_animal(
        &mut self,
        player: PlayerId,
        organism: crate::protocol::OrgId,
        name: &str,
    ) -> Result<(String, ShopView), ErrorInfo> {
        let name = names::check(name, crate::design::MAX_NAME_LEN, names::Kind::Species)
            .map_err(|why| ErrorInfo::new("bad_name", format!("that name can't be used: {why}")))?;
        if self.impersonates(player, &name, names::Kind::Species) {
            return Err(ErrorInfo::new("bad_name", "an animal's name can't look like another player's name"));
        }
        // Nor a wild species' or a rival's lineage name (playtest 8: a stud
        // listed under a rival's lineage name).
        if self.wild_name(&name) {
            return Err(ErrorInfo::new("bad_name", "that name can't be used: it is a wild species' name"));
        }
        if self.rival_name(player, &name) || self.holds_rival_name(player, &name) {
            return Err(ErrorInfo::new("name_taken", "another player's lineage or species has that name"));
        }
        match self.world.organism(organism) {
            Some(o) if o.owner == player => {}
            Some(_) => return Err(ErrorInfo::new("not_owned", format!("#{organism} is not yours"))),
            None => return Err(ErrorInfo::new("not_found", format!("no living organism #{organism}"))),
        }
        if self.economy.animal_names.get(&organism) == Some(&name) {
            return Err(ErrorInfo::new("same_name", format!("#{organism} is already called {name}")));
        }
        self.spend(player, economy::ANIMAL_NAME_PRICE, format!("Named #{organism} \"{name}\""))?;
        // Names of animals no longer alive are dropped as names are given.
        let world = &self.world;
        self.economy.animal_names.retain(|&id, _| world.organism(id).is_some());
        self.economy.animal_names.insert(organism, name.clone());
        Ok((name, self.shop_view(player)))
    }

    /// `set_nameplate`.
    pub(super) fn set_nameplate(
        &mut self,
        player: PlayerId,
        color: &str,
        title: &str,
    ) -> Result<(economy::Nameplate, ShopView), ErrorInfo> {
        if !economy::NAMEPLATE_COLORS.contains(&color) {
            return Err(ErrorInfo::new(
                "bad_color",
                format!("the palette is {}", economy::NAMEPLATE_COLORS.join(", ")),
            ));
        }
        let title = names::check(title, economy::MAX_TITLE_LEN, names::Kind::Title)
            .map_err(|why| ErrorInfo::new("bad_title", format!("that title can't be used: {why}")))?;
        // Nor another player's name, or one inside it (it shows beside the
        // owner's: "Herdmaster4 (alt)").
        if self.wild_name(&title) {
            return Err(ErrorInfo::new("bad_title", "that title can't be used: it is a wild species' name"));
        }
        if self.impersonates(player, &title, names::Kind::Title) {
            return Err(ErrorInfo::new("bad_title", "that title can't be used: it looks like another player's name"));
        }
        if self.rival_name(player, &title) || self.holds_rival_name(player, &title) {
            return Err(ErrorInfo::new("bad_title", "that title can't be used: it is another player's lineage name"));
        }
        let price = if self.economy.nameplates.contains_key(&player) {
            economy::NAMEPLATE_CHANGE_PRICE
        } else {
            economy::NAMEPLATE_PRICE
        };
        self.spend(player, price, format!("Nameplate: {color} \"{title}\""))?;
        let plate = economy::Nameplate { color: color.to_string(), title };
        self.economy.nameplates.insert(player, plate.clone());
        Ok((plate, self.shop_view(player)))
    }

    /// `name_species`: rename a living species `player` founded.
    pub(super) fn name_species(&mut self, player: PlayerId, species: u32, name: &str) -> Result<String, ErrorInfo> {
        let name = names::check(name, crate::design::MAX_NAME_LEN, names::Kind::Species)
            .map_err(|why| ErrorInfo::new("bad_name", format!("that species name can't be used: {why}")))?;
        if self.wild_name(&name) {
            return Err(ErrorInfo::new("bad_name", "that species name can't be used: it is a wild species' name"));
        }
        if self.rival_name(player, &name) {
            return Err(ErrorInfo::new("name_taken", "another player's lineage or species has that name"));
        }
        if self.impersonates(player, &name, names::Kind::Species) {
            return Err(ErrorInfo::new("bad_name", "a species name can't look like another player's name"));
        }
        let tracker = &self.world.chronicle.species;
        let s =
            tracker.get(species).ok_or_else(|| ErrorInfo::new("unknown_species", format!("no species {species}")))?;
        if s.extinct_day.is_some() || s.merged_into.is_some() {
            return Err(ErrorInfo::new("species_gone", "that species is extinct or merged into another"));
        }
        if self.species_namer(s) != Some(player) {
            return Err(ErrorInfo::new(
                "not_yours",
                "only a species your lineage founded (or split off) can be named by you",
            ));
        }
        // Named once: the name stands (the World tab no longer offers it).
        if let Some(&by) = self.economy.named_species.get(&species) {
            return Err(ErrorInfo::new("already_named", format!("{} has already named this species", self.name(by))));
        }
        // Its own name again would charge for nothing (playtest 5: 300).
        if names::fold(&s.name) == names::fold(&name) {
            return Err(ErrorInfo::new("same_name", format!("the species is already called {}", s.name)));
        }
        // Look-alikes count as taken, as for player names.
        if tracker.species.iter().any(|o| o.id != species && names::fold(&o.name) == names::fold(&name)) {
            return Err(ErrorInfo::new("name_taken", "another species has that name"));
        }
        self.spend(player, economy::SPECIES_NAME_PRICE, format!("Named species {species} \"{name}\""))?;
        if let Some(s) = self.world.chronicle.species.species.iter_mut().find(|s| s.id == species) {
            s.name = name.clone();
        }
        self.economy.named_species.insert(species, player);
        Ok(name)
    }

    /// Whether `text` looks like another player's name (`names::impersonates`).
    pub(crate) fn impersonates(&self, player: PlayerId, text: &str, kind: names::Kind) -> bool {
        names::impersonates(text, self.names.iter().filter(|(&p, _)| p != player).map(|(_, n)| n.as_str()), kind)
    }

    /// Whether `text` is a wild species' name (a wild lineage's, or a species
    /// the wild dominates), folded, a plural `s` either way: a player's
    /// lineage "Meadow vole" would found a second species of that name
    /// (playtest 6).
    pub(crate) fn wild_name(&self, text: &str) -> bool {
        // Articles, digits and decorations don't make it another name ("The
        // Meadow vole", "Meadow vole 2", "Meadow vole II", "Red Foxes":
        // playtests 7 and 8).
        let wild_lineages = self.world.lineages.iter().filter(|l| l.owner == WILD).map(|l| l.name.as_str());
        let wild_species =
            (self.world.chronicle.species.species.iter()).filter(|s| s.dominant == WILD).map(|s| s.name.as_str());
        wild_lineages.chain(wild_species).any(|n| names::copies(text, n))
    }

    /// Whether `text` copies the name of another player's living lineage or
    /// species (`names::copies`: "Sunbright Drove II", "New Sunbright Drove"
    /// too): playtest 7 founded a second "Ash Fang Pack" beside its owner's.
    pub(crate) fn rival_name(&self, player: PlayerId, text: &str) -> bool {
        // At least 4 letters left: "L1" and "L2" aren't copies (`copies`).
        let counts = self.world.lineage_counts();
        let lineages = (self.world.lineages.iter().enumerate())
            .filter(|(l, x)| x.owner != WILD && x.owner != player && counts.get(*l).is_some_and(|c| c.alive > 0))
            .map(|(_, x)| x.name.as_str());
        let species = (self.world.chronicle.species.species.iter())
            .filter(|s| s.dominant != WILD && s.dominant != player && s.is_alive())
            .map(|s| s.name.as_str());
        lineages.chain(species).any(|n| names::copies(text, n))
    }

    /// Whether `text` (a nameplate title or an animal's name) holds another
    /// player's living lineage's name, its decorations dropped, anywhere
    /// (`names::holds_name`: "Ironfang chief" for "Ironfang Pack", playtest
    /// 10).
    pub(crate) fn holds_rival_name(&self, player: PlayerId, text: &str) -> bool {
        let counts = self.world.lineage_counts();
        (self.world.lineages.iter().enumerate())
            .filter(|(l, x)| x.owner != WILD && x.owner != player && counts.get(*l).is_some_and(|c| c.alive > 0))
            .any(|(_, x)| names::holds_name(text, &x.name))
    }

    /// The names players chose that others see: nameplate titles, animal
    /// names, players' current lineage names and player-named species,
    /// sorted (`runner::Published::public_names`).
    pub fn public_names(&self) -> Vec<String> {
        // Each player's current lineage (no count of the living each tick).
        let lineages =
            (self.lineage_of.values()).filter_map(|&l| self.world.lineages.get(l as usize)).map(|x| x.name.clone());
        let species = (self.world.chronicle.species.species.iter())
            .filter(|s| self.economy.named_species.contains_key(&s.id))
            .map(|s| s.name.clone());
        let mut v: Vec<String> = (self.economy.nameplates.values().map(|n| n.title.clone()))
            .chain(self.economy.animal_names.values().cloned())
            .chain(lineages)
            .chain(species)
            .collect();
        v.sort();
        v.dedup();
        v
    }

    /// Who may name species `s`: the player credited with a split, or for a
    /// species a founding started (`founding`, or `initial` when players'
    /// lineages were there at the first detection) the owner of its main
    /// lineage. Nobody for a wild species or a gone one.
    pub(crate) fn species_namer(&self, s: &evo_sim::species::Species) -> Option<PlayerId> {
        if !s.is_alive() || s.merged_into.is_some() {
            return None;
        }
        // Named once: its namer stays whoever named it (playtest 5: the
        // namer followed the largest lineage even after naming).
        if let Some(&by) = self.economy.named_species.get(&s.id) {
            return Some(by);
        }
        if s.founder.is_some() {
            return s.founder;
        }
        // An uncredited split (`Balance::speciation_credit`: a copy, a young
        // or small split) is still its own lineage's to name, once.
        if s.origin == evo_sim::species::SpeciesOrigin::Split && self.world.config.balance.speciation_credit <= 0.0 {
            return None;
        }
        // An initial species that holds a wild lineage is the wild one's:
        // nobody renames it for everyone (playtest 4: the leading player
        // renamed the Steppe antelope).
        let wild_lineage = |l: u32| self.world.lineages.get(l as usize).is_some_and(|x| x.owner == evo_sim::WILD);
        if s.origin == evo_sim::species::SpeciesOrigin::Initial && s.lineages.iter().any(|&(l, _)| wild_lineage(l)) {
            return None;
        }
        let owner = self.world.lineages.get(s.main_lineage()? as usize)?.owner;
        (owner != evo_sim::WILD).then_some(owner)
    }

    /// Every bought nameplate.
    pub(super) fn nameplates(&self) -> serde_json::Value {
        let map: BTreeMap<String, &economy::Nameplate> =
            self.economy.nameplates.iter().map(|(p, n)| (p.to_string(), n)).collect();
        serde_json::json!({ "nameplates": map })
    }

    /// Most living members a reinforced lineage may have.
    pub(super) fn reinforce_max_alive(&self) -> usize {
        (self.world.config.lineage_cap as f64 * economy::REINFORCE_MAX_SHARE).floor() as usize
    }

    /// The most `player` had alive at a daily record within the last
    /// ranked season's length of simulated days (0 when none recorded).
    pub(super) fn season_peak(&self, player: PlayerId) -> usize {
        let (today, window) = (self.world.day(), self.cfg.season_days.max(1));
        self.economy
            .alive_days
            .get(&player)
            .map_or(0, |d| d.iter().filter(|&&(day, _)| day + window > today).map(|&(_, n)| n).max().unwrap_or(0))
    }

    /// Note each player's living count for the day (`season_peak`), and
    /// drop the days past the window.
    pub(super) fn note_alive(&mut self, day: u64) {
        let window = self.cfg.season_days.max(1);
        let players: BTreeSet<PlayerId> = self.world.lineages.iter().map(|l| l.owner).filter(|&p| p != WILD).collect();
        for p in players {
            let alive = self.owner_population(p);
            let days = self.economy.alive_days.entry(p).or_default();
            while days.front().is_some_and(|&(d, _)| d + window <= day) {
                days.pop_front();
            }
            if alive > 0 || !days.is_empty() {
                days.push_back((day, alive));
            }
        }
        self.economy.alive_days.retain(|_, d| !d.is_empty());
    }

    /// Reinforcements `player` bought within the last ranked season's
    /// length of simulated days (a rolling window).
    fn reinforcements_this_season(&self, player: PlayerId) -> u32 {
        let (today, window) = (self.world.day(), self.cfg.season_days.max(1));
        self.economy
            .reinforce_days
            .get(&player)
            .map_or(0, |d| d.iter().filter(|&&day| day + window > today).count() as u32)
    }

    /// Why `player` can't buy reinforcements now (their lineage otherwise).
    fn reinforce_blocked(&self, player: PlayerId) -> Result<u32, ErrorInfo> {
        let Some(lineage) = self.lineage_of(player).filter(|_| !self.is_extinct(player)) else {
            return Err(ErrorInfo::new("no_lineage", "you have no living lineage to reinforce"));
        };
        let bought = self.reinforcements_this_season(player);
        if bought >= economy::MAX_REINFORCEMENTS_PER_SEASON {
            return Err(ErrorInfo::new(
                "limit_reached",
                // The wait, not a day number (playtest 9: "day 549" beside
                // `reinforce_next_day` 548 read as off by one); and the size
                // limit that also applies.
                format!(
                    "at most {} reinforcements within a ranked season's length ({} days; retired lineages count), and \
                     only for lineages of at most {} animals{}",
                    economy::MAX_REINFORCEMENTS_PER_SEASON,
                    self.cfg.season_days,
                    self.reinforce_max_alive(),
                    self.reinforce_next(player).map_or(String::new(), |(d, m)| format!(
                        ": the next is possible in {} game day{} (about {m} real minutes)",
                        d.saturating_sub(self.world.day()),
                        if d.saturating_sub(self.world.day()) == 1 { "" } else { "s" }
                    ))
                ),
            ));
        }
        let alive = self.owner_population(player);
        let max = self.reinforce_max_alive();
        if alive > max {
            return Err(ErrorInfo::new(
                "too_large",
                format!("reinforcements are for lineages of at most {max} (yours has {alive})"),
            ));
        }
        let peak = self.season_peak(player);
        if peak > max {
            return Err(ErrorInfo::new(
                "too_large",
                format!(
                    "reinforcements are for lineages of at most {max} alive at any time in the last {} days \
                     (yours had {peak})",
                    self.cfg.season_days
                ),
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
        // Playtest 8: slots were sold to a player whose lineage had retired
        // (they lapse with the weather season, unused).
        if self.is_extinct(player) {
            return Err(ErrorInfo::new("no_lineage", "your lineage has no living animals: found a new one first"));
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
    pub(super) fn reinforce(&mut self, player: PlayerId) -> Result<(Vec<u64>, ShopView, ReinforceSite), ErrorInfo> {
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
        // Where they landed, and how many of the lineage stand around them.
        let landed: Vec<evo_sim::math::Vec2> =
            ids.iter().filter_map(|&id| self.world.organism(id)).map(|o| o.pos).collect();
        let n_landed = landed.len().max(1) as f32;
        let at = landed.iter().fold(evo_sim::math::Vec2::ZERO, |a, &p| a + p) * (1.0 / n_landed);
        let nearby = self
            .world
            .organisms
            .iter()
            .filter(|o| o.alive && o.lineage == lineage && !ids.contains(&o.id))
            .filter(|o| o.pos.dist(at) < evo_sim::World::REINFORCE_CLUSTER)
            .count();
        let group = ids.first().and_then(|&id| self.world.organism(id)).map_or(0, |o| o.group);
        let site = ReinforceSite { at: [at.x, at.y], nearby_count: nearby, group };
        self.spend(
            player,
            price,
            format!(
                "Reinforcements: {} adults joined your lineage at ({:.0}, {:.0}), beside {nearby} of its animals",
                ids.len(),
                at.x,
                at.y
            ),
        )?;
        let (today, window) = (self.world.day(), self.cfg.season_days.max(1));
        let days = self.economy.reinforce_days.entry(player).or_default();
        days.retain(|&d| d + window > today);
        days.push(today);
        let _ = n;
        // New organisms: the next state build indexes the world again.
        self.state_cache.invalidate();
        Ok((ids, self.shop_view(player), site))
    }

    /// One row per other lineage with living adults: `own` against a
    /// sample of its adults, most fertile first.
    fn compat_rows(&self, own: &[&Genome], skip: Option<u32>) -> Vec<CompatibilityRow> {
        let relief = Relief::of(&self.world.config.balance);
        let mut rows: Vec<CompatibilityRow> = (0..self.world.lineages.len() as u32)
            .filter(|&l| Some(l) != skip)
            .filter_map(|l| {
                let theirs = sample_of(self, l);
                let lin = &self.world.lineages[l as usize];
                // `own` is a player's: against a wild lineage the gap counts more.
                let scale = evo_sim::mating::wild_gap_scale(&self.world.config.balance, 1, lin.owner);
                let litter = evo_sim::mating::wild_fertility(&self.world.config.balance, 1, lin.owner);
                let s = spread(own, &theirs, relief, scale, litter)?;
                Some(CompatibilityRow {
                    lineage: l,
                    lineage_name: lin.name.clone(),
                    owner: lin.owner,
                    owner_name: if lin.owner == WILD { "wild".into() } else { self.name(lin.owner) },
                    wild: lin.owner == WILD,
                    compatible: compatible(&s),
                    fertility: s.mean,
                    fertility_low: s.low,
                    fertility_high: s.high,
                    best_fertility: s.best,
                    fertile_share: s.fertile_share,
                    drift_risk: s.drift_risk.to_string(),
                    limiting: s.limiting,
                    gap: s.gap,
                })
            })
            .collect();
        rows.sort_by(|a, b| b.fertility.total_cmp(&a.fertility).then(a.gap.total_cmp(&b.gap)));
        rows
    }

    /// `compatibility`: every other lineage with living adults, most
    /// fertile first, from samples of individuals on both sides.
    pub(super) fn compatibility(&self, player: PlayerId) -> CompatibilityView {
        let Some(mine) = self.lineage_of(player).filter(|_| !self.is_extinct(player)) else {
            return CompatibilityView { lineage: None, rows: Vec::new() };
        };
        let own = sample_of(self, mine);
        if own.is_empty() {
            return CompatibilityView { lineage: None, rows: Vec::new() };
        }
        let mut rows = self.compat_rows(&own, Some(mine));
        // Like the design check: the `compatibility` command gave rivals'
        // numbers exactly, with the limiting gene (playtest 6).
        coarsen_rivals(&mut rows, player);
        CompatibilityView { lineage: Some(mine), rows }
    }

    /// The design-time check (`POST /api/design/compatibility`): founders
    /// as `design` would make them (founder variation included) against
    /// each living lineage's adults, with the drift risk. The player's own
    /// lineage is compared too (a re-founding meets it) and marked by owner.
    pub fn design_compatibility(&self, player: PlayerId, design: &evo_sim::design::FounderDesign) -> DesignCompatView {
        let mut rng = evo_sim::rng::rng_for(self.world.config.seed, 0x00de_5167, player as u64, self.world.tick());
        let founders: Vec<Genome> = (0..SAMPLE).map(|_| design.founder_genome(player, &mut rng)).collect();
        let own: Vec<&Genome> = founders.iter().collect();
        let mut rows = self.compat_rows(&own, None);
        coarsen_rivals(&mut rows, player);
        let fertile_with = rows.iter().filter(|r| r.fertile_share > 0.0).count();
        let at_risk = rows.iter().filter(|r| r.fertile_share <= 0.0 && r.drift_risk != "none").count();
        // Shores the founders can drink from (steep banks stop non-climbers).
        let probe = evo_sim::organism::Organism::new(
            0,
            player,
            founders[0].clone(),
            evo_sim::organism::Sex::Female,
            evo_sim::math::Vec2::ZERO,
        );
        let (reachable, all) = self.world.shore_access(&probe.traits.body);
        let shore_access = (all > 0).then(|| reachable as f32 / all as f32);
        let water_warning = shore_access.filter(|&s| s < 0.5).map(|s| {
            format!(
                "Steep banks: these founders can drink from only {:.0}% of today's shores. Animals that can't climb \
                 (climbing below 0.4) can't get down a steep bank and die of thirst beside the water; more climbing \
                 (or swimming, for deep water) reaches the rest.",
                s * 100.0
            )
        });
        let drought = self.world.map.drought;
        let drought_warning = (drought >= DROUGHT_DESIGN_WARN
            && design.values()[evo_sim::Gene::Estivation.index()] < DROUGHT_TRAIT)
            .then(|| drought_design_text(drought));
        let carnivore_egg_warning = (design.birth == evo_sim::BirthType::IncubatedEggs
            && design.values()[evo_sim::Gene::Carnivory.index()] >= 0.5)
            .then(|| CARNIVORE_EGG_WARNING.to_string());
        let egg_warning = (design.thermo == evo_sim::Thermo::Warm && design.birth == evo_sim::BirthType::AbandonedEggs)
            .then(|| EGG_WARNING.to_string());
        DesignCompatView {
            day: self.world.day(),
            sample: SAMPLE,
            fertile_with,
            at_risk,
            rows,
            shore_access,
            water_warning,
            egg_warning,
            drought_warning,
            carnivore_egg_warning,
            refound: self.refound_terms(player).map(|t| {
                let n = t.founders(design);
                crate::protocol::RefoundView {
                    founders: n,
                    requested: design.founders,
                    cost: crate::game::RefoundTerms::cost(design, n),
                    until_day: t.until_day,
                    minutes_left: t.minutes_left,
                    fee: t.fee,
                    fee_now: t.fee.min(self.balance(player)),
                    fee_owed: t.fee.saturating_sub(self.balance(player)),
                    reason: t.reason,
                }
            }),
            // With the founders that would land (a re-founding gives fewer).
            grace_note: self
                .grace_preview(player, self.refound_terms(player).map_or(design.founders, |t| t.founders(design))),
            name_warning: if self.impersonates(player, &design.name, names::Kind::Lineage) {
                Some("that lineage name looks like another player's name: founding would refuse it".to_string())
            } else if self.wild_name(&design.name) {
                Some("that lineage name is a wild species' name: founding would refuse it".to_string())
            } else if self.rival_name(player, &design.name) {
                Some("another player's lineage or species has that name: founding would refuse it".to_string())
            } else {
                None
            },
        }
    }

    /// Fill each listing's `compatible` and `body_plan_fertility` for
    /// `player` (nothing when they have no living adult): a sample of
    /// their adults against the listed animal.
    pub(super) fn annotate_compatibility(&self, player: PlayerId, views: &mut [ListingView]) {
        let Some(mine) = self.lineage_of(player) else { return };
        let own = sample_of(self, mine);
        if own.is_empty() {
            return;
        }
        let relief = Relief::of(&self.world.config.balance);
        for v in views {
            let Some(o) = self.world.organism(v.organism) else { continue };
            let scale = evo_sim::mating::wild_gap_scale(&self.world.config.balance, player, o.owner);
            let litter = evo_sim::mating::wild_fertility(&self.world.config.balance, player, o.owner);
            if let Some(s) = spread(&own, &[&*o.genome], relief, scale, litter) {
                v.compatible = Some(compatible(&s));
                // Tenths: exact means over a rival's animal solved its body
                // plan (playtest 8).
                v.body_plan_fertility = Some((s.mean * 10.0).round() / 10.0);
            }
        }
    }

    /// The host tally of the day's legacy snapshot (`World::legacy_hosts`),
    /// taken with the daily record.
    pub(super) fn refresh_hosts(&mut self) {
        self.hosts = self.world.legacy_hosts();
    }

    /// `player`'s alleles living in other players' lineages, over all
    /// alleles: what the royalty pays for. Alleles in the wild (a design
    /// that hybridised with a wild species) don't count: they came from
    /// no trade, and at 5 renown a point they paid every lineage about as
    /// much as its income (examples/competition.rs).
    fn traded_share(&self, player: PlayerId) -> f64 {
        let total: f64 = self.hosts.values().sum();
        if total <= 0.0 {
            return 0.0;
        }
        let home = self.lineage_of(player);
        let owner = |l: u32| self.world.lineages.get(l as usize).map_or(WILD, |x| x.owner);
        let out: f64 = self
            .hosts
            .iter()
            .filter(|(&(l, p), _)| p == player && Some(l) != home && owner(l) != WILD)
            .map(|(_, &n)| n)
            .sum::<f64>()
            + 0.0; // an empty sum is -0.0
        out / total
    }

    /// The daily breeder's royalty (`economy::royalty`).
    pub(super) fn pay_royalties(&mut self, day: u64) {
        let players: Vec<(PlayerId, u32)> = self.lineage_of.iter().map(|(&p, &l)| (p, l)).collect();
        for (p, _) in players {
            let exported = self.traded_share(p);
            let pay = economy::royalty(exported, !self.is_extinct(p));
            if pay > 0 {
                let text = format!(
                    "Day {} royalty: {:.2}% of all alleles are yours and live in other players' lineages",
                    // The day just ended, counted from 1, as income names it.
                    day.max(1),
                    exported * 100.0
                );
                let _ = self.post_on(p, pay as i64, LedgerKind::Royalty, text, None, None, day.saturating_sub(1));
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
            traded_share: self.traded_share(player),
            royalty: economy::royalty(self.traded_share(player), home.is_some() && !self.is_extinct(player)),
        })
    }

    /// The soft lineage cap as it stands for `player` (`lineage_stats.crowding`).
    pub(super) fn crowding_view(&self, player: PlayerId) -> Option<CrowdingView> {
        self.lineage_of(player)?;
        let counts = self.world.crowding_counts();
        let count = counts.get(&player).copied().unwrap_or(0.0);
        let total: f32 = counts.values().sum();
        let [litter, hunger] = self.world.crowding_effect_in(count, total);
        let soft = self.world.config.lineage_cap as f32;
        let share = self.world.config.balance.world_crowding;
        let world_cap = Some(share * total.max(soft)).filter(|&c| share > 0.0 && c < soft);
        Some(CrowdingView {
            soft_cap: self.world.config.lineage_cap,
            hard_cap: self.world.player_ceiling(),
            count,
            juvenile_weight: self.world.config.balance.crowding_juveniles,
            litter,
            hunger,
            world_cap,
            world_share: share,
        })
    }
}

/// Where a purchase's reinforcements landed: their centre, how many of the
/// lineage's animals stand within `World::REINFORCE_CLUSTER` tiles of it,
/// and the group they joined (playtest 6: the reply's `group` was the count).
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub(super) struct ReinforceSite {
    pub at: [f32; 2],
    pub nearby_count: usize,
    pub group: u8,
}

/// Real seconds a shop price quoted to a player holds (`Game::quote_shop`).
pub const QUOTE_SECONDS: f64 = 10.0;

/// The head-count prices last shown to a player (`Game::quote_shop`).
#[derive(Clone, Copy, Debug)]
pub(super) struct ShopQuote {
    pub tick: u64,
    pub survey: u64,
    pub restore: u64,
}
