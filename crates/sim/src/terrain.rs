//! Map generation, seasons, weather and movement rules ("Seasons and biomes"
//! and "Movement and terrain" in the design doc).

use rand::Rng;

use crate::math::Vec2;
use crate::rng::{hash4, rng_for, stream, SimRng};
use crate::stats::Body;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Biome {
    DeepWater,
    ShallowWater,
    Marsh,
    Grassland,
    Forest,
    Desert,
    Tundra,
    Mountain,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Season {
    Spring,
    Summer,
    Autumn,
    Winter,
}

impl Season {
    pub fn from_day(day_of_year: u32, days_per_year: u32) -> Season {
        match (day_of_year * 4) / days_per_year.max(1) {
            0 => Season::Spring,
            1 => Season::Summer,
            2 => Season::Autumn,
            _ => Season::Winter,
        }
    }

    /// How strongly the season drives breeding drive.
    pub fn breeding_factor(self) -> f32 {
        match self {
            Season::Spring => 1.0,
            Season::Summer => 0.8,
            Season::Autumn => 0.35,
            Season::Winter => 0.1,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tile {
    pub biome: Biome,
    pub elevation: f32,
    pub moisture: f32,
    /// Annual mean temperature in °C.
    pub base_temp: f32,
    /// Current temperature in °C.
    pub temp: f32,
    pub water_depth: f32,
    pub ice: f32,
    pub snow: f32,
    pub mud: f32,
    pub sand: f32,
    /// 0 flat, 1 cliff.
    pub slope: f32,
    /// Vegetation density for movement (undergrowth, trunks).
    pub veg_density: f32,
    /// Plant food available (food units).
    pub veg: f32,
    pub veg_max: f32,
    /// Carcass meat on the tile (food units).
    pub meat: f32,
}

impl Tile {
    pub fn is_open(&self) -> bool {
        self.veg_density < 0.4
    }

    /// Liquid water that can be drunk.
    pub fn has_drinkable_water(&self) -> bool {
        self.water_depth > 0.0 && self.ice < 0.5
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Weather {
    /// 0 calm, 1 gale.
    pub wind: f32,
}

impl Weather {
    pub fn storm(&self) -> bool {
        self.wind > 0.8
    }

    pub fn grounds_flyers(&self) -> bool {
        self.wind > 0.7
    }
}

#[derive(Clone, Debug)]
pub struct Map {
    pub width: usize,
    pub height: usize,
    pub tiles: Vec<Tile>,
    /// Steps to the nearest drinkable tile (BFS, recomputed hourly). Lets any
    /// organism walk downhill to water without a search.
    pub water_dist: Vec<u16>,
}

/// Annual temperature swing (±) and day/night swing (±), in °C.
const SEASONAL_AMPLITUDE: f32 = 12.0;
const DIURNAL_AMPLITUDE: f32 = 4.0;

impl Map {
    pub fn generate(seed: u64, width: usize, height: usize) -> Map {
        let elev_seed = hash4(seed, stream::MAP, 1, 0);
        let moist_seed = hash4(seed, stream::MAP, 2, 0);
        let mut elevation = vec![0.0f32; width * height];
        let mut moisture = vec![0.0f32; width * height];
        for y in 0..height {
            for x in 0..width {
                let (fx, fy) = (x as f32, y as f32);
                elevation[y * width + x] = fbm(elev_seed, fx / 40.0, fy / 40.0, 5);
                moisture[y * width + x] = fbm(moist_seed, fx / 32.0, fy / 32.0, 4);
            }
        }

        // Noise clusters around 0.5; rank-normalize so biome thresholds read as
        // fractions of the map (e.g. elevation < 0.30 is the lowest 30%).
        percentile_normalize(&mut elevation);
        percentile_normalize(&mut moisture);

        let mut tiles = Vec::with_capacity(width * height);
        for y in 0..height {
            for x in 0..width {
                let e = elevation[y * width + x];
                let m = moisture[y * width + x];
                // North is cold, south is warm; altitude cools.
                let lat = y as f32 / height.max(1) as f32;
                let base_temp = 2.0 + 26.0 * lat - 20.0 * (e - 0.7).max(0.0);
                // Gradient magnitude, scaled so mountains read as steep.
                let ex = elevation[y * width + (x + 1).min(width - 1)] - elevation[y * width + x.saturating_sub(1)];
                let ey = elevation[(y + 1).min(height - 1) * width + x] - elevation[y.saturating_sub(1) * width + x];
                let slope_raw = (ex * ex + ey * ey).sqrt() * 3.0;

                let biome = classify(e, m, base_temp);
                tiles.push(make_tile(biome, e, m, base_temp, slope_raw));
            }
        }
        let water_dist = vec![u16::MAX; tiles.len()];
        let mut map = Map { width, height, tiles, water_dist };
        map.update_environment(0, 12, 120, &Weather { wind: 0.2 }, 0.25);
        map
    }

    pub fn idx(&self, x: usize, y: usize) -> usize {
        y * self.width + x
    }

    pub fn tile_coords(&self, p: Vec2) -> (usize, usize) {
        let x = (p.x.max(0.0) as usize).min(self.width - 1);
        let y = (p.y.max(0.0) as usize).min(self.height - 1);
        (x, y)
    }

    pub fn tile_at(&self, p: Vec2) -> &Tile {
        let (x, y) = self.tile_coords(p);
        &self.tiles[self.idx(x, y)]
    }

    pub fn tile_at_mut(&mut self, p: Vec2) -> &mut Tile {
        let (x, y) = self.tile_coords(p);
        let i = self.idx(x, y);
        &mut self.tiles[i]
    }

    pub fn clamp(&self, p: Vec2) -> Vec2 {
        Vec2::new(
            p.x.clamp(0.0, self.width as f32 - 0.01),
            p.y.clamp(0.0, self.height as f32 - 0.01),
        )
    }

    pub fn tile_center(x: usize, y: usize) -> Vec2 {
        Vec2::new(x as f32 + 0.5, y as f32 + 0.5)
    }

    /// Water (or snow to eat) on this tile or an adjacent one.
    pub fn can_drink_at(&self, p: Vec2) -> bool {
        let (x, y) = self.tile_coords(p);
        for ny in y.saturating_sub(1)..=(y + 1).min(self.height - 1) {
            for nx in x.saturating_sub(1)..=(x + 1).min(self.width - 1) {
                let t = &self.tiles[self.idx(nx, ny)];
                if t.has_drinkable_water() || t.snow > 0.2 {
                    return true;
                }
            }
        }
        false
    }

    fn recompute_water_dist(&mut self) {
        let mut queue = std::collections::VecDeque::new();
        for (i, t) in self.tiles.iter().enumerate() {
            if t.has_drinkable_water() || t.snow > 0.2 {
                self.water_dist[i] = 0;
                queue.push_back(i);
            } else {
                self.water_dist[i] = u16::MAX;
            }
        }
        while let Some(i) = queue.pop_front() {
            let (x, y) = (i % self.width, i / self.width);
            let d = self.water_dist[i] + 1;
            let neighbors = [
                (x > 0).then(|| i - 1),
                (x + 1 < self.width).then(|| i + 1),
                (y > 0).then(|| i - self.width),
                (y + 1 < self.height).then(|| i + self.width),
            ];
            for j in neighbors.into_iter().flatten() {
                if self.water_dist[j] > d {
                    self.water_dist[j] = d;
                    queue.push_back(j);
                }
            }
        }
    }

    /// The neighboring tile closest to water, if water is reachable at all.
    pub fn toward_water(&self, p: Vec2) -> Option<Vec2> {
        let (x, y) = self.tile_coords(p);
        let here = self.water_dist[self.idx(x, y)];
        if here == u16::MAX {
            return None;
        }
        let mut best = (here, Map::tile_center(x, y));
        for ny in y.saturating_sub(1)..=(y + 1).min(self.height - 1) {
            for nx in x.saturating_sub(1)..=(x + 1).min(self.width - 1) {
                let d = self.water_dist[self.idx(nx, ny)];
                if d < best.0 {
                    best = (d, Map::tile_center(nx, ny));
                }
            }
        }
        Some(best.1)
    }

    /// Nearest drinkable tile within a square radius.
    pub fn find_water(&self, p: Vec2, radius: i32) -> Option<Vec2> {
        let (cx, cy) = self.tile_coords(p);
        let mut best: Option<(f32, Vec2)> = None;
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let (x, y) = (cx as i32 + dx, cy as i32 + dy);
                if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
                    continue;
                }
                let t = &self.tiles[self.idx(x as usize, y as usize)];
                if t.has_drinkable_water() || t.snow > 0.2 {
                    let d = (dx * dx + dy * dy) as f32;
                    if best.is_none_or(|(bd, _)| d < bd) {
                        best = Some((d, Map::tile_center(x as usize, y as usize)));
                    }
                }
            }
        }
        best.map(|(_, c)| c)
    }

    pub fn temperature_offset(hour: u32, day_of_year: u32, days_per_year: u32) -> f32 {
        let dpy = days_per_year.max(1) as f32;
        // Temperature lags the sun by 1/16 of a year: warmest early summer,
        // coldest early winter.
        let seasonal = SEASONAL_AMPLITUDE * (std::f32::consts::TAU * (day_of_year as f32 - dpy / 16.0) / dpy).sin();
        // Warmest at 15:00.
        let diurnal = DIURNAL_AMPLITUDE * (std::f32::consts::TAU * (hour as f32 - 9.0) / 24.0).sin();
        seasonal + diurnal
    }

    /// One simulated hour of environment: temperature, snow, ice, plant
    /// regrowth and carcass decay.
    pub fn update_environment(
        &mut self,
        day_of_year: u32,
        hour: u32,
        days_per_year: u32,
        weather: &Weather,
        veg_regrowth: f32,
    ) {
        let offset = Map::temperature_offset(hour, day_of_year, days_per_year);
        let storm_chill = if weather.storm() { 3.0 } else { 0.0 };
        let hours_per_day = 24.0;
        for t in self.tiles.iter_mut() {
            t.temp = t.base_temp + offset - storm_chill;
            // Snow
            if t.temp < 0.0 && t.water_depth < 1.0 {
                t.snow = (t.snow + t.moisture * 0.006).min(1.0);
            } else if t.temp > 0.0 {
                t.snow = (t.snow - t.temp * 0.01).max(0.0);
            }
            // Ice on water: winter lakes become bridges.
            if t.water_depth > 0.0 {
                if t.temp < -2.0 {
                    t.ice = (t.ice + 0.01 * -t.temp).min(1.0);
                } else if t.temp > 0.0 {
                    t.ice = (t.ice - 0.02 * t.temp).max(0.0);
                }
            }
            // Vegetation: logistic regrowth, slowed by cold and snow cover.
            if t.veg_max > 0.0 {
                let warmth = (t.temp / 12.0).clamp(0.0, 1.0);
                let cover = (1.0 - t.snow * 3.0).clamp(0.0, 1.0);
                let growth = veg_regrowth * warmth * cover / hours_per_day;
                // Root term so grazed-out tiles recover.
                t.veg += growth * (t.veg + 0.25 * t.veg_max) * (1.0 - t.veg / t.veg_max);
                t.veg = t.veg.clamp(0.0, t.veg_max);
            }
            if t.meat > 0.0 {
                let rot = if t.temp > 15.0 { 0.95 } else { 0.98 };
                t.meat *= rot;
                if t.meat < 0.05 {
                    t.meat = 0.0;
                }
            }
        }
        self.recompute_water_dist();
    }

    pub fn land_fraction(&self) -> f32 {
        let land = self.tiles.iter().filter(|t| t.water_depth < 1.0).count();
        land as f32 / self.tiles.len() as f32
    }
}

pub fn next_weather(prev: &Weather, rng: &mut SimRng) -> Weather {
    let target: f32 = rng.gen_range(0.0..1.0);
    let wind = (prev.wind * 0.85 + target * 0.15 + crate::rng::normal(rng) * 0.05).clamp(0.0, 1.0);
    Weather { wind }
}

pub fn weather_rng(seed: u64, hour_index: u64) -> SimRng {
    rng_for(seed, stream::ENVIRONMENT, hour_index, 0)
}

fn percentile_normalize(values: &mut [f32]) {
    let mut order: Vec<usize> = (0..values.len()).collect();
    order.sort_by(|&a, &b| values[a].total_cmp(&values[b]));
    let n = (values.len().max(2) - 1) as f32;
    for (rank, &i) in order.iter().enumerate() {
        values[i] = rank as f32 / n;
    }
}

fn classify(e: f32, m: f32, temp: f32) -> Biome {
    if e < 0.16 {
        Biome::DeepWater
    } else if e < 0.22 {
        Biome::ShallowWater
    } else if e > 0.92 {
        Biome::Mountain
    } else if e < 0.30 && m > 0.5 {
        Biome::Marsh
    } else if temp < 4.0 {
        Biome::Tundra
    } else if m < 0.25 && temp > 14.0 {
        Biome::Desert
    } else if m > 0.6 {
        Biome::Forest
    } else {
        Biome::Grassland
    }
}

fn make_tile(biome: Biome, elevation: f32, moisture: f32, base_temp: f32, slope_raw: f32) -> Tile {
    let (water_depth, mud, sand, veg_density, veg_max, slope) = match biome {
        Biome::DeepWater => (3.0, 0.0, 0.0, 0.0, 0.0, 0.0),
        Biome::ShallowWater => (0.4, 0.0, 0.0, 0.0, 12.0, 0.0),
        Biome::Marsh => (0.15, 0.7, 0.0, 0.3, 30.0, 0.0),
        Biome::Grassland => (0.0, 0.0, 0.0, 0.1, 40.0, slope_raw.min(0.5)),
        Biome::Forest => (0.0, 0.0, 0.0, 0.7, 32.0, slope_raw.min(0.5)),
        Biome::Desert => (0.0, 0.0, 0.8, 0.0, 6.0, slope_raw.min(0.4)),
        Biome::Tundra => (0.0, 0.1, 0.0, 0.05, 14.0, slope_raw.min(0.5)),
        Biome::Mountain => (0.0, 0.0, 0.0, 0.1, 8.0, (0.45 + slope_raw).min(1.0)),
    };
    Tile {
        biome,
        elevation,
        moisture,
        base_temp,
        temp: base_temp,
        water_depth,
        ice: 0.0,
        snow: 0.0,
        mud,
        sand,
        slope,
        veg_density,
        veg: veg_max * 0.8,
        veg_max,
        meat: 0.0,
    }
}

// ---------------------------------------------------------------------------
// Movement rules
// ---------------------------------------------------------------------------

/// Outcome of comparing an organism's body to a tile ("Outcomes" in the doc).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Traverse {
    /// Physically impossible: pathing avoids it.
    Blocked,
    /// Speed and energy multipliers. Speed > 1 is "advantaged".
    Passable { speed: f32, energy: f32 },
}

/// Ice thickness needed to hold a body of this mass.
pub fn ice_needed(mass: f32) -> f32 {
    0.1 + mass / 200.0
}

pub fn traverse(body: &Body, tile: &Tile, weather: &Weather, flying: bool) -> Traverse {
    if flying {
        if weather.grounds_flyers() {
            return Traverse::Blocked;
        }
        // Flyers can't maneuver in dense forest.
        let speed = 1.4 * (1.0 - 0.5 * tile.veg_density);
        return Traverse::Passable { speed, energy: 1.6 };
    }

    let mut speed = 1.0f32;
    let mut energy = 1.0f32;
    let height = body.height.max(0.1);

    // Water and ice.
    let on_ice = tile.water_depth > 0.0 && tile.ice >= ice_needed(body.mass);
    if on_ice {
        // Claws grip, hooves slip.
        speed *= 1.0 - 0.6 * body.hoof_slip * (1.0 - body.grip);
    } else if tile.water_depth > 0.0 {
        if tile.water_depth > height * 0.8 {
            if body.swim < 0.25 {
                return Traverse::Blocked;
            }
            let skill = body.swim * (0.5 + 0.5 * body.buoyancy);
            speed *= 0.3 + 1.0 * skill;
            energy *= 1.6 - 0.8 * skill;
        } else {
            // Wading: tall bodies barely notice shallow water.
            speed *= 1.0 - 0.5 * (tile.water_depth / height).min(1.0);
            energy *= 1.2;
        }
    }

    // Soft ground sinks heavy, narrow-footed bodies.
    if tile.snow > 0.0 {
        let sink = tile.snow * body.ground_pressure.min(2.0) / height;
        speed /= 1.0 + sink;
        energy *= 1.0 + 0.8 * sink;
    }
    if tile.mud > 0.0 {
        let sink = tile.mud * body.ground_pressure;
        speed /= 1.0 + sink;
        energy *= 1.0 + 0.5 * sink;
    }
    if tile.sand > 0.0 {
        let sink = tile.sand * body.ground_pressure;
        speed /= 1.0 + sink;
        energy *= 1.0 + 0.5 * sink;
    }

    // Dense vegetation slows wide bodies; climbers slip through.
    if tile.veg_density > 0.0 {
        let hinder = tile.veg_density * (body.width - 0.3 * body.climb).max(0.0);
        speed *= (1.0 - 0.6 * hinder).max(0.3);
    }

    // Cliffs block non-climbers.
    if tile.slope > 0.6 && body.climb < 0.4 {
        return Traverse::Blocked;
    }
    speed *= 1.0 - 0.7 * tile.slope * (1.0 - body.grip);
    energy *= 1.0 + tile.slope;

    Traverse::Passable { speed: speed.max(0.05), energy }
}

// ---------------------------------------------------------------------------
// Noise
// ---------------------------------------------------------------------------

fn lattice(seed: u64, x: i32, y: i32) -> f32 {
    let h = hash4(seed, 0, x as u32 as u64, y as u32 as u64);
    (h >> 40) as f32 / (1u64 << 24) as f32
}

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn value_noise(seed: u64, x: f32, y: f32) -> f32 {
    let (x0, y0) = (x.floor() as i32, y.floor() as i32);
    let (tx, ty) = (smooth(x - x0 as f32), smooth(y - y0 as f32));
    let a = lattice(seed, x0, y0);
    let b = lattice(seed, x0 + 1, y0);
    let c = lattice(seed, x0, y0 + 1);
    let d = lattice(seed, x0 + 1, y0 + 1);
    let top = a + (b - a) * tx;
    let bottom = c + (d - c) * tx;
    top + (bottom - top) * ty
}

/// Fractal noise in roughly [0, 1].
fn fbm(seed: u64, x: f32, y: f32, octaves: u32) -> f32 {
    let (mut sum, mut amp, mut freq, mut norm) = (0.0, 1.0, 1.0, 0.0);
    for o in 0..octaves {
        sum += amp * value_noise(seed.wrapping_add(o as u64), x * freq, y * freq);
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / norm
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::{BirthType, FounderDesign, Thermo};
    use crate::genome::{Gene, Genome};
    use crate::stats::derive;

    fn body(d: FounderDesign) -> Body {
        derive(&Genome::uniform(&d.values(), 1).phenotype()).body
    }

    fn base() -> FounderDesign {
        FounderDesign::new("t", Thermo::Warm, BirthType::Live, 2)
    }

    fn tile(biome: Biome) -> Tile {
        make_tile(biome, 0.5, 0.5, 15.0, 0.0)
    }

    const CALM: Weather = Weather { wind: 0.1 };

    #[test]
    fn deep_water_blocks_non_swimmers_and_admits_swimmers() {
        let deep = tile(Biome::DeepWater);
        assert_eq!(traverse(&body(base()), &deep, &CALM, false), Traverse::Blocked);
        let swimmer = body(base().with(Gene::Swimming, 0.9).with(Gene::FatStores, 0.5));
        assert!(matches!(traverse(&swimmer, &deep, &CALM, false), Traverse::Passable { .. }));
    }

    #[test]
    fn thick_ice_is_a_bridge_but_hooves_slip() {
        let mut lake = tile(Biome::DeepWater);
        lake.ice = 1.0;
        let hoofed = body(base().with(Gene::Hooves, 1.0));
        let clawed = body(base().with(Gene::Claws, 1.0));
        let (Traverse::Passable { speed: hs, .. }, Traverse::Passable { speed: cs, .. }) =
            (traverse(&hoofed, &lake, &CALM, false), traverse(&clawed, &lake, &CALM, false))
        else {
            panic!("ice should be passable");
        };
        assert!(cs > hs);
    }

    #[test]
    fn wide_feet_help_in_snow() {
        let mut snowy = tile(Biome::Tundra);
        snowy.snow = 1.0;
        let narrow = body(base().with(Gene::BodySize, 0.7).with(Gene::Hooves, 1.0));
        let wide = body(base().with(Gene::BodySize, 0.7).with(Gene::FootWidth, 1.0));
        let speed = |b: &Body| match traverse(b, &snowy, &CALM, false) {
            Traverse::Passable { speed, .. } => speed,
            Traverse::Blocked => 0.0,
        };
        assert!(speed(&wide) > speed(&narrow));
    }

    #[test]
    fn cliffs_need_climbing_and_storms_ground_flyers() {
        let mut cliff = tile(Biome::Mountain);
        cliff.slope = 0.9;
        assert_eq!(traverse(&body(base()), &cliff, &CALM, false), Traverse::Blocked);
        let climber = body(base().with(Gene::Climbing, 0.8));
        assert!(matches!(traverse(&climber, &cliff, &CALM, false), Traverse::Passable { .. }));
        let storm = Weather { wind: 0.95 };
        assert_eq!(traverse(&climber, &cliff, &storm, true), Traverse::Blocked);
    }

    #[test]
    fn map_generation_is_deterministic_and_mixed() {
        let a = Map::generate(9, 64, 64);
        let b = Map::generate(9, 64, 64);
        assert_eq!(a.tiles, b.tiles);
        let land = a.land_fraction();
        assert!(land > 0.3 && land < 0.98, "land fraction {land}");
    }
}
