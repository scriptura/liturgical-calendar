// liturgical-calendar-core/src/lib.rs
//
// INV-W1 : `no_std` actif hors contexte test.
// Le harness `cargo test` compile avec `std` — `#![no_std]` inconditionnel
// rendrait le harness inopérant. `cfg_attr` est le pragma canonique (spec §0.2).
//
// `extern crate alloc` est INTERDIT dans tous les contextes (INV-W1).
// `Vec`, `String`, `Box`, `HashMap` sont INTERDITS dans le code de production.
#![cfg_attr(not(test), no_std)]
// INV-W7 : missing_docs activé dès Jalon 1 pour la surface FFI publique.
#![warn(missing_docs)]

//! Engine `liturgical-calendar-core` — projecteur de mémoire O(1) sur `.kald`.
//!
//! Interface FFI C-ABI uniquement. Zéro logique de domaine. Zéro allocation.
//! La staticlib C est produite par :
//! ```bash
//! cargo rustc -p liturgical-calendar-core --release -- --crate-type staticlib
//! ```

/// Types de domaine canoniques : `Precedence`, `Nature`, `Color`, `LiturgicalPeriod`.
pub mod types;
/// Structure `Header` et validation du fichier `.kald`.
pub mod header;
/// Structure `CalendarEntry` et décodeurs du champ `flags`.
pub mod entry;
/// Interface C-ABI : fonctions FFI et codes de retour.
pub mod ffi;

// Panic shim — requis par la staticlib no_std, exclu du harness test.
// Placé ici pour rester dans le même fichier de compilation que lib.rs
// et pouvoir être conditionné par cfg(not(test)).
//
// SAFETY : `abort()` est une fonction C garantie disponible sur toutes les
// cibles supportées (POSIX, WASI, bare-metal via libc stub).
#[cfg(not(test))]
#[cfg(not(feature = "std"))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    extern "C" {
        fn abort() -> !;
    }
    // SAFETY : abort() ne retourne jamais.
    unsafe { abort() }
}
