#![no_main]
//! Cible fuzz : `kal_read_secondary`
//!
//! Layout du corpus d'entrée (minimum 3 octets) :
//!   [0..2]  → secondary_index (u16 little-endian)
//!             Index de début dans la table secondary du .kald.
//!   [2]     → secondary_count (u8)
//!             Nombre d'entrées secondary à lire (0–255).
//!   [3..]   → buf (données .kald arbitraires, potentiellement malformées)
//!
//! Invariant : aucun panic pour toute combinaison d'index et de buffer.
//! Les débordements d'index (secondary_index > taille table) doivent
//! retourner KAL_ERR_OUT_OF_RANGE ou équivalent, pas un UB.
//!
//! Le buffer de sortie `ids` est dimensionné à 255 éléments (valeur maximale
//! de secondary_count), ce qui élimine les KAL_ERR_BUF_TOO_SMALL intentionnels.

use libfuzzer_sys::fuzz_target;
use liturgical_calendar_core::ffi::kal_read_secondary;

fuzz_target!(|data: &[u8]| {
    // Guard minimal : 3 octets requis pour secondary_index + secondary_count.
    if data.len() < 3 {
        return;
    }

    let secondary_index = u16::from_le_bytes([data[0], data[1]]);
    let secondary_count = data[2];
    let buf = &data[3..];

    // Buffer de sortie : capacité maximale pour éviter tout KAL_ERR_BUF_TOO_SMALL
    // parasite qui masquerait un vrai crash.
    let mut ids = [0u16; 255];

    let _ = unsafe {
        kal_read_secondary(
            buf.as_ptr(),
            buf.len(),
            secondary_index,
            secondary_count,
            ids.as_mut_ptr(),
            255,
        )
    };
});
