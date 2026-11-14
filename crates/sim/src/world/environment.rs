//! Hourly and daily world updates.

use super::World;
use crate::config::Balance;
use crate::legacy::LegacySnapshot;
use crate::rng::SimRng;
use crate::terrain::{weather_rng, Ground, Map, Tile};
use rand::Rng;

/// Tiles around a clutch in which other clutches draw egg predators to it
/// (`Balance::nest_predation`).
const NEST_CROWD_RADIUS: usize = 2;

/// Chilling per hour per °C an egg sits below its floor.
const CHILL_RATE: f32 = 0.006;
/// Most °C a sitting parent can cool her eggs by shading them.
pub(crate) const BROOD_SHADE: f32 = 3.0;
/// The egg temperature a sitting warm-blooded parent aims for.
const WARM_BROOD_TEMP: f32 = 32.0;
/// A cold-blooded parent warms her eggs toward her sun-warmed body, by at
/// most `COLD_BROOD_LIFT`.
const COLD_BROOD_TEMP: f32 = 22.0;
const COLD_BROOD_LIFT: f32 = 4.0;

/// The temperatures between which eggs neither chill nor overheat, in °C:
/// warm-blooded eggs need a parent's heat, cold-blooded ones develop at
/// ambient temperatures but cook sooner.
pub(crate) fn egg_range(warm_blooded: bool) -> (f32, f32) {
    if warm_blooded {
        (24.0, 39.0)
    } else {
        (10.0, 34.0)
    }
}

/// How much a nest site damps the air's swing toward the tile's annual mean
/// temperature: water has thermal inertia, a canopy shades and shelters.
pub(crate) fn nest_shelter(g: &Ground, tile: &Tile) -> f32 {
    if tile.water_depth > 0.0 {
        0.5
    } else if g.canopy > 0.0 {
        0.3
    } else {
        0.0
    }
}

/// The temperature eggs on `tile` feel when the air there is `air`.
pub(crate) fn nest_temp(g: &Ground, tile: &Tile, air: f32, climate_temp: f32) -> f32 {
    let mean = g.base_temp + climate_temp;
    mean + (air - mean) * (1.0 - nest_shelter(g, tile))
}

/// A sitting parent's effect on her eggs at nest temperature `site`:
/// `(egg temperature, °C warmed, °C shaded)`. She warms them toward her
/// brooding temperature, by at most `brood_warmth` (warm-blooded) or
/// `COLD_BROOD_LIFT`, and in the heat shades them by at most `BROOD_SHADE`.
pub(crate) fn brood(site: f32, warm: bool, bal: &Balance) -> (f32, f32, f32) {
    let (target, lift) = if warm { (WARM_BROOD_TEMP, bal.brood_warmth) } else { (COLD_BROOD_TEMP, COLD_BROOD_LIFT) };
    if site < target {
        let warmed = (target - site).min(lift.max(0.0));
        (site + warmed, warmed, 0.0)
    } else {
        let (_, hi) = egg_range(warm);
        // Shading only matters once the eggs would come near their ceiling.
        let shaded = (site - (hi - BROOD_SHADE)).clamp(0.0, BROOD_SHADE).min(site - target);
        (site - shaded, 0.0, shaded)
    }
}

/// Expected thermal damage to eggs over one day at a nest site whose
/// seasonal mean air temperature is `mean_air` (the day/night swing on top).
pub(crate) fn nest_stress(
    g: &Ground,
    tile: &Tile,
    mean_air: f32,
    climate_temp: f32,
    warm_blooded: bool,
    bal: &Balance,
) -> f32 {
    let (lo, hi) = egg_range(warm_blooded);
    (0..24)
        .map(|h| {
            let air = mean_air + Map::diurnal_offset(h);
            let t = nest_temp(g, tile, air, climate_temp);
            (lo - t).max(0.0) * CHILL_RATE + (t - hi).max(0.0) * bal.egg_overheat
        })
        .sum()
}

impl World {
    pub(super) fn hourly(&mut self) {
        let hour_index = self.tick / self.config.ticks_per_hour();
        let mut rng = weather_rng(self.config.seed, hour_index);
        let bal = self.config.balance;
        let (doy, dpy) = (self.day_of_year(), self.config.days_per_year);
        self.map.advance_weather(hour_index, doy, dpy, &bal);
        self.weather = self.map.mean_weather();
        self.season_offset = crate::terrain::Map::seasonal_offset(doy, dpy) + self.map.climate_temp;
        self.update_clutches(&mut rng);
        self.density.rebuild(&self.organisms, self.lineages.len());
        self.spoil_shared_caches();
        self.rebuild_caches();
    }

    /// Every tick: this tick's share of the environment chunks, so each chunk
    /// updates once per simulated hour and the cost is spread evenly.
    pub(super) fn environment_slice(&mut self) {
        let tph = self.config.ticks_per_hour();
        let (doy, hour, dpy) = (self.day_of_year(), self.hour(), self.config.days_per_year);
        let bal = self.config.balance;
        self.map.update_chunks(self.tick % tph, tph, doy, hour, dpy, &bal);
    }

    pub(super) fn daily(&mut self) {
        // Remove the dead; indices are stable within a day.
        self.organisms.retain(|o| o.alive);
        self.index.clear();
        for (k, o) in self.organisms.iter().enumerate() {
            self.index.insert(o.id, k);
        }
        self.owner_counts.clear();
        self.lineage_alive.clear();
        for o in &self.organisms {
            *self.owner_counts.entry(o.owner).or_default() += 1;
            let l = o.lineage as usize;
            if self.lineage_alive.len() <= l {
                self.lineage_alive.resize(l + 1, 0);
            }
            self.lineage_alive[l] += 1;
        }
        self.refresh_offline_protection();
        self.rebuild_caches();

        let adults = || self.organisms.iter().filter(|o| o.is_breeding_adult());
        self.group.update(adults().map(|o| (o.pos, &o.pheno)));
        let mut snapshot = LegacySnapshot::compute(self.day(), adults().map(|o| (&*o.genome, o.offspring_raised)));
        if self.config.lod.enabled {
            self.lod_legacy(&mut snapshot);
        }
        self.legacy.push(snapshot);

        if self.day_of_year().is_multiple_of(self.config.days_per_season()) {
            self.slots_used.clear();
        }
        self.chronicle_daily();
    }

    /// Other clutches within `NEST_CROWD_RADIUS` tiles of `pos` (one per
    /// tile: `clutch_at`), what egg predators key on.
    pub(crate) fn clutches_near(&self, pos: crate::math::Vec2) -> u32 {
        let (cx, cy) = self.map.tile_coords(pos);
        let r = NEST_CROWD_RADIUS;
        let mut n = 0;
        for y in cy.saturating_sub(r)..=(cy + r).min(self.map.height - 1) {
            for x in cx.saturating_sub(r)..=(cx + r).min(self.map.width - 1) {
                if (x, y) != (cx, cy) && self.clutch_at.contains_key(&(x, y)) {
                    n += 1;
                }
            }
        }
        n
    }

    /// Incubation: eggs chill without enough heat, hatch on schedule.
    pub(crate) fn update_clutches(&mut self, rng: &mut SimRng) {
        let tick = self.tick;
        // Egg predators search nesting grounds (`Balance::nest_predation`):
        // count each clutch's neighbors before any is removed.
        let predation = self.config.balance.nest_predation;
        let crowd: Vec<u32> = if predation > 0.0 {
            self.clutches.iter().map(|c| self.clutches_near(c.pos)).collect()
        } else {
            Vec::new()
        };
        let clutches = std::mem::take(&mut self.clutches);
        let mut keep = Vec::with_capacity(clutches.len());
        for (ci, mut c) in clutches.into_iter().enumerate() {
            let guardian = c.guardian.and_then(|g| self.index.get(&g).copied()).filter(|&gi| self.organisms[gi].alive);
            let mut done = c.eggs.is_empty();
            // An unguarded clutch may be found and eaten whole.
            if !done
                && c.guardian.is_none()
                && crowd.get(ci).is_some_and(|&n| n > 0 && rng.gen::<f32>() < predation * n as f32 / 24.0)
            {
                self.counters.eggs.entry(c.lineage).or_default().raided += c.eggs.len() as u64;
                self.counters.eggs_lost += c.eggs.len() as u64;
                done = true;
            }

            if !done && tick >= c.hatch_tick {
                let room = self.cap_room(c.lineage);
                let hatching: Vec<_> = c.eggs.drain(..).take(room).collect();
                self.counters.hatched += hatching.len() as u64;
                self.counters.eggs.entry(c.lineage).or_default().hatched += hatching.len() as u64;
                for g in hatching {
                    self.spawn_newborn(
                        g,
                        c.owner,
                        c.lineage,
                        c.pos,
                        c.generation,
                        c.mother,
                        c.father,
                        c.newborn_growth,
                        rng,
                    );
                }
                // The brood is over: the parent recovers from now (`brood_recovery`).
                if let Some(gi) = guardian {
                    let o = &mut self.organisms[gi];
                    let rest = (self.config.balance.brood_recovery
                        * o.traits.recovery_days
                        * self.config.ticks_per_day as f32) as u64;
                    o.breed_ready_tick = o.breed_ready_tick.max(tick + rest);
                }
                done = true;
            }

            if !done {
                let bal = self.config.balance;
                let tile = self.map.tile_at(c.pos);
                let site = nest_temp(self.map.ground_at(c.pos), tile, tile.temp, self.map.climate_temp);
                let present = guardian.filter(|&gi| self.organisms[gi].pos.dist(c.pos) < 1.5);
                let (lo, hi) = egg_range(c.warm_blooded);
                let egg = match present {
                    Some(gi) => {
                        let o = &mut self.organisms[gi];
                        let (egg, warmed, shaded) = brood(site, o.traits.warm, &bal);
                        // Brooding costs the parent (`brood_cost`): warming
                        // burns food, shading in the heat costs water.
                        let per_hour = 1.0 / 24.0;
                        let hunger = bal.hunger_rate * o.traits.stats.metabolism * per_hour;
                        o.needs.hunger = (o.needs.hunger + bal.brood_cost * warmed / 10.0 * hunger).min(100.0);
                        o.needs.thirst =
                            (o.needs.thirst + bal.brood_cost * shaded / 10.0 * bal.thirst_rate * per_hour).min(100.0);
                        egg
                    }
                    None => site,
                };
                // Warm-blooded eggs need far more heat (thermoregulation
                // mismatch for abandoned eggs).
                if egg < lo {
                    c.chill += (lo - egg) * CHILL_RATE;
                } else {
                    c.chill = (c.chill - 0.02).max(0.0);
                }
                if egg > hi {
                    c.overheat += (egg - hi) * bal.egg_overheat;
                } else {
                    c.overheat = (c.overheat - 0.01).max(0.0);
                }
                let counts = self.counters.eggs.entry(c.lineage).or_default();
                if c.chill >= 1.0 {
                    counts.chilled += c.eggs.len() as u64;
                    done = true;
                } else if c.overheat >= 1.0 {
                    counts.overheated += c.eggs.len() as u64;
                    done = true;
                }
                if done {
                    self.counters.eggs_lost += c.eggs.len() as u64;
                }
            }

            if done {
                if let Some(gi) = guardian {
                    self.organisms[gi].nest = None;
                }
            } else {
                keep.push(c);
            }
        }
        self.clutches = keep;
        self.clutch_at.clear();
        for (k, c) in self.clutches.iter().enumerate() {
            self.clutch_at.insert(self.map.tile_coords(c.pos), k);
        }
    }
}
