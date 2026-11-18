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

use std::collections::BTreeMap;

use evo_sim::genome::Genome;
use evo_sim::mating::{fertility_gap, fertility_with, plan_weights, BODY_PLAN_GENES, BODY_PLAN_STERILE};
use evo_sim::{PlayerId, WILD};

use super::Game;
use crate::design::gene_key;
use crate::economy;
use crate::protocol::{
    CompatibilityRow, CompatibilityView, CrowdingView, DesignCompatView, ErrorInfo, GeneHost, GeneSource, LedgerKind,
    ListingView, ScoreBreakdown, ShopView,
};

/// `text` trimmed, if it has 1 to `max` characters and no control
/// characters.
fn clean_text(text: &str, max: usize) -> Option<String> {
    let t = text.trim();
    let n = t.chars().count();
    (n >= 1 && n <= max && !t.chars().any(char::is_control)).then(|| t.to_string())
}

/// Punctuation a title or species name may use besides letters, digits and
/// single spaces: nothing that reads as markup (`<`, `>`, `&`, quotes,
/// slashes, brackets).
const DISPLAY_PUNCTUATION: &str = ".,'!?-:+#()";

/// Words a title or species name may not contain (once folded like a
/// name's skeleton, spaces and punctuation dropped): they would pass for
/// the server's own voice.
const RESERVED_WORDS: &[&str] = &["admin", "moderator", "server", "staff", "system", "official", "developer"];

/// A title or species name shown to other players (playtest 3: bidi
/// overrides, zero-width characters, markup and "Server Admin" were
/// accepted): ASCII letters, digits, single spaces and
/// [`DISPLAY_PUNCTUATION`] only, like player names ASCII so there are no
/// look-alike letters, and none of the [`RESERVED_WORDS`]. The reason when
/// refused.
pub(crate) fn display_text(text: &str, max: usize) -> Result<String, &'static str> {
    let t = clean_text(text, max).ok_or("empty, too long or with control characters")?;
    if !t.chars().all(|c| c.is_ascii_alphanumeric() || c == ' ' || DISPLAY_PUNCTUATION.contains(c)) {
        return Err("only letters, digits, spaces and . , ' ! ? - : + # ( )");
    }
    if t.contains("  ") {
        return Err("no double spaces");
    }
    if !t.chars().any(|c| c.is_ascii_alphanumeric()) {
        return Err("at least one letter or digit");
    }
    let folded = fold(&t);
    if RESERVED_WORDS.iter().any(|w| folded.contains(&fold(w))) {
        return Err("it would pass for the server or its staff");
    }
    Ok(t)
}

/// A text folded for look-alike comparison: a name's skeleton
/// (`accounts::name_skeleton`) of its letters and digits only,
/// with the usual digit stand-ins for letters (`3` e, `4` a, `5` s, `7` t).
fn fold(text: &str) -> String {
    let alnum: String = text
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| match c {
            '3' => 'e',
            '4' => 'a',
            '5' => 's',
            '7' => 't',
            c => c,
        })
        .collect();
    crate::accounts::name_skeleton(&alnum)
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

/// [`Spread`] over the pairs of `a` × `b` (None when either is empty).
pub fn spread(a: &[&Genome], b: &[&Genome], egg_hybrid: f32) -> Option<Spread> {
    let mut fert: Vec<f32> = Vec::with_capacity(a.len() * b.len());
    let mut closest: Option<(f32, &Genome, &Genome)> = None;
    for &x in a {
        for &y in b {
            let gap = fertility_gap(x, y, egg_hybrid);
            fert.push(fertility_with(x, y, egg_hybrid));
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
    let weights = plan_weights(egg_hybrid);
    let (gene, gene_gap) = BODY_PLAN_GENES
        .iter()
        .zip(weights)
        .map(|(&g, w)| (g, (x.expr(g) - y.expr(g)).abs() * w))
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
        }
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
        let title = display_text(title, economy::MAX_TITLE_LEN).map_err(|why| {
            ErrorInfo::new("bad_title", format!("a title has 1 to {} characters: {why}", economy::MAX_TITLE_LEN))
        })?;
        // Nor another player's name (it shows beside the owner's).
        if self.impersonates(player, &title) {
            return Err(ErrorInfo::new("bad_title", "a title can't look like another player's name"));
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
        let name = display_text(name, crate::design::MAX_NAME_LEN).map_err(|why| {
            ErrorInfo::new(
                "bad_name",
                format!("a species name has 1 to {} characters: {why}", crate::design::MAX_NAME_LEN),
            )
        })?;
        if self.impersonates(player, &name) {
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
        // Look-alikes count as taken, as for player names.
        if tracker.species.iter().any(|o| o.id != species && fold(&o.name) == fold(&name)) {
            return Err(ErrorInfo::new("name_taken", "another species has that name"));
        }
        self.spend(player, economy::SPECIES_NAME_PRICE, format!("Named species {species} \"{name}\""))?;
        if let Some(s) = self.world.chronicle.species.species.iter_mut().find(|s| s.id == species) {
            s.name = name.clone();
        }
        self.economy.named_species.insert(species, player);
        Ok(name)
    }

    /// Whether `text` looks like another player's name (folded alike).
    fn impersonates(&self, player: PlayerId, text: &str) -> bool {
        let f = fold(text);
        self.names.iter().any(|(&p, n)| p != player && fold(n) == f)
    }

    /// Who may name species `s`: the player credited with a split, or for a
    /// species a founding started (`founding`, or `initial` when players'
    /// lineages were there at the first detection) the owner of its main
    /// lineage. Nobody for a wild species or a gone one.
    pub(crate) fn species_namer(&self, s: &evo_sim::species::Species) -> Option<PlayerId> {
        if !s.is_alive() || s.merged_into.is_some() {
            return None;
        }
        if s.founder.is_some() {
            return s.founder;
        }
        if s.origin == evo_sim::species::SpeciesOrigin::Split {
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
    fn reinforce_max_alive(&self) -> usize {
        (self.world.config.lineage_cap as f64 * economy::REINFORCE_MAX_SHARE).floor() as usize
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
                format!(
                    "at most {} reinforcements within a ranked season's length ({} days; retired lineages count)",
                    economy::MAX_REINFORCEMENTS_PER_SEASON,
                    self.cfg.season_days
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
        let (today, window) = (self.world.day(), self.cfg.season_days.max(1));
        let days = self.economy.reinforce_days.entry(player).or_default();
        days.retain(|&d| d + window > today);
        days.push(today);
        let _ = n;
        // New organisms: the next state build indexes the world again.
        self.state_cache.invalidate();
        Ok((ids, self.shop_view(player)))
    }

    /// One row per other lineage with living adults: `own` against a
    /// sample of its adults, most fertile first.
    fn compat_rows(&self, own: &[&Genome], skip: Option<u32>) -> Vec<CompatibilityRow> {
        let egg_hybrid = self.world.config.balance.egg_hybrid;
        let mut rows: Vec<CompatibilityRow> = (0..self.world.lineages.len() as u32)
            .filter(|&l| Some(l) != skip)
            .filter_map(|l| {
                let theirs = sample_of(self, l);
                let s = spread(own, &theirs, egg_hybrid)?;
                let lin = &self.world.lineages[l as usize];
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
        CompatibilityView { lineage: Some(mine), rows: self.compat_rows(&own, Some(mine)) }
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
        // Another player's lineage: tenths and no limiting gene. Varying
        // one gene at a time against exact gaps solved a rival's body plan.
        for r in rows.iter_mut().filter(|r| r.owner != player && r.owner != WILD) {
            let tenth = |x: f32| (x * 10.0).round() / 10.0;
            (r.fertility, r.fertility_low, r.fertility_high) =
                (tenth(r.fertility), tenth(r.fertility_low), tenth(r.fertility_high));
            (r.best_fertility, r.fertile_share, r.gap) =
                (tenth(r.best_fertility), tenth(r.fertile_share), tenth(r.gap));
            r.limiting = "body_plan".into();
        }
        let fertile_with = rows.iter().filter(|r| r.fertile_share > 0.0).count();
        let at_risk = rows.iter().filter(|r| r.fertile_share <= 0.0 && r.drift_risk != "none").count();
        DesignCompatView { day: self.world.day(), sample: SAMPLE, fertile_with, at_risk, rows }
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
        let egg_hybrid = self.world.config.balance.egg_hybrid;
        for v in views {
            let Some(o) = self.world.organism(v.organism) else { continue };
            if let Some(s) = spread(&own, &[&*o.genome], egg_hybrid) {
                v.compatible = Some(compatible(&s));
                v.body_plan_fertility = Some(s.mean);
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
            traded_share: self.traded_share(player),
            royalty: economy::royalty(self.traded_share(player), home.is_some() && !self.is_extinct(player)),
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
