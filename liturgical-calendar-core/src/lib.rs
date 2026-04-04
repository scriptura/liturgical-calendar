// SPDX-License-Identifier: MIT
// liturgical-calendar-core — Engine AOT-Only : projecteur de mémoire O(1) sur .kald
//
// Invariants architecturaux (specification.md §0.2) :
//   INV-W1 : no_std sans alloc — Vec, String, Box, HashMap interdits.
//   INV-W2 : stateless, opaque, aucune allocation interne.
//   INV-W3 : interface C-ABI exclusive (extern "C").
//   INV-W4 : l'Engine ne dépend jamais de la Forge.
//   INV-W5 : zéro diagnostic (eprintln!, log::*) — erreurs via codes de retour i32.
//
// Pragma : no_std désactivé uniquement pour les tests unitaires (harness std).
#![cfg_attr(not(test), no_std)]
// Toute opération unsafe dans un bloc unsafe doit être elle-même unsafe.
#![deny(unsafe_op_in_unsafe_fn)]
// Aucun item public sans doc.
#![warn(missing_docs)]

//! Engine liturgical — format binaire `.kald` v2.0.
//!
//! Quatre fonctions FFI (`extern "C"`) :
//! - [`ffi::kal_validate_header`] : validation header + SHA-256
//! - [`ffi::kal_read_entry`]     : lecture O(1) par `(year, doy)`
//! - `kal_read_secondary`        : lecture du Secondary Pool (Jalon 3)
//! - `kal_scan_flags`            : scan vectoriel par masque (Jalon 3)

/// Entrée du calendrier (`CalendarEntry`) et encodage des `flags`.
pub mod entry;
/// Interface FFI C-ABI et codes de retour `KAL_*`.
pub mod ffi;
/// Header binaire `.kald` et validation SHA-256.
pub mod header;
/// Types de domaine liturgiques : `Precedence`, `Nature`, `Color`, `Season`.
pub mod types;

// ─── Note sur le panic handler ────────────────────────────────────────────────
//
// Le `#[panic_handler]` n'est pas défini ici pour éviter un conflit avec std
// dans les doctests. Il est requis uniquement pour produire la `staticlib` C.
//
// Pour construire l'archive statique (.a) et générer kal_engine.h :
//
//   cargo rustc -p liturgical-calendar-core --release -- --crate-type staticlib
//   cbindgen --config liturgical-calendar-core/cbindgen.toml \
//            --crate liturgical-calendar-core --output kal_engine.h
//
// Si le linker signale un `#[panic_handler]` manquant, ajouter dans le wrapper C :
//
//   // panic_shim.rs (compilé avec le staticlib)
//   #[no_mangle] extern "C" fn rust_oom()  { unsafe { libc::abort(); } }
//   // ou fournir via -C panic=abort dans RUSTFLAGS
