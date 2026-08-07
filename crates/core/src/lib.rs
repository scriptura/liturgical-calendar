//! Engine de lecture du format binaire `.kald` v5 — `no_std`, `no_alloc`.
//!
//! Surface publique : [`kal_validate_header`], [`kal_read_entry`],
//! [`kal_read_feast`], [`kal_read_secondary`].
//!
//! Changements v4 → v5 :
//! - [`CalendarEntry`] remplacé par [`TimelineEntry`] (occurrences) + [`FeastEntry`] (invariants).
//! - [`kal_read_feast`] : nouveau — résout un `registry_index` en [`FeastEntry`].
//! - `kal_scan_flags` : supprimé — remplacé par [`kal_read_feast`] + accesseurs sur [`FeastEntry`].

#![cfg_attr(not(test), no_std)]
#![warn(missing_docs)]

/// Structures `FeastEntry`, `TimelineEntry` et constantes de layout.
pub mod entry;
/// Interface C-ABI : fonctions FFI et codes de retour.
pub mod ffi;
/// Structure `Header` v5 et validation du fichier `.kald`.
pub mod header;
/// Projecteur zero-copy sur un buffer `.lits`.
pub mod lits_provider;
/// Types de domaine canoniques : `Precedence`, `Nature`, `Color`, `LiturgicalPeriod`.
pub mod types;

// ── Types principaux ─────────────────────────────────────────────────────────

pub use entry::{FeastEntry, TimelineEntry};
pub use header::Header;
pub use types::{Color, DomainError, LiturgicalPeriod, Nature, Precedence};

// ── Surface FFI ──────────────────────────────────────────────────────────────

pub use ffi::{
    kal_read_entry, kal_read_feast, kal_read_secondary, kal_scan_flags, kal_validate_header,
    kal_validate_header_fast,
};

// ── Codes de retour — ABI C ──────────────────────────────────────────────────

pub use ffi::{
    KAL_ENGINE_OK, KAL_ERR_BUF_TOO_SMALL, KAL_ERR_CHECKSUM, KAL_ERR_FILE_SIZE, KAL_ERR_INDEX_OOB,
    KAL_ERR_MAGIC, KAL_ERR_NULL_PTR, KAL_ERR_POOL_OOB, KAL_ERR_RESERVED, KAL_ERR_SCHEMA,
    KAL_ERR_VERSION,
};
