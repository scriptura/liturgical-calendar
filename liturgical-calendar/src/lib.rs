//! Façade du calendrier liturgique catholique (Novus Ordo, 1969–2399).
//!
//! Ce crate est le point d'entrée unique pour les utilisateurs Rust.
//! Il ré-exporte l'intégralité de la surface publique de
//! [`liturgical-calendar-core`](https://docs.rs/liturgical-calendar-core).
//!
//! # Accès à une entrée
//!
//! ```rust,no_run
//! use liturgical_calendar::{
//!     kal_read_entry, kal_read_feast,
//!     TimelineEntry, FeastEntry, KAL_ENGINE_OK,
//! };
//!
//! let kald: Vec<u8> = std::fs::read("romanus_universale.kald").unwrap();
//! let mut entry = TimelineEntry::zeroed();
//!
//! let rc = unsafe { kal_read_entry(kald.as_ptr(), kald.len(), 2025, 109, &mut entry) };
//! assert_eq!(rc, KAL_ENGINE_OK);
//!
//! if !entry.is_padding() {
//!     // Résolution des invariants de la fête via le Feast Registry.
//!     let mut feast = FeastEntry::zeroed();
//!     let rc2 = unsafe {
//!         kal_read_feast(kald.as_ptr(), kald.len(), entry.primary_index, &mut feast)
//!     };
//!     assert_eq!(rc2, KAL_ENGINE_OK);
//!
//!     let nature     = feast.nature().unwrap();
//!     let precedence = feast.precedence().unwrap();
//!     let color      = feast.color().unwrap();
//!     let _ = (nature, precedence, color);
//! }
//! ```
//!
//! # Labels i18n
//!
//! ```rust,no_run
//! use liturgical_calendar::{
//!     kal_read_entry, kal_read_feast, TimelineEntry, FeastEntry, KAL_ENGINE_OK,
//! };
//! use liturgical_calendar::lits_provider::LitsProvider;
//!
//! let kald: Vec<u8> = std::fs::read("romanus_universale.kald").unwrap();
//! let lits: Vec<u8> = std::fs::read("romanus_universale_la.lits").unwrap();
//! let provider = LitsProvider::new(&lits).unwrap();
//!
//! let mut entry = TimelineEntry::zeroed();
//! unsafe { kal_read_entry(kald.as_ptr(), kald.len(), 2025, 109, &mut entry) };
//!
//! if !entry.is_padding() {
//!     let mut feast = FeastEntry::zeroed();
//!     unsafe { kal_read_feast(kald.as_ptr(), kald.len(), entry.primary_index, &mut feast) };
//!
//!     if let Some(lits_entry) = provider.get(feast.feast_id, 2025) {
//!         println!("{}", lits_entry.label);
//!     }
//! }
//! ```
//!
//! # Intégration C / FFI
//!
//! ```bash
//! cargo build -p liturgical-calendar-core --features gen-headers
//! ```

#![cfg_attr(not(test), no_std)]
#![warn(missing_docs)]

// ── Ré-exportations ───────────────────────────────────────────────────────────

pub use liturgical_calendar_core::{
    FeastEntry,
    Header,

    // FFI — codes de retour
    KAL_ENGINE_OK,
    KAL_ERR_BUF_TOO_SMALL,
    KAL_ERR_CHECKSUM,
    KAL_ERR_FILE_SIZE,
    KAL_ERR_INDEX_OOB,
    KAL_ERR_MAGIC,
    KAL_ERR_NULL_PTR,
    KAL_ERR_POOL_OOB,
    KAL_ERR_RESERVED,
    KAL_ERR_SCHEMA,
    KAL_ERR_VERSION,

    // Structures principales
    TimelineEntry,
    // FFI — lecture
    kal_read_entry,
    kal_read_feast,
    kal_read_secondary,
    kal_validate_header,

    // Provider i18n
    lits_provider,
    // Types de domaine
    types::{Color, DomainError, LiturgicalPeriod, Nature, Precedence},
};
