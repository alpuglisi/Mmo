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
//! - `tiles`: the map's tiles, written as an empty list while a world
//!   snapshot writes them in a section of their own.

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

    pub fn serialize<S: Serializer, K: Serialize + Ord, V: Serialize>(
        m: &HashMap<K, V>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        let mut entries: Vec<(&K, &V)> = m.iter().collect();
        entries.sort_unstable_by(|a, b| a.0.cmp(b.0));
        s.collect_map(entries)
    }

    pub fn deserialize<'de, D: Deserializer<'de>, K: Deserialize<'de> + Eq + Hash, V: Deserialize<'de>>(
        d: D,
    ) -> Result<HashMap<K, V>, D::Error> {
        HashMap::deserialize(d)
    }
}

/// A world's organisms (`World::organisms`): written with the context
/// [`father`] needs, and read back with the genome-derived caches a
/// snapshot leaves out recomputed (`Organism::refresh_derived`) and shared
/// father genomes resolved:
/// `#[serde(with = "crate::serde_util::organisms")]`.
pub mod organisms {
    use std::collections::HashMap;
    use std::sync::Arc;

    use super::father::{Reading, Writing, READING, WRITING};
    use super::*;
    use crate::genome::Genome;
    use crate::organism::Organism;

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
        s.collect_seq(v)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Organism>, D::Error> {
        let placeholder = Arc::new(Genome::uniform(&[0.0; crate::genome::Gene::COUNT], 0));
        READING.with(|c| *c.borrow_mut() = Some(Reading { placeholder: placeholder.clone(), pending: Vec::new() }));
        let clear = Clear(&READING);
        let mut v = Vec::<Organism>::deserialize(d)?;
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
