pub use crate::lock::FeastRegistryLock;
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Enums sémantiques — INV-FORGE-DERIVE : PartialOrd + Ord requis
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Nature {
    Sollemnitas,
    Festum,
    Dominica,
    Memoria,
    Commemoratio,
    Feria,
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
    TempusOrdinarium,
    TempusAdventus,
    TempusNativitatis,
    TempusQuadragesimae,
    TriduumPaschale,
    TempusPaschale,
    /// Phase opérationnelle (Semaine Sainte)
    DiesSancti,
}

/// Classe liturgique du sujet de la fête — ADR-038.
///
/// Axe orthogonal à la préséance. Utilisé exclusivement par la Forge (AOT)
/// pour calculer le `sort_weight` de `ResolutionKey`.
/// L'Engine ignore ce champ — il consomme un output déjà résolu.
///
/// Encodage : lord=0, virgin=1, saint=2, proper=3.
/// Valeur plus faible = priorité plus haute à rang de préséance égal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiturgicalClass {
    Lord = 0,   // Fêtes du Seigneur
    Virgin = 1, // Fêtes de la Vierge Marie
    Saint = 2,  // Fêtes des Saints
    Proper = 3, // Propres locaux (patron, titulaire, dédicace)
}

// ---------------------------------------------------------------------------
// Scope — déduit du chemin du corpus
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Scope {
    Universal,
    National(String), // code ISO-3166-1
    Diocesan(String), // identifiant diocésain
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
    pub collides: Vec<String>,
    pub target: TransferTarget,
}

// ---------------------------------------------------------------------------
// FeastHistoryEntry — une tranche temporelle d'un feast
// ---------------------------------------------------------------------------

/// Correspondance avec `FeastVersionDef` du schème v1.7.0.
#[derive(Debug, Clone)]
pub struct FeastHistoryEntry {
    pub from: u16,
    pub to: u16,
    pub precedence: Option<u8>,
    pub nature: Option<Nature>,
    pub color: Option<Color>,
    pub period: Option<LiturgicalPeriod>,
    pub has_vigil_mass: bool,
    /// Règles de transfert scoped à cette tranche temporelle (vide si absent dans le YAML)
    pub transfers: Vec<TransferDef>,
}

// ---------------------------------------------------------------------------
// FeastDef — fête canonique après parsing + validation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct FeastDef {
    pub slug: String,
    pub scope: Scope,
    /// 0 = temporale universel, 1 = sanctorale
    pub category: u8,
    /// Identifiant numérique optionnel (Martyrologium Romanum)
    pub id: Option<u16>,
    /// None autorisé pour les deltas purs (temporalité héritée de l'universale au merge).
    pub temporality: Option<Temporality>,
    /// Classe liturgique du sujet — ADR-038.
    /// None autorisé dans les deltas (continentalia/nationalia) sans surcharge de classe.
    /// Obligatoire après merge pour toute fête active dans une année résolue.
    pub class: Option<LiturgicalClass>,
    pub history: Vec<FeastHistoryEntry>,
}

impl FeastDef {
    /// Retourne la tranche `history[]` active pour l'année `year`.
    /// `None` si aucune entrée ne couvre l'année (fête hors plage ou future).
    pub fn active_version_for(&self, year: u16) -> Option<&FeastHistoryEntry> {
        self.history.iter().find(|e| year >= e.from && year <= e.to)
    }
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
        Self {
            feasts: BTreeMap::new(),
        }
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

    pub fn len(&self) -> usize {
        self.feasts.len()
    }
    pub fn is_empty(&self) -> bool {
        self.feasts.is_empty()
    }

    /// Fusionne un delta dans l'entrée existante, ou insère si absente.
    ///
    /// Règles de merge (ADR-038) :
    /// - `temporality` : conserve l'existante si le delta n'en a pas.
    /// - `history`     : remplace si le delta en fournit une (non-vide).
    /// - `class`       : delta prime si `Some` — None = héritage de l'universale.
    /// - `id`          : delta prime si `Some`.
    /// - `scope`       : toujours celui du delta (plus local).
    pub fn merge(&mut self, delta: FeastDef) {
        match self.feasts.get_mut(&delta.slug) {
            None => {
                // Nouvelle fête propre à ce scope — insertion directe.
                self.feasts.insert(delta.slug.clone(), delta);
            }
            Some(existing) => {
                if delta.temporality.is_some() {
                    existing.temporality = delta.temporality;
                }
                if !delta.history.is_empty() {
                    existing.history = delta.history;
                }
                if delta.id.is_some() {
                    existing.id = delta.id;
                }
                if delta.class.is_some() {
                    existing.class = delta.class;
                }
                existing.scope = delta.scope;
            }
        }
    }
}

impl Default for FeastRegistry {
    fn default() -> Self {
        Self::new()
    }
}
