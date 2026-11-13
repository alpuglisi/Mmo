//! Hourly and daily world updates.

use super::World;
use crate::legacy::LegacySnapshot;
use crate::rng::SimRng;
use crate::terrain::weather_rng;

impl World {
    pub(super) fn hourly(&mut self) {
        let hour_index = self.tick / self.config.ticks_per_hour();
        let mut rng = weather_rng(self.config.seed, hour_index);
        let bal = self.config.balance;
        let (doy, dpy) = (self.day_of_year(), self.config.days_per_year);
        self.map.advance_weather(hour_index, doy, dpy, &bal);
        self.weather = self.map.mean_weather();
        self.season_offset = crate::terrain::Map::seasonal_offset(doy, dpy);
        self.update_clutches(&mut rng);
        self.density.rebuild(&self.organisms, self.lineages.len());
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
        self.rebuild_caches();

        let adults = || self.organisms.iter().filter(|o| o.is_breeding_adult());
        self.group.update(adults().map(|o| (o.pos, &o.pheno)));
        let mut snapshot = LegacySnapshot::compute(self.day(), adults().map(|o| (&o.genome, o.offspring_raised)));
        if self.config.lod.enabled {
            self.lod_legacy(&mut snapshot);
        }
        self.legacy.push(snapshot);

        if self.day_of_year().is_multiple_of(self.config.days_per_season()) {
            self.slots_used.clear();
        }
    }

    /// Incubation: eggs chill without enough heat, hatch on schedule.
    fn update_clutches(&mut self, rng: &mut SimRng) {
        let tick = self.tick;
        let clutches = std::mem::take(&mut self.clutches);
        let mut keep = Vec::with_capacity(clutches.len());
        for mut c in clutches {
            let guardian = c.guardian.and_then(|g| self.index.get(&g).copied()).filter(|&gi| self.organisms[gi].alive);
            let mut done = c.eggs.is_empty();

            if !done && tick >= c.hatch_tick {
                let room = self.cap_room(c.lineage);
                let hatching: Vec<_> = c.eggs.drain(..).take(room).collect();
                self.counters.hatched += hatching.len() as u64;
                for g in hatching {
                    self.spawn_newborn(g, c.owner, c.lineage, c.pos, c.generation, c.mother, c.father, c.newborn_growth, rng);
                }
                done = true;
            }

            if !done {
                let temp = self.map.tile_at(c.pos).temp;
                let present = guardian.filter(|&gi| self.organisms[gi].pos.dist(c.pos) < 1.5);
                let heat = match present {
                    Some(gi) if self.organisms[gi].traits.warm => 32.0,
                    Some(_) => temp + 4.0,
                    None => temp,
                };
                // Warm-blooded eggs need far more heat (thermoregulation mismatch
                // for abandoned eggs).
                let need = if c.warm_blooded { 30.0 } else { 16.0 };
                if heat < need - 6.0 {
                    c.chill += (need - 6.0 - heat) * 0.006;
                } else {
                    c.chill = (c.chill - 0.02).max(0.0);
                }
                if c.chill >= 1.0 {
                    self.counters.eggs_lost += c.eggs.len() as u64;
                    done = true;
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
