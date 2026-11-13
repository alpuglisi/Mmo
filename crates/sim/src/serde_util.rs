//! Serde helpers for world snapshots (feature `serde`).
//!
//! - `array`: fixed-size arrays longer than 32 (serde only derives up to 32),
//!   written as tuples (no length prefix): `#[serde(with = "crate::serde_util::array")]`.
//! - `array_vec`: the same inside a `Vec`.
//! - `sorted_map`: a `HashMap` written in key order, so equal worlds give
//!   byte-identical snapshots (a `HashMap` iterates in a per-instance order).

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
