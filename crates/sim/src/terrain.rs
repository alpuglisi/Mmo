//! Map generation, rivers, seasons, regional weather and movement rules
//! ("Seasons and biomes" and "Movement and terrain" in the design doc).
//!
//! The environment has three layers:
//! - **Geography**, fixed at generation: elevation and moisture noise give
//!   biomes; rivers are carved by flow accumulation down a pit-free drainage
//!   network, from high, wet ground to lakes, the sea or the map edge.
//! - **Regional weather** on a coarse grid of [`CHUNK`]-sized cells, advanced
//!   once per simulated hour ([`Map::advance_weather`]): fronts drifting
//!   east bring wind, storms and rain; slow-moving dry and wet spells and the
//!   seasons set how much rain falls; each cell integrates rain, snowmelt and
//!   evaporation into a soil `wetness` (0.5 normal, low is drought). Weather
//!   is a pure function of `(seed, hour, cell)` apart from that integration.
//! - **Tile state**, updated chunk by chunk ([`Map::update_chunks`], every
//!   chunk once per simulated hour, a share of them each tick): temperature,
//!   snowfall and melt, ice, water levels (shallow lakes, marsh and rivers
//!   follow regional wetness, rivers also swell with upstream snowmelt),
//!   plant regrowth and carcass decay.
//!
//! Hooks for other systems: [`Map::weather_at`] samples local weather for
//! [`traverse`]; `Tile::wetness`, [`Map::dryness_at`], [`Map::drought`] and
//! [`Map::flood`] are cheap drought and flood signals; [`Map::terrain_version`]
//! and [`Map::chunk_version`] change whenever a tile's passability-relevant
//! fields (water depth, ice, current) cross a 0.1 step, so path caches know
//! when to invalidate.

use std::f32::consts::TAU;

use crate::config::Balance;
use crate::math::Vec2;
use crate::rng::{hash4, rng_for, stream, SimRng};
use crate::stats::Body;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Tile {
    pub biome: Biome,
    pub elevation: f32,
    pub moisture: f32,
    /// Annual mean temperature in °C.
    pub base_temp: f32,
    /// Current temperature in °C.
    pub temp: f32,
    /// Current water depth in m. Shallow lakes, marsh and rivers rise and
    /// fall with regional wetness (0 when dried out); deep water is fixed.
    pub water_depth: f32,
    /// Water depth at normal wetness (0 for dry land).
    pub base_water: f32,
    /// River channel: base current at normal flow, 0..1 (0 = not a river).
    pub river: f32,
    /// Current flow speed, 0..1; deep fast water needs stronger swimmers.
    pub current: f32,
    pub ice: f32,
    pub snow: f32,
    pub mud: f32,
    pub sand: f32,
    /// 0 flat, 1 cliff.
    pub slope: f32,
    /// Vegetation density for movement (undergrowth, trunks).
    pub veg_density: f32,
    /// Canopy height in m (forest); browse up there needs height or climbing.
    pub canopy: f32,
    /// Plant food available (food units).
    pub veg: f32,
    pub veg_max: f32,
    /// Carcass meat on the tile (food units).
    pub meat: f32,
    /// Local wind, 0 calm .. 1 gale (from the regional weather grid).
    pub wind: f32,
    /// Local precipitation, 0..1 (falls as snow below freezing).
    pub rain: f32,
    /// Regional soil wetness, 0 parched .. 1 soaked, 0.5 normal.
    pub wetness: f32,
}

impl Tile {
    pub fn is_open(&self) -> bool {
        self.veg_density < 0.4
    }

    /// Liquid water that can be drunk.
    pub fn has_drinkable_water(&self) -> bool {
        self.water_depth > 0.0 && self.ice < 0.5
    }

    pub fn is_river(&self) -> bool {
        self.river > 0.0
    }

    /// Plant food on this tile out of reach of `body`: a share of a canopy
    /// tile's browse grows above what the body can reach, unless it climbs.
    pub fn browse_out_of_reach(&self, body: &Body, canopy_share: f32) -> f32 {
        if self.canopy <= 0.0 || body.climb >= CLIMB_CANOPY {
            return 0.0;
        }
        let reach = (body.height / (0.6 * self.canopy)).clamp(0.0, 1.0);
        self.veg_max * canopy_share * (1.0 - reach)
    }

    /// Plant food `body` can reach on this tile.
    pub fn reachable_veg(&self, body: &Body, canopy_share: f32) -> f32 {
        (self.veg - self.browse_out_of_reach(body, canopy_share)).max(0.0)
    }

    /// Bits that change when the tile's passability-relevant state crosses a
    /// 0.1 step (see [`Map::terrain_version`]).
    fn pass_signature(&self) -> u32 {
        let q = |v: f32| ((v * 10.0) as u32).min(255);
        q(self.water_depth) | q(self.ice) << 8 | q(self.current) << 16
    }

    fn water_source(&self) -> bool {
        self.has_drinkable_water() || self.snow > 0.2
    }
}

/// Climbing gene value from which a body uses the canopy: reaches all browse
/// and takes refuge from ground predators.
pub const CLIMB_CANOPY: f32 = 0.4;

/// Search radius up to which [`Map::find_water`] looks nearest first.
const NEAR_RADIUS: i32 = 3;
const NEAR_SIDE: usize = 2 * NEAR_RADIUS as usize + 1;

/// Tile offsets `(dx, dy)` within [`NEAR_RADIUS`], by squared distance and
/// then row by row (`dy`, then `dx`), the order in which a row-major scan
/// keeping the first strictly nearer tile settles ties.
const NEAR_OFFSETS: [(i32, i32); NEAR_SIDE * NEAR_SIDE] = {
    let mut out = [(0, 0); NEAR_SIDE * NEAR_SIDE];
    let mut n = 0;
    let mut dy = -NEAR_RADIUS;
    while dy <= NEAR_RADIUS {
        let mut dx = -NEAR_RADIUS;
        while dx <= NEAR_RADIUS {
            // Insertion sort by squared distance; row-major arrival keeps it
            // stable, so equal distances stay in row order.
            let d = dx * dx + dy * dy;
            let mut k = n;
            while k > 0 && out[k - 1].0 * out[k - 1].0 + out[k - 1].1 * out[k - 1].1 > d {
                out[k] = out[k - 1];
                k -= 1;
            }
            out[k] = (dx, dy);
            n += 1;
            dx += 1;
        }
        dy += 1;
    }
    out
};

/// How much a canopy shelters a climbing prey from a non-climbing,
/// non-flying attacker: the chance its attack succeeds is cut by this share.
pub fn canopy_refuge(tile: &Tile, prey: &Body, attacker: &Body) -> f32 {
    if tile.canopy > 0.5 && prey.climb >= CLIMB_CANOPY && attacker.climb < CLIMB_CANOPY && !attacker.can_fly {
        0.7 * prey.climb
    } else {
        0.0
    }
}

/// Local weather at a point, as [`traverse`] reads it.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Weather {
    /// 0 calm, 1 gale.
    pub wind: f32,
}

/// Wind above which a storm blows (chill, exposure on open ground).
pub const STORM_WIND: f32 = 0.8;
/// Wind above which flyers are grounded.
pub const GROUNDING_WIND: f32 = 0.7;

impl Weather {
    pub fn storm(&self) -> bool {
        self.wind > STORM_WIND
    }

    pub fn grounds_flyers(&self) -> bool {
        self.wind > GROUNDING_WIND
    }
}

/// Tiles per side of an environment chunk, which is also a weather cell.
pub const CHUNK: usize = 16;

/// One cell of the regional weather grid.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct WeatherCell {
    /// 0 calm, 1 gale.
    pub wind: f32,
    /// Precipitation, 0..1.
    pub rain: f32,
    /// Soil wetness, 0 parched .. 1 soaked, 0.5 normal.
    pub wetness: f32,
    /// Smoothed snowmelt (snow per tile per hour, decaying 3% an hour);
    /// swells rivers that rise here.
    pub melt: f32,
    land_tiles: u32,
    river_tiles: u32,
}

impl WeatherCell {
    /// River level relative to normal flow (1 normal; below 1 low water,
    /// above swollen).
    pub fn river_level(&self, bal: &Balance) -> f32 {
        river_level(self.wetness, self.melt, bal)
    }
}

#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Map {
    pub width: usize,
    pub height: usize,
    pub tiles: Vec<Tile>,
    /// Steps to the nearest drinkable tile (BFS, recomputed hourly when chunk
    /// updates changed where water is). Lets any organism walk downhill to
    /// water without a search.
    pub water_dist: Vec<u16>,
    /// Chunks (and weather cells) per row and column.
    pub chunks_x: usize,
    pub chunks_y: usize,
    /// Regional weather, `chunks_x × chunks_y`, row-major.
    pub cells: Vec<WeatherCell>,
    /// Weather cell of each river tile's headwaters (the tile's own cell for
    /// other tiles): rivers swell with snowmelt upstream.
    pub catchment: Vec<u32>,
    /// Bumped whenever a chunk update changes some tile's water depth, ice
    /// or current by a 0.1 step. Path caches compare it to invalidate.
    pub terrain_version: u64,
    /// Per-chunk counterpart of `terrain_version`, for local invalidation.
    pub chunk_version: Vec<u64>,
    /// Share of land in drought (regional wetness below `DROUGHT_WETNESS`),
    /// 0..1, updated hourly.
    pub drought: f32,
    /// Share of river tiles in flood (river level above `FLOOD_LEVEL`), 0..1,
    /// updated hourly.
    pub flood: f32,
    /// Climate anomaly in °C added to every tile's temperature (0 normally;
    /// a severe winter lowers it, see `chronicle`).
    pub climate_temp: f32,
    /// Rain-potential anomaly added to every weather cell (0 normally;
    /// negative in a megadrought, positive in a flood year).
    pub climate_wet: f32,
    weather_seed: u64,
    /// Snowmelt per tile gathered by chunk updates since the last weather step.
    melt_acc: Vec<f32>,
    pass_sig: Vec<u32>,
    water_dirty: bool,
}

/// Annual temperature swing (±) and day/night swing (±), in °C.
const SEASONAL_AMPLITUDE: f32 = 12.0;
const DIURNAL_AMPLITUDE: f32 = 4.0;
/// Flow accumulation (rain-weighted upstream tiles) that makes a river.
const RIVER_FLOW: f32 = 60.0;
/// Water shallower than this counts as dried out.
const WATER_GONE: f32 = 0.03;
/// River level rise per unit of smoothed snowmelt above `MELT_BASE`
/// (× `snowmelt_swell`). The threshold keeps the daily melt of light winter
/// snow from swelling rivers; the spring thaw of a deep snowpack does.
const MELT_SWELL: f32 = 10.0;
const MELT_BASE: f32 = 0.08;
/// Regional wetness below which a cell is in drought.
pub const DROUGHT_WETNESS: f32 = 0.3;
/// River level (× normal) above which a river is in flood.
pub const FLOOD_LEVEL: f32 = 1.6;
/// Standard deviation of 3- and 2-octave `fbm` around 0.5 (measured), to
/// turn noise into z-scores.
const NOISE_SD3: f32 = 0.14;
const NOISE_SD2: f32 = 0.16;
/// Wetness gained per hour per unit of rain (× the dry share).
const WET_GAIN: f32 = 0.22;
/// Wetness lost per hour (× wetness), before the seasonal factor.
const EVAPORATION: f32 = 0.003;
/// Wetness gained per unit of snowmelt (× the dry share).
const MELT_WET: f32 = 0.6;
/// Relative swing of rainfall and evaporation over the year.
const RAIN_SEASONALITY: f32 = 0.3;
const EVAP_SEASONALITY: f32 = 0.3;

/// Water level of shallow lakes and marsh relative to normal for a regional
/// wetness (1 at 0.5; 0 means dried out).
pub fn water_level(wetness: f32, bal: &Balance) -> f32 {
    (1.0 + bal.water_response * (wetness - 0.5) / 0.38).clamp(0.0, 1.6)
}

/// River level relative to normal flow for the wetness along its course and
/// the smoothed snowmelt at its headwaters. Big rivers keep a trickle in a
/// drought; small streams dry out.
pub fn river_level(wetness: f32, melt: f32, bal: &Balance) -> f32 {
    water_level(wetness, bal).max(0.15) + bal.snowmelt_swell * (melt - MELT_BASE).max(0.0) * MELT_SWELL
}

/// Per-tile inputs to an environment update.
struct TileEnv {
    offset: f32,
    wind: f32,
    rain: f32,
    wet: f32,
    river_wet: f32,
    river_melt: f32,
}

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

        let (chunks_x, chunks_y) = (width.div_ceil(CHUNK), height.div_ceil(CHUNK));
        let catchment = carve_rivers(&mut tiles, width, height, chunks_x);
        let mut cells =
            vec![
                WeatherCell { wind: 0.3, rain: 0.0, wetness: 0.5, melt: 0.0, land_tiles: 0, river_tiles: 0 };
                chunks_x * chunks_y
            ];
        for (i, t) in tiles.iter().enumerate() {
            let c = &mut cells[(i / width / CHUNK) * chunks_x + (i % width) / CHUNK];
            c.land_tiles += (t.base_water < 1.0) as u32;
            c.river_tiles += t.is_river() as u32;
        }
        let n = tiles.len();
        let mut map = Map {
            width,
            height,
            tiles,
            water_dist: vec![u16::MAX; n],
            chunks_x,
            chunks_y,
            chunk_version: vec![0; cells.len()],
            melt_acc: vec![0.0; cells.len()],
            cells,
            catchment,
            terrain_version: 0,
            drought: 0.0,
            flood: 0.0,
            climate_temp: 0.0,
            climate_wet: 0.0,
            weather_seed: hash4(seed, stream::ENVIRONMENT, u64::MAX, 0),
            pass_sig: vec![0; n],
            water_dirty: true,
        };
        map.update_environment(0, 0, 12, 120, &Balance::default());
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
        Vec2::new(p.x.clamp(0.0, self.width as f32 - 0.01), p.y.clamp(0.0, self.height as f32 - 0.01))
    }

    pub fn tile_center(x: usize, y: usize) -> Vec2 {
        Vec2::new(x as f32 + 0.5, y as f32 + 0.5)
    }

    /// Chunk (and weather cell) index of a tile.
    pub fn chunk_of(&self, x: usize, y: usize) -> usize {
        (y / CHUNK) * self.chunks_x + x / CHUNK
    }

    /// Chunk (and weather cell) index of a point.
    pub fn chunk_at(&self, p: Vec2) -> usize {
        let (x, y) = self.tile_coords(p);
        self.chunk_of(x, y)
    }

    /// Local weather at a point, for [`traverse`] and flight checks.
    pub fn weather_at(&self, p: Vec2) -> Weather {
        Weather { wind: self.tile_at(p).wind }
    }

    /// Regional dryness at a point: 0 soaked, 0.5 normal, 1 parched.
    pub fn dryness_at(&self, p: Vec2) -> f32 {
        1.0 - self.tile_at(p).wetness
    }

    /// Map-wide average weather (for reports).
    pub fn mean_weather(&self) -> Weather {
        let wind = self.cells.iter().map(|c| c.wind).sum::<f32>() / self.cells.len().max(1) as f32;
        Weather { wind }
    }

    /// Water (or snow to eat) on this tile or an adjacent one.
    pub fn can_drink_at(&self, p: Vec2) -> bool {
        let (x, y) = self.tile_coords(p);
        for ny in y.saturating_sub(1)..=(y + 1).min(self.height - 1) {
            for nx in x.saturating_sub(1)..=(x + 1).min(self.width - 1) {
                if self.tiles[self.idx(nx, ny)].water_source() {
                    return true;
                }
            }
        }
        false
    }

    fn recompute_water_dist(&mut self) {
        let mut queue = std::collections::VecDeque::new();
        for (i, t) in self.tiles.iter().enumerate() {
            if t.water_source() {
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
        if radius <= NEAR_RADIUS {
            // Nearest first, ties in row order: the first water found is the
            // one the full scan below would pick.
            for &(dx, dy) in &NEAR_OFFSETS {
                let (x, y) = (cx as i32 + dx, cy as i32 + dy);
                if dx.abs() > radius
                    || dy.abs() > radius
                    || x < 0
                    || y < 0
                    || x >= self.width as i32
                    || y >= self.height as i32
                {
                    continue;
                }
                if self.tiles[self.idx(x as usize, y as usize)].water_source() {
                    return Some(Map::tile_center(x as usize, y as usize));
                }
            }
            return None;
        }
        self.find_water_scan(p, radius)
    }

    /// [`Map::find_water`] by scanning the whole square, row by row.
    fn find_water_scan(&self, p: Vec2, radius: i32) -> Option<Vec2> {
        let (cx, cy) = self.tile_coords(p);
        let mut best: Option<(f32, Vec2)> = None;
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let (x, y) = (cx as i32 + dx, cy as i32 + dy);
                if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
                    continue;
                }
                let t = &self.tiles[self.idx(x as usize, y as usize)];
                if t.water_source() {
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
        Map::seasonal_offset(day_of_year, days_per_year) + Map::diurnal_offset(hour)
    }

    /// The day/night part of [`Map::temperature_offset`]: warmest at 15:00.
    pub fn diurnal_offset(hour: u32) -> f32 {
        DIURNAL_AMPLITUDE * (TAU * (hour as f32 - 9.0) / 24.0).sin()
    }

    /// The seasonal part of [`Map::temperature_offset`], without the
    /// day/night swing: a tile's `base_temp` plus this is its seasonal mean.
    pub fn seasonal_offset(day_of_year: u32, days_per_year: u32) -> f32 {
        let dpy = days_per_year.max(1) as f32;
        // Temperature lags the sun by 1/16 of a year: warmest early summer,
        // coldest early winter.
        SEASONAL_AMPLITUDE * (TAU * (day_of_year as f32 - dpy / 16.0) / dpy).sin()
    }

    /// One simulated hour of weather and environment over the whole map at
    /// once (generation, tests). The world spreads the chunk updates over the
    /// ticks of each hour instead.
    pub fn update_environment(
        &mut self,
        hour_index: u64,
        day_of_year: u32,
        hour: u32,
        days_per_year: u32,
        bal: &Balance,
    ) {
        self.advance_weather(hour_index, day_of_year, days_per_year, bal);
        self.update_chunks(0, 1, day_of_year, hour, days_per_year, bal);
    }

    /// Advance the regional weather grid by one hour: fronts, wind, rain,
    /// wetness, snowmelt, and the map-level drought and flood signals.
    pub fn advance_weather(&mut self, hour_index: u64, day_of_year: u32, days_per_year: u32, bal: &Balance) {
        let dpy = days_per_year.max(1) as f32;
        let doy = day_of_year as f32;
        // Wettest in late winter and spring, driest in late summer.
        let rain_season = RAIN_SEASONALITY * (TAU * (doy + dpy / 12.0) / dpy).cos();
        // Evaporation follows the seasonal temperature (diurnal term is 0 at 9:00).
        let warm = Map::temperature_offset(9, day_of_year, days_per_year) / SEASONAL_AMPLITUDE;
        let evaporation = EVAPORATION * (1.0 + EVAP_SEASONALITY * warm);
        // Stormiest in late autumn.
        let storminess = 0.05 * (TAU * (doy - 0.625 * dpy) / dpy).cos();
        let h = hour_index as f64;
        let seed = self.weather_seed;
        // Noise coordinates in units of `scale` tiles, drifting with time.
        let at = |x: f64, y: f64, vx: f64, vy: f64, scale: f64| {
            (((x - vx * h) / scale) as f32, ((y - vy * h) / scale) as f32)
        };
        let z = |n: f32| (n - 0.5) / NOISE_SD3;
        let (mut dry_land, mut land, mut flooded, mut rivers) = (0u32, 0u32, 0u32, 0u32);
        for (c, cell) in self.cells.iter_mut().enumerate() {
            let x = ((c % self.chunks_x) * CHUNK) as f64 + CHUNK as f64 / 2.0;
            let y = ((c / self.chunks_x) * CHUNK) as f64 + CHUNK as f64 / 2.0;
            // Fronts: two layers drifting at different speeds, so their shapes
            // evolve as they cross the map west to east.
            let (ax, ay) = at(x, y, 0.9, 0.25, 36.0);
            let (bx, by) = at(x, y, 0.5, -0.35, 28.0);
            let front = (z(fbm(seed, ax, ay, 3)) + z(fbm(seed ^ 0xF0, bx, by, 3))) * std::f32::consts::FRAC_1_SQRT_2;
            let (gx, gy) = at(x, y, 1.6, 0.5, 14.0);
            let gust = (fbm(seed ^ 0x6A, gx, gy, 2) - 0.5) / NOISE_SD2;
            // Dry and wet spells: a broad, slow pattern.
            let (sx, sy) = at(x, y, -0.06, 0.08, 70.0);
            let spell = z(fbm(seed ^ 0x5E, sx, sy, 3));

            cell.wind = (0.42 + 0.12 * front + 0.11 * gust + storminess).clamp(0.0, 1.0);
            let potential = front + 0.9 * spell + 1.5 * rain_season + self.climate_wet;
            cell.rain = ((potential - 1.0) * 0.35).clamp(0.0, 1.0);
            let melt = std::mem::take(&mut self.melt_acc[c]);
            cell.melt = cell.melt * 0.97 + melt;
            let dry_share = 1.0 - cell.wetness;
            cell.wetness += (WET_GAIN * cell.rain + MELT_WET * melt) * dry_share - evaporation * cell.wetness;
            cell.wetness = cell.wetness.clamp(0.0, 1.0);

            land += cell.land_tiles;
            if cell.wetness < DROUGHT_WETNESS {
                dry_land += cell.land_tiles;
            }
            rivers += cell.river_tiles;
            if cell.river_level(bal) > FLOOD_LEVEL {
                flooded += cell.river_tiles;
            }
        }
        self.drought = dry_land as f32 / land.max(1) as f32;
        self.flood = flooded as f32 / rivers.max(1) as f32;
    }

    /// Update the chunks whose index is `phase` modulo `phases` for one
    /// simulated hour. Calling it with every phase in turn (one per tick)
    /// updates each chunk once; the order is fixed, so runs stay
    /// deterministic. The water-distance field is refreshed after the last
    /// phase if any chunk changed where water is.
    pub fn update_chunks(
        &mut self,
        phase: u64,
        phases: u64,
        day_of_year: u32,
        hour: u32,
        days_per_year: u32,
        bal: &Balance,
    ) {
        let offset = Map::temperature_offset(hour, day_of_year, days_per_year) + self.climate_temp;
        let n = self.cells.len();
        let mut c = phase as usize;
        while c < n {
            self.update_chunk(c, offset, bal);
            c += phases.max(1) as usize;
        }
        // Once per hour, after the last share of chunks.
        if self.water_dirty && phase + 1 >= phases {
            self.recompute_water_dist();
            self.water_dirty = false;
        }
    }

    /// Weather cell values at a tile, interpolated between cell centers.
    fn sample_cells(&self, x: usize, y: usize) -> (f32, f32, f32) {
        let axis = |v: usize, n: usize| {
            let f = ((v as f32 + 0.5) / CHUNK as f32 - 0.5).clamp(0.0, (n - 1) as f32);
            let i0 = f as usize;
            (i0, (i0 + 1).min(n - 1), f - i0 as f32)
        };
        let (x0, x1, tx) = axis(x, self.chunks_x);
        let (y0, y1, ty) = axis(y, self.chunks_y);
        let cell = |cx: usize, cy: usize| &self.cells[cy * self.chunks_x + cx];
        let mix = |f: fn(&WeatherCell) -> f32| {
            let top = f(cell(x0, y0)) + (f(cell(x1, y0)) - f(cell(x0, y0))) * tx;
            let bottom = f(cell(x0, y1)) + (f(cell(x1, y1)) - f(cell(x0, y1))) * tx;
            top + (bottom - top) * ty
        };
        (mix(|c| c.wind), mix(|c| c.rain), mix(|c| c.wetness))
    }

    fn update_chunk(&mut self, c: usize, offset: f32, bal: &Balance) {
        let (x0, y0) = ((c % self.chunks_x) * CHUNK, (c / self.chunks_x) * CHUNK);
        let (x1, y1) = ((x0 + CHUNK).min(self.width), (y0 + CHUNK).min(self.height));
        let (mut melt, mut changed) = (0.0f32, false);
        for y in y0..y1 {
            for x in x0..x1 {
                let i = y * self.width + x;
                let (wind, rain, wet) = self.sample_cells(x, y);
                let source = &self.cells[self.catchment[i] as usize];
                let env = TileEnv {
                    offset,
                    wind,
                    rain,
                    wet,
                    river_wet: 0.5 * (wet + source.wetness),
                    river_melt: source.melt.max(self.cells[c].melt),
                };
                let t = &mut self.tiles[i];
                let had_water = t.water_source();
                melt += update_tile(t, &env, bal);
                self.water_dirty |= had_water != t.water_source();
                let sig = t.pass_signature();
                if sig != self.pass_sig[i] {
                    self.pass_sig[i] = sig;
                    changed = true;
                }
            }
        }
        self.melt_acc[c] += melt / ((x1 - x0) * (y1 - y0)).max(1) as f32;
        if changed {
            self.chunk_version[c] += 1;
            self.terrain_version += 1;
        }
    }

    pub fn land_fraction(&self) -> f32 {
        let land = self.tiles.iter().filter(|t| t.water_depth < 1.0).count();
        land as f32 / self.tiles.len() as f32
    }
}

/// One hour of a tile's environment. Returns the snow that melted.
fn update_tile(t: &mut Tile, e: &TileEnv, bal: &Balance) -> f32 {
    t.wind = e.wind;
    t.rain = e.rain;
    t.wetness = e.wet;
    // Storms chill, most of all on open ground.
    let chill = if e.wind > STORM_WIND { 3.0 + if t.is_open() { bal.storm_exposure } else { 0.0 } } else { 0.0 };
    t.temp = t.base_temp + e.offset - chill;

    // Precipitation falls as snow below freezing; snow melts above.
    let mut melt = 0.0;
    if t.temp < 0.0 && t.water_depth < 1.0 {
        t.snow = (t.snow + (0.002 + 0.025 * e.rain) * (0.4 + t.moisture)).min(1.0);
    } else if t.temp > 0.0 && t.snow > 0.0 {
        melt = t.snow.min(t.temp * 0.01);
        t.snow -= melt;
    }

    // Water levels: rivers follow wetness along their course and swell with
    // snowmelt at their headwaters; shallow lakes and marsh follow local
    // wetness. Deep water stays.
    if t.river > 0.0 {
        let level = river_level(e.river_wet, e.river_melt, bal);
        t.water_depth = t.base_water * level;
        t.current = (t.river * (0.6 + 0.4 * level)).min(1.0);
    } else if t.base_water > 0.0 && t.base_water < 1.0 {
        let level = water_level(e.wet, bal);
        t.water_depth = t.base_water * level;
        if t.biome == Biome::Marsh {
            t.mud = 0.7 * level.clamp(0.4, 1.0);
        }
    }
    if t.water_depth < WATER_GONE {
        t.water_depth = 0.0;
        t.ice = 0.0;
    }

    // Ice: winter lakes become bridges; currents resist freezing.
    if t.water_depth > 0.0 {
        if t.temp < -2.0 - 6.0 * t.current {
            t.ice = (t.ice + 0.01 * -t.temp).min(1.0);
        } else if t.temp > 0.0 {
            t.ice = (t.ice - 0.02 * t.temp).max(0.0);
        }
    }

    // Vegetation: logistic regrowth, slowed by cold, snow cover and drought.
    if t.veg_max > 0.0 {
        let warmth = (t.temp / 12.0).clamp(0.0, 1.0);
        let cover = (1.0 - t.snow * 3.0).clamp(0.0, 1.0);
        let wet = 1.0 - bal.rain_growth + bal.rain_growth * (e.wet / 0.5).clamp(0.0, 1.4);
        let growth = bal.veg_regrowth * warmth * cover * wet / 24.0;
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
    melt
}

/// Carve rivers into land tiles and return each tile's catchment cell.
///
/// A priority flood from the sea, lakes and map edge gives every land tile a
/// downstream neighbor with no pits (depressions drain over their lowest
/// rim). Each tile then sheds rain weighted by moisture and altitude
/// downstream; tiles that gather more than `RIVER_FLOW` become river.
fn carve_rivers(tiles: &mut [Tile], width: usize, height: usize, chunks_x: usize) -> Vec<u32> {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    let n = width * height;
    let cell_of = |i: usize| ((i / width / CHUNK) * chunks_x + (i % width) / CHUNK) as u32;
    let is_water = |t: &Tile| matches!(t.biome, Biome::DeepWater | Biome::ShallowWater);
    let mut down = vec![usize::MAX; n];
    let mut seen = vec![false; n];
    let mut heap = BinaryHeap::new();
    for (i, t) in tiles.iter().enumerate() {
        let (x, y) = (i % width, i / width);
        if is_water(t) || x == 0 || y == 0 || x + 1 == width || y + 1 == height {
            seen[i] = true;
            // Elevation is in [0, 1], so its bits sort like the value.
            heap.push(Reverse((t.elevation.to_bits(), i)));
        }
    }
    let mut order = Vec::with_capacity(n);
    while let Some(Reverse((bits, i))) = heap.pop() {
        order.push(i);
        let filled = f32::from_bits(bits);
        let (x, y) = (i % width, i / width);
        for ny in y.saturating_sub(1)..=(y + 1).min(height - 1) {
            for nx in x.saturating_sub(1)..=(x + 1).min(width - 1) {
                let j = ny * width + nx;
                if !seen[j] {
                    seen[j] = true;
                    down[j] = i;
                    heap.push(Reverse((tiles[j].elevation.max(filled).to_bits(), j)));
                }
            }
        }
    }

    // Upstream tiles come later in the flood order, so walking it backwards
    // finishes every tile's inflow before passing it on.
    let mut flow: Vec<f32> = tiles.iter().map(|t| 0.25 + t.moisture + 0.5 * t.elevation).collect();
    let mut catchment: Vec<u32> = (0..n).map(cell_of).collect();
    let mut main_inflow = vec![usize::MAX; n];
    for &i in order.iter().rev() {
        if is_water(&tiles[i]) {
            continue;
        }
        let d = down[i];
        if flow[i] >= RIVER_FLOW {
            if main_inflow[i] != usize::MAX {
                catchment[i] = catchment[main_inflow[i]];
            }
            let strength = (flow[i] / RIVER_FLOW).ln();
            let drop = if d == usize::MAX { 0.0 } else { (tiles[i].elevation - tiles[d].elevation).max(0.0) };
            let t = &mut tiles[i];
            t.river = (0.15 + 0.06 * strength + 8.0 * drop).clamp(0.1, 0.8);
            t.base_water = (0.1 + 0.08 * strength).min(0.45).max(t.base_water);
            t.water_depth = t.base_water;
            t.current = t.river;
            t.veg_density *= 0.3;
            t.sand = 0.0;
            t.veg_max = t.veg_max.min(15.0);
            t.veg = t.veg_max * 0.8;
            t.canopy = 0.0;
            if d != usize::MAX && (main_inflow[d] == usize::MAX || flow[i] > flow[main_inflow[d]]) {
                main_inflow[d] = i;
            }
        }
        if d != usize::MAX {
            flow[d] += flow[i];
        }
    }
    catchment
}

/// The hourly environment stream (clutch incubation draws from it).
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
        // Shallower toward the shore, so lakes shrink from the edges.
        Biome::ShallowWater => (0.15 + 0.5 * ((0.22 - elevation) / 0.06).clamp(0.0, 1.0), 0.0, 0.0, 0.0, 12.0, 0.0),
        Biome::Marsh => (0.15, 0.7, 0.0, 0.3, 30.0, 0.0),
        Biome::Grassland => (0.0, 0.0, 0.0, 0.1, 40.0, slope_raw.min(0.5)),
        Biome::Forest => (0.0, 0.0, 0.0, 0.7, 32.0, slope_raw.min(0.5)),
        Biome::Desert => (0.0, 0.0, 0.8, 0.0, 6.0, slope_raw.min(0.4)),
        Biome::Tundra => (0.0, 0.1, 0.0, 0.05, 14.0, slope_raw.min(0.5)),
        Biome::Mountain => (0.0, 0.0, 0.0, 0.1, 8.0, (0.45 + slope_raw).min(1.0)),
    };
    let canopy = if biome == Biome::Forest { 0.9 + 2.0 * (moisture - 0.6).max(0.0) } else { 0.0 };
    Tile {
        biome,
        elevation,
        moisture,
        base_temp,
        temp: base_temp,
        water_depth,
        base_water: water_depth,
        river: 0.0,
        current: 0.0,
        ice: 0.0,
        snow: 0.0,
        mud,
        sand,
        slope,
        veg_density,
        canopy,
        veg: veg_max * 0.8,
        veg_max,
        meat: 0.0,
        wind: 0.3,
        rain: 0.0,
        wetness: 0.5,
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
            // A current needs a stronger swimmer: swollen rivers block weak
            // swimmers that cross the same river easily at low water.
            if body.swim < 0.25 + 0.5 * tile.current {
                return Traverse::Blocked;
            }
            let skill = body.swim * (0.5 + 0.5 * body.buoyancy);
            speed *= (0.3 + 1.0 * skill) * (1.0 - 0.4 * tile.current);
            energy *= (1.6 - 0.8 * skill) * (1.0 + 0.5 * tile.current);
        } else {
            // Wading: tall bodies barely notice shallow water.
            speed *= (1.0 - 0.5 * (tile.water_depth / height).min(1.0)) * (1.0 - 0.3 * tile.current);
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

    // Storms make open ground hard going.
    if weather.storm() && tile.is_open() {
        speed *= 0.85;
        energy *= 1.3;
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

    fn speed(b: &Body, t: &Tile) -> Option<f32> {
        match traverse(b, t, &CALM, false) {
            Traverse::Passable { speed, .. } => Some(speed),
            Traverse::Blocked => None,
        }
    }

    #[test]
    fn swollen_river_blocks_weak_swimmers_but_not_when_low_or_frozen() {
        let walker = body(base().with(Gene::BodySize, 0.4).with(Gene::LegLength, 0.5));
        let paddler = body(base().with(Gene::BodySize, 0.4).with(Gene::Swimming, 0.4));
        let swimmer = body(base().with(Gene::BodySize, 0.4).with(Gene::Swimming, 0.9));
        let mut river = tile(Biome::Grassland);
        river.river = 0.4;
        // Low water: everyone wades or swims across.
        river.water_depth = 0.3 * walker.height;
        river.current = 0.3;
        assert!(speed(&walker, &river).is_some());
        // Spring flood: deep and fast. Only the strong swimmer gets across.
        river.water_depth = 2.0 * walker.height;
        river.current = 0.9;
        assert_eq!(speed(&walker, &river), None);
        assert_eq!(speed(&paddler, &river), None);
        assert!(speed(&swimmer, &river).is_some());
        // The same depth in still water admits the weak swimmer.
        river.current = 0.0;
        assert!(speed(&paddler, &river).is_some());
        // Frozen: a bridge for the walker.
        river.current = 0.9;
        river.ice = 1.0;
        assert!(speed(&walker, &river).is_some());
    }

    /// The biggest river tile, so its normal depth is well above a trickle.
    fn river_tile(map: &Map) -> usize {
        (0..map.tiles.len())
            .filter(|&i| map.tiles[i].is_river())
            .max_by(|&a, &b| map.tiles[a].base_water.total_cmp(&map.tiles[b].base_water).then(b.cmp(&a)))
            .expect("map has rivers")
    }

    #[test]
    fn rivers_run_to_water_and_swell_with_snowmelt() {
        let mut map = Map::generate(9, 64, 64);
        let land = map.tiles.iter().filter(|t| t.base_water < 1.0).count();
        let rivers = map.tiles.iter().filter(|t| t.is_river()).count();
        assert!(rivers * 100 > land && rivers * 8 < land, "{rivers} river tiles on {land} land tiles");
        // Every river tile continues into another river tile, a lake or the
        // sea, or leaves the map.
        for (i, t) in map.tiles.iter().enumerate().filter(|(_, t)| t.is_river()) {
            let (x, y) = (i % 64, i / 64);
            let edge = x == 0 || y == 0 || x == 63 || y == 63;
            let next = (y.saturating_sub(1)..=(y + 1).min(63))
                .flat_map(|ny| (x.saturating_sub(1)..=(x + 1).min(63)).map(move |nx| ny * 64 + nx))
                .filter(|&j| j != i)
                .any(|j| map.tiles[j].is_river() || map.tiles[j].base_water >= 0.3);
            assert!(edge || next, "river tile at ({x},{y}) leads nowhere: {t:?}");
        }

        let bal = Balance::default();
        let i = river_tile(&map);
        let wader = body(base().with(Gene::BodySize, 0.62).with(Gene::LegLength, 0.6));
        map.cells.iter_mut().for_each(|c| c.wetness = 0.5);
        map.update_chunks(0, 1, 40, 12, 120, &bal);
        let normal = map.tiles[i].water_depth;
        assert!(normal > 0.0 && speed(&wader, &map.tiles[i]).is_some(), "depth {normal}");
        // Spring thaw upstream: the river swells and blocks the wader.
        let source = map.catchment[i] as usize;
        map.cells[source].melt = 0.5;
        map.update_chunks(0, 1, 10, 12, 120, &bal);
        let t = &map.tiles[i];
        assert!(t.water_depth > 2.0 * normal, "{} vs {normal}", t.water_depth);
        assert_eq!(speed(&wader, t), None);
        // Winter: the swollen river freezes over and becomes a bridge.
        map.tiles[i].ice = 1.0;
        assert!(speed(&wader, &map.tiles[i]).is_some());
    }

    #[test]
    fn drought_shrinks_water_and_wet_spells_restore_it() {
        let mut map = Map::generate(4, 64, 64);
        let bal = Balance::default();
        let drinkable = |m: &Map| m.tiles.iter().filter(|t| t.has_drinkable_water()).count();
        let set_wetness = |m: &mut Map, w: f32| m.cells.iter_mut().for_each(|c| c.wetness = w);
        set_wetness(&mut map, 0.5);
        map.update_chunks(0, 1, 40, 12, 120, &bal);
        let normal = drinkable(&map);
        let dist: u64 = map.water_dist.iter().map(|&d| d as u64).sum();
        let version = map.terrain_version;

        set_wetness(&mut map, 0.05);
        map.update_chunks(0, 1, 40, 12, 120, &bal);
        let dry = drinkable(&map);
        assert!(dry < normal * 9 / 10, "drought: {dry} drinkable tiles vs {normal}");
        // Organisms have further to walk to water, and paths know terrain changed.
        assert!(map.water_dist.iter().map(|&d| d as u64).sum::<u64>() > dist);
        assert!(map.terrain_version > version);
        assert!(map
            .tiles
            .iter()
            .filter(|t| t.biome == Biome::Marsh && !t.is_river())
            .all(|t| t.water_depth < t.base_water));

        set_wetness(&mut map, 0.8);
        map.update_chunks(0, 1, 40, 12, 120, &bal);
        assert!(drinkable(&map) >= normal);
        // With the response off, lake and marsh levels ignore the weather.
        let fixed = Balance { water_response: 0.0, ..bal };
        set_wetness(&mut map, 0.05);
        map.update_chunks(0, 1, 40, 12, 120, &fixed);
        assert!(map.tiles.iter().filter(|t| !t.is_river()).all(|t| t.water_depth == t.base_water));
        // The map-level signal follows regional wetness.
        set_wetness(&mut map, 0.05);
        map.advance_weather(1, 40, 120, &bal);
        assert!(map.drought > 0.9, "drought share {}", map.drought);
    }

    #[test]
    fn regional_weather_varies_across_the_map_and_over_time() {
        let mut map = Map::generate(3, 128, 128);
        let bal = Balance::default();
        let (mut spread, mut wet_spread, mut rainy_and_dry) = (0.0f32, 0.0f32, false);
        let mut first = Vec::new();
        for h in 1..=96u64 {
            map.update_environment(h, 30 + (h / 24) as u32, (h % 24) as u32, 120, &bal);
            let winds: Vec<f32> = map.cells.iter().map(|c| c.wind).collect();
            let max = winds.iter().copied().fold(f32::MIN, f32::max);
            let min = winds.iter().copied().fold(f32::MAX, f32::min);
            spread = spread.max(max - min);
            let wet: Vec<f32> = map.cells.iter().map(|c| c.wetness).collect();
            wet_spread = wet_spread
                .max(wet.iter().copied().fold(f32::MIN, f32::max) - wet.iter().copied().fold(f32::MAX, f32::min));
            rainy_and_dry |= map.cells.iter().any(|c| c.rain > 0.1) && map.cells.iter().any(|c| c.rain == 0.0);
            if h == 1 {
                first = winds;
            }
        }
        assert!(spread > 0.2, "wind spread {spread}");
        assert!(wet_spread > 0.1, "wetness spread {wet_spread}");
        assert!(rainy_and_dry, "rain should fall in some regions and not others");
        // Weather moves: the wind field an hour in differs from 4 days later.
        assert_ne!(first, map.cells.iter().map(|c| c.wind).collect::<Vec<_>>());
        // Tiles sample the grid smoothly; `weather_at` reads the tile.
        let p = Vec2::new(40.3, 70.8);
        assert_eq!(map.weather_at(p).wind, map.tile_at(p).wind);
        let (a, b) = (map.tiles[map.idx(40, 70)].wind, map.tiles[map.idx(41, 70)].wind);
        assert!((a - b).abs() < 0.1);
    }

    #[test]
    fn chunked_updates_match_a_full_update_and_are_deterministic() {
        let bal = Balance::default();
        let mut full = Map::generate(6, 80, 72);
        let mut sliced = Map::generate(6, 80, 72);
        for h in 1..=30u64 {
            let (doy, hour) = (100 + (h / 24) as u32, (h % 24) as u32);
            full.update_environment(h, doy, hour, 120, &bal);
            sliced.advance_weather(h, doy, 120, &bal);
            for phase in 0..3 {
                sliced.update_chunks(phase, 3, doy, hour, 120, &bal);
            }
        }
        assert_eq!(full.tiles, sliced.tiles);
        assert_eq!(full.cells, sliced.cells);
        assert_eq!(full.water_dist, sliced.water_dist);
        assert_eq!(full.terrain_version, sliced.terrain_version);
    }

    #[test]
    fn canopy_browse_needs_height_or_climbing() {
        let mut forest = tile(Biome::Forest);
        forest.canopy = 1.2;
        forest.veg = forest.veg_max;
        let short = body(base().with(Gene::BodySize, 0.1));
        let tall = body(base().with(Gene::BodySize, 0.7).with(Gene::LegLength, 0.8));
        let climber = body(base().with(Gene::BodySize, 0.1).with(Gene::Climbing, 0.8));
        let reach = |b: &Body, t: &Tile| t.reachable_veg(b, 0.3);
        assert!(reach(&short, &forest) < reach(&tall, &forest));
        assert_eq!(reach(&climber, &forest), forest.veg);
        assert_eq!(reach(&tall, &forest), forest.veg);
        // Grazed down to the browse line, the short body finds nothing.
        forest.veg = forest.browse_out_of_reach(&short, 0.3);
        assert_eq!(reach(&short, &forest), 0.0);
        assert!(canopy_refuge(&forest, &climber, &tall) > 0.0);
        assert_eq!(canopy_refuge(&forest, &short, &tall), 0.0);
        assert_eq!(canopy_refuge(&tile(Biome::Grassland), &climber, &tall), 0.0);
    }

    #[test]
    fn storms_make_open_ground_costly() {
        let open = tile(Biome::Grassland);
        let storm = Weather { wind: 0.9 };
        let b = body(base());
        let (Traverse::Passable { speed: s0, energy: e0 }, Traverse::Passable { speed: s1, energy: e1 }) =
            (traverse(&b, &open, &CALM, false), traverse(&b, &open, &storm, false))
        else {
            panic!("grassland should be passable");
        };
        assert!(s1 < s0 && e1 > e0);
    }

    #[test]
    fn map_generation_is_deterministic_and_mixed() {
        let a = Map::generate(9, 64, 64);
        let b = Map::generate(9, 64, 64);
        assert_eq!(a.tiles, b.tiles);
        assert_eq!(a.cells, b.cells);
        assert_eq!(a.catchment, b.catchment);
        let land = a.land_fraction();
        assert!(land > 0.3 && land < 0.98, "land fraction {land}");
    }

    /// The nearest-first water search picks the tile the full scan does,
    /// ties included, at every radius it handles and off the map's edges.
    #[test]
    fn nearest_first_water_search_matches_full_scan() {
        for seed in 0..4 {
            let mut map = Map::generate(seed, 40, 36);
            // Speckle water about so that ties and misses both occur.
            for (k, t) in map.tiles.iter_mut().enumerate() {
                let h = crate::rng::splitmix64(k as u64 ^ (seed << 32));
                (t.water_depth, t.ice, t.snow) =
                    (if h.is_multiple_of(23) { 0.5 } else { 0.0 }, if h.is_multiple_of(7) { 1.0 } else { 0.0 }, 0.0);
            }
            for y in 0..36 {
                for x in 0..40 {
                    let p = Vec2::new(x as f32 + 0.3, y as f32 + 0.6);
                    for r in 0..=NEAR_RADIUS {
                        assert_eq!(
                            map.find_water(p, r),
                            map.find_water_scan(p, r),
                            "seed {seed} at {x},{y} radius {r}"
                        );
                    }
                }
            }
        }
    }
}
