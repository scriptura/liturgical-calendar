//! Wrapper Flutter — `liturgical-calendar-core` v6.
//!
//! Ce crate expose **une seule** nouvelle fonction C-ABI : [`kal_lits_get_label`].
//!
//! Les six fonctions Core ci-dessous sont exportées automatiquement dans la
//! `cdylib` par le linker (elles portent `#[unsafe(no_mangle)]` dans le `rlib`
//! lié statiquement) — aucun shim redondant :
//!
//! - [`kal_validate_header`]
//! - [`kal_validate_header_fast`]
//! - [`kal_read_entry`]
//! - [`kal_read_feast`]
//! - [`kal_read_secondary`]
//! - [`kal_scan_flags`]
//!
//! [`kal_validate_header`]: liturgical_calendar_core::ffi::kal_validate_header
//! [`kal_validate_header_fast`]: liturgical_calendar_core::ffi::kal_validate_header_fast
//! [`kal_read_entry`]: liturgical_calendar_core::ffi::kal_read_entry
//! [`kal_read_feast`]: liturgical_calendar_core::ffi::kal_read_feast
//! [`kal_read_secondary`]: liturgical_calendar_core::ffi::kal_read_secondary
//! [`kal_scan_flags`]: liturgical_calendar_core::ffi::kal_scan_flags

use liturgical_calendar_core::{
    ffi::{
        KAL_ENGINE_OK, KAL_ERR_BUF_TOO_SMALL, KAL_ERR_FILE_SIZE, KAL_ERR_INDEX_OOB, KAL_ERR_MAGIC,
        KAL_ERR_NULL_PTR, KAL_ERR_VERSION,
    },
    lits_provider::{LitsError, LitsProvider},
};

/// Résout `(feast_id, year)` → `(label, annotation)` depuis un buffer `.lits`.
///
/// Les pointeurs de sortie pointent directement dans `lits_bytes` (zero-copy).
/// Dart doit maintenir `lits_bytes` en vie tant que les données pointées sont
/// utilisées — typiquement le temps d'un appel [`toDartString`].
///
/// `out_annotation_ptr` et `out_annotation_len` sont optionnels (peuvent être
/// `null`). Si non-nuls et annotation absente : `*out_annotation_ptr = null`,
/// `*out_annotation_len = 0`.
///
/// # Codes de retour
///
/// | Code | Constante              | Condition                                  |
/// |------|------------------------|--------------------------------------------|
/// | 0    | `KAL_ENGINE_OK`        | Succès                                     |
/// | -1   | `KAL_ERR_NULL_PTR`     | `lits_bytes`, `out_label_ptr` ou `out_label_len` est null |
/// | -2   | `KAL_ERR_BUF_TOO_SMALL`| Buffer < 32 octets                         |
/// | -3   | `KAL_ERR_MAGIC`        | Magic != `b"LITS"`                         |
/// | -4   | `KAL_ERR_VERSION`      | Version != 1                               |
/// | -6   | `KAL_ERR_FILE_SIZE`    | Layout `.lits` incohérent                  |
/// | -7   | `KAL_ERR_INDEX_OOB`    | `(feast_id, year)` absent du corpus        |
///
/// # Safety
///
/// - `lits_bytes` doit être valide pour `lits_len` octets pendant la durée de
///   l'appel.
/// - `out_label_ptr` et `out_label_len` doivent être non-nuls.
/// - Les pointeurs retournés restent valides tant que `lits_bytes` est en vie.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kal_lits_get_label(
    lits_bytes: *const u8,
    lits_len: usize,
    feast_id: u16,
    year: u16,
    out_label_ptr: *mut *const u8,
    out_label_len: *mut usize,
    out_annotation_ptr: *mut *const u8, // nullable
    out_annotation_len: *mut usize,     // nullable
) -> i32 {
    if lits_bytes.is_null() || out_label_ptr.is_null() || out_label_len.is_null() {
        return KAL_ERR_NULL_PTR;
    }

    let data = unsafe { core::slice::from_raw_parts(lits_bytes, lits_len) };

    let provider = match LitsProvider::new(data) {
        Ok(p) => p,
        Err(e) => {
            return match e {
                LitsError::BufferTooShort => KAL_ERR_BUF_TOO_SMALL,
                LitsError::InvalidMagic => KAL_ERR_MAGIC,
                LitsError::UnsupportedVersion(_) => KAL_ERR_VERSION,
                LitsError::CorruptLayout => KAL_ERR_FILE_SIZE,
            };
        }
    };

    let entry = match provider.get(feast_id, year) {
        Some(e) => e,
        None => return KAL_ERR_INDEX_OOB,
    };

    unsafe {
        *out_label_ptr = entry.label.as_ptr();
        *out_label_len = entry.label.len();

        if !out_annotation_ptr.is_null() && !out_annotation_len.is_null() {
            match entry.annotation {
                Some(ann) => {
                    *out_annotation_ptr = ann.as_ptr();
                    *out_annotation_len = ann.len();
                }
                None => {
                    *out_annotation_ptr = core::ptr::null();
                    *out_annotation_len = 0;
                }
            }
        }
    }

    KAL_ENGINE_OK
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_minimal_lits() -> Vec<u8> {
        let mut buf = vec![0u8; 32];
        buf[0..4].copy_from_slice(b"LITS");
        buf[4..6].copy_from_slice(&1u16.to_le_bytes());
        buf[24..28].copy_from_slice(&32u32.to_le_bytes());
        buf
    }

    #[test]
    fn garde_null_lits_bytes() {
        let mut label_ptr: *const u8 = core::ptr::null();
        let mut label_len: usize = 0;
        let rc = unsafe {
            kal_lits_get_label(
                core::ptr::null(),
                0,
                0,
                0,
                &mut label_ptr,
                &mut label_len,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            )
        };
        assert_eq!(rc, KAL_ERR_NULL_PTR);
    }

    #[test]
    fn garde_null_out_label_ptr() {
        let buf = make_minimal_lits();
        let mut label_len: usize = 0;
        let rc = unsafe {
            kal_lits_get_label(
                buf.as_ptr(),
                buf.len(),
                0,
                0,
                core::ptr::null_mut(),
                &mut label_len,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            )
        };
        assert_eq!(rc, KAL_ERR_NULL_PTR);
    }

    #[test]
    fn feast_absent_retourne_oob() {
        let buf = make_minimal_lits();
        let mut label_ptr: *const u8 = core::ptr::null();
        let mut label_len: usize = 0;
        let rc = unsafe {
            kal_lits_get_label(
                buf.as_ptr(),
                buf.len(),
                42,
                2024,
                &mut label_ptr,
                &mut label_len,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            )
        };
        assert_eq!(rc, KAL_ERR_INDEX_OOB);
    }

    #[test]
    fn magic_invalide_retourne_err() {
        let mut buf = make_minimal_lits();
        buf[0] = b'X';
        let mut label_ptr: *const u8 = core::ptr::null();
        let mut label_len: usize = 0;
        let rc = unsafe {
            kal_lits_get_label(
                buf.as_ptr(),
                buf.len(),
                0,
                0,
                &mut label_ptr,
                &mut label_len,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            )
        };
        assert_eq!(rc, KAL_ERR_MAGIC);
    }

    #[test]
    fn buffer_trop_court_retourne_err() {
        let buf = vec![0u8; 8];
        let mut label_ptr: *const u8 = core::ptr::null();
        let mut label_len: usize = 0;
        let rc = unsafe {
            kal_lits_get_label(
                buf.as_ptr(),
                buf.len(),
                0,
                0,
                &mut label_ptr,
                &mut label_len,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            )
        };
        assert_eq!(rc, KAL_ERR_BUF_TOO_SMALL);
    }

    #[test]
    fn out_annotation_null_ne_plante_pas_si_annotation_absente() {
        let buf = make_minimal_lits();
        let mut label_ptr: *const u8 = core::ptr::null();
        let mut label_len: usize = 0;
        let rc = unsafe {
            kal_lits_get_label(
                buf.as_ptr(),
                buf.len(),
                0,
                0,
                &mut label_ptr,
                &mut label_len,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            )
        };
        assert_eq!(rc, KAL_ERR_INDEX_OOB);
    }
}
