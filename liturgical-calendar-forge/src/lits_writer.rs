//! Étape 6 — Binary Packing : sérialisation `.lits` v1.
//!
//! Invariants garantis ici :
//!   - Endianness Little-Endian canonique (to_le_bytes) — cross-platform.
//!   - Entry Table triée par (feast_id ASC, from ASC) — BTreeMap garantit l'ordre.
//!   - String Pool UTF-8, chaînes null-terminées (0x00), offsets depuis le début du pool.
//!   - pool_offset = 32 + entry_count × 14 (pas de padding inter-sections).
//!   - kald_build_id = kald_checksum[..8] (§9.4 spec).
//!   - annotation_offset = 0xFFFF_FFFF si aucune annotation (sentinelle O(1) côté Engine).

#![allow(missing_docs)]

use std::{
    io::{BufWriter, Write},
    path::Path,
};

use crate::{error::ForgeError, i18n::LabelTable};

/// Sentinelle inscrite dans `annotation_offset` quand aucune annotation n'est définie.
/// L'Engine teste `offset == ANNOTATION_ABSENT` avant tout accès au pool.
const ANNOTATION_ABSENT: u32 = u32::MAX;

/// Produit un fichier `.lits` pour une langue donnée.
///
/// # Paramètres
///
/// * `path`          — chemin de destination.
/// * `table`         — `LabelTable` complète (toutes langues) ; filtrée ici par `lang`.
/// * `lang`          — langue compilée (BCP 47), ex: `"la"`, `"fr"`.
/// * `kald_checksum` — SHA-256 du `.kald` compagnon ; `kald_build_id = checksum[..8]`.
///
/// # Layout binaire
///
/// ```text
/// Header     :  32 octets
/// Entry Table:  entry_count × 14 octets
/// String Pool:  pool_size octets (UTF-8 null-terminé)
/// ```
///
/// ## Header (32 octets)
///
/// | Offset | Champ         | Type    | Valeur                        |
/// |--------|---------------|---------|-------------------------------|
/// |  0.. 4 | magic         | [u8; 4] | b"LITS"                       |
/// |  4.. 6 | version       | u16 LE  | 1                             |
/// |  6..12 | lang          | [u8; 6] | BCP 47, zero-padded           |
/// | 12..20 | kald_build_id | [u8; 8] | kald_checksum[..8]            |
/// | 20..24 | entry_count   | u32 LE  |                               |
/// | 24..28 | pool_offset   | u32 LE  | 32 + entry_count × 14        |
/// | 28..32 | pool_size     | u32 LE  |                               |
///
/// ## Entry Table (14 octets / entrée)
///
/// | Offset | Champ              | Type   | Note                        |
/// |--------|--------------------|--------|-----------------------------|
/// |  0.. 2 | feast_id           | u16 LE |                             |
/// |  2.. 4 | from               | u16 LE |                             |
/// |  4.. 6 | to                 | u16 LE |                             |
/// |  6..10 | label_offset       | u32 LE | offset depuis début pool    |
/// | 10..14 | annotation_offset  | u32 LE | 0xFFFF_FFFF si absent      |
pub(crate) fn write_lits(
    path: &Path,
    table: &LabelTable,
    lang: &str,
    kald_checksum: &[u8; 32],
) -> Result<(), ForgeError> {
    // ── Collecte des entrées pour cette langue ────────────────────────────────
    // BTreeMap garantit feast_id ASC puis from ASC — conforme §9.2.

    let entries: Vec<(u16, u16, u16, &crate::i18n::ResolvedLabel)> = table
        .iter()
        .filter(|((_, _, _, l), _)| l.as_str() == lang)
        .map(|((feast_id, from, to, _), resolved)| (*feast_id, *from, *to, resolved))
        .collect();

    let entry_count: u32 = entries.len() as u32;

    // ── Construction du String Pool ───────────────────────────────────────────
    // Labels et annotations interleaved, null-terminés.
    // `ANNOTATION_ABSENT` (0xFFFF_FFFF) si annotation absente — aucun octet pool.

    let mut pool: Vec<u8> = Vec::new();
    let mut label_offsets: Vec<u32> = Vec::with_capacity(entries.len());
    let mut annotation_offsets: Vec<u32> = Vec::with_capacity(entries.len());

    for (_, _, _, resolved) in &entries {
        // label — toujours présent
        label_offsets.push(pool.len() as u32);
        pool.extend_from_slice(resolved.label.as_bytes());
        pool.push(0x00);

        // annotation — sentinelle si absente
        match &resolved.annotation {
            Some(ann) => {
                annotation_offsets.push(pool.len() as u32);
                pool.extend_from_slice(ann.as_bytes());
                pool.push(0x00);
            }
            None => annotation_offsets.push(ANNOTATION_ABSENT),
        }
    }

    let pool_size: u32 = pool.len() as u32;
    let pool_offset: u32 = 32 + entry_count * 14;

    // ── Header (32 octets) ────────────────────────────────────────────────────

    let mut header = [0u8; 32];

    header[0..4].copy_from_slice(b"LITS");
    header[4..6].copy_from_slice(&1u16.to_le_bytes());

    {
        let lang_bytes = lang.as_bytes();
        debug_assert!(
            lang_bytes.len() <= 6,
            "code langue > 6 octets UTF-8 : {}",
            lang
        );
        let copy_len = lang_bytes.len().min(6);
        header[6..6 + copy_len].copy_from_slice(&lang_bytes[..copy_len]);
    }

    header[12..20].copy_from_slice(&kald_checksum[..8]);
    header[20..24].copy_from_slice(&entry_count.to_le_bytes());
    header[24..28].copy_from_slice(&pool_offset.to_le_bytes());
    header[28..32].copy_from_slice(&pool_size.to_le_bytes());

    // ── Entry Table (14 octets / entrée) ─────────────────────────────────────

    let mut entry_table: Vec<u8> = Vec::with_capacity((entry_count * 14) as usize);

    for ((feast_id, from, to, _), (label_off, ann_off)) in entries
        .iter()
        .zip(label_offsets.iter().zip(annotation_offsets.iter()))
    {
        entry_table.extend_from_slice(&feast_id.to_le_bytes());
        entry_table.extend_from_slice(&from.to_le_bytes());
        entry_table.extend_from_slice(&to.to_le_bytes());
        entry_table.extend_from_slice(&label_off.to_le_bytes());
        entry_table.extend_from_slice(&ann_off.to_le_bytes());
    }

    debug_assert_eq!(
        entry_table.len(),
        (entry_count * 14) as usize,
        "Entry Table : taille calculée incohérente"
    );

    // ── Assemblage et écriture ────────────────────────────────────────────────

    let file = std::fs::File::create(path).map_err(ForgeError::Io)?;
    let mut w = BufWriter::new(file);

    w.write_all(&header).map_err(ForgeError::Io)?;
    w.write_all(&entry_table).map_err(ForgeError::Io)?;
    w.write_all(&pool).map_err(ForgeError::Io)?;
    w.flush().map_err(ForgeError::Io)?;

    Ok(())
}
