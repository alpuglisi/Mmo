//! Debug rendering of the world to an RGB buffer (biomes, rivers, dried-out
//! water, snow, ice, vegetation and organisms colored by owner).

use crate::genome::{PlayerId, WILD};
use crate::terrain::{Biome, Ground, Tile};
use crate::world::World;

pub fn owner_color(owner: PlayerId, carnivory: f32) -> [u8; 3] {
    if owner == WILD {
        return if carnivory > 0.5 { [120, 30, 30] } else { [235, 225, 200] };
    }
    const PALETTE: [[u8; 3]; 8] = [
        [230, 60, 60],
        [60, 140, 255],
        [255, 200, 0],
        [200, 80, 255],
        [0, 220, 200],
        [255, 130, 40],
        [255, 105, 180],
        [140, 255, 80],
    ];
    PALETTE[(owner as usize - 1) % PALETTE.len()]
}

fn tile_color(g: &Ground, t: &Tile) -> [u8; 3] {
    let base: [f32; 3] = match g.biome {
        Biome::DeepWater => [20.0, 50.0, 120.0],
        Biome::ShallowWater => [40.0, 90.0, 160.0],
        Biome::Marsh => [60.0, 90.0, 70.0],
        Biome::Grassland => [110.0, 150.0, 60.0],
        Biome::Forest => [30.0, 90.0, 40.0],
        Biome::Desert => [210.0, 190.0, 130.0],
        Biome::Tundra => [150.0, 160.0, 140.0],
        Biome::Mountain => [120.0, 110.0, 100.0],
    };
    // Grazed-out land looks browner.
    let veg = if g.veg_max > 0.0 { t.veg / g.veg_max } else { 1.0 };
    let mut c = base.map(|v| v * (0.6 + 0.4 * veg));
    // Rivers: brighter blue, darker when swollen; a dry bed shows as sand.
    if g.is_river() {
        c = if t.water_depth > 0.0 {
            let deep = t.water_depth.min(1.0);
            [70.0 - 40.0 * deep, 140.0 - 60.0 * deep, 225.0 - 40.0 * deep]
        } else {
            [175.0, 160.0, 120.0]
        };
    } else if g.base_water > 0.0 && g.base_water < 1.0 && t.water_depth == 0.0 {
        // Dried-out lake shore or marsh: cracked mud.
        c = [150.0, 130.0, 95.0];
    }
    if t.water_depth > 0.0 && t.ice > 0.3 {
        c = [190.0, 215.0, 235.0];
    }
    let snow = (t.snow / 0.5).min(1.0) * 0.85;
    c = c.map(|v| v + (245.0 - v) * snow);
    c.map(|v| v.clamp(0.0, 255.0) as u8)
}

/// Returns `(width, height, rgb bytes)` at `scale` pixels per tile.
pub fn render_rgb(world: &World, scale: usize) -> (usize, usize, Vec<u8>) {
    let scale = scale.max(1);
    let (w, h) = (world.map.width * scale, world.map.height * scale);
    let mut buf = vec![0u8; w * h * 3];
    let put = |x: usize, y: usize, c: [u8; 3], buf: &mut [u8]| {
        if x < w && y < h {
            let k = (y * w + x) * 3;
            buf[k..k + 3].copy_from_slice(&c);
        }
    };
    for ty in 0..world.map.height {
        for tx in 0..world.map.width {
            let i = world.map.idx(tx, ty);
            let c = tile_color(&world.map.ground()[i], &world.map.tiles[i]);
            for dy in 0..scale {
                for dx in 0..scale {
                    put(tx * scale + dx, ty * scale + dy, c, &mut buf);
                }
            }
        }
    }
    let dot = (scale / 2).max(1);
    for o in world.organisms.iter().filter(|o| o.alive) {
        let c = owner_color(o.owner, o.traits.carnivory);
        let (px, py) = ((o.pos.x * scale as f32) as usize, (o.pos.y * scale as f32) as usize);
        for dy in 0..dot {
            for dx in 0..dot {
                put(px + dx, py + dy, c, &mut buf);
            }
        }
    }
    (w, h, buf)
}
