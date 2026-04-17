#![no_main]
//! Cible fuzz : `kal_read_entry`
//!
//! Layout du corpus d'entrée (minimum 4 octets) :
//!   [0..2]  → year  (u16 little-endian)
//!   [2..4]  → doy   (u16 little-endian, day-of-year 1–366)
//!   [4..]   → buf   (données .kald arbitraires, potentiellement malformées)
//!
//! Invariant : aucun panic pour toute combinaison (year, doy, buf).
//! Les cas hors-plage (doy=0, doy=400, buf vide, buf tronqué) doivent
//! retourner un code d'erreur, pas un UB.

use libfuzzer_sys::fuzz_target;
use liturgical_calendar_core::{entry::CalendarEntry, ffi::kal_read_entry};

fuzz_target!(|data: &[u8]| {
    // Guard minimal : 4 octets requis pour year + doy.
    if data.len() < 4 {
        return;
    }

    let year = u16::from_le_bytes([data[0], data[1]]);
    let doy = u16::from_le_bytes([data[2], data[3]]);
    let buf = &data[4..];

    // CalendarEntry::zeroed() garantit un état de départ déterministe.
    // La fonction ne doit pas lire de la mémoire non-initialisée depuis `entry`.
    let mut entry = CalendarEntry::zeroed();

    let _ = unsafe { kal_read_entry(buf.as_ptr(), buf.len(), year, doy, &mut entry) };
});
