//! Étape 6 — Binary Packing : sérialisation `.kald` v2.0.
//!
//! Invariants garantis ici :
//!   - Endianness Little-Endian canonique (to_le_bytes) — cross-platform.
//!   - SHA-256 sur [Data Body ∥ Secondary Pool] — header exclu.
//!   - Validation post-écriture via kal_validate_header.
//!   - pool_offset = 64 + entry_count * 8 (pas de padding inter-sections).

#![allow(missing_docs)]

use std::{
    io::{BufWriter, Write},
    path::Path,
};

use sha2::{Digest, Sha256};
use liturgical_calendar_core::{CalendarEntry, kal_validate_header};

use crate::{error::ForgeError, materialization::PoolBuilder};

/// Nombre d'années couvertes — constant pour la plage 1969–2399.
const YEAR_COUNT:  u32 = 431;
/// Nombre de slots par année — stride constant incluant la Padding Entry doy=59.
const SLOTS_PER_YEAR: u32 = 366;
/// Nombre total d'entrées dans le Data Body.
const ENTRY_COUNT: u32 = YEAR_COUNT * SLOTS_PER_YEAR; // 157 746

/// Produit le fichier `.kald` et retourne le SHA-256 (`checksum[0..8]` = Build ID).
///
/// `all_entries` : 431 tableaux `[CalendarEntry; 366]`, index 0 = année 1969.
/// La vespers_lookahead_pass DOIT avoir été appliquée avant cet appel.
///
/// Retourne `checksum: [u8; 32]` (SHA-256 du [Data Body ∥ Secondary Pool]).
pub(crate) fn write_kald(
    path:        &Path,
    all_entries: Vec<[CalendarEntry; 366]>,
    pool:        PoolBuilder,
    variant_id:  u16,
) -> Result<[u8; 32], ForgeError> {
    assert_eq!(
        all_entries.len() as u32,
        YEAR_COUNT,
        "write_kald : attendu {} années, reçu {}",
        YEAR_COUNT,
        all_entries.len()
    );

    // ── Sérialisation du Data Body ────────────────────────────────────────────
    // Ordre : années croissantes (1969→2399), DOY croissants (0→365).
    // Chaque CalendarEntry : 8 octets LE.

    let mut data_body: Vec<u8> = Vec::with_capacity((ENTRY_COUNT * 8) as usize);

    for year_entries in &all_entries {
        for entry in year_entries.iter() {
            data_body.extend_from_slice(&entry.primary_id.to_le_bytes());
            data_body.extend_from_slice(&entry.secondary_index.to_le_bytes());
            data_body.extend_from_slice(&entry.flags.to_le_bytes());
            data_body.push(entry.secondary_count);
            data_body.push(entry._reserved); // invariant : 0x00
        }
    }

    debug_assert_eq!(data_body.len(), (ENTRY_COUNT * 8) as usize);

    // ── Sérialisation du Secondary Pool ──────────────────────────────────────
    // Tableau de u16 contigus, chacun en LE.

    let pool_bytes: Vec<u8> = pool.data
        .iter()
        .flat_map(|id| id.to_le_bytes())
        .collect();

    let pool_size:   u32 = pool_bytes.len() as u32;
    let pool_offset: u32 = 64 + ENTRY_COUNT * 8;

    // ── SHA-256 sur [Data Body ∥ Secondary Pool] ──────────────────────────────
    // Header exclu — conforme §3.2 spec.
    // Streaming sans allocation intermédiaire (compatible no_alloc pour le Core,
    // mais ici nous sommes dans la Forge std — Vec est admis).

    let mut hasher = Sha256::new();
    hasher.update(&data_body);
    hasher.update(&pool_bytes);
    let computed = hasher.finalize();

    let mut checksum = [0u8; 32];
    checksum.copy_from_slice(&computed);

    // ── Construction du Header (64 octets, LE) ────────────────────────────────

    let mut header = [0u8; 64];
    header[0..4].copy_from_slice(b"KALD");                          // magic
    header[4..6].copy_from_slice(&4u16.to_le_bytes());              // version = 4
    header[6..8].copy_from_slice(&variant_id.to_le_bytes());        // variant_id
    header[8..10].copy_from_slice(&1969u16.to_le_bytes());          // epoch
    header[10..12].copy_from_slice(&(YEAR_COUNT as u16).to_le_bytes()); // range = 431
    header[12..16].copy_from_slice(&ENTRY_COUNT.to_le_bytes());     // entry_count
    header[16..20].copy_from_slice(&pool_offset.to_le_bytes());     // pool_offset
    header[20..24].copy_from_slice(&pool_size.to_le_bytes());       // pool_size
    header[24..56].copy_from_slice(&checksum);                      // SHA-256
    // [56..64] = _reserved = 0x00 × 8 — déjà initialisé.

    // ── Assemblage et écriture ────────────────────────────────────────────────
    // Assemblage in-memory pour la validation post-écriture sans re-lecture disque.

    let total_size = 64usize + data_body.len() + pool_bytes.len();
    let mut full: Vec<u8> = Vec::with_capacity(total_size);
    full.extend_from_slice(&header);
    full.extend_from_slice(&data_body);
    full.extend_from_slice(&pool_bytes);

    debug_assert_eq!(full.len(), total_size);

    {
        let file   = std::fs::File::create(path)?;
        let mut w  = BufWriter::new(file);
        w.write_all(&full)?;
        w.flush()?;
    } // flush + fermeture garantie avant la validation.

    // ── Validation post-écriture via kal_validate_header ─────────────────────
    // Utilise le buffer in-memory — évite une re-lecture disque.
    // SAFETY : `full` est valide et non-null ; len = full.len() exact.
    let rc = unsafe {
        kal_validate_header(
            full.as_ptr(),
            full.len(),
            std::ptr::null_mut(), // out_header ignoré
        )
    };

    if rc != 0 {
        return Err(ForgeError::KaldValidationFailed { code: rc });
    }

    Ok(checksum)
}
