use core::ptr;

use crate::entry::CalendarEntry;
use crate::header::{validate_header, Header};

// ── Codes de retour FFI ───────────────────────────────────────────────────────

/// Succès.
pub const KAL_ENGINE_OK: i32 = 0;
/// Pointeur NULL reçu là où un pointeur valide est requis.
pub const KAL_ERR_NULL_PTR: i32 = -1;
/// Buffer trop court pour contenir un header (< 64 octets).
pub const KAL_ERR_BUF_TOO_SMALL: i32 = -2;
/// Magic invalide (≠ `b"KALD"`).
pub const KAL_ERR_MAGIC: i32 = -3;
/// Version du format non supportée (≠ 4).
pub const KAL_ERR_VERSION: i32 = -4;
/// Checksum SHA-256 incorrect.
pub const KAL_ERR_CHECKSUM: i32 = -5;
/// Taille du fichier incohérente avec les champs d'en-tête.
pub const KAL_ERR_FILE_SIZE: i32 = -6;
/// Index (year, doy) hors plage du Data Body.
pub const KAL_ERR_INDEX_OOB: i32 = -7;
/// Accès hors du Secondary Pool.
pub const KAL_ERR_POOL_OOB: i32 = -8;
/// Champ réservé non nul.
pub const KAL_ERR_RESERVED: i32 = -9;

// Projection 1-à-1 des codes internes vers les constantes publiques FFI.
#[inline]
fn map_header_err(code: i32) -> i32 {
    use crate::header::{
        ERR_BUF_TOO_SMALL, ERR_CHECKSUM, ERR_FILE_SIZE, ERR_MAGIC, ERR_RESERVED, ERR_VERSION,
    };
    match code {
        ERR_BUF_TOO_SMALL => KAL_ERR_BUF_TOO_SMALL,
        ERR_MAGIC => KAL_ERR_MAGIC,
        ERR_VERSION => KAL_ERR_VERSION,
        ERR_FILE_SIZE => KAL_ERR_FILE_SIZE,
        ERR_RESERVED => KAL_ERR_RESERVED,
        ERR_CHECKSUM => KAL_ERR_CHECKSUM,
        _ => unreachable!(),
    }
}

// ── kal_validate_header ───────────────────────────────────────────────────────

/// Valide le header d'un buffer `.kald`.
///
/// `out_header` peut être NULL : la validation est effectuée, aucune écriture n'a lieu.
///
/// # Safety
/// - `data` doit pointer sur un buffer valide d'au moins `len` octets,
///   ou être NULL (→ `KAL_ERR_NULL_PTR`).
/// - `out_header`, si non-NULL, doit pointer sur un `KalHeader` accessible en écriture.
#[no_mangle]
pub unsafe extern "C" fn kal_validate_header(
    data: *const u8,
    len: usize,
    out_header: *mut Header,
) -> i32 {
    // 1. NULL check — première instruction.
    if data.is_null() {
        return KAL_ERR_NULL_PTR;
    }

    // SAFETY: `data` non-NULL, `len` fourni par l'appelant comme taille du buffer.
    // Slice en lecture seule — aucune mutation.
    let slice = unsafe { core::slice::from_raw_parts(data, len) };

    match validate_header(slice) {
        Err(code) => map_header_err(code),
        Ok(header) => {
            // Out-param écrit uniquement après validation complète.
            if !out_header.is_null() {
                // SAFETY: `out_header` non-NULL, taille == size_of::<Header>() garantie
                // par l'appelant C.
                unsafe { out_header.write(header) };
            }
            KAL_ENGINE_OK
        }
    }
}

// ── kal_read_entry ────────────────────────────────────────────────────────────

/// Lit l'entrée calendrier pour `(year, doy)` dans le buffer `.kald`.
///
/// `doy` est 0-based : 0 = 1er janvier.
/// La Padding Entry (`primary_id == 0`) est retournée normalement sans erreur.
///
/// # Safety
/// - `data` doit pointer sur un buffer valide d'au moins `len` octets.
/// - `out_entry` doit pointer sur un `KalCalendarEntry` accessible en écriture.
#[no_mangle]
pub unsafe extern "C" fn kal_read_entry(
    data: *const u8,
    len: usize,
    year: u16,
    doy: u16,
    out_entry: *mut CalendarEntry,
) -> i32 {
    // 1. NULL checks — première instruction.
    if data.is_null() {
        return KAL_ERR_NULL_PTR;
    }
    if out_entry.is_null() {
        return KAL_ERR_NULL_PTR;
    }

    // 2. Guards domaine.
    if !(1969..=2399).contains(&year) {
        return KAL_ERR_INDEX_OOB;
    }
    if doy > 365 {
        return KAL_ERR_INDEX_OOB;
    }

    // 3. Calcul de l'index logique.
    let idx: u32 = (year as u32 - 1969) * 366 + doy as u32;

    // 4. Lecture de `entry_count` à l'offset 12 — sans appel à kal_validate_header.
    if len < 16 {
        return KAL_ERR_FILE_SIZE;
    }

    // SAFETY: `data` non-NULL, `len >= 16` vérifié ci-dessus.
    // `read_unaligned` tolère tout alignement — pas d'hypothèse sur le buffer.
    let entry_count =
        unsafe { u32::from_le_bytes(ptr::read_unaligned(data.add(12) as *const [u8; 4])) };

    // 5. Vérification idx < entry_count.
    if idx >= entry_count {
        return KAL_ERR_INDEX_OOB;
    }

    // 6. Calcul offset et vérification de fenêtre mémoire.
    let offset = 64usize + idx as usize * 8;
    if offset + 8 > len {
        return KAL_ERR_INDEX_OOB;
    }

    // 7. Lecture non-alignée de l'entrée.
    // SAFETY: `offset + 8 <= len`, `data` non-NULL → accès valide.
    // `read_unaligned` tolère tout alignement.
    let entry = unsafe { (data.add(offset) as *const CalendarEntry).read_unaligned() };

    // 8. Écriture du résultat — uniquement après tous les checks.
    // SAFETY: `out_entry` non-NULL, taille == size_of::<CalendarEntry>() garantie.
    unsafe { out_entry.write(entry) };

    KAL_ENGINE_OK
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    // Nombre total d'entrées pour la plage canonique 1969–2399.
    const FULL_ENTRY_COUNT: u32 = 431 * 366;

    // ── Helpers ───────────────────────────────────────────────────────────────

    /// Encode une `CalendarEntry` en 8 octets little-endian.
    fn encode_entry(e: &CalendarEntry) -> [u8; 8] {
        let mut b = [0u8; 8];
        b[0..2].copy_from_slice(&e.primary_id.to_le_bytes());
        b[2..4].copy_from_slice(&e.secondary_index.to_le_bytes());
        b[4..6].copy_from_slice(&e.flags.to_le_bytes());
        b[6] = e.secondary_count;
        b[7] = e._reserved;
        b
    }

    /// Construit un buffer `.kald` valide avec `entry_count` entrées.
    /// Si `slot` est fourni, écrit `entry` à la position `idx`.
    fn make_kald(entry_count: u32, slot: Option<(u32, CalendarEntry)>) -> Vec<u8> {
        let body_len = entry_count as usize * 8;
        let total = 64 + body_len;
        let mut buf = vec![0u8; total];

        if let Some((i, entry)) = slot {
            let offset = 64 + i as usize * 8;
            buf[offset..offset + 8].copy_from_slice(&encode_entry(&entry));
        }

        let mut hasher = Sha256::new();
        hasher.update(&buf[64..]);
        let checksum = hasher.finalize();

        buf[0..4].copy_from_slice(b"KALD");
        buf[4..6].copy_from_slice(&4u16.to_le_bytes());
        // variant_id, epoch, range = 0 (non vérifiés par kal_read_entry)
        buf[12..16].copy_from_slice(&entry_count.to_le_bytes());
        buf[16..20].copy_from_slice(&(64u32 + entry_count * 8).to_le_bytes());
        // pool_size = 0 (déjà 0)
        buf[24..56].copy_from_slice(checksum.as_slice());
        // _reserved = 0 (déjà 0)
        buf
    }

    fn make_valid_kald(n: u32) -> Vec<u8> {
        make_kald(n, None)
    }

    // ── kal_validate_header ───────────────────────────────────────────────────

    #[test]
    fn validate_null_data() {
        let ret = unsafe { kal_validate_header(ptr::null(), 0, ptr::null_mut()) };
        assert_eq!(ret, KAL_ERR_NULL_PTR);
    }

    #[test]
    fn validate_buf_too_small() {
        let buf = [0u8; 63];
        let ret = unsafe { kal_validate_header(buf.as_ptr(), buf.len(), ptr::null_mut()) };
        assert_eq!(ret, KAL_ERR_BUF_TOO_SMALL);
    }

    #[test]
    fn validate_valid_header() {
        let buf = make_valid_kald(4);
        let ret = unsafe { kal_validate_header(buf.as_ptr(), buf.len(), ptr::null_mut()) };
        assert_eq!(ret, KAL_ENGINE_OK);
    }

    #[test]
    fn validate_out_header_written() {
        let buf = make_valid_kald(4);
        let mut hdr = Header {
            magic: [0; 4],
            version: 0,
            variant_id: 0,
            epoch: 0,
            range: 0,
            entry_count: 0,
            pool_offset: 0,
            pool_size: 0,
            checksum: [0; 32],
            _reserved: [0; 8],
        };
        let ret = unsafe { kal_validate_header(buf.as_ptr(), buf.len(), &mut hdr as *mut Header) };
        assert_eq!(ret, KAL_ENGINE_OK);
        assert_eq!(&hdr.magic, b"KALD");
        assert_eq!(hdr.version, 4);
        assert_eq!(hdr.entry_count, 4);
    }

    #[test]
    fn validate_err_magic() {
        let mut buf = make_valid_kald(0);
        buf[0] = b'X';
        let ret = unsafe { kal_validate_header(buf.as_ptr(), buf.len(), ptr::null_mut()) };
        assert_eq!(ret, KAL_ERR_MAGIC);
    }

    #[test]
    fn validate_err_version() {
        let mut buf = make_valid_kald(0);
        buf[4] = 3;
        let ret = unsafe { kal_validate_header(buf.as_ptr(), buf.len(), ptr::null_mut()) };
        assert_eq!(ret, KAL_ERR_VERSION);
    }

    #[test]
    fn validate_err_file_size() {
        let mut buf = make_valid_kald(2);
        buf[12..16].copy_from_slice(&99u32.to_le_bytes());
        let ret = unsafe { kal_validate_header(buf.as_ptr(), buf.len(), ptr::null_mut()) };
        assert_eq!(ret, KAL_ERR_FILE_SIZE);
    }

    #[test]
    fn validate_err_reserved() {
        let mut buf = make_valid_kald(0);
        buf[56] = 0xFF;
        let ret = unsafe { kal_validate_header(buf.as_ptr(), buf.len(), ptr::null_mut()) };
        assert_eq!(ret, KAL_ERR_RESERVED);
    }

    #[test]
    fn validate_err_checksum() {
        let mut buf = make_valid_kald(2);
        buf[64] = 0xFF;
        let ret = unsafe { kal_validate_header(buf.as_ptr(), buf.len(), ptr::null_mut()) };
        assert_eq!(ret, KAL_ERR_CHECKSUM);
    }

    // ── kal_read_entry ────────────────────────────────────────────────────────

    #[test]
    fn read_null_data() {
        let mut e = CalendarEntry::zeroed();
        let ret = unsafe { kal_read_entry(ptr::null(), 0, 1969, 0, &mut e) };
        assert_eq!(ret, KAL_ERR_NULL_PTR);
    }

    #[test]
    fn read_null_out() {
        let buf = make_valid_kald(1);
        let ret = unsafe { kal_read_entry(buf.as_ptr(), buf.len(), 1969, 0, ptr::null_mut()) };
        assert_eq!(ret, KAL_ERR_NULL_PTR);
    }

    #[test]
    fn read_year_too_low() {
        let buf = make_valid_kald(1);
        let mut e = CalendarEntry::zeroed();
        assert_eq!(
            unsafe { kal_read_entry(buf.as_ptr(), buf.len(), 1968, 0, &mut e) },
            KAL_ERR_INDEX_OOB
        );
    }

    #[test]
    fn read_year_too_high() {
        let buf = make_valid_kald(1);
        let mut e = CalendarEntry::zeroed();
        assert_eq!(
            unsafe { kal_read_entry(buf.as_ptr(), buf.len(), 2400, 0, &mut e) },
            KAL_ERR_INDEX_OOB
        );
    }

    #[test]
    fn read_doy_too_high() {
        let buf = make_valid_kald(1);
        let mut e = CalendarEntry::zeroed();
        assert_eq!(
            unsafe { kal_read_entry(buf.as_ptr(), buf.len(), 1969, 366, &mut e) },
            KAL_ERR_INDEX_OOB
        );
    }

    #[test]
    fn read_idx_zero() {
        // year=1969, doy=0 → idx=0
        let entry = CalendarEntry {
            primary_id: 42,
            ..CalendarEntry::zeroed()
        };
        let buf = make_kald(FULL_ENTRY_COUNT, Some((0, entry)));
        let mut got = CalendarEntry::zeroed();
        let ret = unsafe { kal_read_entry(buf.as_ptr(), buf.len(), 1969, 0, &mut got) };
        assert_eq!(ret, KAL_ENGINE_OK);
        assert_eq!(got.primary_id, 42);
    }

    #[test]
    fn read_idx_max() {
        // year=2399, doy=365 → idx = 430*366 + 365
        let max_idx: u32 = 430 * 366 + 365;
        let entry = CalendarEntry {
            primary_id: 7,
            ..CalendarEntry::zeroed()
        };
        let buf = make_kald(FULL_ENTRY_COUNT, Some((max_idx, entry)));
        let mut got = CalendarEntry::zeroed();
        let ret = unsafe { kal_read_entry(buf.as_ptr(), buf.len(), 2399, 365, &mut got) };
        assert_eq!(ret, KAL_ENGINE_OK);
        assert_eq!(got.primary_id, 7);
    }

    #[test]
    fn read_oob_vs_entry_count() {
        // entry_count=1, year=1969, doy=1 → idx=1 ≥ entry_count=1 → OOB
        let buf = make_kald(1, None);
        let mut e = CalendarEntry::zeroed();
        let ret = unsafe { kal_read_entry(buf.as_ptr(), buf.len(), 1969, 1, &mut e) };
        assert_eq!(ret, KAL_ERR_INDEX_OOB);
    }

    #[test]
    fn read_padding_entry_ok() {
        // Padding Entry (primary_id=0) retournée sans erreur.
        let buf = make_kald(1, None);
        let mut e = CalendarEntry {
            primary_id: 99,
            ..CalendarEntry::zeroed()
        };
        let ret = unsafe { kal_read_entry(buf.as_ptr(), buf.len(), 1969, 0, &mut e) };
        assert_eq!(ret, KAL_ENGINE_OK);
        assert!(e.is_padding());
    }
}
