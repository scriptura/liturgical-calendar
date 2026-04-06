// liturgical-calendar-core/src/ffi.rs
//
// Interface C-ABI de l'Engine (spec §7, INV-W3).
// 4 fonctions publiques, zéro logique de domaine, zéro allocation.
//
// Ordre d'exécution obligatoire dans chaque fonction (INV-FFI-1 à FFI-4) :
//   1. NULL checks — premiers, inconditionnels
//   2. Taille/bornes buffer
//   3. Bornes logiques (year, doy, index)
//   4. Lecture du contenu
//   5. Écriture out-params UNIQUEMENT après tous les checks

use crate::entry::CalendarEntry;
use crate::header::{validate_header, Header};

// ─────────────────────────────────────────────────────────────────────────────
// Codes de retour FFI (spec §8)
// ─────────────────────────────────────────────────────────────────────────────

/// Succès.
pub const KAL_ENGINE_OK: i32 = 0;
/// Pointeur nul passé en argument.
pub const KAL_ERR_NULL_PTR: i32 = -1;
/// Buffer out-param insuffisant pour recevoir le résultat.
pub const KAL_ERR_BUF_TOO_SMALL: i32 = -2;
/// Magic bytes invalides (attendu : `b"KALD"`).
pub const KAL_ERR_MAGIC: i32 = -3;
/// Version non supportée (attendu : 4).
pub const KAL_ERR_VERSION: i32 = -4;
/// SHA-256 calculé ≠ checksum stocké dans le header.
pub const KAL_ERR_CHECKSUM: i32 = -5;
/// Taille de fichier incohérente avec `entry_count`/`pool_size`.
pub const KAL_ERR_FILE_SIZE: i32 = -6;
/// Index `(year, doy)` hors bornes ou `entry_count` dépassé.
pub const KAL_ERR_INDEX_OOB: i32 = -7;
/// Accès Secondary Pool hors bornes.
pub const KAL_ERR_POOL_OOB: i32 = -8;
/// Champ `_reserved` non nul.
pub const KAL_ERR_RESERVED: i32 = -9;

// ─────────────────────────────────────────────────────────────────────────────
// 7.1  kal_validate_header
// ─────────────────────────────────────────────────────────────────────────────

/// Valide le header d'un fichier `.kald` et le désérialise.
///
/// `out_header` peut être NULL — dans ce cas, le header validé est ignoré.
///
/// # Sécurité
/// - `data` doit être non-NULL et pointer vers un buffer de `len` octets accessible en lecture.
/// - `out_header` peut être NULL ou pointer vers un [`Header`] accessible en écriture.
///
/// # Retour
/// `KAL_ENGINE_OK` (0) ou un code d'erreur négatif (`KAL_ERR_*`).
#[no_mangle]
pub unsafe extern "C" fn kal_validate_header(
    data: *const u8,
    len: usize,
    out_header: *mut Header,
) -> i32 {
    // INV-FFI-1 : NULL check data — premier, inconditionnel.
    // out_header peut être NULL (spec §7.1).
    if data.is_null() {
        return KAL_ERR_NULL_PTR;
    }

    // INV-FFI-2 : taille minimale avant tout accès.
    if len < 64 {
        return KAL_ERR_FILE_SIZE;
    }

    // SAFETY : null check passé (INV-FFI-1), len >= 64 (INV-FFI-2).
    let bytes = unsafe { core::slice::from_raw_parts(data, len) };

    let header = match validate_header(bytes) {
        Ok(h) => h,
        Err(e) => {
            return match e {
                crate::header::HeaderError::FileTooSmall     => KAL_ERR_FILE_SIZE,
                crate::header::HeaderError::FileSizeMismatch => KAL_ERR_FILE_SIZE,
                crate::header::HeaderError::InvalidMagic     => KAL_ERR_MAGIC,
                crate::header::HeaderError::UnsupportedVersion(_) => KAL_ERR_VERSION,
                crate::header::HeaderError::ReservedNotZero => KAL_ERR_RESERVED,
                crate::header::HeaderError::ChecksumMismatch => KAL_ERR_CHECKSUM,
            };
        }
    };

    // INV-FFI-3 : écriture out-param UNIQUEMENT après tous les checks.
    if !out_header.is_null() {
        // SAFETY : out_header non-NULL, pointeur valide garanti par l'appelant.
        unsafe { out_header.write(header) };
    }

    KAL_ENGINE_OK
}

// ─────────────────────────────────────────────────────────────────────────────
// 7.2  kal_read_entry
// ─────────────────────────────────────────────────────────────────────────────

/// Lit une [`CalendarEntry`] par `(year, doy)` en O(1).
///
/// `kal_read_entry` ne valide pas le checksum SHA-256 — conserver O(1).
/// `entry_count` est lu directement à l'offset 12 du buffer (spec §7.2).
///
/// L'appelant est responsable d'appeler `kal_validate_header` avant cette
/// fonction. La défense en profondeur (`idx >= entry_count`) couvre les
/// headers corrompus ayant échappé à la validation.
///
/// # Paramètres
/// - `data` : buffer complet du `.kald` (non-NULL).
/// - `len` : taille du buffer.
/// - `year` : année grégorienne ∈ [1969, 2399].
/// - `doy` : jour de l'année 0-basé ∈ [0, 365].
/// - `out_entry` : out-param non-NULL, recevra la `CalendarEntry` lue.
///
/// # Retour
/// `KAL_ENGINE_OK` (0) ou un code d'erreur négatif.
#[no_mangle]
pub unsafe extern "C" fn kal_read_entry(
    data: *const u8,
    len: usize,
    year: u16,
    doy: u16,
    out_entry: *mut CalendarEntry,
) -> i32 {
    // INV-FFI-1 : NULL checks — premiers, inconditionnels.
    if data.is_null() || out_entry.is_null() {
        return KAL_ERR_NULL_PTR;
    }

    // INV-FFI-2 : taille minimale (header = 64 octets) avant tout accès.
    if len < 64 {
        return KAL_ERR_FILE_SIZE;
    }

    // Guards domaine — OBLIGATOIREMENT avant tout calcul arithmétique.
    // Sans ce guard, `year as u32 - 1969` wrappe silencieusement en release
    // si year < 1969 (comportement défini en Rust sur u32, mais idx invalide).
    if year < 1969 || year > 2399 {
        return KAL_ERR_INDEX_OOB;
    }
    if doy > 365 {
        return KAL_ERR_INDEX_OOB;
    }

    // Lecture de entry_count directement à l'offset 12 — O(1), sans SHA-256.
    // SAFETY : len >= 64, donc offsets 12..16 sont dans les bornes (INV-FFI-2).
    let entry_count = unsafe {
        u32::from_le_bytes([
            *data.add(12),
            *data.add(13),
            *data.add(14),
            *data.add(15),
        ])
    };

    // Calcul de l'index en u32 — overflow impossible après les guards ci-dessus.
    // Valeur maximale : (2399 - 1969) × 366 + 365 = 157 645
    // u32::MAX = 4 294 967 295 — marge ×27 000.
    let idx: u32 = (year as u32 - 1969) * 366 + doy as u32;

    // Défense en profondeur contre un header corrompu (spec §7.2).
    if idx >= entry_count {
        return KAL_ERR_INDEX_OOB;
    }

    // INV-FFI-2 : validation de la fenêtre mémoire avant déréférencement.
    // offset = 64 + idx * 8 — idx * 8 ≤ 157 645 * 8 = 1 261 160, dans u32.
    let offset: usize = 64 + idx as usize * 8;
    if offset + 8 > len {
        return KAL_ERR_INDEX_OOB;
    }

    // SAFETY : null check passé (INV-FFI-1). offset + 8 ≤ len (INV-FFI-2).
    // CalendarEntry est #[repr(C)], align 2. offset = 64 + idx*8 est pair —
    // alignement garanti. read_unaligned utilisé par conservatisme (cibles strictes).
    let entry = unsafe { (data.add(offset) as *const CalendarEntry).read_unaligned() };

    // INV-FFI-3 : écriture out-param UNIQUEMENT après tous les checks.
    // SAFETY : out_entry non-NULL (INV-FFI-1), alignement garanti par l'appelant.
    unsafe { out_entry.write(entry) };

    KAL_ENGINE_OK
}

// ─────────────────────────────────────────────────────────────────────────────
// 7.3  kal_read_secondary
// ─────────────────────────────────────────────────────────────────────────────

/// Lit `secondary_count` FeastIDs depuis le Secondary Pool.
///
/// Si `secondary_count == 0` : retourne `KAL_ENGINE_OK` immédiatement,
/// sans accès mémoire.
///
/// # Paramètres
/// - `data` : buffer complet du `.kald` (non-NULL).
/// - `len` : taille du buffer.
/// - `secondary_index` : index (en éléments `u16`) dans le Secondary Pool.
/// - `secondary_count` : nombre de FeastIDs à lire.
/// - `out_ids` : buffer fourni par l'appelant, capacité `out_capacity` éléments.
/// - `out_capacity` : taille du buffer `out_ids`.
///
/// # Retour
/// `KAL_ENGINE_OK` (0) ou un code d'erreur négatif.
#[no_mangle]
pub unsafe extern "C" fn kal_read_secondary(
    data: *const u8,
    len: usize,
    secondary_index: u16,
    secondary_count: u8,
    out_ids: *mut u16,
    out_capacity: u8,
) -> i32 {
    // Cas trivial — aucune commémoration.
    if secondary_count == 0 {
        return KAL_ENGINE_OK;
    }

    // INV-FFI-1 : NULL checks.
    if data.is_null() || out_ids.is_null() {
        return KAL_ERR_NULL_PTR;
    }

    // Buffer out_ids insuffisant.
    if out_capacity < secondary_count {
        return KAL_ERR_BUF_TOO_SMALL;
    }

    // INV-FFI-2 : taille minimale.
    if len < 64 {
        return KAL_ERR_FILE_SIZE;
    }

    // Lecture de pool_offset (offset 16) et pool_size (offset 20).
    // SAFETY : len >= 64, offsets dans les bornes.
    let pool_offset = unsafe {
        u32::from_le_bytes([*data.add(16), *data.add(17), *data.add(18), *data.add(19)])
    };
    let pool_size = unsafe {
        u32::from_le_bytes([*data.add(20), *data.add(21), *data.add(22), *data.add(23)])
    };

    // Bounds check — promotion obligatoire en u32 avant addition (spec §7.3).
    // secondary_index (u16 max 65 535) + secondary_count (u8 max 255) = max 65 790
    // → dépasse u16::MAX si calculé en u16.
    let end_idx: u32 = secondary_index as u32 + secondary_count as u32;

    // Promotion en u64 pour le calcul final (spec §7.3).
    // pool_offset (u32) + end_idx * 2 peut approcher 2^32 sur fichier pathologique.
    let byte_start: u64 = pool_offset as u64 + secondary_index as u64 * 2;
    let byte_end: u64   = pool_offset as u64 + end_idx as u64 * 2;

    // Validation que le pool est dans le fichier.
    if pool_offset as u64 + pool_size as u64 > len as u64 {
        return KAL_ERR_POOL_OOB;
    }
    if byte_end > len as u64 {
        return KAL_ERR_POOL_OOB;
    }
    if byte_start >= byte_end {
        return KAL_ERR_POOL_OOB;
    }

    // Lecture des FeastIDs (LE canonique).
    // SAFETY : toutes les bornes vérifiées ci-dessus. out_ids : out_capacity >= secondary_count.
    for i in 0..secondary_count as usize {
        let byte_offset = (byte_start + i as u64 * 2) as usize;
        let id = unsafe {
            u16::from_le_bytes([*data.add(byte_offset), *data.add(byte_offset + 1)])
        };
        // SAFETY : i < secondary_count <= out_capacity.
        unsafe { out_ids.add(i).write(id) };
    }

    KAL_ENGINE_OK
}

// ─────────────────────────────────────────────────────────────────────────────
// 7.4  kal_scan_flags
// ─────────────────────────────────────────────────────────────────────────────

/// Scan linéaire du Data Body — retourne les indices des entrées vérifiant
/// `(flags & flag_mask) == flag_value`. Complexité O(N).
///
/// Le stride constant de 8 octets et l'offset fixe de `flags` à la position 4
/// de chaque entrée permettent une auto-vectorisation par le compilateur
/// (spec §7.4). Aucun intrinsic manuel — la performance doit être mesurée
/// par benchmark avant toute affirmation de gain SIMD.
///
/// # Paramètres
/// - `data` : buffer complet du `.kald` (non-NULL).
/// - `len` : taille du buffer.
/// - `flag_mask` : masque de sélection de bits dans `flags`.
/// - `flag_value` : valeur attendue après application du masque.
/// - `out_indices` : buffer d'indices fourni par l'appelant.
/// - `out_capacity` : capacité du buffer `out_indices`.
/// - `out_count` : out-param — nombre d'entrées trouvées (non-NULL).
///
/// # Retour
/// `KAL_ENGINE_OK` (0) si le scan est complet.
/// `KAL_ERR_BUF_TOO_SMALL` si `out_capacity` est dépassé avant la fin du scan.
#[no_mangle]
pub unsafe extern "C" fn kal_scan_flags(
    data: *const u8,
    len: usize,
    flag_mask: u16,
    flag_value: u16,
    out_indices: *mut u32,
    out_capacity: u32,
    out_count: *mut u32,
) -> i32 {
    // INV-FFI-1 : NULL checks.
    if data.is_null() || out_indices.is_null() || out_count.is_null() {
        return KAL_ERR_NULL_PTR;
    }

    // INV-FFI-2 : taille minimale.
    if len < 64 {
        return KAL_ERR_FILE_SIZE;
    }

    // Lecture de entry_count à l'offset 12.
    // SAFETY : len >= 64, offset 12..16 dans les bornes.
    let entry_count = unsafe {
        u32::from_le_bytes([*data.add(12), *data.add(13), *data.add(14), *data.add(15)])
    };

    let mut found: u32 = 0;

    for idx in 0..entry_count {
        // Offset du champ flags = 64 + idx * 8 + 4.
        let flags_offset: usize = 64 + idx as usize * 8 + 4;

        // Vérification de fenêtre mémoire avant déréférencement.
        if flags_offset + 2 > len {
            break; // Data Body tronqué — arrêt propre.
        }

        // SAFETY : flags_offset + 2 <= len (vérifié ci-dessus).
        let flags = unsafe {
            u16::from_le_bytes([*data.add(flags_offset), *data.add(flags_offset + 1)])
        };

        if (flags & flag_mask) == flag_value {
            if found >= out_capacity {
                // Buffer out-param insuffisant — résultat partiel.
                // INV-FFI-3 : écriture out_count avant retour d'erreur.
                unsafe { out_count.write(found) };
                return KAL_ERR_BUF_TOO_SMALL;
            }
            // SAFETY : found < out_capacity.
            unsafe { out_indices.add(found as usize).write(idx) };
            found += 1;
        }
    }

    // INV-FFI-3 : écriture out-params après tous les checks.
    unsafe { out_count.write(found) };
    KAL_ENGINE_OK
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests unitaires — tâches 1.4 (roadmap)
// ─────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::encode_flags;
    use crate::types::{Color, LiturgicalPeriod, Nature, Precedence};
    use sha2::{Digest, Sha256};

    /// Construit un `.kald` synthétique avec `entry_count` entrées.
    /// Les entrées sont toutes des Padding Entries (zéro), sauf celles
    /// fournies dans `overrides : &[(idx, CalendarEntry)]`.
    fn build_kald(entry_count: u32, overrides: &[(u32, CalendarEntry)]) -> Vec<u8> {
        let data_body_size = entry_count as usize * 8;
        let total = 64 + data_body_size;
        let mut buf = vec![0u8; total];

        buf[0..4].copy_from_slice(b"KALD");
        buf[4..6].copy_from_slice(&4u16.to_le_bytes());
        buf[6..8].copy_from_slice(&0u16.to_le_bytes());
        buf[8..10].copy_from_slice(&1969u16.to_le_bytes());
        buf[10..12].copy_from_slice(&431u16.to_le_bytes());
        buf[12..16].copy_from_slice(&entry_count.to_le_bytes());
        let pool_offset = 64u32 + entry_count * 8;
        buf[16..20].copy_from_slice(&pool_offset.to_le_bytes());
        buf[20..24].copy_from_slice(&0u32.to_le_bytes()); // pool_size = 0

        for (idx, entry) in overrides {
            let off = 64 + *idx as usize * 8;
            buf[off..off + 2].copy_from_slice(&entry.primary_id.to_le_bytes());
            buf[off + 2..off + 4].copy_from_slice(&entry.secondary_index.to_le_bytes());
            buf[off + 4..off + 6].copy_from_slice(&entry.flags.to_le_bytes());
            buf[off + 6] = entry.secondary_count;
            buf[off + 7] = entry._reserved;
        }

        let checksum = Sha256::digest(&buf[64..]);
        buf[24..56].copy_from_slice(checksum.as_slice());
        buf
    }

    // ── kal_validate_header ──────────────────────────────────────────────────

    #[test]
    fn validate_header_ok() {
        let buf = build_kald(0, &[]);
        let mut hdr = Header {
            magic: [0; 4], version: 0, variant_id: 0, epoch: 0,
            range: 0, entry_count: 0, pool_offset: 0, pool_size: 0,
            checksum: [0; 32], _reserved: [0; 8],
        };
        let rc = unsafe { kal_validate_header(buf.as_ptr(), buf.len(), &mut hdr) };
        assert_eq!(rc, KAL_ENGINE_OK);
        assert_eq!(&hdr.magic, b"KALD");
    }

    #[test]
    fn validate_header_null_data() {
        let rc = unsafe { kal_validate_header(core::ptr::null(), 64, core::ptr::null_mut()) };
        assert_eq!(rc, KAL_ERR_NULL_PTR);
    }

    #[test]
    fn validate_header_out_null_is_ok() {
        let buf = build_kald(0, &[]);
        let rc = unsafe { kal_validate_header(buf.as_ptr(), buf.len(), core::ptr::null_mut()) };
        assert_eq!(rc, KAL_ENGINE_OK);
    }

    // ── kal_read_entry ───────────────────────────────────────────────────────

    /// Formule d'index : year=1969, doy=0 → idx=0
    #[test]
    fn index_formula_min() {
        let entry = CalendarEntry {
            primary_id: 0x0042,
            flags: encode_flags(
                Precedence::DominicaePerAnnum,
                Color::Viridis,
                LiturgicalPeriod::TempusOrdinarium,
                Nature::Feria,
            ),
            ..CalendarEntry::zeroed()
        };
        let buf = build_kald(157_746, &[(0, entry)]);
        let mut out = CalendarEntry::zeroed();
        let rc = unsafe { kal_read_entry(buf.as_ptr(), buf.len(), 1969, 0, &mut out) };
        assert_eq!(rc, KAL_ENGINE_OK);
        assert_eq!(out.primary_id, 0x0042);
    }

    /// Formule d'index : year=2399, doy=365 → idx = 430 * 366 + 365 = 157 745
    #[test]
    fn index_formula_max() {
        let idx_max: u32 = (2399u32 - 1969) * 366 + 365;
        assert_eq!(idx_max, 157_745);
        let entry = CalendarEntry { primary_id: 0xBEEF, ..CalendarEntry::zeroed() };
        let buf = build_kald(157_746, &[(idx_max, entry)]);
        let mut out = CalendarEntry::zeroed();
        let rc = unsafe { kal_read_entry(buf.as_ptr(), buf.len(), 2399, 365, &mut out) };
        assert_eq!(rc, KAL_ENGINE_OK);
        assert_eq!(out.primary_id, 0xBEEF);
    }

    #[test]
    fn null_ptr_returns_error() {
        let buf = build_kald(0, &[]);
        let rc = unsafe { kal_read_entry(core::ptr::null(), buf.len(), 1969, 0, core::ptr::null_mut()) };
        assert_eq!(rc, KAL_ERR_NULL_PTR);
    }

    #[test]
    fn year_below_range_oob() {
        let buf = build_kald(157_746, &[]);
        let mut out = CalendarEntry::zeroed();
        let rc = unsafe { kal_read_entry(buf.as_ptr(), buf.len(), 1968, 0, &mut out) };
        assert_eq!(rc, KAL_ERR_INDEX_OOB);
    }

    #[test]
    fn year_above_range_oob() {
        let buf = build_kald(157_746, &[]);
        let mut out = CalendarEntry::zeroed();
        let rc = unsafe { kal_read_entry(buf.as_ptr(), buf.len(), 2400, 0, &mut out) };
        assert_eq!(rc, KAL_ERR_INDEX_OOB);
    }

    #[test]
    fn doy_366_is_oob() {
        let buf = build_kald(157_746, &[]);
        let mut out = CalendarEntry::zeroed();
        let rc = unsafe { kal_read_entry(buf.as_ptr(), buf.len(), 1969, 366, &mut out) };
        assert_eq!(rc, KAL_ERR_INDEX_OOB);
    }

    #[test]
    fn null_out_entry_returns_error() {
        let buf = build_kald(157_746, &[]);
        let rc = unsafe { kal_read_entry(buf.as_ptr(), buf.len(), 1969, 0, core::ptr::null_mut()) };
        assert_eq!(rc, KAL_ERR_NULL_PTR);
    }

    /// Padding Entry à doy=59 en année non-bissextile → KAL_ENGINE_OK, primary_id=0
    #[test]
    fn padding_entry_doy59_non_leap_read() {
        // 1970 est non-bissextile. doy=59 doit contenir une Padding Entry.
        // idx = (1970 - 1969) * 366 + 59 = 425
        let buf = build_kald(157_746, &[]); // toutes les entrées sont zéro = Padding
        let mut out = CalendarEntry::zeroed();
        let rc = unsafe { kal_read_entry(buf.as_ptr(), buf.len(), 1970, 59, &mut out) };
        assert_eq!(rc, KAL_ENGINE_OK);
        assert_eq!(out.primary_id, 0, "Padding Entry attendue à doy=59");
    }

    // ── kal_scan_flags ───────────────────────────────────────────────────────

    #[test]
    fn scan_flags_finds_matching_entries() {
        // Prépare 3 entrées : idx 0 et 2 matchent, idx 1 non.
        let mask_period = 0x0700u16; // bits 8–10
        let period_val = (LiturgicalPeriod::TempusPaschale as u16) << 8;

        let entry_match = CalendarEntry {
            primary_id: 1,
            flags: encode_flags(
                Precedence::DominicaePerAnnum,
                Color::Albus,
                LiturgicalPeriod::TempusPaschale,
                Nature::Feria,
            ),
            ..CalendarEntry::zeroed()
        };
        let entry_no_match = CalendarEntry {
            primary_id: 2,
            flags: encode_flags(
                Precedence::DominicaePerAnnum,
                Color::Viridis,
                LiturgicalPeriod::TempusOrdinarium,
                Nature::Feria,
            ),
            ..CalendarEntry::zeroed()
        };

        let buf = build_kald(3, &[
            (0, entry_match),
            (1, entry_no_match),
            (2, entry_match),
        ]);

        let mut indices = [0u32; 4];
        let mut count = 0u32;
        let rc = unsafe {
            kal_scan_flags(
                buf.as_ptr(), buf.len(),
                mask_period, period_val,
                indices.as_mut_ptr(), 4, &mut count,
            )
        };
        assert_eq!(rc, KAL_ENGINE_OK);
        assert_eq!(count, 2);
        assert_eq!(indices[0], 0);
        assert_eq!(indices[1], 2);
    }
}
