//! Interface C-ABI du Core — `no_std`, `no_alloc`.
//!
//! Toutes les fonctions sont `unsafe extern "C"`. L'appelant garantit la validité
//! des pointeurs et la durée de vie des buffers.
//!
//! Les corps de fonctions `unsafe` utilisent des blocs `unsafe {}` explicites
//! conformément à l'édition Rust 2024 (`unsafe_op_in_unsafe_fn`).

use crate::entry::{FeastEntry, TimelineEntry};
use crate::header::{Header, validate_header, validate_header_fast};

// ── Constantes publiques — surface ABI C ─────────────────────────────────────

/// Succès.
pub const KAL_ENGINE_OK: i32 = 0;
/// Pointeur null passé à une fonction FFI.
pub const KAL_ERR_NULL_PTR: i32 = -1;
/// Buffer trop court (< 80 octets pour un header valide).
pub const KAL_ERR_BUF_TOO_SMALL: i32 = -2;
/// Signature `b"KALD"` absente.
pub const KAL_ERR_MAGIC: i32 = -3;
/// Version du format incompatible avec cet Engine.
pub const KAL_ERR_VERSION: i32 = -4;
/// SHA-256 incorrect — données corrompues.
pub const KAL_ERR_CHECKSUM: i32 = -5;
/// Taille ou offsets du fichier incohérents avec l'header.
pub const KAL_ERR_FILE_SIZE: i32 = -6;
/// `year`, `doy` ou `registry_index` hors plage.
pub const KAL_ERR_INDEX_OOB: i32 = -7;
/// Offset ou count hors des bornes du Secondary Pool.
pub const KAL_ERR_POOL_OOB: i32 = -8;
/// Conservé pour compatibilité ABI — réservé, non émis en v6.
pub const KAL_ERR_RESERVED: i32 = -9;
/// Discriminant de layout incompatible — dérive de schéma Forge / Engine.
pub const KAL_ERR_SCHEMA: i32 = -10;

// ── kal_validate_header ───────────────────────────────────────────────────────

/// Valide le header d'un buffer `.kald` v6.
///
/// `out_header` : si non-NULL, reçoit le `Header` parsé en cas de succès.
///
/// # Safety
/// `buf` doit être valide pour `len` octets. `out_header` doit être valide ou NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kal_validate_header(
    buf: *const u8,
    len: usize,
    out_header: *mut Header,
) -> i32 {
    if buf.is_null() {
        return KAL_ERR_NULL_PTR;
    }

    let data = unsafe { core::slice::from_raw_parts(buf, len) };

    match validate_header(data) {
        Err(code) => code,
        Ok(header) => {
            if !out_header.is_null() {
                unsafe {
                    *out_header = header;
                }
            }
            KAL_ENGINE_OK
        }
    }
}

// ── kal_validate_header_fast ──────────────────────────────────────────────────

/// Valide les invariants structurels du header sans vérifier le SHA-256.
///
/// # Safety
/// `buf` doit être valide pour `len` octets. `out_header` doit être valide ou NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kal_validate_header_fast(
    buf: *const u8,
    len: usize,
    out_header: *mut Header,
) -> i32 {
    if buf.is_null() {
        return KAL_ERR_NULL_PTR;
    }

    let data = unsafe { core::slice::from_raw_parts(buf, len) };

    match validate_header_fast(data) {
        Err(code) => code,
        Ok(header) => {
            if !out_header.is_null() {
                unsafe {
                    *out_header = header;
                }
            }
            KAL_ENGINE_OK
        }
    }
}

// ── kal_read_entry ────────────────────────────────────────────────────────────

/// Lit la `TimelineEntry` pour le slot `(year, doy)`.
///
/// `doy` : 0-based (0 = 1er janvier, 59 = Padding Feb29 hors bissextile, 365 = 31 déc).
/// Un slot `primary_index == 0` est une Padding Entry valide — pas une erreur.
/// v6 : les Padding Entries portent `occurrence_flags[4:2]` (LiturgicalPeriod)
/// et `liturgical_week` même sans fête propre.
///
/// # Safety
/// `buf` valide pour `len` octets. `out` non-NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kal_read_entry(
    buf: *const u8,
    len: usize,
    year: u16,
    doy: u16,
    out: *mut TimelineEntry,
) -> i32 {
    if buf.is_null() || out.is_null() {
        return KAL_ERR_NULL_PTR;
    }
    if doy > 365 {
        return KAL_ERR_INDEX_OOB;
    }

    let data = unsafe { core::slice::from_raw_parts(buf, len) };

    let header = match validate_header_fast(data) {
        Err(code) => return code,
        Ok(h) => h,
    };

    if year < header.epoch || year >= header.epoch.saturating_add(header.range) {
        return KAL_ERR_INDEX_OOB;
    }

    let slot = (year - header.epoch) as u64 * 366 + doy as u64;

    if slot >= header.entry_count as u64 {
        return KAL_ERR_INDEX_OOB;
    }

    let timeline_base = header.registry_offset as u64 + header.registry_count as u64 * 4;
    let byte_offset = timeline_base + slot * 8;

    if byte_offset + 8 > data.len() as u64 {
        return KAL_ERR_BUF_TOO_SMALL;
    }

    let s = &data[byte_offset as usize..byte_offset as usize + 8];

    // v6 layout : bytes 0–1 primary_index, 2–3 secondary_offset,
    //             4 occurrence_flags, 5 secondary_count,
    //             6 liturgical_week, 7 _reserved.
    unsafe {
        *out = TimelineEntry {
            primary_index: u16::from_le_bytes([s[0], s[1]]),
            secondary_offset: u16::from_le_bytes([s[2], s[3]]),
            occurrence_flags: s[4],
            secondary_count: s[5],
            liturgical_week: s[6],
            _reserved: s[7],
        };
    }

    KAL_ENGINE_OK
}

// ── kal_read_feast ────────────────────────────────────────────────────────────

/// Lit le `FeastEntry` pour le `registry_index` donné.
///
/// `registry_index` : 1-based (valeurs valides : `1..=header.registry_count`).
/// `0` est le sentinel Padding — retourne `KAL_ERR_INDEX_OOB`.
///
/// # Safety
/// `buf` valide pour `len` octets. `out` non-NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kal_read_feast(
    buf: *const u8,
    len: usize,
    registry_index: u16,
    out: *mut FeastEntry,
) -> i32 {
    if buf.is_null() || out.is_null() {
        return KAL_ERR_NULL_PTR;
    }
    if registry_index == 0 {
        return KAL_ERR_INDEX_OOB;
    }

    let data = unsafe { core::slice::from_raw_parts(buf, len) };

    let header = match validate_header_fast(data) {
        Err(code) => return code,
        Ok(h) => h,
    };

    if registry_index as u32 > header.registry_count {
        return KAL_ERR_INDEX_OOB;
    }

    let byte_offset = header.registry_offset as u64 + (registry_index - 1) as u64 * 4;

    if byte_offset + 4 > data.len() as u64 {
        return KAL_ERR_BUF_TOO_SMALL;
    }

    let s = &data[byte_offset as usize..byte_offset as usize + 4];

    unsafe {
        *out = FeastEntry {
            feast_id: u16::from_le_bytes([s[0], s[1]]),
            flags: u16::from_le_bytes([s[2], s[3]]),
        };
    }

    KAL_ENGINE_OK
}

// ── kal_read_secondary ────────────────────────────────────────────────────────

/// Lit `count` registry_indices depuis le Secondary Pool.
///
/// # Safety
/// `buf` valide pour `len` octets. `out_indices` valide pour `capacity` u16.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kal_read_secondary(
    buf: *const u8,
    len: usize,
    secondary_offset: u16,
    count: u8,
    out_indices: *mut u16,
    capacity: u8,
) -> i32 {
    if buf.is_null() || out_indices.is_null() {
        return KAL_ERR_NULL_PTR;
    }
    if count == 0 {
        return KAL_ENGINE_OK;
    }
    if capacity < count {
        return KAL_ERR_BUF_TOO_SMALL;
    }

    let data = unsafe { core::slice::from_raw_parts(buf, len) };

    let header = match validate_header_fast(data) {
        Err(code) => return code,
        Ok(h) => h,
    };

    let pool_base = header.pool_offset as u64;
    let pool_end = pool_base + header.pool_size as u64;
    let entry_start = pool_base + secondary_offset as u64 * 2;
    let entry_end = entry_start + count as u64 * 2;

    if entry_end > pool_end || entry_end > data.len() as u64 {
        return KAL_ERR_POOL_OOB;
    }

    let out_slice = unsafe { core::slice::from_raw_parts_mut(out_indices, count as usize) };

    for (i, slot) in out_slice.iter_mut().enumerate().take(count as usize) {
        let off = (entry_start + i as u64 * 2) as usize;
        *slot = u16::from_le_bytes([data[off], data[off + 1]]);
    }

    KAL_ENGINE_OK
}

// ── kal_scan_flags ────────────────────────────────────────────────────────────

/// Scanne la Timeline pour la plage `[year_from, year_to]` et retourne les
/// slots dont `FeastEntry.flags & flag_mask == flag_value`.
///
/// # Safety
/// `buf` valide pour `len` octets. `out_count` non-NULL.
/// `out_indices` valide pour `out_capacity` u32 ou NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kal_scan_flags(
    buf: *const u8,
    len: usize,
    year_from: u16,
    year_to: u16,
    flag_mask: u16,
    flag_value: u16,
    out_indices: *mut u32,
    out_capacity: u32,
    out_count: *mut u32,
) -> i32 {
    if buf.is_null() || out_count.is_null() {
        return KAL_ERR_NULL_PTR;
    }

    let data = unsafe { core::slice::from_raw_parts(buf, len) };

    let header = match validate_header_fast(data) {
        Err(code) => return code,
        Ok(h) => h,
    };

    if year_from > year_to
        || year_from < header.epoch
        || year_to >= header.epoch.saturating_add(header.range)
    {
        return KAL_ERR_INDEX_OOB;
    }

    let registry_base = header.registry_offset as u64;
    let timeline_base = registry_base + header.registry_count as u64 * 4;

    let has_out = !out_indices.is_null();
    let mut count: u32 = 0;

    for y in year_from..=year_to {
        let year_offset = (y - header.epoch) as u64;

        for doy in 0u64..366 {
            let slot = year_offset * 366 + doy;
            if slot >= header.entry_count as u64 {
                break;
            }

            let tl_off = timeline_base + slot * 8;
            if tl_off + 2 > len as u64 {
                continue;
            }
            let primary_index =
                u16::from_le_bytes([data[tl_off as usize], data[tl_off as usize + 1]]);
            if primary_index == 0 {
                continue;
            }

            let reg_off = registry_base + (primary_index - 1) as u64 * 4;
            if reg_off + 4 > len as u64 {
                continue;
            }
            let feast_flags =
                u16::from_le_bytes([data[reg_off as usize + 2], data[reg_off as usize + 3]]);

            if feast_flags & flag_mask == flag_value {
                if has_out && count < out_capacity {
                    unsafe {
                        *out_indices.add(count as usize) = slot as u32;
                    }
                }
                count += 1;
            }
        }
    }

    unsafe {
        *out_count = count;
    }

    if has_out && count > out_capacity {
        KAL_ERR_BUF_TOO_SMALL
    } else {
        KAL_ENGINE_OK
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::TimelineEntry;
    use crate::header::tests::make_valid_kald;

    #[test]
    fn validate_null_buf() {
        let rc = unsafe { kal_validate_header(core::ptr::null(), 0, core::ptr::null_mut()) };
        assert_eq!(rc, KAL_ERR_NULL_PTR);
    }

    #[test]
    fn validate_ok_and_out_written() {
        let buf = make_valid_kald(3, 6);
        let mut h = unsafe { core::mem::zeroed::<Header>() };
        let rc = unsafe { kal_validate_header(buf.as_ptr(), buf.len(), &mut h) };
        assert_eq!(rc, KAL_ENGINE_OK);
        assert_eq!(h.registry_count, 3);
        assert_eq!(h.entry_count, 6);
    }

    #[test]
    fn read_entry_padding_slot() {
        let buf = make_valid_kald(1, 366);
        let mut e = TimelineEntry::zeroed();
        let rc = unsafe { kal_read_entry(buf.as_ptr(), buf.len(), 1969, 0, &mut e) };
        assert_eq!(rc, KAL_ENGINE_OK);
        assert!(e.is_padding());
    }

    #[test]
    fn read_entry_doy_out_of_range() {
        let buf = make_valid_kald(0, 366);
        let mut e = TimelineEntry::zeroed();
        let rc = unsafe { kal_read_entry(buf.as_ptr(), buf.len(), 1969, 366, &mut e) };
        assert_eq!(rc, KAL_ERR_INDEX_OOB);
    }

    #[test]
    fn read_entry_year_out_of_range() {
        let buf = make_valid_kald(0, 366);
        let mut e = TimelineEntry::zeroed();
        let rc = unsafe { kal_read_entry(buf.as_ptr(), buf.len(), 1900, 0, &mut e) };
        assert_eq!(rc, KAL_ERR_INDEX_OOB);
    }

    #[test]
    fn read_feast_sentinel_zero() {
        use crate::entry::FeastEntry;
        let buf = make_valid_kald(2, 0);
        let mut fe = FeastEntry::zeroed();
        let rc = unsafe { kal_read_feast(buf.as_ptr(), buf.len(), 0, &mut fe) };
        assert_eq!(rc, KAL_ERR_INDEX_OOB);
    }

    #[test]
    fn read_feast_out_of_bounds() {
        use crate::entry::FeastEntry;
        let buf = make_valid_kald(2, 0);
        let mut fe = FeastEntry::zeroed();
        let rc = unsafe { kal_read_feast(buf.as_ptr(), buf.len(), 3, &mut fe) };
        assert_eq!(rc, KAL_ERR_INDEX_OOB);
    }

    #[test]
    fn read_secondary_zero_count_ok() {
        let buf = make_valid_kald(0, 0);
        let mut out = [0u16; 4];
        let rc = unsafe { kal_read_secondary(buf.as_ptr(), buf.len(), 0, 0, out.as_mut_ptr(), 4) };
        assert_eq!(rc, KAL_ENGINE_OK);
    }

    #[test]
    fn read_secondary_capacity_insufficient() {
        let buf = make_valid_kald(0, 0);
        let mut out = [0u16; 1];
        let rc = unsafe { kal_read_secondary(buf.as_ptr(), buf.len(), 0, 3, out.as_mut_ptr(), 1) };
        assert_eq!(rc, KAL_ERR_BUF_TOO_SMALL);
    }
}
