//! Serde helpers for world snapshots (feature `serde`).
//!
//! - `array`: fixed-size arrays longer than 32 (serde only derives up to 32),
//!   written as tuples (no length prefix): `#[serde(with = "crate::serde_util::array")]`.
//! - `array_vec`: the same inside a `Vec`.
//! - `sorted_map`: a `HashMap` written in key order, so equal worlds give
//!   byte-identical snapshots (a `HashMap` iterates in a per-instance order).
//! - `organisms` / `organism`: organisms whose genome-derived caches
//!   (skipped in snapshots) are recomputed as they are read.
//! - `father`: a pregnancy's father genome, written as an organism index
//!   when the father is still in the world.
//! - `clutches` / `by_parents`: from version 17, organisms' and eggs'
//!   genomes written beside them, against their parents' genomes when the
//!   parents are in the world (`Genome::to_wire_by_parents`).
//! - `tiles`: the map's tiles, written as an empty list while a world
//!   snapshot writes them in a section of their own.
//! - `before_v12`: `Balance::cold_gestation`, left out while a version 10 or
//!   11 world snapshot is read.
//! - `before_v13`: `Balance::nest_predation`, left out while a version 10,
//!   11 or 12 world snapshot is read.
//! - `before_v14`: `Gene::Obedience` and `Balance::tameness`, left out while
//!   a world snapshot of version 13 or before is read.
//! - `before_v15`: the world's rally orders, left out while a world snapshot
//!   of version 14 or before is read.
//! - `before_v16`: `Balance::migration_onset`, left out while a world snapshot
//!   of version 15 or before is read.
//! - `before_v17`: `Balance::home_range`, left out while a world snapshot of
//!   version 16 or before is read.
//! - `before_v18`: genomes written whole inside their organisms and clutches,
//!   while a world snapshot of version 17 or before is read.
//! - `before_v19`: `Balance::lactation` and `Balance::warm_upkeep`, left out
//!   while a world snapshot of version 18 or before is read.
//! - `before_v20`: `Balance::fang_grazing` and `Balance::scale_cover`, left
//!   out while a world snapshot of version 19 or before is read.
//! - `before_v21`: the `Balance::morph_*` tunables (body plan effects), left
//!   out while a world snapshot of version 20 or before is read.
//! - `before_v22`: `Balance::directional_vision`, `Balance::vision_scan` and
//!   `Organism::heading`, left out while a world snapshot of version 21 or
//!   before is read.
//! - `before_v23`: genomes' morphology segment (and species
//!   representatives' morphology keys) and `Balance::morph_step_rate`, left
//!   out while a world snapshot of version 22 or before is read.
//! - `before_v24`: `Balance::morph_legacy`, left out while a world snapshot
//!   of version 23 or before is read.
//! - `before_v25`: `Balance::roaming_forage`, left out while a world
//!   snapshot of version 24 or before is read.
//! - `before_v26`: the carnivore-diet tunables (`Balance::carcass_keep`,
//!   `carnivore_fat`, `carcass_scent`, `carrion`, `provisioning`,
//!   `breeding_territory`), left out while a world snapshot of version 25 or before
//!   is read (all 0, off).
//! - `before_v27`: the predator brakes (`Balance::prey_switching`,
//!   `condition_litter`), left out while a world snapshot of version 26 or
//!   before is read (0, off).
//! - `before_v29`: the Phase 3 morphology loci (horns, wing aspect,
//!   pattern) in genomes' morphology segment and their tunables
//!   (`Balance::morph_horns`, `morph_wing_shape`, `morph_pattern`), left out
//!   while a world snapshot of version 28 or before is read (defaults, off).
//! - `before_v30`: `Balance::strike_fixes`, left out while a world snapshot
//!   of version 29 or before is read (0, the old strike).
//! - `before_v31`: the food values (`Balance::meat_energy`, `fruit_crop`,
//!   `fruit_energy`, `insect_crop`, `switching_power`) and the map's fruit
//!   and invertebrate layers (`Map::fruit`, `Map::insects`,
//!   `before_v31::vec`), left out while a world snapshot of version 30 or
//!   before is read (0, off; no fruit, no invertebrates).
//! - `before_v34`: the playtest 1 ecology fixes (`Balance::desperate_risk`),
//!   left out while a world snapshot of version 33 or before is read or
//!   hashed (version 33 is another change's; renumber at merge).
//! - `before_v32`: `Balance::decide_interval` and the intents organisms keep
//!   between decisions (`World::last_intents`), left out while a world
//!   snapshot of version 31 or before is read (1, every organism decides
//!   every tick; none kept).
//! - `before_v35`: the lineage crowding tunables (`Balance::lineage_crowding`,
//!   `crowding_ceiling`, `crowding_juveniles`, `crowding_hunger`) and the
//!   mating slots players bought (`World::bonus_slots`,
//!   `before_v35::list`), left out while a world snapshot of version 34 or
//!   before is read (0, off: the old hard lineage cap; none bought).

use std::fmt;
use std::marker::PhantomData;

use serde::de::{Error, SeqAccess, Visitor};
use serde::ser::SerializeTuple;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub mod array {
    use super::*;

    pub fn serialize<S: Serializer, T: Serialize, const N: usize>(a: &[T; N], s: S) -> Result<S::Ok, S::Error> {
        let mut t = s.serialize_tuple(N)?;
        for x in a {
            t.serialize_element(x)?;
        }
        t.end()
    }

    struct ArrayVisitor<T, const N: usize>(PhantomData<T>);

    impl<'de, T: Deserialize<'de>, const N: usize> Visitor<'de> for ArrayVisitor<T, N> {
        type Value = [T; N];

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "an array of length {N}")
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<[T; N], A::Error> {
            let mut v = Vec::with_capacity(N);
            for i in 0..N {
                v.push(seq.next_element()?.ok_or_else(|| A::Error::invalid_length(i, &self))?);
            }
            v.try_into().map_err(|_| A::Error::custom("array length"))
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>, T: Deserialize<'de>, const N: usize>(
        d: D,
    ) -> Result<[T; N], D::Error> {
        d.deserialize_tuple(N, ArrayVisitor::<T, N>(PhantomData))
    }
}

pub mod array_vec {
    use super::*;

    struct Ref<'a, T, const N: usize>(&'a [T; N]);

    impl<T: Serialize, const N: usize> Serialize for Ref<'_, T, N> {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            array::serialize(self.0, s)
        }
    }

    struct Owned<T, const N: usize>([T; N]);

    impl<'de, T: Deserialize<'de>, const N: usize> Deserialize<'de> for Owned<T, N> {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            array::deserialize(d).map(Owned)
        }
    }

    pub fn serialize<S: Serializer, T: Serialize, const N: usize>(v: &[[T; N]], s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(v.iter().map(Ref))
    }

    pub fn deserialize<'de, D: Deserializer<'de>, T: Deserialize<'de>, const N: usize>(
        d: D,
    ) -> Result<Vec<[T; N]>, D::Error> {
        let v: Vec<Owned<T, N>> = Vec::deserialize(d)?;
        Ok(v.into_iter().map(|w| w.0).collect())
    }
}

pub mod sorted_map {
    use std::collections::HashMap;
    use std::hash::Hash;

    use super::*;

    pub fn serialize<S: Serializer, K: Serialize + Ord, V: Serialize, H>(
        m: &HashMap<K, V, H>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        let mut entries: Vec<(&K, &V)> = m.iter().collect();
        entries.sort_unstable_by(|a, b| a.0.cmp(b.0));
        s.collect_map(entries)
    }

    pub fn deserialize<'de, D, K, V, H>(d: D) -> Result<HashMap<K, V, H>, D::Error>
    where
        D: Deserializer<'de>,
        K: Deserialize<'de> + Eq + Hash,
        V: Deserialize<'de>,
        H: std::hash::BuildHasher + Default,
    {
        HashMap::deserialize(d)
    }
}

/// A world's organisms (`World::organisms`): written with the context
/// [`father`] needs, and read back with the genome-derived caches a
/// snapshot leaves out recomputed (`Organism::refresh_derived`) and shared
/// father genomes resolved:
/// `#[serde(with = "crate::serde_util::organisms")]`.
///
/// From world snapshot version 18 (unless a [`before_v18::Guard`] lives)
/// each organism is written as a pair: its genome, then the organism
/// without it ([`by_parents::genome`] writes nothing there). The genome is
/// written against its parents' ([`Genome::to_wire_by_parents`]) when both
/// are earlier in the list, so a reader has them already, and whole
/// otherwise. The organisms' genomes are then left for [`clutches`], whose
/// eggs are written against them too.
pub mod organisms {
    use std::collections::HashMap;
    use std::sync::Arc;

    use super::by_parents::{self, GenomeIn, GenomeOut, Genomes};
    use super::father::{Reading, Writing, READING, WRITING};
    use super::*;
    use crate::genome::wire::Parents;
    use crate::genome::Genome;
    use crate::organism::{OrgId, Organism};

    /// Clears a thread-local context however serialization ends.
    struct Clear<T: 'static>(&'static std::thread::LocalKey<std::cell::RefCell<Option<T>>>);

    impl<T> Drop for Clear<T> {
        fn drop(&mut self) {
            self.0.with(|c| c.borrow_mut().take());
        }
    }

    pub fn serialize<S: Serializer>(v: &[Organism], s: S) -> Result<S::Ok, S::Error> {
        let mut index = HashMap::with_capacity(v.len());
        for (k, o) in v.iter().enumerate() {
            index.entry(Arc::as_ptr(&o.genome)).or_insert(k as u32);
        }
        WRITING.with(|c| *c.borrow_mut() = Some(Writing { index }));
        let _clear = Clear(&WRITING);
        if before_v18::active() {
            return s.collect_seq(v);
        }
        let at: HashMap<OrgId, usize> = v.iter().enumerate().map(|(k, o)| (o.id, k)).collect();
        // A parent's genome, if it is earlier in the list than organism `k`.
        let earlier = |id: Option<OrgId>, k: usize| id.and_then(|id| at.get(&id)).filter(|&&p| p < k);
        let pairs = v.iter().enumerate().map(|(k, o)| {
            let genome = match (earlier(o.mother, k), earlier(o.father, k)) {
                (Some(&m), Some(&f)) => GenomeOut::new(&o.genome, &Parents::new(&v[m].genome, &v[f].genome)),
                _ => GenomeOut::Whole(&o.genome),
            };
            (genome, o)
        });
        let out = {
            let _apart = by_parents::Apart::new();
            s.collect_seq(pairs)
        };
        by_parents::leave_genomes(Genomes { at, genomes: v.iter().map(|o| o.genome.clone()).collect() });
        out
    }

    /// The (genome, organism) pairs of version 17, each genome resolved
    /// as it is read: a genome against its parents' needs them earlier in
    /// the list.
    struct Pairs;

    impl<'de> Visitor<'de> for Pairs {
        /// The organisms, and their indices by id.
        type Value = (Vec<Organism>, HashMap<OrgId, usize>);

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("organisms with their genomes")
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let n = seq.size_hint().unwrap_or(0).min(1 << 20);
            let (mut v, mut at) = (Vec::<Organism>::with_capacity(n), HashMap::<OrgId, usize>::with_capacity(n));
            while let Some((genome, mut o)) = seq.next_element::<(GenomeIn, Organism)>()? {
                o.genome = match genome {
                    GenomeIn::Whole(g) => Arc::from(g),
                    GenomeIn::ByParents(bytes) => {
                        let parent = |id: Option<OrgId>| {
                            id.and_then(|id| at.get(&id))
                                .map(|&p| &v[p].genome)
                                .ok_or_else(|| A::Error::custom("a genome's parent is not earlier in the world"))
                        };
                        let g = Parents::new(parent(o.mother)?, parent(o.father)?).decode(&bytes.0);
                        Arc::new(g.map_err(A::Error::custom)?)
                    }
                };
                at.insert(o.id, v.len());
                v.push(o);
            }
            Ok((v, at))
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Organism>, D::Error> {
        let placeholder = Arc::new(Genome::uniform(&[0.0; crate::genome::Gene::COUNT], 0));
        READING.with(|c| *c.borrow_mut() = Some(Reading { placeholder: placeholder.clone(), pending: Vec::new() }));
        let clear = Clear(&READING);
        let mut v = if before_v18::active() {
            Vec::<Organism>::deserialize(d)?
        } else {
            let (v, at) = {
                let _apart = by_parents::Apart::new();
                d.deserialize_seq(Pairs)?
            };
            by_parents::leave_genomes(Genomes { at, genomes: v.iter().map(|o| o.genome.clone()).collect() });
            v
        };
        let pending = READING.with(|c| c.borrow_mut().take()).map(|r| r.pending).unwrap_or_default();
        drop(clear);
        let mut pending = pending.into_iter();
        for k in 0..v.len() {
            let Some(p) = &v[k].pregnancy else { continue };
            if !Arc::ptr_eq(&p.father, &placeholder) {
                continue;
            }
            let i = pending.next().ok_or_else(|| D::Error::custom("unmatched shared father genome"))? as usize;
            let g = v.get(i).ok_or_else(|| D::Error::custom("shared father genome out of range"))?.genome.clone();
            v[k].pregnancy.as_mut().unwrap().father = g;
        }
        if pending.next().is_some() {
            return Err(D::Error::custom("unmatched shared father genome"));
        }
        v.iter_mut().for_each(Organism::refresh_derived);
        Ok(v)
    }
}

/// A world's clutches (`World::clutches`):
/// `#[serde(with = "crate::serde_util::clutches")]`. From world snapshot
/// version 18 (unless a [`before_v18::Guard`] lives) each clutch is written
/// as a pair: its eggs, then the clutch without them
/// ([`by_parents::eggs`] writes nothing there). An egg is written against
/// its parents' genomes ([`Genome::to_wire_by_parents`]) when both are in
/// the world's organisms, written just before (see [`organisms`]), and
/// whole otherwise. Eggs are most of a snapshot's genomes while a world is
/// laying (a 512² world three days in holds about 20,000).
pub mod clutches {
    use std::sync::Arc;

    use super::by_parents::{self, GenomeIn, GenomeOut};
    use super::*;
    use crate::genome::wire::Parents;
    use crate::genome::Genome;
    use crate::world::Clutch;

    pub fn serialize<S: Serializer>(v: &[Clutch], s: S) -> Result<S::Ok, S::Error> {
        if before_v18::active() {
            return s.collect_seq(v);
        }
        let genomes = by_parents::take_genomes().unwrap_or_default();
        let pairs = v.iter().map(|c| {
            let parents = genomes.get(&c.mother).zip(genomes.get(&c.father)).map(|(m, f)| Parents::new(m, f));
            let eggs: Vec<GenomeOut> = c
                .eggs
                .iter()
                .map(|e| match &parents {
                    Some(p) => GenomeOut::new(e, p),
                    None => GenomeOut::Whole(e),
                })
                .collect();
            (eggs, c)
        });
        let _apart = by_parents::Apart::new();
        s.collect_seq(pairs)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Clutch>, D::Error> {
        if before_v18::active() {
            return Vec::<Clutch>::deserialize(d);
        }
        let genomes = by_parents::take_genomes();
        let pairs = {
            let _apart = by_parents::Apart::new();
            Vec::<(Vec<GenomeIn>, Clutch)>::deserialize(d)?
        };
        let parent = |id| -> Result<&Arc<Genome>, D::Error> {
            genomes
                .as_ref()
                .and_then(|g| g.get(&id))
                .ok_or_else(|| D::Error::custom("an egg's parent is not in the world"))
        };
        pairs
            .into_iter()
            .map(|(eggs, mut c)| {
                let mut parents = None;
                c.eggs = eggs
                    .into_iter()
                    .map(|e| match e {
                        GenomeIn::Whole(g) => Ok(*g),
                        GenomeIn::ByParents(bytes) => {
                            let p = match &parents {
                                Some(p) => p,
                                None => parents.insert(Parents::new(parent(c.mother)?, parent(c.father)?)),
                            };
                            p.decode(&bytes.0).map_err(D::Error::custom)
                        }
                    })
                    .collect::<Result<_, _>>()?;
                Ok(c)
            })
            .collect()
    }
}

/// Genomes written against their parents' (world snapshots from version 17;
/// see [`organisms`] and [`clutches`]).
pub mod by_parents {
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;
    use std::sync::{Arc, LazyLock};

    use super::*;
    use crate::genome::wire::Parents;
    use crate::genome::Genome;
    use crate::organism::OrgId;

    thread_local! {
        /// Inside the pairs `organisms` and `clutches` write: the organism's
        /// genome and the clutch's eggs are written beside it.
        static APART: Cell<bool> = const { Cell::new(false) };
        /// The organisms' genomes by id, left by `organisms` for `clutches`.
        static GENOMES: RefCell<Option<Genomes>> = const { RefCell::new(None) };
    }

    /// Stands for an organism's genome until `organisms` reads it.
    static PLACEHOLDER: LazyLock<Arc<Genome>> =
        LazyLock::new(|| Arc::new(Genome::uniform(&[0.0; crate::genome::Gene::COUNT], 0)));

    /// Genomes and eggs written beside their organisms and clutches on this
    /// thread while this guard lives.
    pub(super) struct Apart(bool);

    impl Apart {
        pub(super) fn new() -> Apart {
            Apart(APART.with(|c| c.replace(true)))
        }
    }

    impl Drop for Apart {
        fn drop(&mut self) {
            APART.with(|c| c.set(self.0));
        }
    }

    /// A world's organisms' genomes, by organism id.
    #[derive(Default)]
    pub(super) struct Genomes {
        /// Organism id → index into `genomes`.
        pub at: HashMap<OrgId, usize>,
        pub genomes: Vec<Arc<Genome>>,
    }

    impl Genomes {
        pub(super) fn get(&self, id: &OrgId) -> Option<&Arc<Genome>> {
            self.at.get(id).map(|&k| &self.genomes[k])
        }
    }

    pub(super) fn leave_genomes(g: Genomes) {
        GENOMES.with(|c| *c.borrow_mut() = Some(g));
    }

    pub(super) fn take_genomes() -> Option<Genomes> {
        GENOMES.with(|c| c.borrow_mut().take())
    }

    /// Forgets the genomes [`organisms`] left on this thread when dropped,
    /// should a world's serialization stop before [`clutches`] takes them.
    pub struct Scope(());

    impl Scope {
        pub fn new() -> Scope {
            Scope(())
        }
    }

    impl Drop for Scope {
        fn drop(&mut self) {
            take_genomes();
        }
    }

    /// A genome's bytes from [`Genome::to_wire_by_parents`].
    pub(super) struct Bytes(pub Vec<u8>);

    impl Serialize for Bytes {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            s.serialize_bytes(&self.0)
        }
    }

    impl<'de> Deserialize<'de> for Bytes {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Bytes, D::Error> {
            struct V;
            impl<'de> Visitor<'de> for V {
                type Value = Bytes;

                fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                    f.write_str("a genome against its parents (bytes)")
                }

                fn visit_bytes<E: Error>(self, v: &[u8]) -> Result<Bytes, E> {
                    Ok(Bytes(v.to_vec()))
                }

                fn visit_byte_buf<E: Error>(self, v: Vec<u8>) -> Result<Bytes, E> {
                    Ok(Bytes(v))
                }

                fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Bytes, A::Error> {
                    let mut v = Vec::new();
                    while let Some(b) = seq.next_element::<u8>()? {
                        v.push(b);
                    }
                    Ok(Bytes(v))
                }
            }
            d.deserialize_bytes(V)
        }
    }

    #[derive(Serialize)]
    pub(super) enum GenomeOut<'a> {
        Whole(&'a Genome),
        ByParents(Bytes),
    }

    impl<'a> GenomeOut<'a> {
        /// `g` against its parents' genomes, or whole if it is not mostly
        /// inherited from them.
        pub(super) fn new(g: &'a Genome, parents: &Parents) -> GenomeOut<'a> {
            match parents.encode(g) {
                Some(b) => GenomeOut::ByParents(Bytes(b)),
                None => GenomeOut::Whole(g),
            }
        }
    }

    #[derive(Deserialize)]
    pub(super) enum GenomeIn {
        Whole(Box<Genome>),
        ByParents(Bytes),
    }

    /// An organism's genome (`Organism::genome`):
    /// `#[serde(with = "crate::serde_util::by_parents::genome")]`. Written
    /// as nothing inside the pairs [`organisms`] writes (the genome is
    /// beside it) and read as a placeholder there; in full anywhere else.
    pub mod genome {
        use super::*;

        pub fn serialize<S: Serializer>(g: &Arc<Genome>, s: S) -> Result<S::Ok, S::Error> {
            if APART.with(Cell::get) {
                s.serialize_unit()
            } else {
                g.serialize(s)
            }
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Arc<Genome>, D::Error> {
            if APART.with(Cell::get) {
                <()>::deserialize(d)?;
                Ok(PLACEHOLDER.clone())
            } else {
                Arc::<Genome>::deserialize(d)
            }
        }
    }

    /// A clutch's eggs (`Clutch::eggs`):
    /// `#[serde(with = "crate::serde_util::by_parents::eggs")]`. Written as
    /// nothing inside the pairs [`clutches`] writes (the eggs are beside
    /// it) and read as none there; in full anywhere else.
    pub mod eggs {
        use super::*;

        pub fn serialize<S: Serializer>(v: &Vec<Genome>, s: S) -> Result<S::Ok, S::Error> {
            if APART.with(Cell::get) {
                s.serialize_unit()
            } else {
                v.serialize(s)
            }
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Genome>, D::Error> {
            if APART.with(Cell::get) {
                <()>::deserialize(d)?;
                Ok(Vec::new())
            } else {
                Vec::<Genome>::deserialize(d)
            }
        }
    }
}

/// A pregnancy's father genome (`Pregnancy::father`):
/// `#[serde(with = "crate::serde_util::father")]`. Written inside a world's
/// organisms (see [`organisms`]), a father genome shared with an organism
/// still in the world (he is usually alive) is written as that organism's
/// index; any other is written in full.
pub mod father {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::sync::Arc;

    use super::*;
    use crate::genome::Genome;

    pub(super) struct Writing {
        /// Genome (by address) → index of the first organism holding it.
        pub index: HashMap<*const Genome, u32>,
    }

    pub(super) struct Reading {
        /// Stands for a shared genome until `organisms` resolves it.
        pub placeholder: Arc<Genome>,
        /// Organism indices of the shared father genomes read, in order.
        pub pending: Vec<u32>,
    }

    thread_local! {
        pub(super) static WRITING: RefCell<Option<Writing>> = const { RefCell::new(None) };
        pub(super) static READING: RefCell<Option<Reading>> = const { RefCell::new(None) };
    }

    #[derive(Serialize)]
    enum Out<'a> {
        Own(&'a Genome),
        Shared(u32),
    }

    #[derive(Deserialize)]
    enum In {
        Own(Box<Genome>),
        Shared(u32),
    }

    pub fn serialize<S: Serializer>(g: &Arc<Genome>, s: S) -> Result<S::Ok, S::Error> {
        let shared = WRITING.with(|c| c.borrow().as_ref().and_then(|w| w.index.get(&Arc::as_ptr(g)).copied()));
        match shared {
            Some(k) => Out::Shared(k),
            None => Out::Own(g),
        }
        .serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Arc<Genome>, D::Error> {
        match In::deserialize(d)? {
            In::Own(g) => Ok(Arc::from(g)),
            In::Shared(k) => READING.with(|c| match c.borrow_mut().as_mut() {
                Some(r) => {
                    r.pending.push(k);
                    Ok(r.placeholder.clone())
                }
                None => Err(D::Error::custom("a shared father genome outside a world's organisms")),
            }),
        }
    }
}

/// One organism, with the caches a snapshot leaves out recomputed:
/// `#[serde(deserialize_with = "crate::serde_util::organism::deserialize")]`.
pub mod organism {
    use super::*;
    use crate::organism::Organism;

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Organism, D::Error> {
        let mut o = Organism::deserialize(d)?;
        o.refresh_derived();
        Ok(o)
    }
}

/// The map's tiles (`Map::tiles`):
/// `#[serde(serialize_with = "crate::serde_util::tiles::serialize")]`.
/// Written in full, except on a thread holding a [`tiles::Apart`] guard (a
/// world snapshot writing them in a section of their own, see
/// `world::snapshot`): then as an empty list. Read as usual.
pub mod tiles {
    use std::cell::Cell;

    use super::*;
    use crate::terrain::Tile;

    thread_local! {
        static APART: Cell<bool> = const { Cell::new(false) };
    }

    /// Tiles serialized on this thread are written as an empty list while
    /// this guard lives.
    pub struct Apart(bool);

    impl Apart {
        pub fn new() -> Apart {
            Apart(APART.with(|c| c.replace(true)))
        }
    }

    impl Drop for Apart {
        fn drop(&mut self) {
            APART.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &[Tile], s: S) -> Result<S::Ok, S::Error> {
        if APART.with(Cell::get) {
            s.collect_seq(std::iter::empty::<&Tile>())
        } else {
            s.collect_seq(v)
        }
    }
}

/// `Balance::cold_gestation`, added in world snapshot version 12:
/// `#[serde(with = "crate::serde_util::before_v12")]`. On a thread holding a
/// [`before_v12::Guard`] (reading or hashing a version 10 or 11 snapshot)
/// it is neither written nor read, so the config has its old layout, and
/// reads as 1.4, the rule those saves ran under. bincode writes a unit as
/// nothing.
pub mod before_v12 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// `cold_gestation` before version 12 (`stats::derive` had it built in).
    pub const OLD_COLD_GESTATION: f32 = 1.4;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_COLD_GESTATION)
        } else {
            f32::deserialize(d)
        }
    }
}

/// `Balance::nest_predation`, added in world snapshot version 13:
/// `#[serde(with = "crate::serde_util::before_v13")]`. On a thread holding a
/// [`before_v13::Guard`] (reading or hashing a version 10 to 12 snapshot) it
/// is neither written nor read and reads as 0, the rule those saves ran
/// under (no egg predation on nesting grounds).
pub mod before_v13 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// `nest_predation` before version 13.
    pub const OLD_NEST_PREDATION: f32 = 0.0;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_NEST_PREDATION)
        } else {
            f32::deserialize(d)
        }
    }
}

/// What world snapshot version 14 added: `Gene::Obedience` (a locus in every
/// genome, a value in every species representative) and
/// `Balance::tameness`. On a thread holding a [`before_v14::Guard`]
/// (reading or hashing a snapshot of version 13 or before) the old layout
/// is written and read: genomes without the obedience locus (`genome::wire`
/// fills it in at the design default), species representatives one value
/// shorter (`before_v14::reps`), and no `tameness`
/// (`#[serde(with = "crate::serde_util::before_v14")]`, read as its
/// default; with every obedience allele at the default it changes nothing).
pub mod before_v14 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// Whether the old layout is in force on this thread.
    pub fn active() -> bool {
        OLD.with(Cell::get)
    }

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if active() {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if active() {
            Ok(crate::config::Balance::default().tameness)
        } else {
            f32::deserialize(d)
        }
    }

    /// Species representatives (`Species::reps`, expressed values by gene
    /// then the morphology key, `species::Expr`): `array_vec`, but in the
    /// old layouts shorter: before version 23 (a [`super::before_v23::Guard`])
    /// without the morphology key, which reads as the default morphology's;
    /// before version 14 also without the last gene (obedience), which reads
    /// as the design default.
    pub mod reps {
        use super::*;
        use crate::genome::Gene;
        use crate::species::{Expr, EXPR_LEN};

        const V13: usize = Gene::COUNT - 1;
        const V20: usize = Gene::COUNT;

        fn old<const N: usize>(v: &[Expr]) -> Vec<[f32; N]> {
            v.iter().map(|e| e[..N].try_into().unwrap()).collect()
        }

        fn fill<const N: usize>(old: Vec<[f32; N]>) -> Vec<Expr> {
            let mut base = [0.0; EXPR_LEN];
            base[Gene::Obedience.index()] = crate::design::default_value(Gene::Obedience);
            base[Gene::COUNT..].copy_from_slice(&crate::species::default_morph_key());
            old.into_iter()
                .map(|o| {
                    let mut e = base;
                    e[..N].copy_from_slice(&o);
                    e
                })
                .collect()
        }

        pub fn serialize<S: Serializer>(v: &[Expr], s: S) -> Result<S::Ok, S::Error> {
            if active() {
                array_vec::serialize(&old::<V13>(v), s)
            } else if super::super::before_v23::active() {
                array_vec::serialize(&old::<V20>(v), s)
            } else {
                array_vec::serialize(v, s)
            }
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Expr>, D::Error> {
            if active() {
                Ok(fill::<V13>(array_vec::deserialize(d)?))
            } else if super::super::before_v23::active() {
                Ok(fill::<V20>(array_vec::deserialize(d)?))
            } else {
                array_vec::deserialize(d)
            }
        }
    }
}

/// What world snapshot version 15 added: the world's rally orders
/// (`World::rally_group`). On a thread holding a [`before_v15::Guard`]
/// (reading or hashing a snapshot of version 14 or before) the field is
/// written as nothing and read as empty (no group was rallying: the command
/// didn't exist).
pub mod before_v15 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// Whether the old layout is in force on this thread.
    pub fn active() -> bool {
        OLD.with(Cell::get)
    }

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer, T: Serialize>(v: &T, s: S) -> Result<S::Ok, S::Error> {
        if active() {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>, T: Deserialize<'de> + Default>(d: D) -> Result<T, D::Error> {
        if active() {
            Ok(T::default())
        } else {
            T::deserialize(d)
        }
    }
}

/// `Balance::migration_onset`, added in world snapshot version 16:
/// `#[serde(with = "crate::serde_util::before_v16")]`. On a thread holding a
/// [`before_v16::Guard`] (reading or hashing a snapshot of version 15 or
/// before) it is neither written nor read and reads as 1, the rule those
/// saves ran under (migrators leave on the first day of autumn).
pub mod before_v16 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// `migration_onset` before version 16.
    pub const OLD_MIGRATION_ONSET: f32 = 1.0;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_MIGRATION_ONSET)
        } else {
            f32::deserialize(d)
        }
    }
}

/// `Balance::home_range` (the home-range form of offline protection), added
/// in world snapshot version 17: `#[serde(with = "crate::serde_util::before_v17")]`.
/// On a thread holding a [`before_v17::Guard`] (reading or hashing a
/// snapshot of version 16 or before) it is neither written nor read and
/// reads as its default: inert in those saves (no range was ever shielded),
/// and the defaults once a server shields one.
pub mod before_v17 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer, T: Serialize>(v: &T, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>, T: Deserialize<'de> + Default>(d: D) -> Result<T, D::Error> {
        if OLD.with(Cell::get) {
            Ok(T::default())
        } else {
            T::deserialize(d)
        }
    }
}

/// Organisms' and eggs' genomes written against their parents', from world
/// snapshot version 18 ([`organisms`], [`clutches`]). On a thread holding a
/// [`before_v18::Guard`] (reading or hashing a snapshot of version 17 or
/// before) every genome is written whole inside its organism or clutch, as
/// before.
pub mod before_v18 {
    use std::cell::Cell;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// Whether the old layout is in force on this thread.
    pub fn active() -> bool {
        OLD.with(Cell::get)
    }

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }
}

/// `Balance::lactation` and `Balance::warm_upkeep`, added in world snapshot
/// version 19: `#[serde(with = "crate::serde_util::before_v19")]`. On a
/// thread holding a [`before_v19::Guard`] (reading or hashing a snapshot of
/// version 18 or before) they are neither written nor read and read as 0,
/// the rules those saves ran under (a warm-blooded live bearer conceives
/// again after her recovery alone; no extra upkeep of endothermy).
pub mod before_v19 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// `lactation` and `warm_upkeep` before version 19: both off.
    pub const OLD_VALUE: f32 = 0.0;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_VALUE)
        } else {
            f32::deserialize(d)
        }
    }
}

/// `Balance::fang_grazing` and `Balance::scale_cover`, added in world
/// snapshot version 20: `#[serde(with = "crate::serde_util::before_v20")]`.
/// On a thread holding a [`before_v20::Guard`] (reading or hashing a
/// snapshot of version 19 or before) they are neither written nor read and
/// read as 0, the rules those saves ran under (long fangs graze as well as
/// none; scales leave fur's warmth alone).
pub mod before_v20 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// `fang_grazing` and `scale_cover` before version 20: both off.
    pub const OLD_VALUE: f32 = 0.0;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_VALUE)
        } else {
            f32::deserialize(d)
        }
    }
}

/// The `Balance::morph_*` tunables (body plan effects), added in world
/// snapshot version 21: `#[serde(with = "crate::serde_util::before_v21")]`.
/// On a thread holding a [`before_v21::Guard`] (reading or hashing a
/// snapshot of version 20 or before) they are neither written nor read and
/// read as 0, the rules those saves ran under (no body plan effects).
pub mod before_v21 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The `morph_*` tunables before version 21: all off.
    pub const OLD_VALUE: f32 = 0.0;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_VALUE)
        } else {
            f32::deserialize(d)
        }
    }
}

/// `Balance::directional_vision`, `Balance::vision_scan` and
/// `Organism::heading`, added in world snapshot version 22:
/// `#[serde(with = "crate::serde_util::before_v22")]`. On a thread holding a
/// [`before_v22::Guard`] (reading or hashing a snapshot of version 21 or
/// before) they are neither written nor read and
/// read as their defaults: directional vision off (`vision_scan` 1), and
/// no heading (zero; `World::restore` then gives each organism its birth
/// heading).
pub mod before_v22 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer, T: Serialize>(v: &T, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>, T: Deserialize<'de> + Default>(d: D) -> Result<T, D::Error> {
        if OLD.with(Cell::get) {
            Ok(T::default())
        } else {
            T::deserialize(d)
        }
    }

    /// `Balance::vision_scan`, read as its default (1) from older snapshots.
    pub mod scan {
        use super::*;

        pub const OLD_VALUE: f32 = 1.0;

        pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
            super::serialize(v, s)
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
            if OLD.with(Cell::get) {
                Ok(OLD_VALUE)
            } else {
                f32::deserialize(d)
            }
        }
    }
}

/// What world snapshot version 23 added: every genome's morphology segment
/// (`crate::morph`) and the morphology key of species representatives. On a
/// thread holding a [`before_v23::Guard`] (reading or hashing a snapshot of
/// version 22 or before) genomes are written and read without the segment,
/// which reads as the contract defaults with origin tags copied from each
/// locus's anchor gene (`MorphGenes::from_genes`), and representatives
/// without the key (`before_v14::reps`).
pub mod before_v23 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// Whether the old layout is in force on this thread.
    pub fn active() -> bool {
        OLD.with(Cell::get)
    }

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    /// `Balance::morph_step_rate`: neither written nor read (read as 0, off)
    /// while the old layout is in force.
    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if active() {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if active() {
            Ok(0.0)
        } else {
            f32::deserialize(d)
        }
    }
}

/// `Balance::morph_legacy`, added in world snapshot version 24:
/// `#[serde(with = "crate::serde_util::before_v24")]`. On a thread holding a
/// [`before_v24::Guard`] (reading or hashing a snapshot of version 23 or
/// before) it is neither written nor read and reads as 0, the rule those
/// saves ran under (morphology alleles left out of the legacy tally).
pub mod before_v24 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// `morph_legacy` before version 24: off.
    pub const OLD_VALUE: f32 = 0.0;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_VALUE)
        } else {
            f32::deserialize(d)
        }
    }
}

/// `Balance::roaming_forage`, added in world snapshot version 25:
/// `#[serde(with = "crate::serde_util::before_v25")]`. On a thread holding a
/// [`before_v25::Guard`] (reading or hashing a snapshot of version 24 or
/// before) it is neither written nor read and reads as 100, the rule those
/// saves ran under (a hunter roaming with no prey in sight eats nothing).
pub mod before_v25 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// `roaming_forage` before version 25: off (hunger never exceeds 100).
    pub const OLD_VALUE: f32 = 100.0;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_VALUE)
        } else {
            f32::deserialize(d)
        }
    }
}

/// The carnivore-diet tunables (`Balance::carcass_keep`, `carnivore_fat`,
/// `carcass_scent`, `carrion`, `provisioning`, `breeding_territory`), added
/// in world snapshot version 26:
/// `#[serde(with = "crate::serde_util::before_v26")]`. On a thread holding a
/// [`before_v26::Guard`] (reading or hashing a snapshot of version 25 or
/// before) they are neither written nor read and read as 0: every rule off,
/// as those worlds ran.
pub mod before_v26 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The carnivore-diet tunables before version 26: all off.
    pub const OLD_VALUE: f32 = 0.0;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_VALUE)
        } else {
            f32::deserialize(d)
        }
    }
}

/// The predator brakes (`Balance::prey_switching`, `condition_litter`),
/// added in world snapshot version 27:
/// `#[serde(with = "crate::serde_util::before_v27")]`. On a thread holding a
/// [`before_v27::Guard`] (reading or hashing a snapshot of version 26 or
/// before) they are neither written nor read and read as 0: off, as those
/// worlds ran.
pub mod before_v27 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The predator brakes before version 27: off.
    pub const OLD_VALUE: f32 = 0.0;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_VALUE)
        } else {
            f32::deserialize(d)
        }
    }
}

/// What world snapshot version 29 added (numbered apart from a concurrent
/// version 28; renumber at merge): the Phase 3 morphology loci (horn size
/// and form, wing aspect, coat pattern) at the end of every genome's
/// morphology segment, and their tunables (`Balance::morph_horns`,
/// `morph_wing_shape`, `morph_pattern`):
/// `#[serde(with = "crate::serde_util::before_v29")]`. On a thread holding a
/// [`before_v29::Guard`] (reading or hashing a snapshot of version 28 or
/// before) genomes are written and read with the core segment only (the new
/// loci read as their defaults, tagged like their anchor genes:
/// `MorphGenes::reset_extra_from`), and the tunables are neither written nor
/// read and read as 0: off, as those worlds ran.
pub mod before_v29 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The Phase 3 tunables before version 29: off.
    pub const OLD_VALUE: f32 = 0.0;

    /// Whether the old layout is in force on this thread.
    pub fn active() -> bool {
        OLD.with(Cell::get)
    }

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if active() {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if active() {
            Ok(OLD_VALUE)
        } else {
            f32::deserialize(d)
        }
    }
}

/// `Balance::strike_fixes`, added in world snapshot version 30:
/// `#[serde(with = "crate::serde_util::before_v30")]`. On a thread holding a
/// [`before_v30::Guard`] (reading or hashing a snapshot of version 29 or
/// before) it is neither written nor read and reads as 0: the old strike, as
/// those worlds ran.
pub mod before_v30 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// `strike_fixes` before version 30: off.
    pub const OLD_VALUE: f32 = 0.0;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_VALUE)
        } else {
            f32::deserialize(d)
        }
    }
}

/// The playtest 1 ecology fixes (`Balance::desperate_risk`), added in world
/// snapshot version 34: `#[serde(with = "crate::serde_util::before_v34")]`.
/// On a thread holding a [`before_v34::Guard`] (reading or hashing a
/// snapshot of version 33 or before) they are neither written nor read and
/// read as 0: off, as those worlds ran.
pub mod before_v34 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The fixes before version 34: off.
    pub const OLD_VALUE: f32 = 0.0;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_VALUE)
        } else {
            f32::deserialize(d)
        }
    }
}

/// `#[serde(with = "crate::serde_util::before_v32")]` on
/// `Balance::decide_interval`, and `before_v32::list` on the intents kept
/// between decisions. On a thread holding a [`before_v32::Guard`] (reading
/// or hashing a snapshot of version 31 or before) neither is written nor
/// read: the interval reads as 1 (every organism decides every tick, as
/// those worlds ran) and no intents are kept.
pub mod before_v32 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// `decide_interval` before version 32: every tick.
    pub const OLD_VALUE: f32 = 1.0;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_VALUE)
        } else {
            f32::deserialize(d)
        }
    }

    /// The same for a value that older versions don't have at all: its
    /// default when reading them.
    pub mod list {
        use super::*;

        pub fn serialize<S: Serializer, T: Serialize>(v: &T, s: S) -> Result<S::Ok, S::Error> {
            if OLD.with(Cell::get) {
                s.serialize_unit()
            } else {
                v.serialize(s)
            }
        }

        pub fn deserialize<'de, D: Deserializer<'de>, T: Deserialize<'de> + Default>(d: D) -> Result<T, D::Error> {
            if OLD.with(Cell::get) {
                Ok(T::default())
            } else {
                T::deserialize(d)
            }
        }
    }
}

/// The food values (`Balance::meat_energy`, `fruit_crop`, `fruit_energy`,
/// `insect_crop`, `switching_power`), added in world snapshot version 31:
/// `#[serde(with = "crate::serde_util::before_v31")]`, and the map's fruit
/// and invertebrate layers (`Map::fruit`, `Map::insects`, [`before_v31::vec`]). On a thread holding a
/// [`before_v31::Guard`] (reading or hashing a snapshot of version 30 or
/// before) they are neither written nor read: the values read as 0 (off,
/// meat and fruit worth what leaves are, no fruit) and the fruit layer as
/// empty (`World::restore` fills it with zeros), as those worlds ran.
pub mod before_v31 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The food values before version 31: off.
    pub const OLD_VALUE: f32 = 0.0;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_VALUE)
        } else {
            f32::deserialize(d)
        }
    }

    /// The map's fruit and invertebrate layers (`Map::fruit`,
    /// `Map::insects`): left out before version 31,
    /// read back as empty.
    pub mod vec {
        use super::*;

        pub fn serialize<S: Serializer>(v: &Vec<f32>, s: S) -> Result<S::Ok, S::Error> {
            if OLD.with(Cell::get) {
                s.serialize_unit()
            } else {
                v.serialize(s)
            }
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<f32>, D::Error> {
            if OLD.with(Cell::get) {
                Ok(Vec::new())
            } else {
                Vec::<f32>::deserialize(d)
            }
        }
    }
}

/// `#[serde(with = "crate::serde_util::before_v35")]` on the lineage
/// crowding tunables (`Balance::lineage_crowding` and the rest), and
/// `before_v35::list` on `World::bonus_slots`. On a thread holding a
/// [`before_v35::Guard`] (reading or hashing a snapshot of version 32 or
/// before) none is written or read: the tunables read as 0 (off, the hard
/// lineage cap those worlds ran under) and no slots were bought.
pub mod before_v35 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The crowding tunables before version 35: off.
    pub const OLD_VALUE: f32 = 0.0;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_VALUE)
        } else {
            f32::deserialize(d)
        }
    }

    /// The same for a value that older versions don't have at all: its
    /// default when reading them.
    pub mod list {
        use super::*;

        pub fn serialize<S: Serializer, T: Serialize>(v: &T, s: S) -> Result<S::Ok, S::Error> {
            if OLD.with(Cell::get) {
                s.serialize_unit()
            } else {
                v.serialize(s)
            }
        }

        pub fn deserialize<'de, D: Deserializer<'de>, T: Deserialize<'de> + Default>(d: D) -> Result<T, D::Error> {
            if OLD.with(Cell::get) {
                Ok(T::default())
            } else {
                T::deserialize(d)
            }
        }
    }
}

/// `#[serde(with = "crate::serde_util::before_v36")]` on the predator
/// tunables (`Balance::pack_hunt`, `territory_harass`, `attack_grace`),
/// and `before_v36::list` on `World::attack_orders`. On a thread holding a
/// [`before_v36::Guard`] (reading or hashing a snapshot of version 35 or
/// before) none is written or read: the tunables read as 0 (off) and no
/// attack orders were given.
pub mod before_v36 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The predator tunables before version 36: off.
    pub const OLD_VALUE: f32 = 0.0;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_VALUE)
        } else {
            f32::deserialize(d)
        }
    }

    /// The same for a value that older versions don't have at all: its
    /// default when reading them.
    pub mod list {
        use super::*;

        pub fn serialize<S: Serializer, T: Serialize>(v: &T, s: S) -> Result<S::Ok, S::Error> {
            if OLD.with(Cell::get) {
                s.serialize_unit()
            } else {
                v.serialize(s)
            }
        }

        pub fn deserialize<'de, D: Deserializer<'de>, T: Deserialize<'de> + Default>(d: D) -> Result<T, D::Error> {
            if OLD.with(Cell::get) {
                Ok(T::default())
            } else {
                T::deserialize(d)
            }
        }
    }
}

/// `#[serde(with = "crate::serde_util::before_v42::list")]` on the species
/// hysteresis settings (`ScoringConfig::species_confirm` and the rest) and
/// the species tracker's state for them (`SpeciesTracker::candidates`,
/// `Species::merging`). On a thread holding a [`before_v42::Guard`] (reading
/// or hashing a snapshot of version 41 or before) none is written or read:
/// they read as their defaults, 0, which is the old tracker (a split counts
/// at its first detection, a merge at once, no re-use of merged identities).
pub mod before_v42 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub mod list {
        use super::*;

        pub fn serialize<S: Serializer, T: Serialize>(v: &T, s: S) -> Result<S::Ok, S::Error> {
            if OLD.with(Cell::get) {
                s.serialize_unit()
            } else {
                v.serialize(s)
            }
        }

        pub fn deserialize<'de, D: Deserializer<'de>, T: Deserialize<'de> + Default>(d: D) -> Result<T, D::Error> {
            if OLD.with(Cell::get) {
                Ok(T::default())
            } else {
                T::deserialize(d)
            }
        }
    }
}

/// What world snapshot version 41 added: migration deadlines and
/// post-arrival settle circles (`World::group_plans`). On a thread holding
/// a [`before_v41::Guard`] (reading or hashing a snapshot of version 36 or
/// before; 37 to 40 are other changes') the field is written as nothing and
/// read as empty: standing migrations get their deadline from the restore.
pub mod before_v41 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer, T: Serialize>(v: &T, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>, T: Deserialize<'de> + Default>(d: D) -> Result<T, D::Error> {
        if OLD.with(Cell::get) {
            Ok(T::default())
        } else {
            T::deserialize(d)
        }
    }
}

/// `#[serde(with = "crate::serde_util::before_v43")]` on the competitive
/// balance tunables of playtest 2 (`Balance::egg_hybrid`, `age_spread`,
/// `dormancy_toll`, `emergence_lag`). On a thread holding a
/// [`before_v43::Guard`] (reading or hashing a snapshot of version 42 or
/// before) none is written or read: the tunables read as 0 (off).
pub mod before_v43 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The competitive balance tunables before version 43: off.
    pub const OLD_VALUE: f32 = 0.0;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_VALUE)
        } else {
            f32::deserialize(d)
        }
    }
}

/// `#[serde(with = "crate::serde_util::before_v44")]` on the playtest 2
/// tunables (`Balance::toxic_learning` and the rest), and `before_v44::list`
/// on the values added with them (`World::toxic_lessons`). On a thread
/// holding a [`before_v44::Guard`] (reading or hashing a snapshot of version
/// 43 or before) none is written or read: the tunables read as 0 (off) and
/// the values as empty.
pub mod before_v44 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The playtest 2 tunables before version 44: off.
    pub const OLD_VALUE: f32 = 0.0;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_VALUE)
        } else {
            f32::deserialize(d)
        }
    }

    /// The same for a value that older versions don't have at all: its
    /// default when reading them.
    pub mod list {
        use super::*;

        pub fn serialize<S: Serializer, T: Serialize>(v: &T, s: S) -> Result<S::Ok, S::Error> {
            if OLD.with(Cell::get) {
                s.serialize_unit()
            } else {
                v.serialize(s)
            }
        }

        pub fn deserialize<'de, D: Deserializer<'de>, T: Deserialize<'de> + Default>(d: D) -> Result<T, D::Error> {
            if OLD.with(Cell::get) {
                Ok(T::default())
            } else {
                T::deserialize(d)
            }
        }
    }
}

/// What world snapshot version 45 added: a migration's headway
/// (`groups::MigrationPlan::best_median`, `progress_tick`). On a thread
/// holding a [`before_v45::Guard`] (reading or hashing a snapshot of
/// version 44 or before) neither is written or read: the best reads as
/// infinite and the tick as 0 (measured afresh at the next check).
pub mod before_v45 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    /// An `f32` older versions lack, read as infinity.
    pub mod infinite {
        use super::*;

        pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
            if OLD.with(Cell::get) {
                s.serialize_unit()
            } else {
                v.serialize(s)
            }
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
            if OLD.with(Cell::get) {
                Ok(f32::INFINITY)
            } else {
                f32::deserialize(d)
            }
        }
    }

    /// A `u64` older versions lack, read as 0.
    pub mod zero {
        use super::*;

        pub fn serialize<S: Serializer>(v: &u64, s: S) -> Result<S::Ok, S::Error> {
            if OLD.with(Cell::get) {
                s.serialize_unit()
            } else {
                v.serialize(s)
            }
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
            if OLD.with(Cell::get) {
                Ok(0)
            } else {
                u64::deserialize(d)
            }
        }
    }
}

/// `#[serde(with = "crate::serde_util::before_v46")]` on the competitive
/// balance tunables of playtest 3 (`Balance::eye_hybrid`, `winter_toll`,
/// `wake_spread`). On a thread holding a
/// [`before_v46::Guard`] (reading or hashing a snapshot of version 45 or
/// before) none is written or read: the tunables read as 0 (off).
pub mod before_v46 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The competitive balance tunables before version 46: off.
    pub const OLD_VALUE: f32 = 0.0;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_VALUE)
        } else {
            f32::deserialize(d)
        }
    }
}

/// `#[serde(with = "crate::serde_util::before_v47")]` on the playtest 3
/// founding and drinking tunables (`Balance::reachable_water`,
/// `founding_conditions`), and `before_v47::list` on the state they keep
/// (`World::lineage_homes`). On a thread holding a [`before_v47::Guard`]
/// (reading or hashing a snapshot of version 46 or before) none is written
/// or read: the tunables read as 0 (the old drinking and founding rules)
/// and no homes are known.
pub mod before_v47 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The tunables before version 47: off.
    pub const OLD_VALUE: f32 = 0.0;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_VALUE)
        } else {
            f32::deserialize(d)
        }
    }

    /// The same for a value that older versions don't have at all: its
    /// default when reading them.
    pub mod list {
        use super::*;

        pub fn serialize<S: Serializer, T: Serialize>(v: &T, s: S) -> Result<S::Ok, S::Error> {
            if OLD.with(Cell::get) {
                s.serialize_unit()
            } else {
                v.serialize(s)
            }
        }

        pub fn deserialize<'de, D: Deserializer<'de>, T: Deserialize<'de> + Default>(d: D) -> Result<T, D::Error> {
            if OLD.with(Cell::get) {
                Ok(T::default())
            } else {
                T::deserialize(d)
            }
        }
    }
}

/// What world snapshot version 48 added: territory claims waiting on a
/// migration and where migrations started (`GroupPlans::pending`). On a
/// thread holding a [`before_v48::Guard`] (reading or hashing a snapshot of
/// version 47 or before) the field is written as nothing and read as empty.
pub mod before_v48 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer, T: Serialize>(v: &T, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>, T: Deserialize<'de> + Default>(d: D) -> Result<T, D::Error> {
        if OLD.with(Cell::get) {
            Ok(T::default())
        } else {
            T::deserialize(d)
        }
    }
}

/// `#[serde(with = "crate::serde_util::before_v49")]` on the competitive
/// balance tunables of playtest 4 (`Balance::winter_rest`,
/// `toxic_avoidance`, `egg_attendance`, `player_species`, `raised_cap`). On a thread holding a
/// [`before_v49::Guard`] (reading or hashing a snapshot of version 48 or
/// before) none is written or read: the tunables read as 0 (off).
pub mod before_v49 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The competitive balance tunables before version 49: off.
    pub const OLD_VALUE: f32 = 0.0;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_VALUE)
        } else {
            f32::deserialize(d)
        }
    }
}

/// `#[serde(with = "crate::serde_util::before_v50")]` on the playtest 4
/// founding safety tunable (`Balance::founding_safety`). On a thread holding
/// a [`before_v50::Guard`] (reading or hashing a snapshot of version 49 or
/// before) it is neither written nor read: it reads as 0, the old rules.
pub mod before_v50 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The tunable before version 50: off.
    pub const OLD_VALUE: f32 = 0.0;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_VALUE)
        } else {
            f32::deserialize(d)
        }
    }
}

/// `#[serde(with = "crate::serde_util::before_v51")]` on the playtest 4
/// predator and dormancy tunables (`Balance::player_packs`,
/// `dormant_water`, `possessed_sleep`). On a thread holding a
/// [`before_v51::Guard`] (reading or hashing a snapshot of version 50 or
/// before) none is written or read: they read as 0 (off).
pub mod before_v51 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The tunable before version 51: off.
    pub const OLD_VALUE: f32 = 0.0;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_VALUE)
        } else {
            f32::deserialize(d)
        }
    }
}

/// `#[serde(with = "crate::serde_util::before_v55")]` on the playtest 5
/// eggs-and-young tunable (`Balance::brood_care`). On a thread holding a
/// [`before_v55::Guard`] (reading or hashing a snapshot of version 54 or
/// before) it is neither written nor read: it reads as 0, the old rules.
pub mod before_v55 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The tunable before version 55: off.
    pub const OLD_VALUE: f32 = 0.0;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_VALUE)
        } else {
            f32::deserialize(d)
        }
    }
}

/// `#[serde(with = "crate::serde_util::before_v56")]` on the playtest 5
/// balance tunables (`Balance::thermo_hybrid`, `flight_upkeep`,
/// `avoid_cap`, `cold_heat`, `species_credit`). On a thread holding a
/// [`before_v56::Guard`] (reading or hashing a snapshot of version 55 or
/// before) none is written or read: they read as 0 (off).
pub mod before_v56 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The tunable before version 56: off.
    pub const OLD_VALUE: f32 = 0.0;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_VALUE)
        } else {
            f32::deserialize(d)
        }
    }
}

/// `#[serde(with = "crate::serde_util::before_v57")]` on the playtest 5
/// survival tunables (`Balance::dry_beds`, `drink_reach`). On a thread
/// holding a [`before_v57::Guard`] (reading or hashing a snapshot of
/// version 56 or before) none is written or read: they read as 0 (off).
pub mod before_v57 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The tunable before version 57: off.
    pub const OLD_VALUE: f32 = 0.0;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_VALUE)
        } else {
            f32::deserialize(d)
        }
    }
}

/// `#[serde(with = "crate::serde_util::before_v58")]` on the era's daily
/// niche tallies (`Chronicle::niche_tally`). On a thread holding a
/// [`before_v58::Guard`] (reading or hashing a snapshot of version 57 or
/// before) it is neither written nor read: it reads as empty.
pub mod before_v58 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer, T: Serialize>(v: &T, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>, T: Deserialize<'de> + Default>(d: D) -> Result<T, D::Error> {
        if OLD.with(Cell::get) {
            Ok(T::default())
        } else {
            T::deserialize(d)
        }
    }
}

/// `#[serde(with = "crate::serde_util::before_v61")]` on the playtest 6
/// founding tunable (`Balance::founding_drought`). On a thread holding a
/// [`before_v61::Guard`] (reading or hashing a snapshot of version 60 or
/// before) it is neither written nor read: it reads as 0 (off).
pub mod before_v61 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The tunable before version 61: off.
    pub const OLD_VALUE: f32 = 0.0;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_VALUE)
        } else {
            f32::deserialize(d)
        }
    }
}

/// `#[serde(with = "crate::serde_util::before_v62")]` on the playtest 6
/// balance tunables (`Balance::toxin_upkeep`, `hungry_hunters`,
/// `toxin_source`, `speciation_credit`, `clutch_cover`). On a thread
/// holding a [`before_v62::Guard`] (reading or hashing a snapshot of
/// version 61 or before) none is written or read: they read as 0 (off).
pub mod before_v62 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The tunable before version 62: off.
    pub const OLD_VALUE: f32 = 0.0;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_VALUE)
        } else {
            f32::deserialize(d)
        }
    }
}

/// `#[serde(with = "crate::serde_util::before_v63")]` on the playtest 6
/// predator tunable (`Balance::prey_larder`). On a thread holding a
/// [`before_v63::Guard`] (reading or hashing a snapshot of version 62 or
/// before) it is neither written nor read: it reads as 0 (off).
pub mod before_v63 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The tunable before version 63: off.
    pub const OLD_VALUE: f32 = 0.0;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_VALUE)
        } else {
            f32::deserialize(d)
        }
    }
}

/// What world snapshot version 66 added: territory holds loosened for
/// starving holders and territories kept while their group migrates
/// (`GroupPlans::held`). On a thread holding a [`before_v66::Guard`]
/// (reading or hashing a snapshot of version 65 or before) the field is
/// written as nothing and read as empty.
pub mod before_v66 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer, T: Serialize>(v: &T, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>, T: Deserialize<'de> + Default>(d: D) -> Result<T, D::Error> {
        if OLD.with(Cell::get) {
            Ok(T::default())
        } else {
            T::deserialize(d)
        }
    }
}

/// `#[serde(with = "crate::serde_util::before_v70")]` on the playtest 7
/// founding tunable (`Balance::founding_rivals`). On a thread holding a
/// [`before_v70::Guard`] (reading or hashing a snapshot of version 69 or
/// before) it is neither written nor read: it reads as 0 (off).
pub mod before_v70 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The tunable before version 70: off.
    pub const OLD_VALUE: f32 = 0.0;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_VALUE)
        } else {
            f32::deserialize(d)
        }
    }
}

/// `#[serde(with = "crate::serde_util::before_v72")]` on the playtest 7
/// predator tunables (`Balance::winter_larder`, `prey_floor`). On a thread
/// holding a [`before_v72::Guard`] (reading or hashing a snapshot of
/// version 71 or before) they are neither written nor read: they read as 0
/// (off).
pub mod before_v72 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The tunables before version 72: off.
    pub const OLD_VALUE: f32 = 0.0;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_VALUE)
        } else {
            f32::deserialize(d)
        }
    }
}

/// `#[serde(with = "crate::serde_util::before_v73")]` on the playtest 7
/// balance tunables (`Balance::honest_signals`, `wild_isolation`). On a thread holding a
/// [`before_v73::Guard`] (reading or hashing a snapshot of version 72 or
/// before) it is neither written nor read: it reads as 0 (off).
pub mod before_v73 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The tunable before version 73: off.
    pub const OLD_VALUE: f32 = 0.0;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_VALUE)
        } else {
            f32::deserialize(d)
        }
    }
}

/// `#[serde(with = "crate::serde_util::before_v77")]` on the playtest 8
/// egg and sleep tunables (`Balance::clutch_split`, `dry_spell`). On a
/// thread holding a [`before_v77::Guard`] (reading or hashing a snapshot
/// of version 76 or before) they are neither written nor read: they read
/// as 0 (off).
pub mod before_v77 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The tunable before version 77: off.
    pub const OLD_VALUE: f32 = 0.0;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_VALUE)
        } else {
            f32::deserialize(d)
        }
    }
}

/// `#[serde(with = "crate::serde_util::before_v81")]` on the playtest 8
/// founder tunable (`Balance::founder_youth`). On a thread holding a
/// [`before_v81::Guard`] (reading or hashing a snapshot of version 80 or
/// before) it is neither written nor read: it reads as 0 (off).
pub mod before_v81 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The tunable before version 81: off.
    pub const OLD_VALUE: f32 = 0.0;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_VALUE)
        } else {
            f32::deserialize(d)
        }
    }
}

/// `#[serde(with = "crate::serde_util::before_v82")]` on the playtest 8
/// founding tunable (`Balance::rival_reach`). On a thread holding a
/// [`before_v82::Guard`] (reading or hashing a snapshot of version 81 or
/// before) it is neither written nor read: it reads as 0 (off).
pub mod before_v82 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The tunable before version 82: off.
    pub const OLD_VALUE: f32 = 0.0;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_VALUE)
        } else {
            f32::deserialize(d)
        }
    }
}

/// `#[serde(with = "crate::serde_util::before_v83")]` on the playtest 9
/// balance tunables (`Balance::wild_barrier`, `world_crowding`,
/// `wild_refuge`). On a
/// thread holding a [`before_v83::Guard`] (reading or hashing a snapshot of
/// version 82 or before) they are neither written nor read: they read as 0
/// (off).
pub mod before_v83 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The tunable before version 83: off.
    pub const OLD_VALUE: f32 = 0.0;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_VALUE)
        } else {
            f32::deserialize(d)
        }
    }
}

/// `#[serde(with = "crate::serde_util::before_v85")]` on the playtest 9
/// placement and pack tunables (`Balance::rival_clear`, `pack_cohesion`).
/// On a thread holding a [`before_v85::Guard`] (reading or hashing a
/// snapshot of version 84 or before) they are neither written nor read:
/// they read as 0 (off).
pub mod before_v85 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The tunable before version 85: off.
    pub const OLD_VALUE: f32 = 0.0;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_VALUE)
        } else {
            f32::deserialize(d)
        }
    }
}

/// `#[serde(with = "crate::serde_util::before_v89")]` on the playtest 10
/// balance tunables (`Balance::nest_raids`, `share_litters`, `refuge_ramp`).
/// On a thread holding a [`before_v89::Guard`] (reading or hashing a
/// snapshot of version 88 or before) they are neither written nor read:
/// they read as 0 (off).
pub mod before_v89 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The tunable before version 89: off.
    pub const OLD_VALUE: f32 = 0.0;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_VALUE)
        } else {
            f32::deserialize(d)
        }
    }
}

/// `#[serde(with = "crate::serde_util::before_v90")]` on the playtest 10
/// placement tunables (`Balance::reinforce_clear`, `founding_together`).
/// On a thread holding a [`before_v90::Guard`] (reading or hashing a
/// snapshot of version 89 or before) they are neither written nor read:
/// they read as 0 (off).
pub mod before_v90 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The tunable before version 90: off.
    pub const OLD_VALUE: f32 = 0.0;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_VALUE)
        } else {
            f32::deserialize(d)
        }
    }
}

/// `#[serde(with = "crate::serde_util::before_v92")]` on the playtest 11
/// balance tunables (`Balance::graced_hunting`, `share_memory`,
/// `wild_rebound`), and (`before_v92::wide`) on
/// `LegacySnapshot::world_alleles`. On a thread holding a
/// [`before_v92::Guard`] (reading or hashing a snapshot of version 91 or
/// before) they are neither written nor read: they read as 0 (off).
pub mod before_v92 {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        static OLD: Cell<bool> = const { Cell::new(false) };
    }

    /// The tunable before version 92: off.
    pub const OLD_VALUE: f32 = 0.0;

    /// The old layout on this thread while this guard lives.
    pub struct Guard(bool);

    impl Guard {
        pub fn new() -> Guard {
            Guard(OLD.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            OLD.with(|c| c.set(self.0));
        }
    }

    pub fn serialize<S: Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        if OLD.with(Cell::get) {
            s.serialize_unit()
        } else {
            v.serialize(s)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        if OLD.with(Cell::get) {
            Ok(OLD_VALUE)
        } else {
            f32::deserialize(d)
        }
    }

    /// The same layout rule for an `f64` (`LegacySnapshot::world_alleles`),
    /// under the same guard.
    pub mod wide {
        use super::*;

        pub fn serialize<S: Serializer>(v: &f64, s: S) -> Result<S::Ok, S::Error> {
            if OLD.with(Cell::get) {
                s.serialize_unit()
            } else {
                v.serialize(s)
            }
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
            if OLD.with(Cell::get) {
                Ok(0.0)
            } else {
                f64::deserialize(d)
            }
        }
    }
}
