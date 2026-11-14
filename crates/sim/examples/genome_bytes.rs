//! Where a snapshot's bytes go, and how small the genomes could be:
//! `cargo run --release --features serde --example genome_bytes -- [size] [scale] [days] [later days]`.
//!
//! Builds the standard world (founding populations × `scale`), runs it for
//! `days`, then once a day for `later days` more, and each time: times
//! `World::snapshot`; sizes each world field and each organism field as a
//! column, raw and LZ4-compressed alone; then sizes the organisms' genomes
//! in candidate encodings (as `genome::wire` stores them, column-wise raw
//! bits, a global dictionary of exact bit patterns, a dictionary per locus,
//! residuals against the lineage's median with an entropy model), counts
//! alleles copied from a parent in the world, sizes eggs and young against
//! their parents' genomes, and the genomes as version 17 writes them.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::Write;
use std::sync::Arc;
use std::time::Instant;

use bincode::Options;
use evo_sim::genome::{Genome, ALLELES};
use evo_sim::{presets, PlayerId, World, WorldConfig};

fn mb(n: usize) -> f64 {
    n as f64 / 1e6
}

fn lz4(b: &[u8]) -> usize {
    lz4_flex::block::compress(b).len()
}

fn deflate(b: &[u8]) -> usize {
    let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
    e.write_all(b).unwrap();
    e.finish().unwrap().len()
}

fn alleles(g: &Genome) -> Vec<(u32, PlayerId)> {
    g.loci().iter().chain(g.load()).flat_map(|l| [l.a, l.b]).map(|a| (a.value.to_bits(), a.origin)).collect()
}

fn varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push(v as u8 | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

fn bits_for(n: usize) -> usize {
    (usize::BITS - (n.max(1) - 1).leading_zeros()) as usize
}

fn planes(cols: &[Vec<u32>]) -> Vec<u8> {
    let mut out = Vec::new();
    for c in cols {
        for b in 0..4 {
            out.extend(c.iter().map(|v| (v >> (8 * b)) as u8));
        }
    }
    out
}

fn ser<T: serde::Serialize + ?Sized>(v: &T) -> Vec<u8> {
    bincode::DefaultOptions::new().serialize(v).unwrap()
}

/// Raw and LZ4-alone sizes of one part.
fn part(name: &str, raw: &[u8]) -> (usize, usize) {
    let z = lz4(raw);
    println!("    {name:24} {:8.3} MB raw {:8.3} MB LZ4", mb(raw.len()), mb(z));
    (raw.len(), z)
}

fn entropy(counts: &HashMap<u64, usize>) -> f64 {
    let n: usize = counts.values().sum();
    counts.values().map(|&c| c as f64 * (n as f64 / c as f64).log2()).sum()
}

fn measure(w: &World) {
    let mut snap_ms = Vec::new();
    let mut snap = Vec::new();
    for _ in 0..3 {
        let t = Instant::now();
        snap = w.snapshot();
        snap_ms.push(t.elapsed().as_secs_f64() * 1e3);
    }
    snap_ms.sort_by(f64::total_cmp);
    let world_raw = ser(w);
    let world_lz4 = lz4(&world_raw);
    println!(
        "  snapshot {:.2} MB (median of 3: {:.0} ms); payload bincode {:.2} MB (tiles included), LZ4 alone {:.2} MB",
        mb(snap.len()),
        snap_ms[1],
        mb(world_raw.len()),
        mb(world_lz4)
    );
    // Distinct genomes of the organisms, in order, and their lineages.
    let mut seen = HashSet::new();
    let (genomes, lineage_of): (Vec<&Genome>, Vec<u32>) =
        w.organisms.iter().filter(|o| seen.insert(Arc::as_ptr(&o.genome))).map(|o| (&*o.genome, o.lineage)).unzip();
    let wire_cat: Vec<u8> = genomes.iter().flat_map(|g| g.to_wire()).collect();
    println!("  world fields (each serialized and compressed alone):");
    let mut sum = (0, 0);
    let mut add = |name: &str, raw: &[u8]| {
        let (r, z) = part(name, raw);
        sum.0 += r;
        sum.1 += z;
    };
    add("map tiles (bincode)", &ser(&w.map.tiles));
    add("weather", &ser(&w.weather));
    add("clutches", &ser(&w.clutches));
    add("group", &ser(&w.group));
    add("counters", &ser(&w.counters));
    add("legacy", &ser(&w.legacy));
    add("lineages", &ser(&w.lineages));
    add("chronicle", &ser(&w.chronicle));
    println!("  organisms' fields (each a column over all organisms):");
    add("org.genome (wire)", &wire_cat);
    macro_rules! col {
        ($($f:ident),*) => {$(
            add(concat!("org.", stringify!($f)), &w.organisms.iter().flat_map(|o| ser(&o.$f)).collect::<Vec<u8>>());
        )*};
    }
    col!(id, owner, generation, lineage, sex, pos, age_ticks, growth, matured, needs, health, alive, death, killer);
    col!(mother, father, breed_ready_tick, offspring_raised, nest, last_water, wander_target, forage_target, action);
    col!(target, activity, kin_nearby, sheltered, alert, bright_aversion, search_image, strike_ready_tick);
    col!(possessed_by, order, infection, path, dormancy, fat, range, cache, cache_pos, coat, group);
    let preg: Vec<u8> = w
        .organisms
        .iter()
        .flat_map(|o| {
            ser(&o.pregnancy.as_ref().map(|p| (0u32, p.father_id, p.father_lineage, p.fertility, p.due_tick)))
        })
        .collect();
    add("org.pregnancy", &preg);
    println!(
        "    {:24} {:8.3} MB raw {:8.3} MB LZ4 (LOD, caches, density, orders, ...)",
        "the rest (subtracted)",
        mb(world_raw.len().saturating_sub(sum.0)),
        mb(world_lz4.saturating_sub(sum.1))
    );

    // Genome statistics.
    let all: Vec<Vec<(u32, PlayerId)>> = genomes.iter().map(|g| alleles(g)).collect();
    let n = all.len();
    let total = n * ALLELES;
    let (mut zero, mut one, mut prev) = (0usize, 0usize, 0usize);
    let mut counts: HashMap<u32, u32> = HashMap::new();
    let mut tags: BTreeSet<PlayerId> = BTreeSet::new();
    for g in &all {
        let mut p = None;
        for &(v, o) in g {
            match v {
                0 => zero += 1,
                0x3f80_0000 => one += 1,
                _ if p == Some(v) => prev += 1,
                _ => {}
            }
            p = Some(v);
            *counts.entry(v).or_default() += 1;
            tags.insert(o);
        }
    }
    let shared = counts.iter().filter(|&(&v, &c)| c > 1 && v != 0).map(|(_, &c)| c as usize).sum::<usize>();
    println!(
        "  {n} genomes, {total} alleles: {:.1}% 0.0, {:.1}% 1.0, {:.1}% same as the allele before; {} distinct values; \
         {:.1}% of the other alleles share their value with another allele; {} distinct origin tags",
        100.0 * zero as f64 / total as f64,
        100.0 * one as f64 / total as f64,
        100.0 * prev as f64 / total as f64,
        counts.len(),
        100.0 * shared as f64 / (total - zero) as f64,
        tags.len()
    );

    // Wire components (bytes): tag lists, classes, origin indices, values.
    let (mut tag_b, mut class_b, mut idx_bits, mut val_bits) = (0usize, 0usize, 0usize, 0usize);
    for g in &all {
        let mut t: Vec<PlayerId> = Vec::new();
        let mut p = None;
        for &(v, o) in g {
            if !t.contains(&o) {
                t.push(o);
            }
            if v != 0 && v != 0x3f80_0000 && p != Some(v) {
                let e = v >> 23;
                val_bits += if (120..127).contains(&e) { 26 } else { 35 };
            }
            p = Some(v);
        }
        tag_b += 1 + 4 * t.len();
        class_b += ALLELES.div_ceil(4);
        idx_bits += ALLELES * bits_for(t.len());
    }
    println!(
        "  as stored (genome::wire): {:.3} MB raw, {:.3} MB LZ4 alone: tags {:.3}, classes {:.3}, origin indices {:.3}, \
         values {:.3} MB",
        mb(wire_cat.len()),
        mb(lz4(&wire_cat)),
        mb(tag_b),
        mb(class_b),
        mb(idx_bits / 8),
        mb(val_bits / 8)
    );

    // Column-wise raw bits.
    let cols: Vec<Vec<u32>> = (0..ALLELES).map(|k| all.iter().map(|g| g[k].0).collect()).collect();
    let pl = planes(&cols);
    println!(
        "  column-wise value bits, byte planes: {:.3} MB raw, LZ4 {:.3} MB, deflate {:.3} MB",
        mb(pl.len()),
        mb(lz4(&pl)),
        mb(deflate(&pl))
    );

    // Global dictionary.
    let d = counts.len();
    println!(
        "  global dictionary: {d} values × 4 B + {} bits per allele = {:.3} MB",
        bits_for(d),
        mb(d * 4 + total * bits_for(d) / 8)
    );

    // Per locus (a and b slots together): sorted distinct values, gaps as
    // varints; per allele an index of ceil(log2 n) bits, or as column bytes.
    let loci = ALLELES / 2;
    let (mut dict_raw, mut idx_fixed_bits) = (Vec::new(), 0usize);
    let mut idx_cols: Vec<u8> = Vec::new();
    let mut dict_sizes = Vec::new();
    for l in 0..loci {
        let set: BTreeSet<u32> = all.iter().flat_map(|g| [g[2 * l].0, g[2 * l + 1].0]).collect();
        let sorted: Vec<u32> = set.into_iter().collect();
        dict_sizes.push(sorted.len());
        let mut last = 0u32;
        for &v in &sorted {
            varint(&mut dict_raw, (v - last) as u64);
            last = v;
        }
        let pos: HashMap<u32, u32> = sorted.iter().enumerate().map(|(i, &v)| (v, i as u32)).collect();
        let b = bits_for(sorted.len());
        idx_fixed_bits += 2 * n * b;
        let bytes = b.div_ceil(8);
        for slot in [2 * l, 2 * l + 1] {
            let ix: Vec<u32> = all.iter().map(|g| pos[&g[slot].0]).collect();
            for byte in 0..bytes {
                idx_cols.extend(ix.iter().map(|i| (i >> (8 * byte)) as u8));
            }
        }
    }
    dict_sizes.sort_unstable();
    println!(
        "  per-locus dictionaries: distinct values per locus median {}, max {}; dictionary gaps {:.3} MB raw ({:.3} MB \
         deflate), indices {:.3} MB bit-packed, {:.3} MB as byte columns ({:.3} LZ4, {:.3} deflate)",
        dict_sizes[dict_sizes.len() / 2],
        dict_sizes.last().unwrap(),
        mb(dict_raw.len()),
        mb(deflate(&dict_raw)),
        mb(idx_fixed_bits / 8),
        mb(idx_cols.len()),
        mb(lz4(&idx_cols)),
        mb(deflate(&idx_cols)),
    );

    // Origin tags column-wise.
    let tag_pos: HashMap<PlayerId, u32> = tags.iter().enumerate().map(|(i, &t)| (t, i as u32)).collect();
    let tb = bits_for(tags.len()).div_ceil(8).max(1);
    let mut tcols = Vec::new();
    for k in 0..ALLELES {
        let ix: Vec<u32> = all.iter().map(|g| tag_pos[&g[k].1]).collect();
        for byte in 0..tb {
            tcols.extend(ix.iter().map(|i| (i >> (8 * byte)) as u8));
        }
    }
    println!(
        "  origin tags column-wise: {:.3} MB raw, LZ4 {:.3} MB, deflate {:.3} MB",
        mb(tcols.len()),
        mb(lz4(&tcols)),
        mb(deflate(&tcols))
    );

    // Copies of a parent's allele at the same locus (a parent still in the
    // world): the exact repeats inheritance makes.
    let by_id: HashMap<u64, &Genome> = w.organisms.iter().map(|o| (o.id, &*o.genome)).collect();
    let (mut with_parents, mut copied, mut copy_candidates) = (0usize, 0usize, 0usize);
    let mut is_copy: HashSet<(usize, usize)> = HashSet::new();
    let mut seen = HashSet::new();
    let firsts = w.organisms.iter().filter(|o| seen.insert(Arc::as_ptr(&o.genome)));
    for (gi, o) in firsts.enumerate() {
        let parents: Vec<Vec<(u32, PlayerId)>> =
            [o.mother, o.father].iter().flatten().filter_map(|id| by_id.get(id)).map(|g| alleles(g)).collect();
        if parents.is_empty() {
            continue;
        }
        with_parents += 1;
        for (k, &(v, _)) in all[gi].iter().enumerate() {
            if v == 0 {
                continue;
            }
            copy_candidates += 1;
            let l = k & !1;
            if parents.iter().any(|p| p[l].0 == v || p[l + 1].0 == v) {
                copied += 1;
                is_copy.insert((gi, k));
            }
        }
    }
    println!(
        "  {with_parents} genomes with a parent in the world: {copied} of their {copy_candidates} non-zero alleles copy \
         a parent's allele at the same locus ({:.1}% of all non-zero alleles)",
        100.0 * copied as f64 / (total - zero) as f64
    );

    // Values against a predictor, the median of the value's lineage at its
    // locus: the residual (difference of the bit patterns, zigzagged) costs
    // its bit length less the leading one, plus an entropy-coded length per
    // allele slot; classes (0.0, parent copy, stored) entropy-coded per slot.
    let mut groups: HashMap<(u32, usize), Vec<u32>> = HashMap::new();
    for (g, &lin) in all.iter().zip(&lineage_of) {
        for (k, &(v, _)) in g.iter().enumerate() {
            if v != 0 {
                groups.entry((lin, k / 2)).or_default().push(v);
            }
        }
    }
    let median: HashMap<(u32, usize), u32> = groups
        .into_iter()
        .map(|(key, mut v)| {
            v.sort_unstable();
            (key, v[v.len() / 2])
        })
        .collect();
    let (mut raw_bits, mut stored) = (0f64, 0usize);
    let mut len_counts: Vec<HashMap<u64, usize>> = vec![HashMap::new(); ALLELES];
    let mut class_counts: Vec<HashMap<u64, usize>> = vec![HashMap::new(); ALLELES];
    let mut zz_col = Vec::new();
    for (gi, (g, &lin)) in all.iter().zip(&lineage_of).enumerate() {
        for k in 0..ALLELES {
            let v = g[k].0;
            let class = if v == 0 {
                0
            } else if is_copy.contains(&(gi, k)) {
                1
            } else {
                2
            };
            *class_counts[k].entry(class).or_default() += 1;
            if class != 2 {
                continue;
            }
            let d = v as i64 - median[&(lin, k / 2)] as i64;
            let zz = ((d << 1) ^ (d >> 63)) as u64;
            let len = 64 - zz.leading_zeros() as u64;
            raw_bits += len.saturating_sub(1) as f64;
            *len_counts[k].entry(len).or_default() += 1;
            stored += 1;
            zz_col.push(zz as u32);
        }
    }
    let len_bits: f64 = len_counts.iter().map(entropy).sum();
    let class_bits: f64 = class_counts.iter().map(entropy).sum();
    let copy_bits = copied as f64; // which of the parent's two alleles
    let model = (raw_bits + len_bits + class_bits + copy_bits) / 8.0;
    println!(
        "  model: classes {:.3} MB, parent copies {:.3} MB, residual lengths {:.3} MB, residual bits {:.3} MB \
         ({stored} stored values, {:.1} bits each) = {:.3} MB for values and classes, against {:.3} MB as stored",
        mb((class_bits / 8.0) as usize),
        mb((copy_bits / 8.0) as usize),
        mb((len_bits / 8.0) as usize),
        mb((raw_bits / 8.0) as usize),
        (raw_bits + len_bits) / stored as f64,
        mb(model as usize),
        mb(class_b + val_bits / 8)
    );
    let zp = planes(&[zz_col]);
    println!("  residuals as byte planes: LZ4 {:.3} MB, deflate {:.3} MB", mb(lz4(&zp)), mb(deflate(&zp)));

    // Eggs and organisms against their parents' genomes: each allele a copy
    // of one of the parent's two alleles at its locus (a choice bit, none
    // when the parent is homozygous) or an exception.
    let by_id: HashMap<u64, &Genome> = w.organisms.iter().map(|o| (o.id, &*o.genome)).collect();
    let rel = |g: &Genome, m: &Genome, f: &Genome| -> (usize, usize) {
        let (c, m, f) = (alleles(g), alleles(m), alleles(f));
        let (mut choice, mut exc) = (0, 0);
        for k in 0..ALLELES {
            let p = if k % 2 == 0 { &m } else { &f };
            let (a, b) = (p[k & !1], p[k | 1]);
            if c[k] == a || c[k] == b {
                choice += usize::from(a != b);
            } else {
                exc += 1;
            }
        }
        (choice, exc)
    };
    let (mut eggs, mut eggs_rel, mut choice, mut exc) = (0, 0, 0, 0);
    for c in &w.clutches {
        eggs += c.eggs.len();
        if let (Some(m), Some(f)) = (by_id.get(&c.mother), by_id.get(&c.father)) {
            for e in &c.eggs {
                let (ch, ex) = rel(e, m, f);
                eggs_rel += 1;
                choice += ch;
                exc += ex;
            }
        }
    }
    println!(
        "  {} clutches, {eggs} eggs ({eggs_rel} with both parents in the world): {:.1} choice bits and {:.2} exceptions \
         per egg: about {:.3} MB by reference, against {:.3} MB as stored",
        w.clutches.len(),
        choice as f64 / eggs_rel.max(1) as f64,
        exc as f64 / eggs_rel.max(1) as f64,
        mb(choice / 8 + eggs_rel + exc * 9 + (eggs - eggs_rel) * 260),
        mb(w.clutches.iter().flat_map(|c| &c.eggs).map(|g| g.to_wire().len() + 1).sum::<usize>())
    );
    let index: HashMap<u64, usize> = w.organisms.iter().enumerate().map(|(k, o)| (o.id, k)).collect();
    let (mut kids, mut choice, mut exc) = (0, 0, 0);
    for (k, o) in w.organisms.iter().enumerate() {
        let (Some(mi), Some(fi)) = (o.mother.and_then(|m| index.get(&m)), o.father.and_then(|f| index.get(&f))) else {
            continue;
        };
        if *mi < k && *fi < k {
            let (ch, ex) = rel(&o.genome, &w.organisms[*mi].genome, &w.organisms[*fi].genome);
            kids += 1;
            choice += ch;
            exc += ex;
        }
    }
    println!(
        "  {kids} organisms with both parents earlier in the world: {:.1} choice bits and {:.2} exceptions each",
        choice as f64 / kids.max(1) as f64,
        exc as f64 / kids.max(1) as f64
    );

    // The genomes as snapshots write them from version 17: against the
    // parents' where they can be, whole otherwise.
    let (mut orgs, mut orgs_by, mut eggs, mut eggs_by) = (Vec::new(), 0, Vec::new(), 0);
    for (k, o) in w.organisms.iter().enumerate() {
        let parent = |id: Option<u64>| id.and_then(|id| index.get(&id)).filter(|&&p| p < k);
        let by = match (parent(o.mother), parent(o.father)) {
            (Some(&m), Some(&f)) => o.genome.to_wire_by_parents(&w.organisms[m].genome, &w.organisms[f].genome),
            _ => None,
        };
        orgs_by += usize::from(by.is_some());
        orgs.extend(by.unwrap_or_else(|| o.genome.to_wire()));
    }
    for c in &w.clutches {
        for e in &c.eggs {
            let by = match (by_id.get(&c.mother), by_id.get(&c.father)) {
                (Some(m), Some(f)) => e.to_wire_by_parents(m, f),
                _ => None,
            };
            eggs_by += usize::from(by.is_some());
            eggs.extend(by.unwrap_or_else(|| e.to_wire()));
        }
    }
    println!(
        "  as version 17 writes them: organisms' genomes {:.3} MB raw, {:.3} MB LZ4 ({orgs_by} of {} by parents); \
         eggs {:.3} MB raw, {:.3} MB LZ4 ({eggs_by} of {eggs_n} by parents)",
        mb(orgs.len()),
        mb(lz4(&orgs)),
        w.organisms.len(),
        mb(eggs.len()),
        mb(lz4(&eggs)),
        eggs_n = w.clutches.iter().map(|c| c.eggs.len()).sum::<usize>()
    );
}

fn main() {
    let nums: Vec<u64> = std::env::args().skip(1).filter_map(|a| a.parse().ok()).collect();
    let size = nums.first().copied().unwrap_or(512) as usize;
    let scale = nums.get(1).copied().unwrap_or(5) as u32;
    let days = nums.get(2).copied().unwrap_or(2) as u32;
    let later = nums.get(3).copied().unwrap_or(0) as u32;
    let mut w =
        presets::standard_world(WorldConfig { seed: 7, width: size, height: size, ..Default::default() }, scale);
    w.run_days(days);
    for d in 0..=later {
        if d > 0 {
            w.run_days(1);
        }
        println!("{size}x{size}, scale {scale}, day {}: {} agents", days + d, w.alive_count());
        measure(&w);
    }
}
