//! The tile grid for the client's map (`GET /api/terrain` and
//! `GET /api/terrain/dynamic`).
//!
//! The world thread copies the map into byte layers ([`TerrainRaw`],
//! [`DynamicRaw`]: one `u8` per tile, row-major) and the HTTP handler
//! base64-encodes them off that thread. A 128×128 map is 16 KiB per layer;
//! the dynamic part (water, ice, snow, vegetation, wetness) is five layers,
//! about 110 KB of JSON, which the client polls every few seconds.

use base64::Engine;
use evo_sim::terrain::{Biome, Map};
use evo_sim::World;

use crate::protocol::{season_name, TerrainDynamic, TerrainView};

pub fn biome_index(b: Biome) -> u8 {
    match b {
        Biome::DeepWater => 0,
        Biome::ShallowWater => 1,
        Biome::Marsh => 2,
        Biome::Grassland => 3,
        Biome::Forest => 4,
        Biome::Desert => 5,
        Biome::Tundra => 6,
        Biome::Mountain => 7,
    }
}

/// `v` in `0..=max` to `0..=255`, saturating.
pub fn quantize(v: f32, max: f32) -> u8 {
    if !v.is_finite() || v <= 0.0 {
        return 0;
    }
    (v / max * 255.0).round().min(255.0) as u8
}

/// Meters to whole centimeters, capped at 2.55 m.
pub fn centimeters(m: f32) -> u8 {
    quantize(m, 2.55)
}

#[derive(Clone, Debug, PartialEq)]
pub struct DynamicRaw {
    pub tick: u64,
    pub day: u64,
    pub hour: u32,
    pub season: &'static str,
    pub water: Vec<u8>,
    pub ice: Vec<u8>,
    pub snow: Vec<u8>,
    pub veg: Vec<u8>,
    pub wetness: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TerrainRaw {
    pub width: usize,
    pub height: usize,
    pub terrain_version: u64,
    pub biome: Vec<u8>,
    pub elevation: Vec<u8>,
    pub river: Vec<u8>,
    pub base_water: Vec<u8>,
    pub dynamic: DynamicRaw,
}

fn layer(map: &Map, f: impl Fn(&evo_sim::terrain::Tile) -> u8) -> Vec<u8> {
    map.tiles.iter().map(f).collect()
}

fn ground_layer(map: &Map, f: impl Fn(&evo_sim::terrain::Ground) -> u8) -> Vec<u8> {
    map.ground().iter().map(f).collect()
}

pub fn dynamic_raw(world: &World) -> DynamicRaw {
    let m = &world.map;
    DynamicRaw {
        tick: world.tick(),
        day: world.day(),
        hour: world.hour(),
        season: season_name(world.season()),
        water: layer(m, |t| centimeters(t.water_depth)),
        ice: layer(m, |t| quantize(t.ice, 1.0)),
        snow: layer(m, |t| quantize(t.snow, 0.5)),
        veg: m
            .ground()
            .iter()
            .zip(&m.tiles)
            .map(|(g, t)| if g.veg_max > 0.0 { quantize(t.veg / g.veg_max, 1.0) } else { 0 })
            .collect(),
        wetness: layer(m, |t| quantize(t.wetness, 1.0)),
    }
}

pub fn terrain_raw(world: &World) -> TerrainRaw {
    let m = &world.map;
    TerrainRaw {
        width: m.width,
        height: m.height,
        terrain_version: m.terrain_version,
        biome: ground_layer(m, |g| biome_index(g.biome)),
        elevation: ground_layer(m, |g| quantize(g.elevation, 1.0)),
        river: ground_layer(m, |g| quantize(g.river, 1.0)),
        base_water: ground_layer(m, |g| centimeters(g.base_water)),
        dynamic: dynamic_raw(world),
    }
}

fn b64(v: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(v)
}

impl DynamicRaw {
    pub fn encode(&self) -> TerrainDynamic {
        TerrainDynamic {
            tick: self.tick,
            day: self.day,
            hour: self.hour,
            season: self.season.into(),
            water: b64(&self.water),
            ice: b64(&self.ice),
            snow: b64(&self.snow),
            veg: b64(&self.veg),
            wetness: b64(&self.wetness),
        }
    }
}

impl TerrainRaw {
    pub fn encode(&self) -> TerrainView {
        TerrainView {
            width: self.width,
            height: self.height,
            terrain_version: self.terrain_version,
            biome: b64(&self.biome),
            elevation: b64(&self.elevation),
            river: b64(&self.river),
            base_water: b64(&self.base_water),
            dynamic: self.dynamic.encode(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{build_world, WorldSettings};

    fn decode(s: &str) -> Vec<u8> {
        base64::engine::general_purpose::STANDARD.decode(s).unwrap()
    }

    #[test]
    fn layers_cover_every_tile_and_round_trip() {
        let mut world =
            build_world(&WorldSettings { seed: 5, width: 48, height: 40, wild: false, ..Default::default() });
        world.run_days(1);
        let raw = terrain_raw(&world);
        let view = raw.encode();
        assert_eq!((view.width, view.height), (48, 40));
        for layer in [
            &view.biome,
            &view.elevation,
            &view.river,
            &view.base_water,
            &view.dynamic.water,
            &view.dynamic.snow,
            &view.dynamic.wetness,
        ] {
            assert_eq!(decode(layer).len(), 48 * 40);
        }
        let biome = decode(&view.biome);
        for (k, g) in world.map.ground().iter().enumerate() {
            assert_eq!(biome[k], biome_index(g.biome));
        }
        // Water tiles have depth, the highest tile is near 255.
        let water = decode(&view.dynamic.water);
        assert!(world.map.tiles.iter().zip(&water).any(|(t, &w)| t.water_depth > 0.1 && w > 0));
        assert!(*decode(&view.elevation).iter().max().unwrap() >= 250);
        assert_eq!(view.dynamic.tick, world.tick());
    }

    #[test]
    fn quantization_saturates() {
        assert_eq!(quantize(-1.0, 1.0), 0);
        assert_eq!(quantize(f32::NAN, 1.0), 0);
        assert_eq!(quantize(0.5, 1.0), 128);
        assert_eq!(quantize(3.0, 1.0), 255);
        assert_eq!(centimeters(0.42), 42);
        assert_eq!(centimeters(9.0), 255);
    }
}
