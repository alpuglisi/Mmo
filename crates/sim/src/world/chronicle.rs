//! The daily hook for species detection, eras, catastrophes and
//! achievements (`crate::species`, `crate::chronicle`).
//!
//! Detection and records only read the world. Catastrophes change it
//! through existing systems (`Map::climate_temp`, `Map::climate_wet`,
//! infections) and draw only from their own stream (`stream::CHRONICLE`),
//! so with none scheduled a world runs exactly as without this module.

use std::collections::BTreeMap;

use rand::Rng;

use super::World;
use crate::chronicle::{
    AchievementKind, Catastrophe, CatastropheKind, EraRank, EraRecord, NicheLeader, NICHE_MIN_ADULTS, NICHE_MIN_SHARE,
};
use crate::genome::{Gene, Genome, PlayerId, WILD};
use crate::organism::Infection;
use crate::rng::{rng_for, stream};
use crate::species::{DetectParams, Sample, SpeciesEvent};
use crate::terrain::{Biome, Map, CHUNK};

/// Severe winter: °C added to every tile, × severity.
const WINTER_CHILL: f32 = -10.0;
/// Megadrought and flood year: rain-potential bias, × severity.
const DROUGHT_BIAS: f32 = -1.5;
const FLOOD_BIAS: f32 = 1.2;
/// Epidemic: share of susceptible agents infected at onset, then daily
/// (× severity).
const EPIDEMIC_ONSET: f32 = 0.15;
const EPIDEMIC_DAILY: f32 = 0.02;

/// Diet niche of an expressed carnivory.
pub fn diet_niche(carnivory: f32) -> &'static str {
    if carnivory < 0.3 {
        "diet:herbivore"
    } else if carnivory < 0.6 {
        "diet:omnivore"
    } else {
        "diet:carnivore"
    }
}

/// Biome niche of a tile's biome (both water biomes are one niche).
pub fn biome_niche(b: Biome) -> &'static str {
    match b {
        Biome::DeepWater | Biome::ShallowWater => "biome:water",
        Biome::Marsh => "biome:marsh",
        Biome::Grassland => "biome:grassland",
        Biome::Forest => "biome:forest",
        Biome::Desert => "biome:desert",
        Biome::Tundra => "biome:tundra",
        Biome::Mountain => "biome:mountain",
    }
}

fn sample_of(genome: &Genome, weight: f64, lineage: u32, owner: PlayerId) -> Sample {
    let mut expr = [0.0; Gene::COUNT];
    for &g in Gene::ALL {
        expr[g.index()] = genome.expr(g);
    }
    let mut origins = BTreeMap::new();
    genome.tally_origins(1.0, &mut origins);
    Sample { expr, weight, lineage, owner, origins }
}

/// Evenly spaced picks of `take` out of `len`.
fn spaced(len: usize, take: usize) -> impl Iterator<Item = usize> {
    (0..take).map(move |k| k * len / take)
}

impl World {
    /// The current era (0-based).
    pub fn era(&self) -> u64 {
        self.day() / self.config.era_days()
    }

    /// Schedule a catastrophe starting on `start_day` (today or later) for
    /// `days` days (`None`: its usual length, see `CatastropheKind::timing`).
    /// Returns its id. It starts at the first daily update on or after
    /// `start_day` once no other catastrophe is under way.
    pub fn schedule_catastrophe(&mut self, kind: CatastropheKind, start_day: u64, days: Option<u64>) -> u32 {
        let dps = self.config.days_per_season() as f32;
        let days = days.unwrap_or_else(|| (kind.timing().1 * dps).round().max(1.0) as u64);
        let id = self.chronicle.catastrophes.len() as u32;
        self.chronicle.catastrophes.push(Catastrophe {
            id,
            kind,
            severity: self.config.scoring.catastrophe_severity,
            start_day,
            end_day: start_day + days.max(1),
            started: false,
            ended: false,
            population_before: 0,
            population_after: None,
            players_before: Vec::new(),
            survivors: Vec::new(),
        });
        id
    }

    /// Breeding adults sampled for species detection (see `crate::species`):
    /// up to `species_per_stratum` evenly spaced members per (lineage,
    /// region), cohorts as strata of their own, at most about
    /// `species_max_samples` in all.
    pub fn species_samples(&self) -> Vec<Sample> {
        let sc = self.config.scoring;
        let mut strata: BTreeMap<(u32, usize), Vec<usize>> = BTreeMap::new();
        for (i, o) in self.organisms.iter().enumerate() {
            if o.is_breeding_adult() {
                strata.entry((o.lineage, self.map.chunk_at(o.pos))).or_default().push(i);
            }
        }
        let today = self.tick as f64 / self.config.ticks_per_day as f64;
        let cohorts: Vec<(&super::Cohort, u32)> = if self.config.lod.enabled {
            self.cohorts()
                .filter(|(_, c)| !c.pool.is_empty())
                .map(|(_, c)| {
                    let a = c.adults(today);
                    (c, a[0] + a[1])
                })
                .filter(|(_, a)| *a > 0)
                .collect()
        } else {
            Vec::new()
        };
        let n = strata.len() + cohorts.len();
        let per = (sc.species_max_samples / n.max(1)).clamp(1, sc.species_per_stratum.max(1));
        let mut out = Vec::new();
        for ((lineage, _), members) in &strata {
            let take = per.min(members.len());
            let w = members.len() as f64 / take as f64;
            for k in spaced(members.len(), take) {
                let o = &self.organisms[members[k]];
                out.push(sample_of(&o.genome, w, *lineage, o.owner));
            }
        }
        for (c, adults) in cohorts {
            let take = per.min(c.pool.len()).min(adults as usize).max(1);
            let w = adults as f64 / take as f64;
            for k in spaced(c.pool.len(), take) {
                out.push(sample_of(&c.pool[k].genome, w, c.lineage, c.owner));
            }
        }
        out
    }

    /// Daily: lineage lives, catastrophes, species detection, era end.
    pub(super) fn chronicle_daily(&mut self) {
        let day = self.day();
        let counts = self.population_counts();
        self.track_lineages(day, &counts);
        self.run_catastrophes(day, &counts);
        let interval = self.config.scoring.species_interval_days as u64;
        if interval > 0 && day.is_multiple_of(interval) {
            self.detect_species(day);
        }
        let era_days = self.config.era_days();
        if day > 0 && day.is_multiple_of(era_days) {
            self.close_era(day / era_days - 1, day);
        }
    }

    /// Players with a living lineage.
    fn living_players(&self, counts: &[super::LineageCount]) -> Vec<PlayerId> {
        let mut players: Vec<PlayerId> = self
            .lineages
            .iter()
            .zip(counts)
            .filter(|(l, c)| l.owner != WILD && c.alive > 0)
            .map(|(l, _)| l.owner)
            .collect();
        players.sort_unstable();
        players.dedup();
        players
    }

    fn track_lineages(&mut self, day: u64, counts: &[super::LineageCount]) {
        let dpy = self.config.days_per_year.max(1) as u64;
        let ch = &mut self.chronicle;
        for (l, lineage) in self.lineages.iter().enumerate() {
            if ch.lineages.len() <= l {
                ch.lineages.push(crate::chronicle::LineageLife { founded_day: day, ..Default::default() });
            }
            let alive = counts.get(l).is_some_and(|c| c.alive > 0);
            let life = &mut ch.lineages[l];
            if life.extinct_day.is_some() {
                continue;
            }
            if !alive {
                life.extinct_day = Some(day);
                continue;
            }
            let years = (life.days(day) / dpy) as u32;
            if lineage.owner != WILD && years > life.years_awarded {
                life.years_awarded = years;
                ch.award(day, lineage.owner, AchievementKind::LineageLongevity { lineage: l as u32, years });
            }
        }
    }

    fn run_catastrophes(&mut self, day: u64, counts: &[super::LineageCount]) {
        let sc = self.config.scoring;
        let year = self.year();
        if sc.catastrophes
            && year >= sc.catastrophe_first_year as u64
            && self.chronicle.planned_year.is_none_or(|y| y < year)
        {
            self.chronicle.planned_year = Some(year);
            let mut rng = rng_for(self.config.seed, stream::CHRONICLE, year, 0);
            if rng.gen::<f32>() < sc.catastrophe_chance {
                let kind = CatastropheKind::ALL[rng.gen_range(0..CatastropheKind::ALL.len())];
                let (season, len) = kind.timing();
                let (dpy, dps) = (self.config.days_per_year as u64, self.config.days_per_season() as u64);
                let days = (len * dps as f32).round().max(1.0) as u64;
                let mut start = year * dpy + season as u64 * dps;
                if kind == CatastropheKind::Epidemic {
                    start += rng.gen_range(0..dpy.saturating_sub(days).max(1));
                }
                let busy = self.chronicle.catastrophes.iter().any(|c| !c.ended);
                if !busy && start + days > day {
                    self.schedule_catastrophe(kind, start.max(day), Some(days));
                }
            }
        }
        // End the active one, then start the next due.
        if let Some(k) = self.chronicle.catastrophes.iter().position(|c| c.is_active() && day >= c.end_day) {
            self.end_catastrophe(k, day, counts);
        }
        if self.chronicle.active_catastrophe().is_none() {
            if let Some(k) = self.chronicle.catastrophes.iter().position(|c| !c.started && c.start_day <= day) {
                let players = self.living_players(counts);
                let pop = counts.iter().map(|c| c.alive).sum();
                let c = &mut self.chronicle.catastrophes[k];
                // Late starts keep their length.
                c.end_day = day + (c.end_day - c.start_day);
                c.start_day = day;
                c.started = true;
                c.population_before = pop;
                c.players_before = players;
                let (kind, sev) = (c.kind, c.severity);
                match kind {
                    CatastropheKind::SevereWinter => self.map.climate_temp = WINTER_CHILL * sev,
                    CatastropheKind::Megadrought => self.map.climate_wet = DROUGHT_BIAS * sev,
                    CatastropheKind::FloodYear => self.map.climate_wet = FLOOD_BIAS * sev,
                    CatastropheKind::Epidemic => self.epidemic_infections(day, k as u64, EPIDEMIC_ONSET * sev),
                }
                return;
            }
        }
        if let Some(c) = self.chronicle.active_catastrophe() {
            if c.kind == CatastropheKind::Epidemic {
                let (id, sev) = (c.id as u64, c.severity);
                self.epidemic_infections(day, id, EPIDEMIC_DAILY * sev);
            }
        }
    }

    fn end_catastrophe(&mut self, k: usize, day: u64, counts: &[super::LineageCount]) {
        let living = self.living_players(counts);
        let pop = counts.iter().map(|c| c.alive).sum();
        self.map.climate_temp = 0.0;
        self.map.climate_wet = 0.0;
        let c = &mut self.chronicle.catastrophes[k];
        c.ended = true;
        c.end_day = day;
        c.population_after = Some(pop);
        c.survivors = c.players_before.iter().copied().filter(|p| living.contains(p)).collect();
        let (id, survivors) = (c.id, c.survivors.clone());
        for p in survivors {
            self.chronicle.award(day, p, AchievementKind::SurvivedCatastrophe { catastrophe: id });
        }
    }

    /// Infect each susceptible agent with chance `p`.
    fn epidemic_infections(&mut self, day: u64, id: u64, p: f32) {
        let mut rng = rng_for(self.config.seed, stream::CHRONICLE, day, 1 + id);
        for i in 0..self.organisms.len() {
            let o = &self.organisms[i];
            if o.alive && o.infection == Infection::Susceptible && rng.gen::<f32>() < p {
                self.infect(i);
            }
        }
    }

    fn detect_species(&mut self, day: u64) {
        let samples = self.species_samples();
        let params = DetectParams::from_config(&self.config);
        let names: Vec<String> = self.lineages.iter().map(|l| l.name.clone()).collect();
        let events = self.chronicle.species.detect(day, &samples, &params, |l| {
            names.get(l as usize).cloned().unwrap_or_else(|| format!("Lineage {l}"))
        });
        for e in events {
            if let SpeciesEvent::Appeared { species, founder: Some(p), .. } = e {
                self.chronicle.award(day, p, AchievementKind::SpeciesFounded { species });
            }
        }
    }

    /// Allele shares per niche among breeding adults (agents and cohort
    /// members): the leader of each niche with enough adults.
    pub fn niche_leaders(&self) -> Vec<NicheLeader> {
        let mut niches: BTreeMap<&'static str, (f64, BTreeMap<PlayerId, f64>)> = BTreeMap::new();
        let mut add = |niche: &'static str, genome: &Genome, w: f64| {
            let e = niches.entry(niche).or_default();
            e.0 += w;
            genome.tally_origins(w, &mut e.1);
        };
        for o in self.organisms.iter().filter(|o| o.is_breeding_adult()) {
            add(diet_niche(o.genome.expr(Gene::Carnivory)), &o.genome, 1.0);
            add(biome_niche(self.map.tile_at(o.pos).biome), &o.genome, 1.0);
        }
        if self.config.lod.enabled {
            let today = self.tick as f64 / self.config.ticks_per_day as f64;
            for (r, c) in self.cohorts() {
                let a = c.adults(today);
                let adults = (a[0] + a[1]) as f64;
                if adults == 0.0 || c.pool.is_empty() {
                    continue;
                }
                let cx = self.map.chunks_x;
                let center = Map::tile_center(
                    ((r % cx) * CHUNK + CHUNK / 2).min(self.map.width - 1),
                    ((r / cx) * CHUNK + CHUNK / 2).min(self.map.height - 1),
                );
                let biome = biome_niche(self.map.tile_at(center).biome);
                let w = adults / c.pool.len() as f64;
                for e in &c.pool {
                    add(diet_niche(e.genome.expr(Gene::Carnivory)), &e.genome, w);
                    add(biome, &e.genome, w);
                }
            }
        }
        niches
            .into_iter()
            .filter(|(_, (adults, _))| *adults >= NICHE_MIN_ADULTS)
            .filter_map(|(niche, (adults, alleles))| {
                let total: f64 = alleles.values().sum();
                let (player, n) = alleles.iter().fold((WILD, -1.0), |b, (&p, &n)| if n > b.1 { (p, n) } else { b });
                (total > 0.0).then(|| NicheLeader { niche: niche.to_string(), player, share: n / total, adults })
            })
            .collect()
    }

    fn close_era(&mut self, era: u64, day: u64) {
        let era_days = self.config.era_days();
        let (first_day, last_day) = (era * era_days, day - 1);
        let (ranking, breeding_adults) = match self.legacy.last() {
            Some(s) => {
                let mut rows: Vec<EraRank> = s
                    .alleles
                    .keys()
                    .map(|&p| EraRank { player: p, share: s.share(p), weighted_share: s.weighted_share(p) })
                    .collect();
                rows.sort_by(|a, b| b.share.total_cmp(&a.share).then(a.player.cmp(&b.player)));
                (rows, s.breeding_adults)
            }
            None => (Vec::new(), 0),
        };
        let niches = self.niche_leaders();
        for n in &niches {
            if n.player != WILD && n.share >= NICHE_MIN_SHARE {
                self.chronicle.award(
                    day,
                    n.player,
                    AchievementKind::NicheDominance { era, niche: n.niche.clone(), share: n.share },
                );
            }
        }
        let catastrophes = self
            .chronicle
            .catastrophes
            .iter()
            .filter(|c| c.started && (first_day..=last_day).contains(&c.start_day))
            .map(|c| c.id)
            .collect();
        let species_alive = self.chronicle.species.alive().count();
        self.chronicle.eras.push(EraRecord {
            era,
            first_day,
            last_day,
            ranking,
            breeding_adults,
            niches,
            catastrophes,
            species_alive,
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::chronicle::{AchievementKind, CatastropheKind};
    use crate::organism::Infection;
    use crate::presets;
    use crate::species::SpeciesOrigin;
    use crate::{World, WorldConfig};

    fn world(seed: u64, days_per_year: u32) -> World {
        let mut cfg = WorldConfig { seed, width: 48, height: 48, days_per_year, ..Default::default() };
        cfg.scoring.species_interval_days = 1;
        presets::standard_world(cfg, 1)
    }

    #[test]
    fn species_are_detected_from_the_start_with_stable_ids() {
        let mut w = world(3, 120);
        w.run_days(1);
        let t = &w.chronicle.species;
        assert_eq!(t.last_detection, Some(0));
        let first: Vec<(u32, String)> = t.species.iter().map(|s| (s.id, s.name.clone())).collect();
        // Antelope and herd grazers are one kind (as fox and pack hunters
        // can be, through founders that are kin across the two designs);
        // the other designs are kinds of their own.
        assert!((5..w.lineages.len()).contains(&first.len()), "{first:?}");
        for name in ["Meadow vole", "Marsh newt", "Toxic frogs", "Burrowing omnivores"] {
            assert!(first.iter().any(|f| f.1 == name), "{name}: {first:?}");
        }
        assert!(t.species.iter().all(|s| s.origin == SpeciesOrigin::Initial));
        w.run_days(4);
        let t = &w.chronicle.species;
        assert_eq!(t.last_detection, Some(4));
        let alive: Vec<(u32, String)> = t.alive().map(|s| (s.id, s.name.clone())).collect();
        assert_eq!(alive, first, "ids are kept across detections");
    }

    #[test]
    fn an_extinct_lineage_ends_its_species() {
        let mut w = world(5, 120);
        w.run_days(1);
        let newts = w.lineages.iter().position(|l| l.name == "Marsh newt").unwrap() as u32;
        let sp = w.chronicle.species.alive().find(|s| s.main_lineage() == Some(newts)).unwrap().id;
        for i in 0..w.organisms.len() {
            if w.organisms[i].lineage == newts {
                w.kill(i, crate::organism::DeathCause::Predation);
            }
        }
        w.run_days(3);
        let s = w.chronicle.species.get(sp).unwrap();
        assert_eq!(s.extinct_day, Some(0), "{s:?}");
        assert_eq!(w.chronicle.lineages[newts as usize].extinct_day, Some(1));
    }

    #[test]
    fn severe_winter_chills_the_map_and_survivors_are_awarded() {
        let mut w = world(7, 120);
        w.run_days(1);
        let before = w.map.tiles.iter().map(|t| t.temp).sum::<f32>() / w.map.tiles.len() as f32;
        let id = w.schedule_catastrophe(CatastropheKind::SevereWinter, 2, Some(3));
        w.run_days(2);
        let c = w.chronicle.active_catastrophe().expect("started on day 2").clone();
        assert_eq!((c.id, c.start_day, c.end_day), (id, 2, 5));
        assert_eq!(w.map.climate_temp, -10.0);
        assert!(!c.players_before.is_empty());
        w.run_days(1);
        let during = w.map.tiles.iter().map(|t| t.temp).sum::<f32>() / w.map.tiles.len() as f32;
        assert!(during < before - 7.0, "{before} -> {during}");
        w.run_days(2);
        let c = &w.chronicle.catastrophes[id as usize];
        assert!(c.ended && w.chronicle.active_catastrophe().is_none());
        assert_eq!(w.map.climate_temp, 0.0);
        assert!(c.population_after.is_some());
        let awarded: Vec<_> = w
            .chronicle
            .achievements
            .iter()
            .filter(|a| a.kind == AchievementKind::SurvivedCatastrophe { catastrophe: id })
            .map(|a| a.player)
            .collect();
        assert_eq!(awarded, c.survivors);
        assert!(!awarded.is_empty());
    }

    #[test]
    fn megadrought_dries_the_soil_and_epidemic_infects() {
        let mut w = world(9, 120);
        let wet = |w: &World| w.map.cells.iter().map(|c| c.wetness).sum::<f32>() / w.map.cells.len() as f32;
        let mut calm = world(9, 120);
        w.schedule_catastrophe(CatastropheKind::Megadrought, 0, Some(20));
        w.run_days(12);
        calm.run_days(12);
        assert!(wet(&w) < wet(&calm) - 0.1, "{} vs {}", wet(&w), wet(&calm));

        let mut e = world(9, 120);
        e.run_days(1);
        let sick = |w: &World| w.organisms.iter().filter(|o| o.alive && o.infection != Infection::Susceptible).count();
        let before = sick(&e);
        e.schedule_catastrophe(CatastropheKind::Epidemic, 1, Some(5));
        e.run_days(1);
        let alive = e.organisms.iter().filter(|o| o.alive).count();
        assert!(sick(&e) >= before + alive / 10, "{before} -> {} of {alive}", sick(&e));
    }

    #[test]
    fn random_catastrophes_skip_the_first_year_and_eras_close_with_rankings() {
        let mut cfg = WorldConfig { seed: 2, width: 48, height: 48, days_per_year: 8, ..Default::default() };
        cfg.scoring.catastrophes = true;
        cfg.scoring.catastrophe_chance = 1.0;
        cfg.scoring.era_years = 1;
        let mut w = presets::standard_world(cfg, 1);
        w.run_days(8);
        assert!(w.chronicle.catastrophes.is_empty(), "nothing in year 0");
        assert_eq!(w.chronicle.eras.len(), 0);
        w.run_days(1);
        assert_eq!(w.chronicle.catastrophes.len(), 1, "year 1 drew one");
        let era = &w.chronicle.eras[0];
        assert_eq!((era.era, era.first_day, era.last_day), (0, 0, 7));
        assert!(!era.ranking.is_empty() && era.ranking.windows(2).all(|r| r[0].share >= r[1].share));
        let total: f64 = era.ranking.iter().map(|r| r.share).sum();
        assert!((total - 1.0).abs() < 1e-6);
        assert!(!era.niches.is_empty());
        // Lineages alive a whole year earn longevity.
        let longevity = w
            .chronicle
            .achievements
            .iter()
            .filter(|a| matches!(a.kind, AchievementKind::LineageLongevity { years: 1, .. }))
            .count();
        assert!(longevity > 0);
        assert!(w.chronicle.achievements.iter().all(|a| a.player != crate::WILD));
    }

    #[test]
    fn cohorts_of_abstract_regions_are_sampled() {
        let mut cfg = WorldConfig { seed: 6, width: 64, height: 64, ..Default::default() };
        cfg.lod.enabled = true;
        let mut w = presets::standard_world(cfg, 1);
        w.run_days(2);
        assert_eq!(w.alive_count(), 0, "no focus: everything runs as cohorts");
        let samples = w.species_samples();
        assert!(!samples.is_empty());
        let adults: f64 = samples.iter().map(|s| s.weight).sum();
        let expected: usize = w.population_counts().iter().map(|c| c.adults).sum();
        assert!((adults - expected as f64).abs() < 1.0, "{adults} vs {expected}");
        assert!(w.chronicle.species.alive().count() >= 5);
    }

    #[test]
    fn default_worlds_have_no_catastrophes() {
        let mut w = presets::standard_world(
            WorldConfig { seed: 4, width: 48, height: 48, days_per_year: 4, ..Default::default() },
            1,
        );
        w.run_days(12);
        assert!(w.chronicle.catastrophes.is_empty());
        assert_eq!(w.map.climate_temp, 0.0);
    }
}
