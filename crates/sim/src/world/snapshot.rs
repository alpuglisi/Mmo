//! World snapshots (feature `serde`): the whole simulation state as bytes,
//! so a server can stop and carry on exactly where it was.
//!
//! All randomness is derived from `(seed, stream, tick, id)`, so the state
//! plus the tick fully determine the future: a restored world steps exactly
//! like the original (`tests::restored_world_continues_exactly`).
//!
//! The static map ([`Terrain`]: biomes, relief, rivers, ground cover) is not
//! in a snapshot: the snapshot names it by content hash, and the caller
//! stores it once per world ([`Terrain::encode`]) and hands it back to
//! [`World::restore`]. Everything that changes (vegetation, water levels,
//! snow and ice, weather, drought and flood) is in the snapshot.
//!
//! Layout (integers little-endian):
//!
//! | bytes | field |
//! | --- | --- |
//! | 8 | magic `EVOSNAP\0` |
//! | 4 | format version ([`SNAPSHOT_VERSION`]) |
//! | 4 | flags (bit 0: payload is LZ4-compressed) |
//! | 8 | config hash ([`WorldConfig::snapshot_hash`]) |
//! | 8 | tick |
//! | 8 | organisms (agents, dead ones not yet removed included) |
//! | 8 | payload length before compression |
//! | 8 | FNV-1a 64 checksum of the stored payload |
//! | 8 | terrain content hash ([`Terrain::hash`]; from version 6) |
//! | … | payload: `bincode` of `World`, LZ4 block-compressed |
//!
//! The header stays readable across format versions, so a server can say
//! why it refuses a snapshot, and can look up the terrain before decoding.
//! Skipped and rebuilt on restore: the id → index map, the spatial grid
//! (rebuilt every tick anyway), A* scratch buffers and phase timings.
//! Everything else, caches that steer the future included (hourly density,
//! cohort representatives, LOD passability, cache and clutch lookups), is
//! stored.
//!
//! Only snapshots of this format version restore. Version 5 moved the
//! terrain out of the snapshot; version 6 added lakes and floodplains to map
//! generation (`Ground::plain`, `Tile::silt`) and a wind direction to the
//! weather cells, which older maps can't be given deterministically, so
//! saves from before version 6 don't load (start the world afresh).
//!
//! A terrain blob ([`Terrain::encode`]): magic `EVOTERR\0`, format version
//! ([`TERRAIN_VERSION`], u32), flags (u32, bit 0 LZ4), content hash, width,
//! height, payload length before compression, payload checksum (u64 each),
//! then `bincode` of the [`Terrain`], LZ4-compressed. The content hash is
//! FNV-1a 64 of that `bincode`.

use std::fmt;
use std::sync::Arc;

use super::spatial::SpatialGrid;
use super::World;
use crate::config::WorldConfig;
use crate::terrain::Terrain;

/// Bump on any change to the serialized state (a field added, removed,
/// reordered or retyped in any type the world holds).
pub const SNAPSHOT_VERSION: u32 = 6;
/// Bump on any change to [`Terrain`]'s serialized form.
pub const TERRAIN_VERSION: u32 = 1;

const MAGIC: &[u8; 8] = b"EVOSNAP\0";
/// Header length before version 6 (no terrain hash).
const HEADER_LEN_OLD: usize = 8 + 4 + 4 + 8 + 8 + 8 + 8 + 8;
const HEADER_LEN: usize = HEADER_LEN_OLD + 8;
const FLAG_LZ4: u32 = 1;
const TERRAIN_MAGIC: &[u8; 8] = b"EVOTERR\0";
const TERRAIN_HEADER_LEN: usize = 8 + 4 + 4 + 8 * 5;

/// The fixed header of a snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotHeader {
    pub version: u32,
    pub flags: u32,
    pub config_hash: u64,
    pub tick: u64,
    pub organisms: u64,
    pub raw_len: u64,
    pub checksum: u64,
    /// Content hash of the terrain the snapshot needs ([`Terrain::hash`]);
    /// `None` before version 6 (such snapshots no longer restore, but their
    /// header still reads, so the version can be reported).
    pub terrain: Option<u64>,
}

impl SnapshotHeader {
    fn len(&self) -> usize {
        if self.version >= 6 {
            HEADER_LEN
        } else {
            HEADER_LEN_OLD
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotError {
    /// Not a snapshot (or terrain blob), or cut short.
    BadHeader,
    /// Written by another format version; it can't be read by this build.
    Version { found: u32, expected: u32 },
    /// The payload doesn't match its checksum (corrupt or truncated file).
    Checksum,
    /// The payload doesn't decode (or its config doesn't match the header).
    Decode(String),
    /// The snapshot needs a terrain that was not given (not stored, or not
    /// found).
    MissingTerrain { hash: u64 },
    /// The terrain given is not the one the snapshot names.
    WrongTerrain { expected: u64, found: u64 },
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SnapshotError::BadHeader => write!(f, "not a world snapshot (bad or truncated header)"),
            SnapshotError::Version { found, expected } => {
                write!(f, "snapshot format version {found}, this build reads version {expected}")
            }
            SnapshotError::Checksum => write!(f, "snapshot payload checksum mismatch (corrupt or truncated)"),
            SnapshotError::Decode(e) => write!(f, "snapshot payload does not decode: {e}"),
            SnapshotError::MissingTerrain { hash } => {
                write!(f, "the snapshot's terrain {hash:016x} is missing (it is stored once per world)")
            }
            SnapshotError::WrongTerrain { expected, found } => {
                write!(f, "the snapshot needs terrain {expected:016x}, not {found:016x}")
            }
        }
    }
}

impl std::error::Error for SnapshotError {}

fn fnv1a(bytes: &[u8]) -> u64 {
    // Eight bytes at a time: fast enough for tens of MB (not a standard
    // FNV, but stable, which is all a checksum needs).
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let (chunks, rest) = bytes.as_chunks::<8>();
    for c in chunks {
        h = (h ^ u64::from_le_bytes(*c)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    for &b in rest {
        h = (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn decode_err(e: impl fmt::Display) -> SnapshotError {
    SnapshotError::Decode(e.to_string())
}

/// The LZ4 payload after a header, checked and decompressed.
fn payload(
    stored: &[u8],
    flags: u32,
    raw_len: u64,
    checksum: u64,
) -> Result<std::borrow::Cow<'_, [u8]>, SnapshotError> {
    if fnv1a(stored) != checksum {
        return Err(SnapshotError::Checksum);
    }
    if flags & FLAG_LZ4 == 0 {
        return Ok(stored.into());
    }
    let len = usize::try_from(raw_len).map_err(|_| SnapshotError::BadHeader)?;
    Ok(lz4_flex::block::decompress(stored, len).map_err(decode_err)?.into())
}

impl Terrain {
    /// Content hash: FNV-1a 64 of the terrain's `bincode`. Computed once,
    /// then cached (`Map::edit_terrain` forgets it).
    pub fn hash(&self) -> u64 {
        *self.hash.get_or_init(|| fnv1a(&bincode::serialize(self).expect("terrain serializes")))
    }

    /// The terrain as bytes, to store once per world (see the module docs
    /// for the layout). [`Terrain::decode`] reads it back.
    pub fn encode(&self) -> Vec<u8> {
        let raw = bincode::serialize(self).expect("terrain serializes");
        let hash = fnv1a(&raw);
        let _ = self.hash.set(hash);
        let payload = lz4_flex::block::compress(&raw);
        let mut out = Vec::with_capacity(TERRAIN_HEADER_LEN + payload.len());
        out.extend_from_slice(TERRAIN_MAGIC);
        out.extend_from_slice(&TERRAIN_VERSION.to_le_bytes());
        out.extend_from_slice(&FLAG_LZ4.to_le_bytes());
        for v in [hash, self.width as u64, self.height as u64, raw.len() as u64, fnv1a(&payload)] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&payload);
        out
    }

    /// The content hash in a terrain blob's header, without decoding it.
    pub fn encoded_hash(bytes: &[u8]) -> Result<u64, SnapshotError> {
        if bytes.len() < TERRAIN_HEADER_LEN || &bytes[..8] != TERRAIN_MAGIC {
            return Err(SnapshotError::BadHeader);
        }
        Ok(u64::from_le_bytes(bytes[16..24].try_into().unwrap()))
    }

    /// A terrain from [`Terrain::encode`] bytes. Fails on another format
    /// version, a corrupt payload or one that doesn't match its hash.
    pub fn decode(bytes: &[u8]) -> Result<Terrain, SnapshotError> {
        let hash = Terrain::encoded_hash(bytes)?;
        let u32_at = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
        let u64_at = |o: usize| u64::from_le_bytes(bytes[o..o + 8].try_into().unwrap());
        let (version, flags) = (u32_at(8), u32_at(12));
        if version != TERRAIN_VERSION {
            return Err(SnapshotError::Version { found: version, expected: TERRAIN_VERSION });
        }
        let (width, height, raw_len, checksum) = (u64_at(24), u64_at(32), u64_at(40), u64_at(48));
        let raw = payload(&bytes[TERRAIN_HEADER_LEN..], flags, raw_len, checksum)?;
        if fnv1a(&raw) != hash {
            return Err(decode_err("terrain does not match its content hash"));
        }
        let t: Terrain = bincode::deserialize(&raw).map_err(decode_err)?;
        let n = t.width * t.height;
        if (t.width as u64, t.height as u64) != (width, height) || t.ground.len() != n || t.catchment.len() != n {
            return Err(decode_err("terrain does not match its header"));
        }
        let _ = t.hash.set(hash);
        Ok(t)
    }
}

impl WorldConfig {
    /// Stable hash of every setting (balance included), stored in snapshot
    /// headers so a server can tell whether its configuration changed.
    pub fn snapshot_hash(&self) -> u64 {
        fnv1a(&bincode::serialize(self).expect("config serializes"))
    }

    /// Can a world snapshotted under `self` continue under `other`? The map,
    /// clock and calendar must match; balance and caps may change.
    pub fn snapshot_compatible(&self, other: &WorldConfig) -> bool {
        self.seed == other.seed
            && self.width == other.width
            && self.height == other.height
            && self.ticks_per_day == other.ticks_per_day
            && self.days_per_year == other.days_per_year
            && self.lod.enabled == other.lod.enabled
    }
}

impl SnapshotHeader {
    /// Read the header without decoding the payload.
    pub fn read(bytes: &[u8]) -> Result<SnapshotHeader, SnapshotError> {
        if bytes.len() < HEADER_LEN_OLD || &bytes[..8] != MAGIC {
            return Err(SnapshotError::BadHeader);
        }
        let u32_at = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
        let u64_at = |o: usize| u64::from_le_bytes(bytes[o..o + 8].try_into().unwrap());
        let version = u32_at(8);
        let terrain = if version >= 6 {
            if bytes.len() < HEADER_LEN {
                return Err(SnapshotError::BadHeader);
            }
            Some(u64_at(56))
        } else {
            None
        };
        Ok(SnapshotHeader {
            version,
            flags: u32_at(12),
            config_hash: u64_at(16),
            tick: u64_at(24),
            organisms: u64_at(32),
            raw_len: u64_at(40),
            checksum: u64_at(48),
            terrain,
        })
    }

    fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&self.version.to_le_bytes());
        out.extend_from_slice(&self.flags.to_le_bytes());
        for v in [self.config_hash, self.tick, self.organisms, self.raw_len, self.checksum] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        if let Some(t) = self.terrain {
            out.extend_from_slice(&t.to_le_bytes());
        }
    }
}

impl World {
    /// The whole simulation state as bytes, except the static terrain
    /// (`self.map.terrain()`), which the snapshot names by hash (see the
    /// module docs for the format). [`World::restore`] continues it exactly.
    pub fn snapshot(&self) -> Vec<u8> {
        let raw = bincode::serialize(self).expect("world serializes");
        let payload = lz4_flex::block::compress(&raw);
        let header = SnapshotHeader {
            version: SNAPSHOT_VERSION,
            flags: FLAG_LZ4,
            config_hash: self.config.snapshot_hash(),
            tick: self.tick,
            organisms: self.organisms.len() as u64,
            raw_len: raw.len() as u64,
            checksum: fnv1a(&payload),
            terrain: Some(self.map.terrain().hash()),
        };
        let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
        header.write(&mut out);
        out.extend_from_slice(&payload);
        out
    }

    /// A world from [`World::snapshot`] bytes and the terrain it names
    /// (`SnapshotHeader::terrain`), with its derived lookups rebuilt. Fails
    /// on another format version, a corrupt payload, or a missing or wrong
    /// terrain.
    pub fn restore(bytes: &[u8], terrain: Option<Arc<Terrain>>) -> Result<World, SnapshotError> {
        let h = SnapshotHeader::read(bytes)?;
        if h.version != SNAPSHOT_VERSION {
            return Err(SnapshotError::Version { found: h.version, expected: SNAPSHOT_VERSION });
        }
        let hash = h.terrain.ok_or(SnapshotError::BadHeader)?;
        let raw = payload(&bytes[h.len()..], h.flags, h.raw_len, h.checksum)?;
        let terrain = terrain.ok_or(SnapshotError::MissingTerrain { hash })?;
        if terrain.hash() != hash {
            return Err(SnapshotError::WrongTerrain { expected: hash, found: terrain.hash() });
        }
        let mut w: World = bincode::deserialize(&raw).map_err(decode_err)?;
        let m = &w.map;
        if m.terrain().hash() != hash
            || (terrain.width, terrain.height) != (m.width, m.height)
            || terrain.ground.len() != m.tiles.len()
        {
            return Err(decode_err("payload does not match its terrain"));
        }
        w.map.set_terrain(terrain);
        if w.config.snapshot_hash() != h.config_hash || w.tick != h.tick {
            return Err(SnapshotError::Decode("payload does not match its header".into()));
        }
        w.rebuild_derived();
        Ok(w)
    }

    /// Rebuild what snapshots skip.
    fn rebuild_derived(&mut self) {
        self.index = self.organisms.iter().enumerate().map(|(k, o)| (o.id, k)).collect();
        self.grid = SpatialGrid::new(self.config.width, self.config.height, super::GRID_CELL);
        self.grid.rebuild(&self.organisms);
    }
}

#[cfg(test)]
mod tests;
