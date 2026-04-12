//! Engine de lecture du format binaire `.kald` — `no_std`, `no_alloc`.
//!
//! Surface publique : [`kal_validate_header`], [`kal_read_entry`].
//! Types de domaine : [`types`].

#![cfg_attr(not(test), no_std)]
#![warn(missing_docs)]

/// Structure `CalendarEntry` et décodeurs du champ `flags`.
pub mod entry;
/// Interface C-ABI : fonctions FFI et codes de retour.
pub mod ffi;
/// Structure `Header` et validation du fichier `.kald`.
pub mod header;
/// Types de domaine canoniques : `Precedence`, `Nature`, `Color`, `LiturgicalPeriod`.
pub mod types;

pub use entry::CalendarEntry;
pub use ffi::{
    kal_read_entry, kal_validate_header, KAL_ENGINE_OK, KAL_ERR_BUF_TOO_SMALL, KAL_ERR_CHECKSUM,
    KAL_ERR_FILE_SIZE, KAL_ERR_INDEX_OOB, KAL_ERR_MAGIC, KAL_ERR_NULL_PTR, KAL_ERR_POOL_OOB,
    KAL_ERR_RESERVED, KAL_ERR_VERSION,
};
pub use header::Header;
pub use types::{Color, DomainError, LiturgicalPeriod, Nature, Precedence};
