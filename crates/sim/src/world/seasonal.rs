//! Seasonal strategies as genes ("Seasons and biomes" in the design doc).
//!
//! | Strategy | Rule |
//! | --- | --- |
//! | Hibernation / brumation | Fatten in autumn; dormant while the seasonal mean is below `dormancy_temp` in autumn and winter: near-zero hunger and thirst, safe from weather, no breeding, found in its den only by a hunter that comes close. Warm hibernators sleep until spring (rousing early only when parched, or starving with no fat left); brumators wake with the weather. Mismatches: a cold-blooded hibernator may never wake, a warm-blooded brumator burns fat. |
//! | Migration | Summer range at its founding spot (young inherit their mother's), winter range `migration_range` of the map further south; heads for the best pasture near water on that latitude in autumn and back in spring, with a death risk per tile travelled. |
//! | Estivation | Fatten in spring; dormant while the summer seasonal mean is above `estivation_temp`, or through a summer drought (`Map::dryness_at` past the drought line). |
//! | Food caching | Gathers plant food into a cache in summer and autumn, eats from it in winter and spring. Anyone else can raid it. |
//! | Torpor | On cold nights while resting: cheaper metabolism, cold costs nothing extra, but still exposed and slow to react until morning. |
//!
//! Everything is a few comparisons per organism per tick; deep sleepers skip
//! perception entirely, so a hibernating population is cheaper than an
//! active one.

use rand::Rng;

use super::{Intent, World};
use crate::genome::Gene;
use crate::math::Vec2;
use crate::organism::{Action, DeathCause, Dormancy, Organism};
use crate::rng::SimRng;
use crate::stats::WinterForm;
use crate::terrain::Season;

/// Degrees past the threshold at which a sleeper that wakes with the weather
/// wakes (hysteresis, so it doesn't flicker).
const WAKE_MARGIN: f32 = 2.0;
/// Tiles of latitude within which a migrator counts as in its seasonal range.
const RANGE_SLACK: f32 = 3.0;
/// Share of the migration distance a migrator may roam from its range line
/// before heading back (on top of `RANGE_SLACK`), so it doesn't pace a
/// line all season.
const RANGE_ROAM: f32 = 0.3;
/// How far from its cache a cacher gathers food, in tiles.
pub(crate) const CACHE_RANGE: f32 = 8.0;
/// Daily fraction of cached food that spoils.
const CACHE_SPOIL: f32 = 0.01;
/// Threat at which a torpid animal rouses.
pub(crate) const TORPOR_ALERT: f32 = 0.4;
/// Hunger above which a sleeper won't start a dormancy without fat.
const LEAN_LIMIT: f32 = 60.0;
/// Hunger at which a sleeper that can heat itself rouses once its fat is gone.
const AROUSAL_HUNGER: f32 = 90.0;
/// Thirst at which a sleeper that can heat itself rouses to drink, and
/// below which it settles into a dormancy.
const AROUSAL_THIRST: f32 = 80.0;
const THIRST_LIMIT: f32 = 50.0;
/// Plant efficiency (1 - carnivory) needed to cache or raid caches.
const CACHE_DIET: f32 = 0.3;
/// Regional dryness (`Map::dryness_at`) above which an estivator also goes
/// dormant in summer, heat or not: the drought line of `terrain`.
const ESTIVATION_DRYNESS: f32 = 1.0 - crate::terrain::DROUGHT_WETNESS;
/// Dryness drop below [`ESTIVATION_DRYNESS`] at which a drought sleeper wakes.
const DRYNESS_MARGIN: f32 = 0.1;

impl World {
    /// Seasonal mean temperature at `p` (no day/night swing or storms).
    pub(crate) fn seasonal_temp(&self, p: Vec2) -> f32 {
        self.map.tile_at(p).base_temp + self.season_offset
    }

    /// Fat the organism would still lay down now (0 outside its fattening
    /// season).
    pub(crate) fn fat_room(&self, o: &Organism) -> f32 {
        if o.traits.seasonal.fattens_in(self.season()) {
            (self.fat_capacity(o) - o.fat).max(0.0)
        } else {
            0.0
        }
    }

    pub(crate) fn fat_capacity(&self, o: &Organism) -> f32 {
        self.config.balance.fat_capacity * (1.0 + o.pheno[Gene::FatStores])
    }

    /// Hunger points of food gained: they go to hunger first, then to fat
    /// (only in the fattening season).
    pub(crate) fn feed(&mut self, i: usize, gained: f32, fat_room: f32) {
        let o = &mut self.organisms[i];
        if fat_room <= 0.0 {
            o.needs.hunger -= gained;
            return;
        }
        let h = gained.min(o.needs.hunger.max(0.0));
        o.needs.hunger -= h;
        o.fat += (gained - h).min(fat_room);
    }

    /// Hunger multiplier of the current dormancy state (1 when awake).
    pub(crate) fn dormant_rate(&self, o: &Organism) -> f32 {
        let bal = &self.config.balance;
        let s = &o.traits.seasonal;
        match o.dormancy {
            Dormancy::Awake => 1.0,
            Dormancy::Winter => match s.winter {
                Some((WinterForm::WarmBrumator, k)) => bal.dormant_metabolism * (2.0 - k) * bal.brumation_mismatch,
                Some((_, k)) => bal.dormant_metabolism * (2.0 - k),
                None => 1.0,
            },
            Dormancy::Summer => bal.dormant_metabolism * (2.0 - s.estivation),
            Dormancy::Torpor => 1.0 - (1.0 - bal.torpor_metabolism) * s.torpor,
        }
    }

    /// Enter or leave a dormancy, and let caches spoil. Runs in `apply`
    /// after the action, before needs. Returns a cause of death for a
    /// sleeper that never wakes.
    pub(crate) fn seasonal_tick(&mut self, i: usize, intent: &Intent, moved: f32, rng: &mut SimRng) -> Option<DeathCause> {
        let bal = self.config.balance;
        let season = self.season();
        let daytime = self.is_daytime();
        let dt = self.config.dt_days();
        let pos = self.organisms[i].pos;
        let temp = self.seasonal_temp(pos);
        let dry = self.map.dryness_at(pos);
        let o = &mut self.organisms[i];
        if o.cache > 0.0 {
            o.cache *= 1.0 - CACHE_SPOIL * dt;
        }
        let s = o.traits.seasonal;
        // A sleeper that can heat itself rouses when out of fat and hungry, or
        // thirsty; a cold-blooded one sleeps on.
        let can_rouse = |o: &Organism| {
            o.traits.warm && ((o.fat <= 0.0 && o.needs.hunger >= AROUSAL_HUNGER) || o.needs.thirst >= AROUSAL_THIRST)
        };
        match o.dormancy {
            Dormancy::Awake => {
                let free = o.pregnancy.is_none() && o.nest.is_none() && intent.threat < 0.3 && intent.action != Action::Flee;
                let fed = (o.fat > 0.0 || o.needs.hunger < LEAN_LIMIT) && o.needs.thirst < THIRST_LIMIT;
                let cold_season = matches!(season, Season::Autumn | Season::Winter);
                if !free {
                    return None;
                }
                if s.winter.is_some() && cold_season && temp < bal.dormancy_temp && fed {
                    o.dormancy = Dormancy::Winter;
                } else if s.estivation > 0.0
                    && season == Season::Summer
                    && (temp > bal.estivation_temp || dry > ESTIVATION_DRYNESS)
                    && fed
                {
                    o.dormancy = Dormancy::Summer;
                } else if s.torpor > 0.0
                    && !daytime
                    && o.needs.cold > 0.0
                    && matches!(intent.action, Action::Rest | Action::Idle)
                    && moved < 0.01
                {
                    o.dormancy = Dormancy::Torpor;
                }
            }
            Dormancy::Winter => {
                let Some((form, _)) = s.winter else {
                    o.dormancy = Dormancy::Awake;
                    return None;
                };
                let wake = if form.self_waking() {
                    matches!(season, Season::Spring | Season::Summer)
                } else {
                    season == Season::Summer || temp >= bal.dormancy_temp + WAKE_MARGIN
                };
                if wake && form == WinterForm::ColdHibernator && rng.gen::<f32>() < bal.dormancy_mismatch {
                    // Can't heat itself to wake: never wakes.
                    return Some(DeathCause::Exposure);
                }
                if wake || can_rouse(o) {
                    o.dormancy = Dormancy::Awake;
                }
            }
            Dormancy::Summer => {
                let relieved = temp <= bal.estivation_temp - WAKE_MARGIN && dry <= ESTIVATION_DRYNESS - DRYNESS_MARGIN;
                if season != Season::Summer || relieved || can_rouse(o) {
                    o.dormancy = Dormancy::Awake;
                }
            }
            Dormancy::Torpor => {
                if daytime || intent.action != Action::Dormant {
                    o.dormancy = Dormancy::Awake;
                }
            }
        }
        None
    }

    /// Where a migrator should be heading now, if it is out of its seasonal
    /// range: its summer range in spring and summer, the winter range south
    /// of it in autumn and winter. A spot on the right latitude near the
    /// range's longitude that the body can stand on.
    pub(crate) fn seasonal_destination(&self, o: &Organism, dependent: bool) -> Option<Vec2> {
        let k = o.traits.seasonal.migration;
        if k <= 0.0 || dependent {
            return None;
        }
        let south = matches!(self.season(), Season::Autumn | Season::Winter);
        let h = self.map.height as f32;
        let shift = self.config.balance.migration_range * k * h;
        let ty = if south { o.range.y + shift } else { o.range.y }.clamp(0.5, h - 0.5);
        // Once under way it goes all the way; settled, it roams a while first.
        let slack = if o.action == Action::Migrate { RANGE_SLACK } else { RANGE_SLACK + RANGE_ROAM * shift };
        if (o.pos.y - ty).abs() <= slack {
            return None;
        }
        // The best pasture with water nearby among a few spots on that
        // latitude, nearer the range's longitude first (ties keep the nearer).
        let mut best: Option<(f32, Vec2)> = None;
        for dx in [0.0, 4.0, -4.0, 8.0, -8.0, 12.0, -12.0, 16.0, -16.0] {
            let p = self.map.clamp(Vec2::new(o.range.x + dx, ty));
            if !self.passable_for(o, p) {
                continue;
            }
            let (x, y) = self.map.tile_coords(p);
            let t = &self.map.tiles[self.map.idx(x, y)];
            let water = self.map.water_dist[self.map.idx(x, y)].min(20) as f32;
            let score = t.veg_max / 40.0 - 0.1 * water - 0.01 * dx.abs();
            if best.is_none_or(|(s, _)| score > s) {
                best = Some((score, p));
            }
        }
        Some(best.map_or_else(|| self.map.clamp(Vec2::new(o.pos.x, ty)), |(_, p)| p))
    }

    /// Death risk of a migration leg: per tile travelled, worse for low
    /// endurance, halved by group navigation.
    pub(crate) fn journey_death(&self, i: usize, moved: f32, rng: &mut SimRng) -> bool {
        let o = &self.organisms[i];
        let risk = self.config.balance.migration_risk;
        if risk <= 0.0 || moved <= 0.0 || o.traits.seasonal.migration <= 0.0 {
            return false;
        }
        let nav = if o.kin_nearby >= 3 { 1.0 - 0.5 * o.pheno[Gene::GroupTolerance] } else { 1.0 };
        let hazard = risk * moved * (1.5 - o.traits.stats.endurance).max(0.2) * nav;
        rng.gen::<f32>() < hazard
    }

    /// Most plant food (food units) the organism hoards.
    pub(crate) fn cache_capacity(&self, o: &Organism) -> f32 {
        let eff = (1.0 - o.traits.carnivory).max(0.05);
        self.config.balance.cache_capacity * o.traits.seasonal.caching * o.food_per_hunger() / eff
    }

    /// Utility of gathering for the cache: in summer and autumn, when fed,
    /// for an adult cacher that eats plants and has room left.
    pub(crate) fn cache_utility(&self, o: &Organism) -> f32 {
        if o.traits.seasonal.caching <= 0.0
            || !o.matured
            || 1.0 - o.traits.carnivory < CACHE_DIET
            || o.needs.hunger > 40.0
            || !matches!(self.season(), Season::Summer | Season::Autumn)
        {
            return 0.0;
        }
        let room = 1.0 - o.cache / self.cache_capacity(o).max(1e-6);
        0.3 * room.max(0.0)
    }

    /// Whether a hungry cacher should go eat from its cache: in winter and
    /// spring, or when very hungry, if the cache holds a bite.
    pub(crate) fn cache_to_eat(&self, o: &Organism, bite: f32) -> Option<Vec2> {
        let lean = matches!(self.season(), Season::Winter | Season::Spring) || o.needs.hunger > 70.0;
        let eff = 1.0 - o.traits.carnivory;
        o.cache_pos.filter(|_| lean && o.cache * eff >= bite)
    }

    /// Gather plant food from the current tile into the cache. The cache is
    /// placed where gathering starts and stays there until emptied.
    pub(crate) fn gather(&mut self, i: usize) {
        let dt = self.config.dt_days();
        let cap = self.cache_capacity(&self.organisms[i]);
        let o = &self.organisms[i];
        let eff = 1.0 - o.traits.carnivory;
        let pos = o.pos;
        let home = match o.cache_pos {
            Some(c) if o.cache > 0.5 => c,
            _ => pos,
        };
        if pos.dist(home) > CACHE_RANGE || o.cache >= cap || eff < CACHE_DIET {
            return;
        }
        let rate = 0.5 * super::apply::MAX_INTAKE_PER_DAY * dt * o.food_per_hunger() / eff;
        let tile = self.map.tile_at_mut(pos);
        let got = rate.min(tile.veg).min(cap - o.cache);
        tile.veg -= got;
        let o = &mut self.organisms[i];
        o.cache += got;
        o.cache_pos = Some(home);
    }

    /// Eat food out of a cache on this tile: one's own when grazing at it, or
    /// anyone's when hungry (`raider`).
    pub(crate) fn eat_cache(&mut self, i: usize, raider: bool) {
        let o = &self.organisms[i];
        let eff = 1.0 - o.traits.carnivory;
        if eff < CACHE_DIET || o.needs.hunger < if raider { 30.0 } else { 5.0 } {
            return;
        }
        let owner = if raider {
            match self.cache_at.get(&self.map.tile_coords(o.pos)) {
                Some(&j) if j != i => j,
                _ => return,
            }
        } else {
            i
        };
        let c = &self.organisms[owner];
        let Some(at) = c.cache_pos else {
            return;
        };
        if c.cache <= 0.0 || self.map.tile_coords(at) != self.map.tile_coords(o.pos) {
            return;
        }
        let want = (super::apply::MAX_INTAKE_PER_DAY * self.config.dt_days()).min(o.needs.hunger);
        let fph = o.food_per_hunger();
        let got = (want * fph / eff).min(c.cache);
        self.organisms[owner].cache -= got;
        self.feed(i, got * eff / fph, 0.0);
    }

    /// Tile → organism index of every cache, for raids. Rebuilt hourly and
    /// after the daily cleanup (indices change).
    pub(crate) fn rebuild_caches(&mut self) {
        let mut at = std::mem::take(&mut self.cache_at);
        at.clear();
        for (k, o) in self.organisms.iter().enumerate() {
            if let Some(c) = o.cache_pos.filter(|_| o.alive && o.cache > 0.5) {
                at.insert(self.map.tile_coords(c), k);
            }
        }
        self.cache_at = at;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::{BirthType, FounderDesign, Thermo};
    use crate::terrain::{Biome, Map};
    use crate::WorldConfig;

    /// A flat grassland world at `base_temp` with `veg` food per tile and no
    /// water, starting on `day` of the year.
    fn flat_world(base_temp: f32, veg: f32, day: u32) -> World {
        let mut w = World::new(WorldConfig { seed: 7, width: 24, height: 24, ..Default::default() });
        for t in &mut w.map.tiles {
            (t.biome, t.water_depth, t.mud, t.sand, t.slope, t.veg_density) = (Biome::Grassland, 0.0, 0.0, 0.0, 0.0, 0.1);
            (t.base_temp, t.veg, t.veg_max, t.snow, t.ice) = (base_temp, veg, veg, 0.0, 0.0);
        }
        set_day(&mut w, day);
        w
    }

    fn set_day(w: &mut World, day: u32) {
        w.tick = day as u64 * w.config.ticks_per_day as u64;
        w.season_offset = Map::seasonal_offset(day, w.config.days_per_year);
    }

    fn design(thermo: Thermo) -> FounderDesign {
        FounderDesign::new("t", thermo, BirthType::Live, 2).with(Gene::BodySize, 0.3)
    }

    /// Found `n` adults that won't breed on their own.
    fn add(w: &mut World, d: &FounderDesign, n: u32) -> Vec<usize> {
        let lineage = w.lineages.len() as u32;
        w.found_lineage_unchecked(lineage + 1, &d.clone().founders(n));
        let idx: Vec<usize> = (0..w.organisms.len()).filter(|&k| w.organisms[k].lineage == lineage).collect();
        for &k in &idx {
            let o = &mut w.organisms[k];
            (o.needs.hunger, o.needs.thirst, o.needs.breeding, o.breed_ready_tick) = (20.0, 0.0, 0.0, u64::MAX);
        }
        idx
    }

    #[test]
    fn hibernators_sleep_through_winter_on_almost_nothing() {
        let mut w = flat_world(0.0, 0.0, 95);
        let hib = add(&mut w, &design(Thermo::Warm).with(Gene::Hibernation, 0.9), 4);
        let awake = add(&mut w, &design(Thermo::Warm), 4);
        w.run_days(1);
        for &k in &hib {
            let o = &w.organisms[k];
            assert_eq!(o.dormancy, Dormancy::Winter);
            assert!(o.needs.hunger < 25.0, "hibernator hunger {}", o.needs.hunger);
            assert_eq!(o.action, Action::Dormant);
            assert!(o.matured && !o.can_breed(0));
        }
        for &k in &awake {
            assert!(w.organisms[k].needs.hunger > 40.0, "active hunger {}", w.organisms[k].needs.hunger);
        }
        // Hunters can still find a sleeper's den; a sleeping hunter threatens nothing.
        let o = &w.organisms[hib[0]];
        assert!(!o.sheltered && !super::super::decide::is_armed(o));
        // A warm sleeper rouses to drink when parched, and doesn't settle
        // back while still thirsty.
        w.organisms[hib[0]].needs.thirst = AROUSAL_THIRST;
        w.step();
        assert_eq!(w.organisms[hib[0]].dormancy, Dormancy::Awake);
        w.step();
        assert_eq!(w.organisms[hib[0]].dormancy, Dormancy::Awake);
    }

    #[test]
    fn hibernators_fatten_in_autumn() {
        let mut w = flat_world(20.0, 40.0, 65);
        let hib = add(&mut w, &design(Thermo::Warm).with(Gene::Hibernation, 0.9), 4);
        let other = add(&mut w, &design(Thermo::Warm), 4);
        for &k in hib.iter().chain(&other) {
            w.organisms[k].needs.hunger = 0.0;
        }
        w.run_days(1);
        let cap = w.fat_capacity(&w.organisms[hib[0]]);
        let fat: Vec<f32> = hib.iter().map(|&k| w.organisms[k].fat).collect();
        assert!(fat.iter().all(|&f| f > 0.3 * cap), "fat {fat:?} of {cap}");
        assert!(other.iter().all(|&k| w.organisms[k].fat == 0.0));
        // Not before autumn.
        let mut w = flat_world(20.0, 40.0, 35);
        let hib = add(&mut w, &design(Thermo::Warm).with(Gene::Hibernation, 0.9), 2);
        w.run_days(1);
        assert_eq!(w.organisms[hib[0]].fat, 0.0);
    }

    /// Warm hibernators wake in spring by heating themselves; cold-blooded
    /// hibernators (affinity mismatch) often never wake.
    #[test]
    fn cold_blooded_hibernators_may_never_wake() {
        let mut w = flat_world(20.0, 10.0, 1);
        let warm = add(&mut w, &design(Thermo::Warm).with(Gene::Hibernation, 0.9), 40);
        let cold = add(&mut w, &design(Thermo::Cold).with(Gene::Hibernation, 0.9), 40);
        for &k in warm.iter().chain(&cold) {
            w.organisms[k].dormancy = Dormancy::Winter;
        }
        w.step();
        assert!(warm.iter().all(|&k| w.organisms[k].alive && w.organisms[k].dormancy == Dormancy::Awake));
        let dead = cold.iter().filter(|&&k| !w.organisms[k].alive).count();
        assert!((8..=32).contains(&dead), "{dead} of 40 cold hibernators never woke");
        assert!(cold.iter().all(|&k| w.organisms[k].death.is_none_or(|c| c == DeathCause::Exposure)));
    }

    /// Brumators wake with the weather, not the calendar; a warm-blooded
    /// brumator (affinity mismatch) burns fat while dormant.
    #[test]
    fn brumators_wake_with_the_weather() {
        let mut w = flat_world(0.0, 10.0, 1);
        let brum = add(&mut w, &design(Thermo::Cold).with(Gene::Brumation, 0.9), 2);
        let warm_brum = add(&mut w, &design(Thermo::Warm).with(Gene::Brumation, 0.9), 2);
        let hib = add(&mut w, &design(Thermo::Warm).with(Gene::Hibernation, 0.9), 2);
        for &k in brum.iter().chain(&warm_brum).chain(&hib) {
            w.organisms[k].dormancy = Dormancy::Winter;
        }
        let rate = |w: &World, k: usize| w.dormant_rate(&w.organisms[k]);
        let (a, b) = (rate(&w, warm_brum[0]), rate(&w, hib[0]));
        // Same founder spread on both genes, so compare per unit of strength.
        let strength = |k: usize| w.organisms[k].traits.seasonal.winter.unwrap().1;
        let ratio = (a / (2.0 - strength(warm_brum[0]))) / (b / (2.0 - strength(hib[0])));
        assert!((ratio - w.config.balance.brumation_mismatch).abs() < 0.01, "ratio {ratio}");
        // Still cold in early spring: the brumator sleeps on, the hibernator wakes.
        w.step();
        assert_eq!(w.organisms[brum[0]].dormancy, Dormancy::Winter);
        assert_eq!(w.organisms[hib[0]].dormancy, Dormancy::Awake);
        for t in &mut w.map.tiles {
            t.base_temp = 20.0;
        }
        w.step();
        assert_eq!(w.organisms[brum[0]].dormancy, Dormancy::Awake);
    }

    #[test]
    fn estivators_sleep_through_summer_heat() {
        let mut w = flat_world(28.0, 10.0, 40);
        let est = add(&mut w, &design(Thermo::Warm).with(Gene::Estivation, 0.9), 4);
        let awake = add(&mut w, &design(Thermo::Warm), 4);
        w.run_days(1);
        for &k in &est {
            let o = &w.organisms[k];
            assert_eq!(o.dormancy, Dormancy::Summer);
            assert!(o.needs.thirst < 5.0 && o.health == 100.0, "thirst {} health {}", o.needs.thirst, o.health);
        }
        // The heat costs the active ones water and health.
        assert!(awake.iter().all(|&k| w.organisms[k].needs.thirst > 15.0 && w.organisms[k].health < 100.0));
        // Summer over: awake again.
        set_day(&mut w, 61);
        w.step();
        assert!(est.iter().all(|&k| w.organisms[k].dormancy == Dormancy::Awake));
    }

    #[test]
    fn estivators_sleep_through_a_summer_drought() {
        let mut w = flat_world(5.0, 10.0, 40);
        let set_wetness = |w: &mut World, x: f32| {
            w.map.cells.iter_mut().for_each(|c| c.wetness = x);
            w.map.tiles.iter_mut().for_each(|t| t.wetness = x);
        };
        set_wetness(&mut w, 0.5);
        let est = add(&mut w, &design(Thermo::Warm).with(Gene::Estivation, 0.9), 4);
        w.step();
        // Mild and normal: no reason to sleep.
        assert!(est.iter().all(|&k| w.organisms[k].dormancy == Dormancy::Awake));
        set_wetness(&mut w, 0.05);
        w.step();
        assert!(est.iter().all(|&k| w.organisms[k].dormancy == Dormancy::Summer));
        // The rains come back: awake again.
        set_wetness(&mut w, 0.8);
        w.step();
        assert!(est.iter().all(|&k| w.organisms[k].dormancy == Dormancy::Awake));
    }

    #[test]
    fn migrators_head_south_in_autumn_and_home_in_spring() {
        let mut w = flat_world(15.0, 10.0, 0);
        let k = add(&mut w, &design(Thermo::Warm).with(Gene::Migration, 1.0), 2)[0];
        let mut o = w.organisms[k].clone();
        o.traits.seasonal.migration = 1.0;
        let shift = w.config.balance.migration_range * 24.0;
        let mut at = |day: u32, y: f32| {
            set_day(&mut w, day);
            let mut o = o.clone();
            o.range = Vec2::new(o.range.x, 4.0);
            o.pos = Vec2::new(o.pos.x, y);
            w.seasonal_destination(&o, false).map(|p| p.y)
        };
        let south = at(65, 4.0).expect("heads south in autumn");
        assert!((south - (4.0 + shift)).abs() < 1e-3, "winter range at {south}");
        assert_eq!(at(100, 4.0 + shift), None, "already in its winter range");
        assert!((at(5, 4.0 + shift).expect("heads home in spring") - 4.0).abs() < 1e-3);
        assert_eq!(at(40, 5.0), None, "home for the summer");
        // Dependent young follow their mother instead; non-migrators stay.
        assert_eq!(w.seasonal_destination(&o, true), None);
        let still = add(&mut w, &design(Thermo::Warm), 2)[0];
        let mut s = w.organisms[still].clone();
        s.pos.y = 20.0;
        assert_eq!(w.seasonal_destination(&s, false), None);
    }

    #[test]
    fn migration_journeys_are_dangerous() {
        let trip = |risk: f32| {
            let mut w = flat_world(15.0, 40.0, 65);
            w.config.balance.migration_risk = risk;
            let m = add(&mut w, &design(Thermo::Warm).with(Gene::Migration, 1.0), 20);
            for &k in &m {
                w.organisms[k].range.y = 0.5;
                w.organisms[k].pos.y = 1.0;
            }
            w.run_days(1);
            let lost = w.counters.deaths.get(&DeathCause::Journey).copied().unwrap_or(0);
            // At the edge of its winter range or beyond (it wanders there,
            // looking for water).
            let south = m.iter().filter(|&&k| w.organisms[k].alive && w.organisms[k].pos.y > 3.5).count();
            (lost, south)
        };
        let (lost, _) = trip(0.5);
        assert!(lost >= 10, "{lost} of 20 lost on the journey");
        let (lost, south) = trip(0.0);
        assert_eq!(lost, 0);
        assert!(south >= 15, "{south} of 20 made it south");
    }

    #[test]
    fn cachers_hoard_in_autumn_eat_it_in_winter_and_others_raid_it() {
        let mut w = flat_world(15.0, 40.0, 65);
        let c = add(&mut w, &design(Thermo::Warm).with(Gene::Caching, 1.0), 2)[0];
        w.organisms[c].needs.hunger = 0.0;
        w.run_days(1);
        let o = &w.organisms[c];
        assert!(o.cache > 0.3 * w.cache_capacity(o), "cached {} of {}", o.cache, w.cache_capacity(o));
        let at = o.cache_pos.expect("a cache");
        // Winter, nothing to graze: it lives off the cache.
        set_day(&mut w, 95);
        for t in &mut w.map.tiles {
            (t.veg, t.veg_max, t.base_temp) = (0.0, 0.0, 20.0);
        }
        let before = w.organisms[c].cache;
        w.organisms[c].needs.hunger = 60.0;
        w.organisms[c].pos = at;
        for _ in 0..4 {
            w.step();
        }
        assert!(w.organisms[c].cache < before && w.organisms[c].needs.hunger < 60.0);
        // A hungry stranger on the cache's tile eats from it.
        let r = add(&mut w, &design(Thermo::Warm), 2)[0];
        w.rebuild_caches();
        let left = w.organisms[c].cache;
        w.organisms[r].pos = at;
        w.organisms[r].needs.hunger = 80.0;
        w.eat_cache(r, true);
        assert!(w.organisms[c].cache < left && w.organisms[r].needs.hunger < 80.0);
    }

    #[test]
    fn torpor_saves_energy_on_cold_nights_only() {
        // Winter midnight at -5 °C base: a cold night.
        let mut w = flat_world(-5.0, 10.0, 100);
        let t = add(&mut w, &design(Thermo::Warm).with(Gene::Torpor, 1.0), 4);
        for &k in &t {
            w.organisms[k].needs.energy = 40.0;
        }
        for _ in 0..4 {
            w.step();
        }
        let torpid: Vec<usize> = t.iter().copied().filter(|&k| w.organisms[k].dormancy == Dormancy::Torpor).collect();
        assert!(!torpid.is_empty());
        let k = torpid[0];
        let s = w.organisms[k].traits.seasonal.torpor;
        let expect = 1.0 - (1.0 - w.config.balance.torpor_metabolism) * s;
        assert!((w.dormant_rate(&w.organisms[k]) - expect).abs() < 1e-6 && expect < 1.0);
        // Shallow: still a breeder, unlike a deep sleeper.
        assert!(!w.organisms[k].dormancy.is_deep());
        // Wakes in the morning.
        while !w.is_daytime() {
            w.step();
        }
        w.step();
        assert!(t.iter().all(|&k| w.organisms[k].dormancy != Dormancy::Torpor));
    }
}
