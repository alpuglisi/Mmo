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
//! | 4 | flags (bit 0: payload is LZ4-compressed; bit 1: tiles apart, from version 11) |
//! | 8 | config hash ([`WorldConfig::snapshot_hash`]) |
//! | 8 | tick |
//! | 8 | organisms (agents, dead ones not yet removed included) |
//! | 8 | payload length before compression |
//! | 8 | FNV-1a 64 checksum of everything after the header |
//! | 8 | terrain content hash ([`Terrain::hash`]; from version 6) |
//! | 8 | stored payload length (only with tiles apart) |
//! | … | payload: `bincode` of `World` (varint integers from version 7), LZ4 block-compressed |
//! | … | tiles (only with tiles apart): deflate of the tile planes |
//!
//! With tiles apart (version 11), the payload writes the map's tiles as an
//! empty list (`serde_util::tiles`) and they follow it as planes: for each
//! of the 12 `f32` fields of a [`Tile`](crate::terrain::Tile) in declaration
//! order, each tile's bits XORed with those of its western neighbour (its
//! northern one at the start of a row, nothing for the first tile), split
//! into four byte planes (least significant first) of one byte per tile.
//! Fields that vary smoothly across the map (temperature, wind, wetness)
//! then leave long runs of zero high bytes, and deflate (fast level) stores
//! the tiles in about 60% of the space LZ4 took for them interleaved. The
//! two parts are encoded, and decoded, on two threads.
//!
//! The header stays readable across format versions, so a server can say
//! why it refuses a snapshot, and can look up the terrain before decoding.
//! Skipped and rebuilt on restore: the id → index map, the spatial grid
//! (rebuilt every tick anyway), A* scratch buffers, phase timings, and each
//! organism's genome-derived caches (phenotype, traits, recessive load:
//! pure functions of the genome, recomputed by `Organism::refresh_derived`
//! to the bit). Everything else, caches that steer the future included
//! (A* paths, hourly density, cohort representatives, LOD passability,
//! cache and clutch lookups), is stored.
//!
//! Genomes are written compactly but exactly (`genome::wire`: origin tags
//! interned per genome, 0.0 and 1.0 alleles in two bits, other values
//! bit-packed with their exponent in three bits), and a pregnancy's father
//! genome as the index of the father while he is in the world
//! (`serde_util::father`), so the restore shares it with him again.
//!
//! Snapshots of this format version restore, and of version 10, which holds
//! the same state with the tiles inside the payload
//! ([`SnapshotHeader::restorable`]). Version 5 moved the
//! terrain out of the snapshot; version 6 added lakes and floodplains to map
//! generation (`Ground::plain`, `Tile::silt`) and a wind direction to the
//! weather cells, which older maps can't be given deterministically, so
//! saves from before version 6 don't load (start the world afresh).
//! Version 7 is the leaner organism encoding above; a version 6 save is
//! refused like any other version. Version 8 adds three balance parameters
//! to the saved config (`Balance::threat_caution`, `alert_guard`,
//! `pasture_search`). Version 9 adds each hunter's search image
//! (`Organism::search_image`) and the `Balance::search_image` parameter.
//! Version 10 adds player groups and territories (`Organism::group`,
//! migration orders keyed by player and group, the world's claimed
//! territories and claim times). Version 11 writes the tiles apart (above).
//!
//! A terrain blob ([`Terrain::encode`]): magic `EVOTERR\0`, format version
//! ([`TERRAIN_VERSION`], u32), flags (u32, bit 0 LZ4), content hash, width,
//! height, payload length before compression, payload checksum (u64 each),
//! then `bincode` of the [`Terrain`], LZ4-compressed. The content hash is
//! FNV-1a 64 of that `bincode`.

use std::fmt;
use std::io::{Read, Write};
use std::sync::Arc;

use bincode::Options;

use super::spatial::SpatialGrid;
use super::World;
use crate::config::WorldConfig;
use crate::terrain::{Terrain, Tile};

/// Bump on any change to the serialized state (a field added, removed,
/// reordered or retyped in any type the world holds).
pub const SNAPSHOT_VERSION: u32 = 11;
/// The oldest format version that still restores: the same state as
/// version 11, with the tiles inside the payload.
const OLDEST_RESTORABLE: u32 = 10;
/// Bump on any change to [`Terrain`]'s serialized form.
pub const TERRAIN_VERSION: u32 = 1;

const MAGIC: &[u8; 8] = b"EVOSNAP\0";
/// Header length before version 6 (no terrain hash).
const HEADER_LEN_OLD: usize = 8 + 4 + 4 + 8 + 8 + 8 + 8 + 8;
const HEADER_LEN: usize = HEADER_LEN_OLD + 8;
const FLAG_LZ4: u32 = 1;
const FLAG_TILES_APART: u32 = 2;
/// `f32` fields of a [`Tile`], each written as four byte planes.
const TILE_FIELDS: usize = 12;
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

    /// Whether [`World::restore`] reads snapshots of this format version
    /// (this one, or the previous one with the same state).
    pub fn restorable(&self) -> bool {
        (OLDEST_RESTORABLE..=SNAPSHOT_VERSION).contains(&self.version)
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

/// `bincode` settings of a world payload (from version 7): integers as
/// varints (small counts, ids, ticks and enum tags take a byte or two
/// instead of four or eight), little-endian, no trailing bytes. Floats are
/// written whole either way. Terrain blobs and hashes keep plain
/// `bincode::serialize` (fixed-width integers).
fn payload_options() -> impl bincode::Options {
    bincode::DefaultOptions::new()
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
    decompress(stored, flags, raw_len)
}

/// A payload decompressed (if its flags say LZ4).
fn decompress(stored: &[u8], flags: u32, raw_len: u64) -> Result<std::borrow::Cow<'_, [u8]>, SnapshotError> {
    if flags & FLAG_LZ4 == 0 {
        return Ok(stored.into());
    }
    let len = usize::try_from(raw_len).map_err(|_| SnapshotError::BadHeader)?;
    Ok(lz4_flex::block::decompress(stored, len).map_err(decode_err)?.into())
}

/// A tile's fields as bits, in declaration order (destructured, so a new
/// field fails to compile here until the encoding takes it).
fn tile_bits(t: &Tile) -> [u32; TILE_FIELDS] {
    let Tile { temp, water_depth, current, ice, snow, mud, veg, meat, wind, rain, wetness, silt } = *t;
    [temp, water_depth, current, ice, snow, mud, veg, meat, wind, rain, wetness, silt].map(f32::to_bits)
}

fn tile_from_bits(b: [u32; TILE_FIELDS]) -> Tile {
    let [temp, water_depth, current, ice, snow, mud, veg, meat, wind, rain, wetness, silt] = b.map(f32::from_bits);
    Tile { temp, water_depth, current, ice, snow, mud, veg, meat, wind, rain, wetness, silt }
}

/// The tile whose bits predict tile `i`'s: its western neighbour, else its
/// northern one, else none.
fn predictor(i: usize, width: usize) -> Option<usize> {
    if !i.is_multiple_of(width) {
        Some(i - 1)
    } else {
        i.checked_sub(width)
    }
}

/// The tiles as deflated byte planes (see the module docs).
fn encode_tiles(tiles: &[Tile], width: usize) -> Vec<u8> {
    let n = tiles.len();
    let bits: Vec<[u32; TILE_FIELDS]> = tiles.iter().map(tile_bits).collect();
    let xored: Vec<[u32; TILE_FIELDS]> = (0..n)
        .map(|i| {
            let pred = predictor(i, width).map_or([0; TILE_FIELDS], |p| bits[p]);
            std::array::from_fn(|f| bits[i][f] ^ pred[f])
        })
        .collect();
    drop(bits);
    // Plane by plane, writing one byte stream at a time.
    let mut planes = vec![0u8; n * TILE_FIELDS * 4];
    for (f, field) in planes.chunks_exact_mut(n * 4).enumerate() {
        let (p0, rest) = field.split_at_mut(n);
        let (p1, rest) = rest.split_at_mut(n);
        let (p2, p3) = rest.split_at_mut(n);
        for (i, x) in xored.iter().enumerate() {
            let [b0, b1, b2, b3] = x[f].to_le_bytes();
            (p0[i], p1[i], p2[i], p3[i]) = (b0, b1, b2, b3);
        }
    }
    drop(xored);
    let mut e = flate2::write::DeflateEncoder::new(Vec::with_capacity(planes.len() / 3), flate2::Compression::fast());
    e.write_all(&planes).expect("deflate into memory");
    e.finish().expect("deflate into memory")
}

/// The tiles of a `width × height` map from [`encode_tiles`] bytes.
fn decode_tiles(stored: &[u8], width: usize, height: usize) -> Result<Vec<Tile>, SnapshotError> {
    let n = width * height;
    let len = n * TILE_FIELDS * 4;
    let mut planes = Vec::with_capacity(len);
    flate2::read::DeflateDecoder::new(stored).take(len as u64 + 1).read_to_end(&mut planes).map_err(decode_err)?;
    if planes.len() != len {
        return Err(decode_err("the tiles do not match the map's size"));
    }
    let mut bits = vec![[0u32; TILE_FIELDS]; n];
    for (f, field) in planes.chunks_exact(n * 4).enumerate() {
        let (p0, rest) = field.split_at(n);
        let (p1, rest) = rest.split_at(n);
        let (p2, p3) = rest.split_at(n);
        for (i, b) in bits.iter_mut().enumerate() {
            b[f] = u32::from_le_bytes([p0[i], p1[i], p2[i], p3[i]]);
        }
    }
    drop(planes);
    for i in 0..n {
        if let Some(p) = predictor(i, width) {
            let pred = bits[p];
            for (b, q) in bits[i].iter_mut().zip(pred) {
                *b ^= q;
            }
        }
    }
    Ok(bits.into_iter().map(tile_from_bits).collect())
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
        let (payload, raw_len, tiles) = std::thread::scope(|s| {
            let tiles = s.spawn(|| encode_tiles(&self.map.tiles, self.map.width));
            let raw = {
                let _apart = crate::serde_util::tiles::Apart::new();
                payload_options().serialize(self).expect("world serializes")
            };
            let payload = lz4_flex::block::compress(&raw);
            (payload, raw.len(), tiles.join().expect("tile encoding panicked"))
        });
        let mut body = Vec::with_capacity(8 + payload.len() + tiles.len());
        body.extend_from_slice(&(payload.len() as u64).to_le_bytes());
        body.extend_from_slice(&payload);
        body.extend_from_slice(&tiles);
        self.snapshot_with(SNAPSHOT_VERSION, FLAG_LZ4 | FLAG_TILES_APART, raw_len, &body)
    }

    fn snapshot_with(&self, version: u32, flags: u32, raw_len: usize, body: &[u8]) -> Vec<u8> {
        let header = SnapshotHeader {
            version,
            flags,
            config_hash: self.config.snapshot_hash(),
            tick: self.tick,
            organisms: self.organisms.len() as u64,
            raw_len: raw_len as u64,
            checksum: fnv1a(body),
            terrain: Some(self.map.terrain().hash()),
        };
        let mut out = Vec::with_capacity(HEADER_LEN + body.len());
        header.write(&mut out);
        out.extend_from_slice(body);
        out
    }

    /// A version 10 snapshot (tiles inside the payload), as earlier builds
    /// wrote, to test that they still restore.
    #[cfg(test)]
    fn snapshot_v10(&self) -> Vec<u8> {
        let raw = payload_options().serialize(self).expect("world serializes");
        self.snapshot_with(OLDEST_RESTORABLE, FLAG_LZ4, raw.len(), &lz4_flex::block::compress(&raw))
    }

    /// A world from [`World::snapshot`] bytes (or a version 10 snapshot's)
    /// and the terrain it names (`SnapshotHeader::terrain`), with its
    /// derived lookups rebuilt. Fails on another format version, a corrupt
    /// payload, or a missing or wrong terrain.
    pub fn restore(bytes: &[u8], terrain: Option<Arc<Terrain>>) -> Result<World, SnapshotError> {
        let h = SnapshotHeader::read(bytes)?;
        if !h.restorable() {
            return Err(SnapshotError::Version { found: h.version, expected: SNAPSHOT_VERSION });
        }
        let hash = h.terrain.ok_or(SnapshotError::BadHeader)?;
        let body = &bytes[h.len()..];
        if fnv1a(body) != h.checksum {
            return Err(SnapshotError::Checksum);
        }
        let terrain = terrain.ok_or(SnapshotError::MissingTerrain { hash })?;
        if terrain.hash() != hash {
            return Err(SnapshotError::WrongTerrain { expected: hash, found: terrain.hash() });
        }
        let (stored, tiles) = if h.flags & FLAG_TILES_APART != 0 {
            let (len, rest) = body.split_at_checked(8).ok_or(SnapshotError::BadHeader)?;
            let len =
                usize::try_from(u64::from_le_bytes(len.try_into().unwrap())).map_err(|_| SnapshotError::BadHeader)?;
            let (stored, tiles) = rest.split_at_checked(len).ok_or(SnapshotError::BadHeader)?;
            (stored, Some(tiles))
        } else {
            (body, None)
        };
        let (world, tiles) = std::thread::scope(|s| {
            let tiles = tiles.map(|t| s.spawn(|| decode_tiles(t, terrain.width, terrain.height)));
            let world = decompress(stored, h.flags, h.raw_len)
                .and_then(|raw| payload_options().deserialize::<World>(&raw).map_err(decode_err));
            (world, tiles.map(|t| t.join().expect("tile decoding panicked")))
        });
        let mut w = world?;
        if let Some(tiles) = tiles {
            if !w.map.tiles.is_empty() {
                return Err(decode_err("tiles both inside and apart from the payload"));
            }
            w.map.tiles = tiles?;
        }
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
