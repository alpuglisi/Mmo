//! The film's map art, after the client's (`client/src/world/STYLE.md`,
//! `palette.ts`, `colors.ts`): soft painted ground in the client's ground,
//! season and water tokens, light from the top-left, and flat-shaded
//! creatures with one highlight, a uniform dark outline and a contact
//! shadow offset down-right, tinted per lineage by the client's golden-angle
//! hues.

use evo_sim::terrain::{Biome, Ground, Season, Tile};
use evo_sim::World;

use super::raster::{Canvas, Rgb};

// Tokens from client/src/world/palette.ts.
const GRASS: [f32; 3] = [126.0, 166.0, 88.0];
const FOREST: [f32; 3] = [58.0, 112.0, 70.0];
const MARSH: [f32; 3] = [92.0, 124.0, 88.0];
const DESERT: [f32; 3] = [222.0, 200.0, 146.0];
const TUNDRA: [f32; 3] = [166.0, 172.0, 150.0];
const MOUNTAIN: [f32; 3] = [130.0, 119.0, 104.0];
const PEAK: [f32; 3] = [172.0, 164.0, 152.0];
const BANK: [f32; 3] = [176.0, 160.0, 120.0];
const MUD: [f32; 3] = [150.0, 130.0, 95.0];
const BARE: [f32; 3] = [150.0, 136.0, 100.0];
const SHORE: [f32; 3] = [120.0, 186.0, 196.0];
const SHALLOW: [f32; 3] = [62.0, 132.0, 172.0];
const DEEP: [f32; 3] = [30.0, 74.0, 128.0];
const ABYSS: [f32; 3] = [22.0, 54.0, 104.0];
const RIVER: [f32; 3] = [74.0, 146.0, 196.0];
const ICE: [f32; 3] = [214.0, 232.0, 240.0];
const SNOW: [f32; 3] = [244.0, 246.0, 249.0];
pub const OUTLINE: Rgb = [0x15, 0x18, 0x1c];
pub const SHADOW: Rgb = [0x0b, 0x0e, 0x12];

fn season_green(s: Season) -> [f32; 3] {
    match s {
        Season::Spring => [110.0, 168.0, 82.0],
        Season::Summer => [150.0, 168.0, 76.0],
        Season::Autumn => [190.0, 132.0, 64.0],
        Season::Winter => [134.0, 134.0, 112.0],
    }
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// A tile's colour in the client's tokens: biome ground with the season's
/// green in its plant cover (grass 60%, forest 35%, marsh and tundra 25%),
/// grazed toward bare ground, water by depth, rivers, dried beds, ice and
/// snow on top.
fn tile_color(g: &Ground, t: &Tile, season: Season) -> [f32; 3] {
    let green = season_green(season);
    let (base, cover) = match g.biome {
        Biome::Grassland => (GRASS, 0.6),
        Biome::Forest => (FOREST, 0.35),
        Biome::Marsh => (MARSH, 0.25),
        Biome::Tundra => (TUNDRA, 0.25),
        Biome::Desert => (DESERT, 0.0),
        Biome::Mountain => (if g.elevation > 0.8 { PEAK } else { MOUNTAIN }, 0.0),
        Biome::ShallowWater | Biome::DeepWater => (SHALLOW, 0.0),
    };
    let mut c = mix(base, green, cover);
    if g.veg_max > 0.0 {
        c = mix(BARE, c, 0.35 + 0.65 * (t.veg / g.veg_max));
    }
    if t.water_depth > 0.0 {
        c = if g.is_river() {
            RIVER
        } else if t.water_depth < 0.3 {
            mix(SHORE, SHALLOW, t.water_depth / 0.3)
        } else if t.water_depth < 1.0 {
            mix(SHALLOW, DEEP, (t.water_depth - 0.3) / 0.7)
        } else {
            mix(DEEP, ABYSS, (t.water_depth - 1.0) / 2.0)
        };
        if t.ice > 0.3 {
            c = mix(c, ICE, (t.ice - 0.3) / 0.4);
        }
    } else if g.is_river() {
        c = BANK;
    } else if g.base_water > 0.0 {
        c = MUD;
    }
    mix(c, SNOW, (t.snow / 0.5).min(1.0) * 0.9)
}

/// Integer hash to 0–1, for texture grain.
fn grain(x: u32, y: u32) -> f32 {
    let mut h = x.wrapping_mul(0x9E37_79B1) ^ y.wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    (h & 0xFFFF) as f32 / 65535.0
}

/// Smooth value noise in 0..1 on a lattice of spacing 1 (for the edge warp).
fn vnoise(x: f32, y: f32) -> f32 {
    let (ix, iy) = (x.floor(), y.floor());
    let (fx, fy) = (x - ix, y - iy);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let h = |dx: f32, dy: f32| grain((ix + dx) as i32 as u32, (iy + dy) as i32 as u32);
    let top = h(0.0, 0.0) + (h(1.0, 0.0) - h(0.0, 0.0)) * sx;
    let bottom = h(0.0, 1.0) + (h(1.0, 1.0) - h(0.0, 1.0)) * sx;
    top + (bottom - top) * sy
}

/// The client's colour for an owner index (`colors.ts` `ownerColor`): hues
/// spread by the golden angle, skipping the yellow band.
pub fn hue_color(k: u32) -> Rgb {
    let mut hue = (k as f32 * 137.508 + 200.0) % 360.0;
    if hue > 40.0 && hue < 70.0 {
        hue = (hue + 40.0) % 360.0;
    }
    let (s, l) = (0.7f32, 0.58f32);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((hue / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match hue {
        h if h < 60.0 => (c, x, 0.0),
        h if h < 120.0 => (x, c, 0.0),
        h if h < 180.0 => (0.0, c, x),
        h if h < 240.0 => (0.0, x, c),
        h if h < 300.0 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [((r + m) * 255.0).round() as u8, ((g + m) * 255.0).round() as u8, ((b + m) * 255.0).round() as u8]
}

/// Colour of lineage `l` in the film and charts.
pub fn lineage_color(l: usize) -> Rgb {
    hue_color(l as u32 + 1)
}

fn lighten(c: Rgb, t: f32) -> Rgb {
    c.map(|v| (v as f32 + (255.0 - v as f32) * t) as u8)
}

/// Paint the world's map into `canvas` at (x0, y0), `side` pixels square:
/// tile colours blended between tile centres, with grain and a top-left
/// light on the relief, then every living organism.
pub fn paint_map(canvas: &mut Canvas, world: &World, x0: i64, y0: i64, side: usize) {
    let (mw, mh) = (world.map.width, world.map.height);
    let season = world.season();
    let ground = world.map.ground();
    let colors: Vec<[f32; 3]> = (0..mw * mh).map(|i| tile_color(&ground[i], &world.map.tiles[i], season)).collect();
    let scale = side as f32 / mw.max(mh) as f32;
    // Bilinear sample of a per-tile field at fractional tile coordinates.
    let at = |fx: f32, fy: f32| {
        let (fx, fy) = (fx.clamp(0.0, mw as f32 - 1.0), fy.clamp(0.0, mh as f32 - 1.0));
        let (tx, ty) = (fx.floor() as usize, fy.floor() as usize);
        let (ax, ay) = (fx - tx as f32, fy - ty as f32);
        let (tx1, ty1) = ((tx + 1).min(mw - 1), (ty + 1).min(mh - 1));
        (tx, ty, tx1, ty1, ax, ay)
    };
    let elev = |fx: f32, fy: f32| {
        let (tx, ty, tx1, ty1, ax, ay) = at(fx, fy);
        let e = |x: usize, y: usize| ground[y * mw + x].elevation;
        let top = e(tx, ty) + (e(tx1, ty) - e(tx, ty)) * ax;
        let bottom = e(tx, ty1) + (e(tx1, ty1) - e(tx, ty1)) * ax;
        top + (bottom - top) * ay
    };
    for py in 0..side {
        let gy = (py as f32 + 0.5) / scale - 0.5;
        if gy >= mh as f32 {
            break;
        }
        for px in 0..side {
            let gx = (px as f32 + 0.5) / scale - 0.5;
            if gx >= mw as f32 {
                break;
            }
            // A soft warp of the sampling point breaks the tile grid into
            // painted, organic edges.
            let (nx, ny) = (px as f32 / (scale * 0.9), py as f32 / (scale * 0.9));
            let fx = gx + (vnoise(nx, ny) - 0.5) * 0.9;
            let fy = gy + (vnoise(nx + 37.2, ny + 11.7) - 0.5) * 0.9;
            let (tx, ty, tx1, ty1, ax, ay) = at(fx, fy);
            let top = mix(colors[ty * mw + tx], colors[ty * mw + tx1], ax);
            let bottom = mix(colors[ty1 * mw + tx], colors[ty1 * mw + tx1], ax);
            let mut c = mix(top, bottom, ay);
            // Light from the top-left on the relief, and a fine grain.
            let slope = elev(gx, gy) - elev(gx - 0.7, gy - 0.7);
            let shade = 1.0 + (slope * 3.5).clamp(-0.14, 0.14) + (grain(px as u32, py as u32) - 0.5) * 0.05;
            c = c.map(|v| v * shade);
            let k = ((y0 as usize + py) * canvas.w + x0 as usize + px) * 3;
            if k + 2 < canvas.px.len() {
                canvas.px[k] = c[0].clamp(0.0, 255.0) as u8;
                canvas.px[k + 1] = c[1].clamp(0.0, 255.0) as u8;
                canvas.px[k + 2] = c[2].clamp(0.0, 255.0) as u8;
            }
        }
    }
    // Creatures, back to front: contact shadow down-right, body in the
    // lineage colour with a top-left highlight, dark outline.
    let mut orgs: Vec<_> = world.organisms.iter().filter(|o| o.alive).collect();
    orgs.sort_by(|a, b| a.pos.y.total_cmp(&b.pos.y));
    for o in orgs {
        let (cx, cy) = (x0 as f32 + o.pos.x * scale, y0 as f32 + o.pos.y * scale);
        let r = (scale * 0.22 * (0.6 + 1.6 * o.mass().max(0.0).sqrt() / 3.0)).clamp(1.6, scale * 0.6);
        let col = lineage_color(o.lineage as usize);
        canvas.ellipse(cx + r * 0.35, cy + r * 0.55, r * 1.05, r * 0.6, SHADOW, 0.32);
        canvas.disc(cx, cy, r + 1.0, OUTLINE, 0.9);
        canvas.disc(cx, cy, r, col, 1.0);
        canvas.disc(cx - r * 0.3, cy - r * 0.3, r * 0.45, lighten(col, 0.45), 0.8);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_hues_match_the_client() {
        // colors.ts ownerColor(1..=3).
        assert_eq!(hue_color(1), [223, 73, 129]);
        assert_eq!(hue_color(2), [85, 223, 73]);
        assert_eq!(hue_color(3), [104, 73, 223]);
    }

    #[test]
    fn text_draws_inside_its_width() {
        let mut c = Canvas::new(80, 20, [0, 0, 0]);
        let w = c.text(2, 2, "Year 1", 1, [255, 255, 255], 1.0);
        assert_eq!(w, super::super::raster::text_width("Year 1", 1));
        let lit: Vec<usize> = (0..80).filter(|&x| (0..20).any(|y| c.px[(y * 80 + x) * 3] > 0)).collect();
        assert!(!lit.is_empty() && *lit.last().unwrap() < 2 + w as usize);
    }
}
