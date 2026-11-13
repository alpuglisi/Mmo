//! Genetic distance and fertility between every pair of founding designs:
//! `cargo run --release --example distances`.
//! Order: the standard world's lineages (0 vole … 7 burrowing omnivores).

use evo_sim::{mating, presets, Genome};

fn main() {
    let designs: Vec<_> =
        presets::wild_designs().into_iter().map(|(_, d)| d).chain(presets::player_designs()).collect();
    let genomes: Vec<Genome> = designs.iter().map(|d| Genome::uniform(&d.values(), 0)).collect();
    let short = |s: &str| s.chars().take(8).collect::<String>();
    for (title, f) in [
        ("distance", &(|a: &Genome, b: &Genome| a.distance(b)) as &dyn Fn(&Genome, &Genome) -> f32),
        ("fertility", &|a: &Genome, b: &Genome| mating::fertility(a, b)),
    ] {
        print!("{title:<10}");
        for d in &designs {
            print!(" {:>8}", short(&d.name));
        }
        println!();
        for (i, a) in genomes.iter().enumerate() {
            print!("{:<10}", short(&designs[i].name));
            for b in &genomes {
                print!(" {:>8.3}", f(a, b));
            }
            println!();
        }
        println!();
    }
}
