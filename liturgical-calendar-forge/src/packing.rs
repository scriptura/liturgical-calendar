//! Étape 6 — Binary Packing : sérialisation `.kald` v5.
//!
//! `build_kald` et `write_kald` prennent les données PRÉ-CALCULÉES par `forge/lib.rs` :
//!   - `all_entries`    : 431 × [TimelineEntry; 366] — après vespers_lookahead_pass
//!   - `feast_registry` : FeastRegistryBuilder — après Pass 1 build_feast_registry
//!   - `pool`           : PoolBuilder — rempli par generate_year
//!
//! L'orchestration 2 passes (Pass1 → generate_year → vespers) reste dans `forge/lib.rs`.
//!
//! Invariants :
//!   - Endianness LE canonique.
//!   - SHA-256 sur [Registry ∥ Timeline ∥ Pool] — header exclu.
//!   - `registry_offset = 80`, `pool_offset = 80 + registry_count×4 + entry_count×8`.
//!   - Validation post-écriture via `kal_validate_header`.

#![allow(missing_docs)]

use std::{
    io::{BufWriter, Write},
    path::Path,
};

use liturgical_calendar_core::{
    entry::{KALD_FORMAT_VERSION, LAYOUT_DISCRIMINANT, TimelineEntry},
    kal_validate_header,
};
use sha2::{Digest, Sha256};

use crate::{
    error::ForgeError,
    materialization::{FeastRegistryBuilder, PoolBuilder},
};

/// Années couvertes : 1969–2399.
const YEAR_COUNT: u32 = 431;
/// Slots par année (stride constant, incluant Padding doy=59).
const SLOTS_PER_YEAR: u32 = 366;
/// Entrées totales dans la Timeline.
const ENTRY_COUNT: u32 = YEAR_COUNT * SLOTS_PER_YEAR; // 157 746

// ─── build_kald ───────────────────────────────────────────────────────────────

/// Sérialise le corpus en buffer binaire `.kald` v5 — sans I/O.
///
/// Paramètres pré-calculés par `forge/lib.rs` :
/// - `all_entries`    : 431 tableaux `[TimelineEntry; 366]`, index 0 = année 1969.
///   La `vespers_lookahead_pass` DOIT avoir été appliquée.
/// - `feast_registry` : FeastRegistryBuilder complet (Pass 1 terminée).
/// - `pool`           : PoolBuilder rempli par `generate_year`.
///
/// Retourne `(checksum, full_bytes)` :
///   `checksum`   : SHA-256([Registry ∥ Timeline ∥ Pool]).
///   `full_bytes` : [Header ∥ Registry ∥ Timeline ∥ Pool].
pub(crate) fn build_kald(
    all_entries: Vec<[TimelineEntry; 366]>,
    feast_registry: &FeastRegistryBuilder,
    pool: PoolBuilder,
    variant_id: u16,
) -> Result<([u8; 32], Vec<u8>), ForgeError> {
    assert_eq!(
        all_entries.len() as u32,
        YEAR_COUNT,
        "build_kald : attendu {} années, reçu {}",
        YEAR_COUNT,
        all_entries.len()
    );

    let registry_count = feast_registry.registry_count();

    // ── Sérialisation du Feast Registry ──────────────────────────────────────
    // Array dense : position 0 = registry_index 1.

    let mut registry_bytes: Vec<u8> = Vec::with_capacity(registry_count as usize * 4);
    for entry in feast_registry.as_slice() {
        registry_bytes.extend_from_slice(&entry.feast_id.to_le_bytes());
        registry_bytes.extend_from_slice(&entry.flags.to_le_bytes());
    }
    debug_assert_eq!(registry_bytes.len(), registry_count as usize * 4);

    // ── Sérialisation de la Timeline ─────────────────────────────────────────
    // Années croissantes (1969→2399), DOY croissants (0→365).

    let mut timeline_bytes: Vec<u8> = Vec::with_capacity((ENTRY_COUNT * 8) as usize);
    for year_entries in &all_entries {
        for e in year_entries.iter() {
            timeline_bytes.extend_from_slice(&e.primary_index.to_le_bytes());
            timeline_bytes.extend_from_slice(&e.secondary_offset.to_le_bytes());
            timeline_bytes.push(e.occurrence_flags);
            timeline_bytes.push(e.secondary_count);
            timeline_bytes.push(e.liturgical_week);
            timeline_bytes.push(e._reserved);
        }
    }
    debug_assert_eq!(timeline_bytes.len(), (ENTRY_COUNT * 8) as usize);

    // ── Sérialisation du Secondary Pool ──────────────────────────────────────

    let pool_bytes: Vec<u8> = pool.data.iter().flat_map(|i| i.to_le_bytes()).collect();
    let pool_size: u32 = pool_bytes.len() as u32;
    let pool_offset: u32 = 80u32 + registry_count * 4 + ENTRY_COUNT * 8;

    // ── SHA-256(Registry ∥ Timeline ∥ Pool) ──────────────────────────────────

    let checksum: [u8; 32] = {
        let mut h = Sha256::new();
        h.update(&registry_bytes);
        h.update(&timeline_bytes);
        h.update(&pool_bytes);
        let c = h.finalize();
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&c);
        arr
    };

    // ── Header (80 octets) ───────────────────────────────────────────────────

    // feast_id_base : min feast_id du corpus, métadonnée diagnostique.
    let feast_id_base: u16 = feast_registry
        .as_slice()
        .iter()
        .map(|e| e.feast_id)
        .min()
        .unwrap_or(0);

    let mut header = [0u8; 80];
    header[0..4].copy_from_slice(b"KALD");
    header[4..6].copy_from_slice(&KALD_FORMAT_VERSION.to_le_bytes());
    header[6..8].copy_from_slice(&variant_id.to_le_bytes());
    header[8..10].copy_from_slice(&1969u16.to_le_bytes());
    header[10..12].copy_from_slice(&(YEAR_COUNT as u16).to_le_bytes());
    header[12..16].copy_from_slice(&ENTRY_COUNT.to_le_bytes());
    header[16..20].copy_from_slice(&pool_offset.to_le_bytes());
    header[20..24].copy_from_slice(&pool_size.to_le_bytes());
    header[24..28].copy_from_slice(&80u32.to_le_bytes()); // registry_offset = 80
    header[28..32].copy_from_slice(&registry_count.to_le_bytes());
    header[32..34].copy_from_slice(&feast_id_base.to_le_bytes());
    // [34..36] _reserved = 0x0000
    header[36..68].copy_from_slice(&checksum);
    header[68..76].copy_from_slice(&LAYOUT_DISCRIMINANT.to_le_bytes());
    // [76..80] _reserved2 = 0x00000000

    // ── Assemblage ───────────────────────────────────────────────────────────

    let total = 80 + registry_bytes.len() + timeline_bytes.len() + pool_bytes.len();
    let mut full: Vec<u8> = Vec::with_capacity(total);
    full.extend_from_slice(&header);
    full.extend_from_slice(&registry_bytes);
    full.extend_from_slice(&timeline_bytes);
    full.extend_from_slice(&pool_bytes);
    debug_assert_eq!(full.len(), total);

    // ── Validation post-construction ──────────────────────────────────────────

    let rc = unsafe { kal_validate_header(full.as_ptr(), full.len(), std::ptr::null_mut()) };
    if rc != 0 {
        return Err(ForgeError::KaldValidationFailed { code: rc });
    }

    Ok((checksum, full))
}

// ─── write_kald ───────────────────────────────────────────────────────────────

/// Produit le fichier `.kald` v5 sur disque et retourne le SHA-256.
///
/// Même pré-conditions que `build_kald` : vespers_lookahead_pass déjà appliquée.
pub(crate) fn write_kald(
    path: &Path,
    all_entries: Vec<[TimelineEntry; 366]>,
    feast_registry: &FeastRegistryBuilder,
    pool: PoolBuilder,
    variant_id: u16,
) -> Result<[u8; 32], ForgeError> {
    let (checksum, full) = build_kald(all_entries, feast_registry, pool, variant_id)?;

    let file = std::fs::File::create(path).map_err(ForgeError::Io)?;
    let mut w = BufWriter::new(file);
    w.write_all(&full).map_err(ForgeError::Io)?;
    w.flush().map_err(ForgeError::Io)?;

    Ok(checksum)
}
