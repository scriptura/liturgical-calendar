// ffi.rs — Interface FFI C-ABI de l'Engine (specification.md §7–8).
//
// Invariants FFI obligatoires (§7.0) :
//   INV-FFI-1 : NULL check en première instruction — chaque pointeur, inconditionnellement.
//   INV-FFI-2 : validation de la fenêtre mémoire avant tout accès (off + n ≤ len).
//   INV-FFI-3 : out-params écrits uniquement après succès complet de toutes les validations.
//   INV-FFI-4 : chaque bloc unsafe porte un commentaire SAFETY justifié.
//
// Ordre d'évaluation canonique (INV-FFI-1) :
//   null-check → taille → bornes domaine → lecture contenu → écriture out-param
//
// Aucun diagnostic (eprintln!, log::*) — INV-W5.
// Aucune allocation — INV-W1/INV-W2.

use crate::entry::CalendarEntry;
use crate::header::{validate_header, Header, HeaderError, HEADER_SIZE};

// ─── Codes de retour FFI (specification.md §8) ────────────────────────────────

/// Succès.
pub const KAL_ENGINE_OK: i32 = 0;
/// Pointeur nul passé en argument.
pub const KAL_ERR_NULL_PTR: i32 = -1;
/// Buffer out-param insuffisant.
pub const KAL_ERR_BUF_TOO_SMALL: i32 = -2;
/// Magic invalide.
pub const KAL_ERR_MAGIC: i32 = -3;
/// Version non supportée.
pub const KAL_ERR_VERSION: i32 = -4;
/// SHA-256 invalide.
pub const KAL_ERR_CHECKSUM: i32 = -5;
/// Taille de fichier incohérente avec `entry_count` / `pool_size`.
pub const KAL_ERR_FILE_SIZE: i32 = -6;
/// Index `(year, doy)` hors bornes.
pub const KAL_ERR_INDEX_OOB: i32 = -7;
/// Accès Secondary Pool hors bornes.
pub const KAL_ERR_POOL_OOB: i32 = -8;
/// Champ `_reserved` non nul.
pub const KAL_ERR_RESERVED: i32 = -9;

// ─── Conversion HeaderError → code de retour ─────────────────────────────────

#[inline(always)]
fn header_err_to_code(e: HeaderError) -> i32 {
    match e {
        HeaderError::FileTooSmall => KAL_ERR_FILE_SIZE,
        HeaderError::InvalidMagic => KAL_ERR_MAGIC,
        HeaderError::UnsupportedVersion(_) => KAL_ERR_VERSION,
        HeaderError::FileSizeMismatch => KAL_ERR_FILE_SIZE,
        HeaderError::ReservedNotZero => KAL_ERR_RESERVED,
        HeaderError::ChecksumMismatch => KAL_ERR_CHECKSUM,
    }
}

// ─── 7.1 kal_validate_header ─────────────────────────────────────────────────

/// Valide un fichier `.kald` complet (header + SHA-256 + cohérence de taille).
///
/// # Paramètres
/// - `data`       : pointeur vers le buffer complet du fichier `.kald`.
/// - `len`        : taille du buffer en octets.
/// - `out_header` : out-param optionnel (peut être NULL). Écrit uniquement en cas de succès complet. L'appelant ne doit pas lire `*out_header` si la valeur de retour est ≠ `KAL_ENGINE_OK`.
///
/// # Validations séquentielles (arrêt au premier échec)
/// 1. `data != NULL`   → `KAL_ERR_NULL_PTR`
/// 2. `len >= 64`       → `KAL_ERR_FILE_SIZE`
/// 3. `magic == "KALD"` → `KAL_ERR_MAGIC`
/// 4. `version == 4`    → `KAL_ERR_VERSION`
/// 5. `len == 64 + entry_count*8 + pool_size` → `KAL_ERR_FILE_SIZE`
/// 6. `_reserved == 0`  → `KAL_ERR_RESERVED`
/// 7. SHA-256 sur `data[64..len]` == `checksum` → `KAL_ERR_CHECKSUM`
///
/// # Retour
/// `KAL_ENGINE_OK` (0) si toutes les validations passent.
///
/// # Safety
/// `data` doit pointer vers au moins `len` octets valides et accessibles en lecture.
/// `out_header`, si non-NULL, doit pointer vers un `Header` aligné et accessible en écriture.
#[no_mangle]
pub unsafe extern "C" fn kal_validate_header(
    data: *const u8,
    len: usize,
    out_header: *mut Header,
) -> i32 {
    // INV-FFI-1 : NULL check — data uniquement. out_header peut être NULL (optionnel).
    if data.is_null() {
        return KAL_ERR_NULL_PTR;
    }

    // Construire un slice sûr, puis déléguer à la validation pure-Rust.
    // SAFETY: null check passé (INV-FFI-1). len est fourni par l'appelant —
    // l'appelant garantit que data[0..len] est accessible en lecture.
    let bytes = unsafe { core::slice::from_raw_parts(data, len) };

    match validate_header(bytes) {
        Ok(hdr) => {
            // INV-FFI-3 : écriture de l'out-param uniquement après succès total.
            if !out_header.is_null() {
                // SAFETY: out_header non-null, l'appelant garantit l'accessibilité
                // en écriture. Header est #[repr(C)], align ≥ 4 — write() ne requiert
                // pas d'alignement particulier (utilise write_unaligned si nécessaire).
                unsafe { out_header.write(hdr) };
            }
            KAL_ENGINE_OK
        }
        Err(e) => header_err_to_code(e),
    }
}

// ─── 7.2 kal_read_entry ──────────────────────────────────────────────────────

/// Lit l'entrée `(year, doy)` depuis un fichier `.kald` en mémoire.
///
/// Complexité O(1) — accès direct par formule d'index (specification.md §2.3).
///
/// # Paramètres
/// - `data`      : pointeur vers le buffer `.kald` complet.
/// - `len`       : taille du buffer en octets.
/// - `year`      : année grégorienne dans `[1969, 2399]`.
/// - `doy`       : Day of Year 0-based dans `[0, 365]`. Janvier 1 = 0.
/// - `out_entry` : out-param obligatoire (non-NULL). Écrit uniquement en cas de succès.
///
/// # Formule d'index (§2.3)
/// ```text
/// idx    = (year - 1969) × 366 + doy        [en u32, après guards]
/// offset = 64 + idx × 8                     [en octets depuis début fichier]
/// ```
///
/// # Retour
/// `KAL_ENGINE_OK` (0) si l'entrée est lue. La Padding Entry (`primary_id == 0`)
/// est retournée normalement — l'interprétation appartient à l'appelant.
///
/// # Safety
/// `data` doit pointer vers au moins `len` octets valides et accessibles en lecture.
/// `out_entry` doit pointer vers un `CalendarEntry` accessible en écriture.
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

    // Guards domaine — OBLIGATOIREMENT avant tout calcul arithmétique.
    // Justification : sans ce guard, (year as u32 - 1969) wrappe silencieusement
    // en release si year < 1969, produisant un idx invalide énorme (≈ u32::MAX).
    if !(1969..=2399).contains(&year) {
        return KAL_ERR_INDEX_OOB;
    }
    if doy > 365 {
        return KAL_ERR_INDEX_OOB;
    }

    // Taille minimale pour lire le header (entry_count à l'offset 12).
    if len < HEADER_SIZE {
        return KAL_ERR_FILE_SIZE;
    }

    // Lecture de entry_count depuis le header (offset 12, 4 octets LE).
    // Pas de SHA-256 ici — kal_read_entry est O(1), pas O(N).
    // L'appelant est responsable d'avoir validé le fichier via kal_validate_header.
    // SAFETY: null check (INV-FFI-1), len >= 64 → offset 12+4=16 ≤ len.
    let entry_count =
        u32::from_le_bytes(unsafe { [*data.add(12), *data.add(13), *data.add(14), *data.add(15)] });

    // Calcul de l'index en u32 — overflow impossible après les guards ci-dessus.
    // Valeur maximale : (2399 − 1969) × 366 + 365 = 430 × 366 + 365 = 157 645.
    // u32::MAX = 4 294 967 295 — marge × 27 000. Promotion u64 non nécessaire.
    let idx: u32 = (year as u32 - 1969) * 366 + doy as u32;

    // Défense en profondeur : protection contre un header corrompu ayant passé
    // les guards de domaine mais portant un entry_count anormal.
    if idx >= entry_count {
        return KAL_ERR_INDEX_OOB;
    }

    // INV-FFI-2 : validation de la fenêtre mémoire avant déréférencement.
    // offset = 64 + idx * 8. Dépassement u32 impossible : idx_max * 8 = 1 261 160 < u32::MAX.
    let offset: usize = HEADER_SIZE + idx as usize * 8;
    if offset + 8 > len {
        return KAL_ERR_INDEX_OOB;
    }

    // Désérialisation LE canonique champ par champ — cross-platform correct.
    // Alternative : pointer cast + read_unaligned (spec §7.2) — LE-only.
    // Choix : from_le_bytes explicite — déterminisme garanti sur toutes cibles CI.
    // SAFETY: null check (INV-FFI-1), offset + 8 ≤ len (INV-FFI-2).
    let entry_bytes = unsafe { core::slice::from_raw_parts(data.add(offset), 8) };
    let entry = CalendarEntry {
        primary_id: u16::from_le_bytes([entry_bytes[0], entry_bytes[1]]),
        secondary_index: u16::from_le_bytes([entry_bytes[2], entry_bytes[3]]),
        flags: u16::from_le_bytes([entry_bytes[4], entry_bytes[5]]),
        secondary_count: entry_bytes[6],
        _reserved: entry_bytes[7],
    };

    // INV-FFI-3 : écriture de l'out-param uniquement après succès complet.
    // SAFETY: out_entry non-null (INV-FFI-1). CalendarEntry est #[repr(C)].
    unsafe { out_entry.write(entry) };
    KAL_ENGINE_OK
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::{encode_flags, CalendarEntry};
    use crate::types::{Color, Nature, Precedence, Season};
    use sha2::{Digest, Sha256};
    use std::ptr;

    // ── Helper : construit un buffer .kald valide ──────────────────────────────

    /// Sérialise une `CalendarEntry` en 8 octets LE.
    fn serialize_entry(e: &CalendarEntry) -> [u8; 8] {
        let mut buf = [0u8; 8];
        buf[0..2].copy_from_slice(&e.primary_id.to_le_bytes());
        buf[2..4].copy_from_slice(&e.secondary_index.to_le_bytes());
        buf[4..6].copy_from_slice(&e.flags.to_le_bytes());
        buf[6] = e.secondary_count;
        buf[7] = e._reserved;
        buf
    }

    /// Construit un buffer .kald complet à partir d'un vecteur d'entrées.
    ///
    /// `epoch` : première année (ex. 1969). `range` : nombre d'années.
    /// Les entries doivent couvrir exactement `range × 366` slots.
    fn build_kald(epoch: u16, range: u16, entries: &[CalendarEntry]) -> Vec<u8> {
        assert_eq!(
            entries.len(),
            range as usize * 366,
            "entries.len() doit valoir range × 366"
        );
        let entry_count = entries.len() as u32;
        let pool_size: u32 = 0;
        let pool_offset: u32 = 64 + entry_count * 8;

        let mut body = Vec::with_capacity(entries.len() * 8);
        for e in entries {
            body.extend_from_slice(&serialize_entry(e));
        }

        let checksum: [u8; 32] = Sha256::digest(&body).into();

        let mut buf = Vec::with_capacity(64 + body.len());
        buf.extend_from_slice(b"KALD");
        buf.extend_from_slice(&4u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes()); // variant_id
        buf.extend_from_slice(&epoch.to_le_bytes());
        buf.extend_from_slice(&range.to_le_bytes());
        buf.extend_from_slice(&entry_count.to_le_bytes());
        buf.extend_from_slice(&pool_offset.to_le_bytes());
        buf.extend_from_slice(&pool_size.to_le_bytes());
        buf.extend_from_slice(&checksum);
        buf.extend_from_slice(&[0u8; 8]); // _reserved
        buf.extend_from_slice(&body);
        buf
    }

    /// Construit un .kald minimal pour 1 an (366 entrées, epoch=1969).
    /// Entrée à l'index `special_idx` reçoit la valeur `special` ; les autres sont vides.
    fn build_one_year_kald(special_idx: usize, special: CalendarEntry) -> Vec<u8> {
        let mut entries = vec![CalendarEntry::zeroed(); 366];
        if special_idx < 366 {
            entries[special_idx] = special;
        }
        build_kald(1969, 1, &entries)
    }

    // ── Tests kal_validate_header ──────────────────────────────────────────────

    #[test]
    fn validate_header_ok() {
        let kald = build_one_year_kald(0, CalendarEntry::zeroed());
        let rc = unsafe { kal_validate_header(kald.as_ptr(), kald.len(), ptr::null_mut()) };
        assert_eq!(rc, KAL_ENGINE_OK);
    }

    #[test]
    fn validate_header_null_data() {
        let rc = unsafe { kal_validate_header(ptr::null(), 64, ptr::null_mut()) };
        assert_eq!(rc, KAL_ERR_NULL_PTR);
    }

    #[test]
    fn validate_header_writes_out_param() {
        let kald = build_one_year_kald(0, CalendarEntry::zeroed());
        let mut out = unsafe { core::mem::zeroed::<Header>() };
        let rc = unsafe { kal_validate_header(kald.as_ptr(), kald.len(), &mut out) };
        assert_eq!(rc, KAL_ENGINE_OK);
        assert_eq!(out.magic, *b"KALD");
        assert_eq!(out.version, 4);
        assert_eq!(out.epoch, 1969);
        assert_eq!(out.entry_count, 366);
    }

    #[test]
    fn validate_header_bad_magic() {
        let mut kald = build_one_year_kald(0, CalendarEntry::zeroed());
        kald[0] = b'X';
        let rc = unsafe { kal_validate_header(kald.as_ptr(), kald.len(), ptr::null_mut()) };
        assert_eq!(rc, KAL_ERR_MAGIC);
    }

    #[test]
    fn validate_header_bad_version() {
        let mut kald = build_one_year_kald(0, CalendarEntry::zeroed());
        kald[4] = 3;
        kald[5] = 0; // version = 3
        let rc = unsafe { kal_validate_header(kald.as_ptr(), kald.len(), ptr::null_mut()) };
        assert_eq!(rc, KAL_ERR_VERSION);
    }

    #[test]
    fn validate_header_checksum_error() {
        let mut kald = build_one_year_kald(0, CalendarEntry::zeroed());
        kald[64] ^= 0xFF; // corrompre Data Body
        let rc = unsafe { kal_validate_header(kald.as_ptr(), kald.len(), ptr::null_mut()) };
        assert_eq!(rc, KAL_ERR_CHECKSUM);
    }

    // ── Tests kal_read_entry — formule d'index ─────────────────────────────────

    #[test]
    fn index_formula_first_entry() {
        // year=1969, doy=0 → idx=0
        let sentinel = CalendarEntry {
            primary_id: 0xABCD,
            secondary_index: 0,
            flags: 0,
            secondary_count: 0,
            _reserved: 0,
        };
        let kald = build_one_year_kald(0, sentinel);
        let mut out = CalendarEntry::zeroed();
        let rc = unsafe { kal_read_entry(kald.as_ptr(), kald.len(), 1969, 0, &mut out) };
        assert_eq!(rc, KAL_ENGINE_OK);
        assert_eq!(out.primary_id, 0xABCD);
    }

    #[test]
    fn index_formula_last_year() {
        // year=2399, doy=365 → idx = 430*366+365 = 157 445
        // Pour ce test, on a besoin d'un .kald de 431 ans.
        let idx = 430usize * 366 + 365;
        let sentinel = CalendarEntry {
            primary_id: 0x1234,
            secondary_index: 0,
            flags: 0,
            secondary_count: 0,
            _reserved: 0,
        };
        let mut entries = vec![CalendarEntry::zeroed(); 431 * 366];
        entries[idx] = sentinel;
        let kald = build_kald(1969, 431, &entries);
        let mut out = CalendarEntry::zeroed();
        let rc = unsafe { kal_read_entry(kald.as_ptr(), kald.len(), 2399, 365, &mut out) };
        assert_eq!(rc, KAL_ENGINE_OK);
        assert_eq!(out.primary_id, 0x1234);
    }

    #[test]
    fn index_formula_mid_year() {
        // year=1970, doy=100 → idx = 1*366+100 = 466
        let sentinel = CalendarEntry {
            primary_id: 0x0042,
            secondary_index: 0,
            flags: 0,
            secondary_count: 0,
            _reserved: 0,
        };
        let mut entries = vec![CalendarEntry::zeroed(); 431 * 366];
        entries[466] = sentinel;
        let kald = build_kald(1969, 431, &entries);
        let mut out = CalendarEntry::zeroed();
        let rc = unsafe { kal_read_entry(kald.as_ptr(), kald.len(), 1970, 100, &mut out) };
        assert_eq!(rc, KAL_ENGINE_OK);
        assert_eq!(out.primary_id, 0x0042);
    }

    // ── Tests kal_read_entry — limites domaine ────────────────────────────────

    #[test]
    fn oob_year_too_small() {
        // year=1968 < 1969 → guard domaine → KAL_ERR_INDEX_OOB avant tout calcul
        let kald = build_one_year_kald(0, CalendarEntry::zeroed());
        let mut out = CalendarEntry::zeroed();
        let rc = unsafe { kal_read_entry(kald.as_ptr(), kald.len(), 1968, 0, &mut out) };
        assert_eq!(rc, KAL_ERR_INDEX_OOB);
    }

    #[test]
    fn oob_year_too_large() {
        // year=2400 > 2399 → KAL_ERR_INDEX_OOB
        let kald = build_one_year_kald(0, CalendarEntry::zeroed());
        let mut out = CalendarEntry::zeroed();
        let rc = unsafe { kal_read_entry(kald.as_ptr(), kald.len(), 2400, 0, &mut out) };
        assert_eq!(rc, KAL_ERR_INDEX_OOB);
    }

    #[test]
    fn oob_doy_366() {
        // doy=366 > 365 → KAL_ERR_INDEX_OOB
        let kald = build_one_year_kald(0, CalendarEntry::zeroed());
        let mut out = CalendarEntry::zeroed();
        let rc = unsafe { kal_read_entry(kald.as_ptr(), kald.len(), 1969, 366, &mut out) };
        assert_eq!(rc, KAL_ERR_INDEX_OOB);
    }

    #[test]
    fn oob_year_max_u16() {
        // year=u16::MAX (65535) → guard domaine → KAL_ERR_INDEX_OOB
        let kald = build_one_year_kald(0, CalendarEntry::zeroed());
        let mut out = CalendarEntry::zeroed();
        let rc = unsafe { kal_read_entry(kald.as_ptr(), kald.len(), u16::MAX, 0, &mut out) };
        assert_eq!(rc, KAL_ERR_INDEX_OOB);
    }

    // ── Tests kal_read_entry — NULL ptr ───────────────────────────────────────

    #[test]
    fn null_data_ptr() {
        let mut out = CalendarEntry::zeroed();
        let rc = unsafe { kal_read_entry(ptr::null(), 64, 1969, 0, &mut out) };
        assert_eq!(rc, KAL_ERR_NULL_PTR);
    }

    #[test]
    fn null_out_entry_ptr() {
        let kald = build_one_year_kald(0, CalendarEntry::zeroed());
        let rc = unsafe { kal_read_entry(kald.as_ptr(), kald.len(), 1969, 0, ptr::null_mut()) };
        assert_eq!(rc, KAL_ERR_NULL_PTR);
    }

    // ── Test Padding Entry doy=59 ─────────────────────────────────────────────

    #[test]
    fn padding_entry_doy_59_non_leap() {
        // 1969 n'est pas bissextile (1969 % 4 == 1).
        // La Forge place une Padding Entry (primary_id=0) à doy=59.
        // L'Engine retourne KAL_ENGINE_OK et l'entrée avec primary_id=0.
        let padding = CalendarEntry::zeroed(); // primary_id=0 = Padding Entry
        let kald = build_one_year_kald(59, padding);
        let mut out = CalendarEntry {
            primary_id: 0xFFFF,
            ..CalendarEntry::zeroed()
        };
        let rc = unsafe { kal_read_entry(kald.as_ptr(), kald.len(), 1969, 59, &mut out) };
        assert_eq!(rc, KAL_ENGINE_OK);
        assert_eq!(
            out.primary_id, 0,
            "doy=59 sur année non-bissextile → Padding Entry"
        );
        assert!(out.is_padding());
    }

    #[test]
    fn real_entry_doy_59_leap_year_simulation() {
        // Pour simuler une année bissextile : placer une vraie fête à doy=59.
        // primary_id != 0 → pas une Padding Entry.
        let flags = encode_flags(
            Precedence::FeriaePerAnnumEtMemoriaeAdLibitum,
            Color::Viridis,
            Season::TempusOrdinarium,
            Nature::Feria,
        );
        let real_entry = CalendarEntry {
            primary_id: 0x0001,
            secondary_index: 0,
            flags,
            secondary_count: 0,
            _reserved: 0,
        };
        let kald = build_one_year_kald(59, real_entry);
        let mut out = CalendarEntry::zeroed();
        let rc = unsafe { kal_read_entry(kald.as_ptr(), kald.len(), 1969, 59, &mut out) };
        assert_eq!(rc, KAL_ENGINE_OK);
        assert_ne!(out.primary_id, 0);
        assert!(!out.is_padding());
    }

    // ── Test flags preservés à la lecture ────────────────────────────────────

    #[test]
    fn read_entry_preserves_all_fields() {
        let flags = encode_flags(
            Precedence::SollemnitatesGenerales,
            Color::Albus,
            Season::TempusPaschale,
            Nature::Sollemnitas,
        );
        let original = CalendarEntry {
            primary_id: 0x0100,
            secondary_index: 0x0003,
            flags,
            secondary_count: 2,
            _reserved: 0,
        };
        let kald = build_one_year_kald(200, original);
        let mut out = CalendarEntry::zeroed();
        let rc = unsafe { kal_read_entry(kald.as_ptr(), kald.len(), 1969, 200, &mut out) };
        assert_eq!(rc, KAL_ENGINE_OK);
        assert_eq!(out.primary_id, 0x0100);
        assert_eq!(out.secondary_index, 0x0003);
        assert_eq!(out.flags, flags);
        assert_eq!(out.secondary_count, 2);
        assert_eq!(out.precedence(), Ok(Precedence::SollemnitatesGenerales));
        assert_eq!(out.color(), Ok(Color::Albus));
        assert_eq!(out.season(), Ok(Season::TempusPaschale));
        assert_eq!(out.nature(), Ok(Nature::Sollemnitas));
    }

    // ── Test OOB via entry_count (index valide dans le domaine mais hors fichier) ─

    #[test]
    fn oob_idx_vs_entry_count() {
        // Fichier 1 an (entry_count=366). Demander year=1970 → idx=366 ≥ entry_count.
        let kald = build_one_year_kald(0, CalendarEntry::zeroed());
        let mut out = CalendarEntry::zeroed();
        let rc = unsafe { kal_read_entry(kald.as_ptr(), kald.len(), 1970, 0, &mut out) };
        // year=1970 est dans [1969, 2399] (guard passé), mais idx=366 ≥ entry_count=366
        assert_eq!(rc, KAL_ERR_INDEX_OOB);
    }

    // ── Test buffer trop court ────────────────────────────────────────────────

    #[test]
    fn buffer_too_small_for_header() {
        let buf = [0u8; 63];
        let mut out = CalendarEntry::zeroed();
        let rc = unsafe { kal_read_entry(buf.as_ptr(), buf.len(), 1969, 0, &mut out) };
        assert_eq!(rc, KAL_ERR_FILE_SIZE);
    }
}
