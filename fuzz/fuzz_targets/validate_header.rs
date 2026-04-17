#![no_main]
//! Cible fuzz : `kal_validate_header`
//!
//! Invariant vérifié : la fonction ne doit jamais provoquer de panic (UB,
//! out-of-bounds, division par zéro, unwrap) quel que soit l'input brut.
//!
//! Résultats attendus : KAL_ENGINE_OK ou un code d'erreur documenté.
//! Tout autre comportement observable (signal SIGABRT, SIGSEGV, etc.)
//! constitue un crash rapporté par libFuzzer.

use libfuzzer_sys::fuzz_target;
use liturgical_calendar_core::ffi::kal_validate_header;
use std::ptr::null_mut;

fuzz_target!(|data: &[u8]| {
    // `null_mut()` pour out_error : le reader doit tolérer un pointeur nul
    // sur le paramètre de diagnostic (paramètre optionnel par convention FFI).
    let _ = unsafe { kal_validate_header(data.as_ptr(), data.len(), null_mut()) };
});
