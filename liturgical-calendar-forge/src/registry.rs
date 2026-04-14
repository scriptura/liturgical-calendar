use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Enums sémantiques — INV-FORGE-DERIVE : PartialOrd + Ord requis
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Nature {
    Sollemnitas,
    Festum,
    Memoria,
    Feria,
    Commemoratio,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Color {
    Albus,
    Rubeus,
    Viridis,
    Violaceus,
    Rosaceus,
    Niger,
    Aureus,
}

/// Période liturgique (champ `season` dans history)
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum LiturgicalPeriod {
    Adventus,
    Nativitas,
    Epiphania,
    Quadragesima,
    Pascha,
    TemporisOrdinarii,
}

// ---------------------------------------------------------------------------
// Scope — déduit du chemin du corpus
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Scope {
    Universal,
    National(String),  // code ISO-3166-1
    Diocesan(String),  // identifiant diocésain
}

// ---------------------------------------------------------------------------
// Temporality — exclusif (un seul bloc YAML)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Temporality {
    /// Fête à date fixe : mois + jour (pseudo-DOY calculé à la canonicalisation)
    Fixed { month: u8, day: u8 },
    /// Fête mobile relative à une ancre (post-desucrage pentecostes)
    Mobile { anchor: String, offset: i32 },
    /// Fête de Tempus Ordinarium — ordinal ∈ [1,34]
    Ordinal { ordinal: u8 },
}

// ---------------------------------------------------------------------------
// TransferTarget — cible d'un transfert en cas de collision
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum TransferTarget {
    /// Décalage avant fixe (≥ 1 jours)
    Offset(u32),
    /// Date fixe absolue
    Date { month: u8, day: u8 },
    /// Cible mobile (ancre primitive uniquement, offset signé admis)
    Mobile { anchor: String, offset: i32 },
}

// ---------------------------------------------------------------------------
// TransferDef — une règle de transfert
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct TransferDef {
    /// Slug de la fête avec laquelle la collision est déclarée
    pub collides: String,
    pub target: TransferTarget,
}

// ---------------------------------------------------------------------------
// FeastHistoryEntry — une tranche temporelle d'un feast
// ---------------------------------------------------------------------------

/// Correspondance avec `FeastVersionDef` du schème v1.7.0.
#[derive(Debug, Clone)]
pub struct FeastHistoryEntry {
    pub from:           u16,
    pub to:             u16,
    pub precedence:     u8,
    pub nature:         Nature,
    pub color:          Color,
    pub season:         Option<LiturgicalPeriod>,
    pub has_vigil_mass: bool,
    /// Règles de transfert scoped à cette tranche temporelle (vide si absent dans le YAML)
    pub transfers:      Vec<TransferDef>,
}

// ---------------------------------------------------------------------------
// FeastDef — fête canonique après parsing + validation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct FeastDef {
    pub slug:        String,
    pub scope:       Scope,
    /// 0 = temporale universel, ≥ 1 = sanctorale
    pub category:    u8,
    /// Identifiant numérique optionnel (Martyrologium Romanum)
    pub id:          Option<u16>,
    pub temporality: Temporality,
    pub history:     Vec<FeastHistoryEntry>,
}

// ---------------------------------------------------------------------------
// FeastRegistry — INV-FORGE-2 : BTreeMap, pas de HashMap
// ---------------------------------------------------------------------------

pub struct FeastRegistry {
    /// slug → FeastDef, ordre lexicographique garanti
    pub(crate) feasts: BTreeMap<String, FeastDef>,
}

impl FeastRegistry {
    pub fn new() -> Self {
        Self { feasts: BTreeMap::new() }
    }

    /// Insère ou remplace (slug = clé)
    pub fn insert(&mut self, def: FeastDef) {
        self.feasts.insert(def.slug.clone(), def);
    }

    pub fn contains(&self, slug: &str) -> bool {
        self.feasts.contains_key(slug)
    }

    pub fn get(&self, slug: &str) -> Option<&FeastDef> {
        self.feasts.get(slug)
    }

    /// Itération en ordre lexicographique de slug (BTreeMap garanti)
    pub fn iter(&self) -> impl Iterator<Item = &FeastDef> {
        self.feasts.values()
    }

    pub fn len(&self) -> usize { self.feasts.len() }
    pub fn is_empty(&self) -> bool { self.feasts.is_empty() }
}

impl Default for FeastRegistry {
    fn default() -> Self { Self::new() }
}
