// SPDX-License-Identifier: MIT
// liturgical-calendar-forge — Hiérarchie d'erreurs (specification.md §10)
//
// Trois niveaux :
//   ParseError   : erreurs syntaxiques YAML et de structure (Étape 1)
//   RegistryError: erreurs sémantiques du FeastRegistry (Étape 1)
//   ForgeError   : erreurs de résolution et de compilation (Étapes 3–5)

use std::fmt;

// ─── ParseError ───────────────────────────────────────────────────────────────

/// Erreurs de parsing YAML et de validation structurelle (Étape 1 — Groupe A/C/D).
#[derive(Debug)]
pub enum ParseError {
    /// Syntaxe YAML invalide.
    MalformedYaml(String),
    /// `format_version` ≠ 1.
    UnsupportedSchemaVersion { found: u32 },
    /// Aucun bloc `date` ni `mobile` sur une fête.
    MissingTemporalityField { slug: String },
    /// Blocs `date` et `mobile` tous deux présents.
    AmbiguousTemporalityField { slug: String },
    /// Date fixe impossible (ex: mois=2, jour=30).
    InvalidDate { slug: String, month: u8, day: u8 },
    /// Cycle dans le graphe des ancres mobiles.
    CircularDependency { slug: String, anchor: String },
    /// Ancre inconnue (non dans { pascha, adventus, pentecostes }).
    UnknownAnchor { slug: String, anchor: String },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::MalformedYaml(s)                       => write!(f, "YAML invalide : {s}"),
            ParseError::UnsupportedSchemaVersion { found }     => write!(f, "format_version {found} ≠ 1"),
            ParseError::MissingTemporalityField { slug }       => write!(f, "'{slug}' : ni `date` ni `mobile`"),
            ParseError::AmbiguousTemporalityField { slug }     => write!(f, "'{slug}' : `date` et `mobile` simultanés"),
            ParseError::InvalidDate { slug, month, day }       => write!(f, "'{slug}' : date invalide {month}/{day}"),
            ParseError::CircularDependency { slug, anchor }    => write!(f, "'{slug}' : cycle sur ancre '{anchor}'"),
            ParseError::UnknownAnchor { slug, anchor }         => write!(f, "'{slug}' : ancre inconnue '{anchor}'"),
        }
    }
}

// ─── RegistryError ────────────────────────────────────────────────────────────

/// Erreurs sémantiques du FeastRegistry (Étape 1 — Groupe B/C/D, codes V1–V6).
#[derive(Debug)]
pub enum RegistryError {
    /// Slug déclaré deux fois dans le même scope (V2a).
    DuplicateSlug { slug: String, scope: String },
    /// Collision sur un `id` explicite (V2b).
    FeastIDConflict { id: u16, slug_a: String, slug_b: String },
    /// `id` YAML ≠ FeastID du lock pour ce slug (INV-FORGE-3).
    FeastIDLockConflict { slug: String, yaml_id: u16, lock_id: u16 },
    /// Deux entrées `history[]` actives la même année (V1 / V2d).
    TemporalOverlap { slug: String, year: u16, conflicting_entries: usize },
    /// `precedence > 12` (V2 / V2-Bis).
    InvalidPrecedenceValue(u8),
    /// Dépassement 4095 séquences pour (scope, category) (V3 / V2c).
    FeastIDExhausted { scope: String, category: u8 },
    /// `from > to`, ou hors [1969, 2399] (V4 / V3b).
    InvalidTemporalRange { from: u16, to: u16 },
    /// Valeur `nature` non reconnue (V5).
    UnknownNatureString(String),
    /// Syntaxe du slug invalide (V6).
    InvalidSlugSyntax(String),
    /// Valeur `color` non reconnue.
    UnknownColorString(String),
    /// Valeur `season` non reconnue.
    UnknownSeasonString(String),
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RegistryError::DuplicateSlug { slug, scope }              => write!(f, "slug '{slug}' dupliqué dans scope '{scope}'"),
            RegistryError::FeastIDConflict { id, slug_a, slug_b }     => write!(f, "FeastID 0x{id:04X} revendiqué par '{slug_a}' et '{slug_b}'"),
            RegistryError::FeastIDLockConflict { slug, yaml_id, lock_id } => write!(f, "'{slug}' : id YAML 0x{yaml_id:04X} ≠ lock 0x{lock_id:04X}"),
            RegistryError::TemporalOverlap { slug, year, conflicting_entries } => write!(f, "'{slug}' : {conflicting_entries} entrées history actives en {year}"),
            RegistryError::InvalidPrecedenceValue(v)                  => write!(f, "precedence {v} > 12 (réservé système)"),
            RegistryError::FeastIDExhausted { scope, category }       => write!(f, "FeastID épuisé pour scope={scope}, category={category}"),
            RegistryError::InvalidTemporalRange { from, to }          => write!(f, "plage [{from}, {to}] invalide (domaine [1969, 2399])"),
            RegistryError::UnknownNatureString(s)                     => write!(f, "nature inconnue : '{s}'"),
            RegistryError::InvalidSlugSyntax(s)                       => write!(f, "slug syntaxe invalide : '{s}'"),
            RegistryError::UnknownColorString(s)                      => write!(f, "color inconnue : '{s}'"),
            RegistryError::UnknownSeasonString(s)                     => write!(f, "season inconnue : '{s}'"),
        }
    }
}

// ─── ForgeError ───────────────────────────────────────────────────────────────

/// Erreurs de compilation (Étapes 3–5, codes V7–V11) et erreurs système.
#[derive(Debug)]
pub enum ForgeError {
    /// Erreur de parsing YAML (délégation).
    Parse(ParseError),
    /// Erreur de registre (délégation).
    Registry(RegistryError),
    /// Erreur I/O système.
    Io(std::io::Error),
    /// Deux Solennités de même scope/rang sur le même DOY — V7.
    SolemnityCollision {
        slug_a:     String,
        slug_b:     String,
        precedence: u8,
        scope_a:    String,
        scope_b:    String,
        doy:        u16,
        year:       u16,
    },
    /// Fête transférable sans slot libre dans [doy+1, doy+7] — V8.
    TransferFailed {
        slug:       String,
        origin_doy: u16,
        blocked_at: u16,
        year:       u16,
    },
    /// Table non stable après Passe 5 — V9 (ResolutionIncomplete).
    ResolutionIncomplete {
        doy:    u16,
        year:   u16,
        detail: String,
    },
    /// `primary_id` muté dans le pipeline — V9 (FeastIDMutated).
    FeastIDMutated {
        slug:        String,
        expected_id: u16,
        found_id:    u16,
        doy:         u16,
        year:        u16,
    },
    /// Padding Entry manquante à doy=59 sur année non-bissextile — V10.
    PaddingEntryMissing { year: u16, doy: u16 },
    /// Bits réservés 14–15 du champ `flags` non nuls.
    FlagsReservedBitSet { doy: u16, year: u16 },
    /// Secondary Pool > 65 535 entrées u16 — V11.
    SecondaryPoolOverflow { pool_len: u32, max_capacity: u32 },
    /// Pâques calculé hors plage [81, 115] ou divergent de la table de référence.
    EasterMismatch { year: u16, computed: u16, expected: u16 },
}

impl fmt::Display for ForgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ForgeError::Parse(e)                               => write!(f, "PARSE: {e}"),
            ForgeError::Registry(e)                            => write!(f, "REGISTRY: {e}"),
            ForgeError::Io(e)                                  => write!(f, "IO: {e}"),
            ForgeError::SolemnityCollision { slug_a, slug_b, precedence, doy, year, .. } =>
                write!(f, "SolemnityCollision: '{slug_a}' vs '{slug_b}' (prec={precedence}) doy={doy} year={year}"),
            ForgeError::TransferFailed { slug, origin_doy, blocked_at, year } =>
                write!(f, "TransferFailed: '{slug}' origin={origin_doy} blocked_at={blocked_at} year={year}"),
            ForgeError::ResolutionIncomplete { doy, year, detail } =>
                write!(f, "ResolutionIncomplete: doy={doy} year={year} : {detail}"),
            ForgeError::FeastIDMutated { slug, expected_id, found_id, doy, year } =>
                write!(f, "FeastIDMutated: '{slug}' expected=0x{expected_id:04X} found=0x{found_id:04X} doy={doy} year={year}"),
            ForgeError::PaddingEntryMissing { year, doy }     => write!(f, "PaddingEntryMissing: year={year} doy={doy}"),
            ForgeError::FlagsReservedBitSet { doy, year }     => write!(f, "FlagsReservedBitSet: doy={doy} year={year}"),
            ForgeError::SecondaryPoolOverflow { pool_len, max_capacity } =>
                write!(f, "SecondaryPoolOverflow: pool_len={pool_len} > max={max_capacity}"),
            ForgeError::EasterMismatch { year, computed, expected } =>
                write!(f, "EasterMismatch: year={year} computed={computed} expected={expected}"),
        }
    }
}

impl From<ParseError>    for ForgeError { fn from(e: ParseError)    -> Self { ForgeError::Parse(e) } }
impl From<RegistryError> for ForgeError { fn from(e: RegistryError) -> Self { ForgeError::Registry(e) } }
impl From<std::io::Error> for ForgeError { fn from(e: std::io::Error) -> Self { ForgeError::Io(e) } }
