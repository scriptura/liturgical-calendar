// benches/core_bench.rs
//
// Benchmarks Divan — liturgical-calendar-core, couverture FFI complète.
//
// Exécution :
//   cargo bench -p liturgical-calendar-core
//   cargo bench -p liturgical-calendar-core -- --filter kal_read_entry

use liturgical_calendar_core::{
    entry::{FeastEntry, KALD_FORMAT_VERSION, LAYOUT_DISCRIMINANT, TimelineEntry},
    ffi::{
        KAL_ENGINE_OK, kal_read_entry, kal_read_feast, kal_read_secondary, kal_scan_flags,
        kal_validate_header,
    },
};
use sha2::{Digest, Sha256};
use std::sync::OnceLock;

// ── Helpers de fixtures ───────────────────────────────────────────────────────

/// Encode une `FeastEntry` en 4 octets little-endian.
fn encode_feast(e: &FeastEntry) -> [u8; 4] {
    let mut b = [0u8; 4];
    b[0..2].copy_from_slice(&e.feast_id.to_le_bytes());
    b[2..4].copy_from_slice(&e.flags.to_le_bytes());
    b
}

/// Encode une `TimelineEntry` en 8 octets little-endian.
fn encode_entry(e: &TimelineEntry) -> [u8; 8] {
    let mut b = [0u8; 8];
    b[0..2].copy_from_slice(&e.primary_index.to_le_bytes());
    b[2..4].copy_from_slice(&e.secondary_offset.to_le_bytes());
    b[4] = e.occurrence_flags;
    b[5] = e.secondary_count;
    b[6..8].copy_from_slice(&e._reserved.to_le_bytes());
    b
}

/// Construit un buffer `.kald` v5 valide.
///
/// `registry` : FeastEntry à sérialiser dans le Feast Registry.
/// `slots`    : `(idx_dans_timeline, TimelineEntry)` à écrire dans le Data Body.
/// `pool`     : contenu brut du Secondary Pool (u16 LE concaténés).
fn build_kald(
    entry_count: u32,
    registry: &[FeastEntry],
    slots: &[(u32, TimelineEntry)],
    pool: &[u8],
) -> Vec<u8> {
    let registry_count = registry.len() as u32;
    let registry_size = registry_count as usize * 4;
    let timeline_size = entry_count as usize * 8;
    let pool_offset = 80u32 + registry_count * 4 + entry_count * 8;
    let total = 80 + registry_size + timeline_size + pool.len();
    let mut buf = vec![0u8; total];

    // Feast Registry
    for (i, fe) in registry.iter().enumerate() {
        let off = 80 + i * 4;
        buf[off..off + 4].copy_from_slice(&encode_feast(fe));
    }

    // Timeline Body
    for &(idx, ref entry) in slots {
        let off = 80 + registry_size + idx as usize * 8;
        buf[off..off + 8].copy_from_slice(&encode_entry(entry));
    }

    // Secondary Pool
    if !pool.is_empty() {
        let pool_start = 80 + registry_size + timeline_size;
        buf[pool_start..pool_start + pool.len()].copy_from_slice(pool);
    }

    // SHA-256(Registry ∥ Timeline ∥ Pool)
    let mut hasher = Sha256::new();
    hasher.update(&buf[80..]);
    let checksum = hasher.finalize();

    // Header (80 octets)
    buf[0..4].copy_from_slice(b"KALD");
    buf[4..6].copy_from_slice(&KALD_FORMAT_VERSION.to_le_bytes());
    buf[6..8].copy_from_slice(&0u16.to_le_bytes()); // variant_id
    buf[8..10].copy_from_slice(&1969u16.to_le_bytes()); // epoch
    buf[10..12].copy_from_slice(&431u16.to_le_bytes()); // range
    buf[12..16].copy_from_slice(&entry_count.to_le_bytes()); // entry_count
    buf[16..20].copy_from_slice(&pool_offset.to_le_bytes()); // pool_offset
    buf[20..24].copy_from_slice(&(pool.len() as u32).to_le_bytes()); // pool_size
    buf[24..28].copy_from_slice(&80u32.to_le_bytes()); // registry_offset
    buf[28..32].copy_from_slice(&registry_count.to_le_bytes()); // registry_count
    // [32..36] feast_id_base + _reserved = 0x00
    buf[36..68].copy_from_slice(checksum.as_slice()); // SHA-256
    buf[68..76].copy_from_slice(&LAYOUT_DISCRIMINANT.to_le_bytes()); // discriminant
    // [76..80] _reserved2 = 0x00

    // Validation post-construction — panique si le buffer est invalide.
    let rc = unsafe { kal_validate_header(buf.as_ptr(), buf.len(), core::ptr::null_mut()) };
    assert_eq!(rc, KAL_ENGINE_OK, "build_kald a produit un header invalide");

    buf
}

// ── Registre synthétique ──────────────────────────────────────────────────────

/// 13 FeastEntry avec flags cyclés sur Precedence 0..12.
/// registry_index 1-based : entry[i] ↔ registry_index i+1.
fn synthetic_registry() -> Vec<FeastEntry> {
    (0u16..13)
        .map(|i| FeastEntry {
            feast_id: i + 1,
            flags: i, // Precedence = i (bits [3:0])
        })
        .collect()
}

// ── Fixtures statiques ────────────────────────────────────────────────────────

// 0 entrée — mesure le coût fixe : header parsing + discriminant + SHA-256 vide.
static KALD_EMPTY: OnceLock<Vec<u8>> = OnceLock::new();

// 431 ans × 366 slots — corpus synthétique complet (~1.26 Mo Timeline).
// primary_index cyclé sur 1..=13, occurrence_flags cyclé sur 0..2.
// Feast Registry : 13 entrées avec Precedence 0..12.
// Utilisé par kal_validate_header (SHA-256 sur ~1.26 Mo), kal_read_entry,
// kal_read_feast, et kal_scan_flags.
static KALD_FULL: OnceLock<Vec<u8>> = OnceLock::new();

// 1 entrée + Secondary Pool de 2 registry_index — fixture minimale pour
// kal_read_secondary.
static KALD_WITH_SECONDARY: OnceLock<Vec<u8>> = OnceLock::new();

fn kald_empty() -> &'static [u8] {
    KALD_EMPTY.get_or_init(|| build_kald(0, &[], &[], &[]))
}

fn kald_full() -> &'static [u8] {
    KALD_FULL.get_or_init(|| {
        const N: u32 = 431 * 366; // 157 746 entrées
        let registry = synthetic_registry();
        let slots: Vec<(u32, TimelineEntry)> = (0..N)
            .map(|i| {
                (
                    i,
                    TimelineEntry {
                        primary_index: (i % 13 + 1) as u16, // registry_index 1-based
                        secondary_offset: 0,
                        occurrence_flags: (i % 3) as u8, // 0, 1, 2 — simule vesperae/vigilia
                        secondary_count: 0,
                        _reserved: 0,
                    },
                )
            })
            .collect();
        build_kald(N, &registry, &slots, &[])
    })
}

fn kald_with_secondary() -> &'static [u8] {
    KALD_WITH_SECONDARY.get_or_init(|| {
        let registry = vec![FeastEntry {
            feast_id: 1,
            flags: 0,
        }];
        let entry = TimelineEntry {
            primary_index: 1,
            secondary_offset: 0,
            occurrence_flags: 0,
            secondary_count: 2,
            _reserved: 0,
        };
        // Secondary Pool : registry_index 2 et 3 (simulés, pas dans le registre — OK
        // pour ce benchmark car kal_read_secondary ne résout pas les FeastEntry).
        let mut pool = [0u8; 4];
        pool[0..2].copy_from_slice(&2u16.to_le_bytes());
        pool[2..4].copy_from_slice(&3u16.to_le_bytes());
        build_kald(1, &registry, &[(0, entry)], &pool)
    })
}

// ── kal_validate_header ───────────────────────────────────────────────────────
//
// `empty` → SHA-256 sur 0 octets : mesure le coût fixe (header parsing).
// `full`  → SHA-256 sur ~1.26 Mo : coût dominant en production.

#[divan::bench]
fn kal_validate_header_empty() {
    let buf = kald_empty();
    let rc = unsafe {
        kal_validate_header(
            divan::black_box(buf.as_ptr()),
            divan::black_box(buf.len()),
            core::ptr::null_mut(),
        )
    };
    assert_eq!(rc, KAL_ENGINE_OK);
}

#[divan::bench]
fn kal_validate_header_full_431y() {
    let buf = kald_full();
    let rc = unsafe {
        kal_validate_header(
            divan::black_box(buf.as_ptr()),
            divan::black_box(buf.len()),
            core::ptr::null_mut(),
        )
    };
    assert_eq!(rc, KAL_ENGINE_OK);
}

// ── kal_read_entry ────────────────────────────────────────────────────────────
//
// Chemin O(1) : validate_header_fast + calcul d'offset + lecture 8 octets.
// Les deux coordonnées évitent l'élimination par le compilateur.

#[divan::bench]
fn kal_read_entry_hot() {
    let buf = kald_full();
    let mut entry = TimelineEntry::zeroed();
    let rc = unsafe {
        kal_read_entry(
            divan::black_box(buf.as_ptr()),
            divan::black_box(buf.len()),
            divan::black_box(2025u16),
            divan::black_box(109u16), // Pâques 2025, DOY 0-based
            &mut entry as *mut TimelineEntry,
        )
    };
    assert_eq!(rc, KAL_ENGINE_OK);
    divan::black_box(entry);
}

#[divan::bench]
fn kal_read_entry_last_slot() {
    let buf = kald_full();
    let mut entry = TimelineEntry::zeroed();
    let rc = unsafe {
        kal_read_entry(
            divan::black_box(buf.as_ptr()),
            divan::black_box(buf.len()),
            divan::black_box(2399u16),
            divan::black_box(364u16),
            &mut entry as *mut TimelineEntry,
        )
    };
    assert_eq!(rc, KAL_ENGINE_OK);
    divan::black_box(entry);
}

// ── kal_read_feast ────────────────────────────────────────────────────────────
//
// Chemin chaud v5 : appelé après chaque kal_read_entry non-Padding.
// O(1) : validate_header_fast + offset registry + lecture 4 octets.
// Mesure le coût marginal du second appel dans le pattern 2 étapes.

#[divan::bench]
fn kal_read_feast_hot() {
    let buf = kald_full();
    let mut feast = FeastEntry::zeroed();
    let rc = unsafe {
        kal_read_feast(
            divan::black_box(buf.as_ptr()),
            divan::black_box(buf.len()),
            divan::black_box(1u16), // registry_index 1 — première fête du registre
            &mut feast as *mut FeastEntry,
        )
    };
    assert_eq!(rc, KAL_ENGINE_OK);
    divan::black_box(feast);
}

// ── kal_read_secondary ────────────────────────────────────────────────────────
//
// `zero_count` → retour immédiat : overhead FFI pur.
// `count_2`    → lecture de 2 u16 depuis le Secondary Pool.

#[divan::bench]
fn kal_read_secondary_zero_count() {
    let buf = kald_with_secondary();
    let mut ids = [0u16; 4];
    let rc = unsafe {
        kal_read_secondary(
            divan::black_box(buf.as_ptr()),
            divan::black_box(buf.len()),
            divan::black_box(0u16),
            divan::black_box(0u8),
            ids.as_mut_ptr(),
            4u8,
        )
    };
    assert_eq!(rc, KAL_ENGINE_OK);
}

#[divan::bench]
fn kal_read_secondary_count_2() {
    let buf = kald_with_secondary();
    let mut ids = [0u16; 4];
    let rc = unsafe {
        kal_read_secondary(
            divan::black_box(buf.as_ptr()),
            divan::black_box(buf.len()),
            divan::black_box(0u16),
            divan::black_box(2u8),
            ids.as_mut_ptr(),
            4u8,
        )
    };
    assert_eq!(rc, KAL_ENGINE_OK);
    divan::black_box(ids);
}

// ── kal_scan_flags ────────────────────────────────────────────────────────────
//
// Seule fonction O(n) — scan linéaire sur FeastEntry.flags via registry_index.
// Access pattern : Timeline[i].primary_index → Registry[idx].flags.
// L'indirection Registry est O(1) par slot (offset direct), le scan reste
// séquentiel sur la Timeline (stride 8, cache-friendly).
//
// Fixture : 13 fêtes avec Precedence 0..12, Timeline cyclée → ~1/13 des slots
// matchent Precedence=0 (≈ 12 130 slots sur le corpus complet).
//
// `single_year` → 366 entrées : coût minimal d'un scan annuel.
// `full_431y`   → 157 746 entrées : débit sur le corpus complet.

#[divan::bench]
fn kal_scan_flags_single_year() {
    let buf = kald_full();
    let mut indices = vec![0u32; 366];
    let mut count = 0u32;
    let rc = unsafe {
        kal_scan_flags(
            divan::black_box(buf.as_ptr()),
            divan::black_box(buf.len()),
            divan::black_box(2025u16),
            divan::black_box(2025u16),
            divan::black_box(0x000Fu16), // mask  : Precedence bits [3:0]
            divan::black_box(0u16),      // value : Precedence 0
            indices.as_mut_ptr(),
            indices.len() as u32,
            &mut count,
        )
    };
    assert_eq!(rc, KAL_ENGINE_OK);
    divan::black_box(count);
}

#[divan::bench]
fn kal_scan_flags_full_431y() {
    let buf = kald_full();
    let mut indices = vec![0u32; 431 * 366];
    let mut count = 0u32;
    let rc = unsafe {
        kal_scan_flags(
            divan::black_box(buf.as_ptr()),
            divan::black_box(buf.len()),
            divan::black_box(1969u16),
            divan::black_box(2399u16),
            divan::black_box(0x000Fu16),
            divan::black_box(0u16),
            indices.as_mut_ptr(),
            indices.len() as u32,
            &mut count,
        )
    };
    assert_eq!(rc, KAL_ENGINE_OK);
    divan::black_box(count);
}

fn main() {
    divan::main();
}
