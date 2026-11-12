//! Follow one organism tick by tick: `cargo run --release --example trace -- <owner> [ticks]`.
//! Set SEED to pick a world. Useful when tuning behavior or balance.

use evo_sim::{presets, World, WorldConfig};

fn main() {
    let mut args = std::env::args().skip(1);
    let owner: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(1);
    let ticks: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(48 * 4);
    let seed = std::env::var("SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(1);
    let mut w = World::new(WorldConfig { seed, ..Default::default() });
    for (o, d) in presets::wild_designs() {
        w.found_lineage_unchecked(o, &d);
    }
    for (k, d) in presets::player_designs().into_iter().enumerate() {
        w.found_lineage(k as u32 + 1, &d).expect("preset designs fit the budget");
    }
    let Some(id) = w.organisms.iter().find(|o| o.owner == owner).map(|o| o.id) else {
        eprintln!("no organism owned by {owner}");
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
