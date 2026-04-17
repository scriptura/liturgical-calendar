//! Étape 6 — Binary Packing : sérialisation `.lits` v1.
//!
//! Invariants garantis ici :
//!   - Endianness Little-Endian canonique (to_le_bytes) — cross-platform.
//!   - Entry Table triée par (feast_id ASC, from ASC) — BTreeMap garantit l'ordre.
//!   - String Pool UTF-8, chaînes null-terminées (0x00), offsets depuis le début du pool.
//!   - pool_offset = 32 + entry_count × 10 (pas de padding inter-sections).
//!   - kald_build_id = kald_checksum[..8] (§9.4 spec — cohérence entre artefacts).
//!
//! Le `.lits` est produit après le `.kald` (FeastIDs définitivement alloués).
//! Un fichier `.lits` est produit par langue compilée.

#![allow(missing_docs)]

use std::{
    io::{BufWriter, Write},
    path::Path,
};

use crate::{error::ForgeError, i18n::LabelTable};

/// Produit un fichier `.lits` pour une langue donnée.
///
/// # Paramètres
///
/// * `path`          — chemin de destination du fichier `.lits`.
/// * `table`         — `LabelTable` complète (toutes langues) ; filtrée ici par `lang`.
/// * `lang`          — langue compilée, ex: `"la"`, `"fr"`. Encodée sur 6 octets dans le header.
/// * `kald_checksum` — SHA-256 du `.kald` compagnon (retourné par `write_kald`).
///   `kald_build_id` dans le header = `kald_checksum[..8]`.
///
/// # Invariant de tri
///
/// La `LabelTable` est un `BTreeMap<(feast_id, from, to, lang), title>`. Pour une `lang`
/// fixée, l'itération produit les entrées triées par `(feast_id ASC, from ASC)` —
/// conforme à §9.2 (recherche binaire côté `LitsProvider`).
pub(crate) fn write_lits(
    path:          &Path,
    table:         &LabelTable,
    lang:          &str,
    kald_checksum: &[u8; 32],
) -> Result<(), ForgeError> {
    // ── Collecte des entrées pour cette langue ────────────────────────────────
    // Filtre sur `lang` ; l'ordre BTreeMap garantit feast_id ASC puis from ASC.
    // Structure intermédiaire : (feast_id, from, to, title_str)

    let entries: Vec<(u16, u16, u16, &str)> = table
        .iter()
        .filter(|((_, _, _, l), _)| l.as_str() == lang)
        .map(|((feast_id, from, to, _), title)| (*feast_id, *from, *to, title.as_str()))
        .collect();

    let entry_count: u32 = entries.len() as u32;

    // ── Construction du String Pool ───────────────────────────────────────────
    // Chaînes UTF-8 null-terminées, concaténées sans alignement interne.
    // `str_offsets[i]` = offset en octets depuis le début du pool vers entries[i].title.

    let mut pool:        Vec<u8> = Vec::new();
    let mut str_offsets: Vec<u32> = Vec::with_capacity(entries.len());

    for (_, _, _, title) in &entries {
        str_offsets.push(pool.len() as u32);
        pool.extend_from_slice(title.as_bytes());
        pool.push(0x00); // null-terminator
    }

    let pool_size:   u32 = pool.len() as u32;
    // pool_offset = taille du header + taille de l'Entry Table
    let pool_offset: u32 = 32 + entry_count * 10;

    // ── Construction du Header (32 octets, LE) ────────────────────────────────
    //
    // Offset | Champ          | Type      | Valeur
    // -------|----------------|-----------|--------------------------------
    //  0..4  | magic          | [u8; 4]   | b"LITS"
    //  4..6  | version        | u16 LE    | 1
    //  6..12 | lang           | [u8; 6]   | code langue UTF-8, zero-padded
    // 12..20 | kald_build_id  | [u8; 8]   | kald_checksum[..8]
    // 20..24 | entry_count    | u32 LE    |
    // 24..28 | pool_offset    | u32 LE    | 32 + entry_count × 10
    // 28..32 | pool_size      | u32 LE    | taille du String Pool en octets

    let mut header = [0u8; 32]; // initialisé à 0x00

    header[0..4].copy_from_slice(b"LITS");
    header[4..6].copy_from_slice(&1u16.to_le_bytes());

    // Encodage du code langue sur exactement 6 octets, UTF-8, zero-padded.
    // Un code langue valide (ex: "la", "fr", "en") tient toujours en ≤ 6 octets UTF-8.
    {
        let lang_bytes = lang.as_bytes();
        debug_assert!(lang_bytes.len() <= 6, "code langue > 6 octets UTF-8 : {}", lang);
        let copy_len = lang_bytes.len().min(6);
        header[6..6 + copy_len].copy_from_slice(&lang_bytes[..copy_len]);
        // header[6+copy_len..12] = 0x00 déjà initialisé
    }

    header[12..20].copy_from_slice(&kald_checksum[..8]);
    header[20..24].copy_from_slice(&entry_count.to_le_bytes());
    header[24..28].copy_from_slice(&pool_offset.to_le_bytes());
    header[28..32].copy_from_slice(&pool_size.to_le_bytes());

    // ── Sérialisation de l'Entry Table ────────────────────────────────────────
    // Chaque entrée : 10 octets
    //   feast_id   : u16 LE  (2 octets)
    //   from       : u16 LE  (2 octets)
    //   to         : u16 LE  (2 octets)  — 0xFFFF si open-ended
    //   str_offset : u32 LE  (4 octets)  — offset depuis début du String Pool

    let mut entry_table: Vec<u8> = Vec::with_capacity((entry_count * 10) as usize);

    for ((feast_id, from, to, _), str_offset) in entries.iter().zip(str_offsets.iter()) {
        entry_table.extend_from_slice(&feast_id.to_le_bytes());
        entry_table.extend_from_slice(&from.to_le_bytes());
        entry_table.extend_from_slice(&to.to_le_bytes());
        entry_table.extend_from_slice(&str_offset.to_le_bytes());
    }

    debug_assert_eq!(
        entry_table.len(),
        (entry_count * 10) as usize,
        "Entry Table : taille calculée incohérente"
    );

    // ── Assemblage et écriture ────────────────────────────────────────────────
    // BufWriter pour limiter les appels système (flush explicite avant fermeture).

    let file  = std::fs::File::create(path)
        .map_err(ForgeError::Io)?;
    let mut w = BufWriter::new(file);

    w.write_all(&header)?;
    w.write_all(&entry_table)?;
    w.write_all(&pool)?;
    w.flush()?;
    // fermeture du BufWriter via Drop : flush déjà effectué explicitement.

    Ok(())
}
