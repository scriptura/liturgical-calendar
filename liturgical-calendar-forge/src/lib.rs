#![allow(missing_docs)] // activé en Jalon 3

pub mod error;
pub mod registry;
pub mod parsing;
pub mod canonicalization;

// Re-exports publics
pub use error::ForgeError;
pub use registry::FeastRegistry;
pub use parsing::{ingest_corpus, parse_feast_from_yaml};
pub use canonicalization::{
    CanonicalizedYear, PreResolvedTransfers, AnchorTable,
    canonicalize_year, compute_easter, build_anchor_table,
    resolve_adventus, resolve_nativitas, resolve_epiphania,
    resolve_tempus_ordinarium, MONTH_STARTS, is_leap_year,
};
