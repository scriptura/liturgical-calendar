use std::fmt;

// ---------------------------------------------------------------------------
// ParseError — violations détectées fichier par fichier
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum ParseError {
    /// V6 — slug invalide (regex [a-z][a-z0-9_]*)
    InvalidSlugSyntax(String),
    /// V1 — YAML malformé ou champ inconnu (deny_unknown_fields)
    MalformedYaml(String),
    /// V1 — version != 1
    UnsupportedSchemaVersion(u32),
    /// Champ `class` absent après merge — détecté avant le pipeline annuel.
    MissingClassAfterMerge { slug: String },
    /// Temporalité : date: ET mobile: simultanément
    AmbiguousTemporalityField { slug: String },
    /// Temporalité : ni date: ni mobile:
    MissingTemporalityField { slug: String },
    /// V4a — anchor=tempus_ordinarium + offset présent
    OffsetOnOrdinalAnchor { slug: String },
    /// V4a — anchor=tempus_ordinarium + ordinal absent
    MissingOrdinal { slug: String },
    /// V4a — ordinal hors [1,34]
    OrdinalOutOfRange { slug: String, ordinal: u8 },
    /// V4a — anchor≠tempus_ordinarium + ordinal présent
    OrdinalOnNonOrdinalAnchor { slug: String, anchor: String },
    /// V-Natura-Memoria — nature=memoria + precedence ∉ {11,12}
    InvalidMemoriaPrecedence {
        slug: String,
        from: u16,
        found_precedence: u8,
    },
    /// V-Vigilia — has_vigil_mass=true + nature≠sollemnitas
    VigiliaNonSollemnitas {
        slug: String,
        from: u16,
        nature: String,
    },
    /// V3a — date invalide (mois/jour incohérent)
    InvalidDate { slug: String, month: u8, day: u8 },
    /// V-T1 — plus d'un champ parmi offset/date/mobile dans un transfer
    TransferAmbiguous { slug: String, collides: String },
    /// V-T1 — aucun champ dans un transfer
    TransferEmpty { slug: String, collides: String },
    /// V-T2 — collides référence un slug absent du registry
    UnknownCollidesTarget { slug: String, collides: String },
    /// V-T3 — collides dupliqué dans la liste transfers d'une entrée history
    TransferDuplicateCollides {
        slug: String,
        from: u16,
        collides: String,
    },
    /// V-T4 — offset direct == 0 (u32, valeur invalide)
    TransferOffsetNotPositive {
        slug: String,
        collides: String,
        offset: u32,
    },
    /// V-T5 — anchor mobile de transfer n'est pas une ancre primitive
    TransferMobileInvalidAnchor {
        slug: String,
        collides: String,
        anchor: String,
    },
    /// V-I1 — i18n/la/{slug}.yaml absent ou clé {from}.{field} manquante.
    /// Fatale : chaque entrée history[] doit avoir un titre latin.
    I18nMissingLatinKey {
        slug: String,
        from: u16,
        field: String,
    },
    /// V-I2 — Clé orpheline dans un dictionnaire i18n.
    I18nOrphanKey {
        slug: String,
        lang: String,
        from: u16,
        field: String,
    },
    /// V-I3 — Label i18n présent mais invalide.
    I18nInvalidLabel {
        slug: String,
        lang: String,
        from: u16,
        reason: &'static str,
    },
}

// ---------------------------------------------------------------------------
// RegistryError — violations de cohérence globale
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum RegistryError {
    /// V2-Bis — precedence > 12
    InvalidPrecedenceValue(u8),
    /// Classe liturgique inconnue (ADR-038)
    UnknownClassString(String),
    /// V5 — nature string inconnue (avec hint optionnel)
    UnknownNatureString(String),
    /// Couleur inconnue
    UnknownColorString(String),
    /// Période inconnue
    UnknownPeriodString(String),
    /// V3b — from > to, from < 1969, ou to > 2399
    InvalidTemporalRange { from: u16, to: u16 },
    /// V2d — deux entrées history actives la même année
    TemporalOverlap,
}

// ---------------------------------------------------------------------------
// ForgeError — enveloppe top-level
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum ForgeError {
    // ── Variants Session A ─────────────────────────────────────────────────
    Parse(ParseError),
    Registry(RegistryError),
    /// Ancre non résolue lors du calcul de PreResolvedTransfers.
    UnresolvedAnchor {
        anchor: String,
    },
    Io(std::io::Error),

    // ── Variants Session B ─────────────────────────────────────────────────
    /// V7 — Deux Solennités irréconciliables sur le même slot DOY.
    SolemnityCollision {
        slug_a: String,
        slug_b: String,
        precedence: u8,
        doy: u16,
        year: u16,
    },
    /// V8 — Fête transférable sans slot libre dans [doy+1, doy+7].
    TransferFailed {
        slug: String,
        origin_doy: u16,
        blocked_at: u16,
        year: u16,
    },
    /// V9 — FeastID muté entre élection et packing (corruption pipeline).
    FeastIDMutated {
        slug: String,
        expected_id: u16,
        found_id: u16,
        doy: u16,
        year: u16,
    },
    /// V10 — Padding Entry absente à doy=59 pour année non-bissextile.
    PaddingEntryMissing {
        year: u16,
        doy: u16,
    },
    /// V11 — Secondary Pool dépasse u16::MAX entrées (déduplication insuffisante).
    SecondaryPoolOverflow {
        pool_len: u32,
        max_capacity: u32,
    },
    /// V12 — secondary_count dépasse u8::MAX pour un slot DOY.
    SecondaryCountOverflow {
        doy: u16,
        year: u16,
        count: usize,
    },
    /// Passe 5 — Table finale incohérente après clôture transitive.
    ResolutionIncomplete {
        doy: u16,
        year: u16,
        detail: String,
    },

    // ── Variants Session C (v5) ────────────────────────────────────────────
    /// Feast Registry saturé — plus de 65 535 fêtes distinctes dans le corpus.
    RegistryOverflow,
    /// `feast_id` présent dans une année résolue mais absent du Feast Registry.
    /// Indique un bug dans la Pass 1 (`build_feast_registry`) : invariant normalement
    /// inviolable en production.
    FeastNotInRegistry {
        feast_id: u16,
        year: u16,
        doy: u16,
    },

    // ── Variants artefacts ─────────────────────────────────────────────────
    /// Le `.lits` existant a été produit depuis un `.kald` différent.
    ArtifactBuildIdMismatch {
        lits_path: std::path::PathBuf,
        lits_build_id: [u8; 8],
        kald_build_id: [u8; 8],
    },
    /// Le `.kald` sur disque n'a pas pu être relu pour vérification du build ID.
    ArtifactVerificationFailed {
        kald_path: std::path::PathBuf,
        reason: String,
    },
    FeastIDLockConflict {
        slug: String,
        yaml_id: u16,
        lock_id: u16,
    },
    /// Plus d'IDs disponibles pour ce couple (scope, category).
    FeastIDExhausted {
        scope: u8,
        category: u8,
    },
    /// Plus de variant_id disponibles (> 65 535 scopes — cas théorique).
    VariantIDExhausted,
    /// Fichier .lock illisible ou corrompu.
    LockFileMalformed(String),
    /// Post-merge : champ obligatoire absent après fusion universale + override.
    MissingResolvedField {
        feast_id: u16,
        year: u16,
        doy: u16,
        field: &'static str,
    },
    /// Validation post-écriture `kal_validate_header` échouée.
    KaldValidationFailed {
        code: i32,
    },
}

impl From<ParseError> for ForgeError {
    fn from(e: ParseError) -> Self {
        ForgeError::Parse(e)
    }
}
impl From<RegistryError> for ForgeError {
    fn from(e: RegistryError) -> Self {
        ForgeError::Registry(e)
    }
}
impl From<std::io::Error> for ForgeError {
    fn from(e: std::io::Error) -> Self {
        ForgeError::Io(e)
    }
}

impl fmt::Display for ForgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}
impl std::error::Error for ForgeError {}
