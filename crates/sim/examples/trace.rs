//! Follow one organism tick by tick: `cargo run --release --example trace -- <lineage> [ticks]`.
//! Lineages in the standard world: 0 vole, 1 antelope, 2 newt, 3 fox, 4 pack hunters,
//! 5 herd grazers, 6 toxic frogs, 7 burrowing omnivores.
//! Set SEED to pick a world. Useful when tuning behavior or balance.

use evo_sim::{presets, WorldConfig};

fn main() {
    let mut args = std::env::args().skip(1);
    let lineage: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(1);
    let ticks: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(48 * 4);
    let seed = std::env::var("SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(1);
    let mut w = presets::standard_world(WorldConfig { seed, ..Default::default() }, 1);
    let Some(id) = w.organisms.iter().find(|o| o.lineage == lineage).map(|o| o.id) else {
        eprintln!("no organism in lineage {lineage}");
        return;
    };
    let o = w.organism(id).unwrap();
    println!(
        "organism {id}: mass {:.2} kg, metabolism {:.2}, speed {:.2}, perception {:.2}, power {:.2}",
        o.mass(),
        o.traits.stats.metabolism,
        o.traits.stats.speed,
        o.traits.stats.perception,
        o.power()
    );
    for t in 0..ticks {
        w.step();
        let Some(o) = w.organism(id) else {
            println!("tick {t}: died");
            break;
        };
        let tile = w.map.tile_at(o.pos);
        println!(
            "t{t:4} {:<12} -> {:<12} hunger {:5.1} thirst {:5.1} energy {:5.1} hp {:5.1} kin {:2} at ({:.1},{:.1}) {:?} veg {:.1} {:.0}°C",
            format!("{:?}", o.action),
            o.target.map_or("-".into(), |p| format!("({:.0},{:.0})", p.x, p.y)),
            o.needs.hunger,
            o.needs.thirst,
            o.needs.energy,
            o.health,
            o.kin_nearby,
            o.pos.x,
            o.pos.y,
            tile.biome,
            tile.veg,
            tile.temp
        );
    }
}
