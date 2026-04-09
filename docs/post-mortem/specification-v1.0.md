# Spécification Technique : Liturgical Calendar v2.3

**Statut** : Canonique / Ready for Implementation  
**Architecture** : Producer-Consumer / DOD / Fast-Slow Path / AOT / FFI-First  
**Workspace** : `liturgical-calendar-forge` (std) / `liturgical-calendar-core` (no_std, no_alloc)  
**Langage Domaine** : Latin (Strictement Canonique)  
**Déterminisme** : Bit-for-bit reproductible  
**Date de Révision** : 2026-03-05
**Version** : 2.3  
**Basé sur** : v2.2 (2026-03-05)

---

## 0. Séparation Workspace : Forge et Engine

### 0.1 Deux Composants Distincts

Le workspace est divisé en deux crates aux responsabilités strictement disjointes. Cette partition est **non négociable** : toute dépendance de l'Engine vers la Forge constitue une violation architecturale.

| Composant      | Crate Cargo                 | Modèle mémoire                | Rôle                                                                                             |
| -------------- | --------------------------- | ----------------------------- | ------------------------------------------------------------------------------------------------ |
| **The Forge**  | `liturgical-calendar-forge` | `std`                         | Producer : ingestion YAML, validation canonique (V1–V6), parsing, génération du `.kald`          |
| **The Engine** | `liturgical-calendar-core`  | `#![no_std]` **sans** `alloc` | Consumer : Slow Path algorithmique, Fast Path (Query sur `.kald`), layout mémoire, interface FFI |

### 0.2 Invariants Structurels du Workspace

**INV-W1 — Engine `no_std` sans `alloc`**

Le crate `liturgical-calendar-core` ne déclare ni `extern crate alloc` ni `extern crate std`. Tout type contenant `Vec`, `String`, `Box`, ou `HashMap` est interdit dans ce crate. Toutes les structures du crate ont une taille statique connue à la compilation.

**INV-W2 — Engine Stateless et Opaque**

L'Engine ne possède aucune mémoire. Il opère exclusivement sur des buffers fournis par l'appelant (`&[u8]`, out-params). Aucune allocation interne. Les seuls états internes autorisés sont des tableaux statiques de taille fixe (`[T; N]`).

**INV-W3 — Interface C-ABI Obligatoire**

Toutes les fonctions publiques de l'Engine exportées vers l'extérieur utilisent `extern "C"`. Aucune signature publique ne retourne `Result<T, E>` à la frontière FFI — les erreurs transitent par un `i32` (code de retour) et des out-params. Cette contrainte garantit l'interopérabilité avec Zig, C, et Wasm sans glue layer.

**INV-W4 — Flux unidirectionnel Forge → Engine**

La Forge dépend de l'Engine (elle l'utilise comme bibliothèque pour le Slow Path lors de la compilation AOT). L'Engine ne dépend jamais de la Forge. Le graphe de dépendances est acyclique et unidirectionnel.

**INV-W5 — Zéro diagnostic de l'Engine**

L'Engine ne produit aucun output (`eprintln!`, `println!`, `log::*`). Tout chemin d'erreur remonte via code de retour entier ou out-param. Le diagnostic est la responsabilité exclusive de la couche appelante (Runtime, `std`).

### 0.3 Responsabilités par Composant

**Forge (`liturgical-calendar-forge`, std)** :

- Ingestion et validation des fichiers YAML de configuration liturgique
- Validation canonique V1–V6 (unicité temporelle, domaines, FeastID, cohérence)
- `FeastRegistry` (BTreeMap, allocation, import/export)
- Fonctions de normalisation chaînes (`normalize_color`, `normalize_nature`) — requièrent `String`
- Sérialisation binaire (`Calendar::write_kald`) — requiert I/O fichier
- Appel du Slow Path de l'Engine pour générer le Data Body du `.kald`

**Engine (`liturgical-calendar-core`, no_std, no_alloc)** :

- Types de domaine canoniques : `DayPacked`, `Day`, `Precedence`, `Nature`, `Color`, `Season`
- Algorithme Meeus/Jones/Butcher (`compute_easter`) — arithmétique pure
- `SeasonBoundaries::compute` — arithmétique pure
- `SlowPath::compute(year, day_of_year) -> Result<Day, DomainError>` — sans allocation
- `validate_header(bytes: &[u8]) -> Result<Header, HeaderError>` — sans I/O
- `DomainError` — types primitifs uniquement, `Copy`, pas de `String`
- Fonctions FFI `extern "C"` exposées via `kal_*`

### 0.4 Contrat de Types à la Frontière FFI

Les types suivants sont **FFI-compatibles** (traversent la frontière `extern "C"`) :

| Type                    | Représentation                   | Raison                     |
| ----------------------- | -------------------------------- | -------------------------- |
| `DayPacked`             | `#[repr(transparent)]` sur `u32` | ✅ Compatible FFI          |
| `Header`                | `#[repr(C)]`                     | ✅ Compatible FFI          |
| `i32`                   | type C natif                     | ✅ Code de retour standard |
| `*const u8` / `*mut u8` | pointeur C                       | ✅ Buffer appelant         |
| `u16`, `i16`, `u32`     | types primitifs                  | ✅ Compatible FFI          |

Les types suivants **ne traversent pas** la frontière FFI :

| Type                   | Raison                                                    |
| ---------------------- | --------------------------------------------------------- |
| `Result<T, E>`         | Type Rust non-représentable en C                          |
| `Day` (struct logique) | Pas de `#[repr(C)]` — usage interne uniquement            |
| `CorruptionInfo`       | Contient `&'static str` — requiert `*const c_char` en FFI |
| `String`, `Vec<T>`     | Interdits dans l'Engine                                   |

---

## Philosophie Architecturale

**Principe Fondamental** : liturgical-calendar est un moteur déterministe AOT capable de produire un artefact annuel figé appelé **Kalendarium**, sérialisé au format `.kald` (magic `KALD`).

Le système est **complet et autonome**, capable de calculer le calendrier liturgique pour n'importe quelle année grégorienne canonique (1583-4099) via son **Slow Path algorithmique**.

Le **Fast Path** (fichier `.kald`) n'est pas un cache obligatoire ni un fallback : c'est une **optimisation spatiale et temporelle délibérée** pour une plage de travail spécifique choisie par l'utilisateur.

**Conception en Deux Niveaux** :

1. **Slow Path (Citoyen de Première Classe)** :
   - Calcul algorithmique des règles liturgiques
   - Couvre l'intégralité du calendrier grégorien (1583-4099)
   - Latence : <10µs par jour
   - Aucune dépendance externe

2. **Fast Path (Optimisation Optionnelle)** :
   - Pré-calcul AOT d'une fenêtre temporelle choisie
   - Typiquement : -50/+300 ans autour de l'année courante
   - Latence : <100ns par jour (gain ×100)
   - Fichier `.kald` : luxe de performance pour les années critiques

**Cas d'Usage** :

- **Application mobile** : Fichier `.kald` intégré couvrant 2000-2100 (optimisation pour utilisateurs contemporains), Slow Path pour requêtes historiques/futures
- **Serveur liturgique** : Fenêtre glissante régénérée annuellement (année courante ±50 ans), Slow Path pour archives/projections
- **Recherche historique** : Pas de Fast Path, Slow Path uniquement (1583-2025)
- **Calendrier perpétuel** : Fast Path 1900-2200 (ère moderne complète), Slow Path pour hors-limites

**L'utilisateur choisit sa plage d'optimisation. Le système continue de fonctionner pour toutes les autres années.**

---

## 1. Vocabulaire du Domaine (Ubiquitous Language)

Toutes les définitions utilisent le Latin Canonique. Les Enums Rust sont annotées `#[repr(u8)]` pour garantir la représentation binaire exacte.

### 1.1 Types Fondamentaux (Correction Audit #2 - Séparation Logic/Packed)

**IMPORTANT** : Le système utilise deux représentations distinctes pour garantir la séparation des responsabilités :

```rust
/// Représentation LOGIQUE pour la Forge et le Slow Path
/// Structure riche avec validations, conversions, et métadonnées
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Day {
    pub precedence: Precedence,
    pub nature: Nature,
    pub color: Color,
    pub season: Season,
    pub feast_id: u32,
}

/// Représentation PACKED pour le Runtime (Fast Path)
/// Transparente au u32 pour zero-cost abstraction
#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct DayPacked(u32);

/// Information détaillée sur une corruption de Data Body
///
/// Retournée par DayPacked::try_from_u32 pour permettre un log structuré.
/// Le champ offset est rempli par le Provider au moment de l'accès.
#[derive(Debug, Clone)]
pub struct CorruptionInfo {
    /// Valeur u32 brute lue dans le Data Body
    pub packed_value: u32,
    /// Nom du champ invalide ("precedence", "nature", "color", "season", "reserved")
    pub invalid_field: &'static str,
    /// Valeur numérique du champ invalide
    pub invalid_value: u8,
    /// Offset dans le fichier .kald (rempli par le Provider)
    pub offset: Option<usize>,
}

impl DayPacked {
    /// Construction sécurisée avec validation des bits
    ///
    /// Retourne CorruptionInfo détaillé en cas d'échec, permettant un log
    /// structuré sans allocation dans le happy path.
    pub fn try_from_u32(packed: u32) -> Result<Self, CorruptionInfo> {
        Day::try_from_u32(packed)
            .map(|_| Self(packed))
            .map_err(|e| CorruptionInfo {
                packed_value: packed,
                invalid_field: e.field_name(),
                invalid_value: e.field_value(),
                offset: None,  // Sera rempli par le Provider
            })
    }

    /// Extraction du u32 brut (zero-cost)
    #[inline(always)]
    pub fn as_u32(&self) -> u32 {
        self.0
    }

    /// Conversion vers la forme logique (pour debugging/affichage)
    pub fn to_logic(&self) -> Result<Day, CorruptionInfo> {
        Day::try_from_u32(self.0)
            .map_err(|e| CorruptionInfo {
                packed_value: self.0,
                invalid_field: e.field_name(),
                invalid_value: e.field_value(),
                offset: None,
            })
    }

    /// Crée un jour marqué comme invalide (pour erreurs)
    ///
    /// INVARIANT : 0xFFFFFFFF est hors domaine valide.
    /// Décomposé selon le layout DayPacked v2.0 :
    ///   Precedence bits [31:28] = 15 → hors domaine (max = 12), rejeté par try_from_u8.
    ///   Nature bits [27:25] = 7 → hors domaine (max = 4), rejeté par try_from_u8.
    /// Aucune entrée liturgique valide ne peut produire cette valeur.
    /// Pas de collision possible avec une entrée décodable.
    ///
    /// NE PAS utiliser 0x00000000 : décode en (TriduumSacrum, Sollemnitas, Albus, TempusOrdinarium, id=0),
    /// valeur sémantiquement valide — ambiguïté fatale pour la détection de corruption.
    pub fn invalid() -> Self {
        Self(0xFFFFFFFF)
    }

    /// Teste si ce DayPacked est la sentinelle d'erreur
    #[inline(always)]
    pub fn is_invalid(&self) -> bool {
        self.0 == 0xFFFFFFFF
    }

    /// Comparaison de Precedence entre deux entrées packed.
    ///
    /// INVARIANT : seuls les bits [31:28] sont comparés.
    /// Une valeur numérique plus faible représente une force d'éviction plus élevée.
    ///
    /// ANTI-PATTERN INTERDIT :
    ///   `day_a.as_u32() < day_b.as_u32()`  ← compare Nature, Color, Season : résultat faux.
    ///
    /// USAGE CORRECT :
    ///   `day_a.precedence_cmp(&day_b) == Ordering::Less`  ← Precedence uniquement.
    ///   ou : `day_a < day_b` via PartialOrd (délègue à cette méthode).
    #[inline(always)]
    pub fn precedence_cmp(&self, other: &Self) -> core::cmp::Ordering {
        let pa = (self.0 >> 28) & 0xF;
        let pb = (other.0 >> 28) & 0xF;
        pa.cmp(&pb)
    }
}

/// PartialOrd sur DayPacked compare uniquement les bits de Precedence [31:28].
///
/// GARANTIE : `day_a < day_b` ↔ `day_a` a une force d'éviction PLUS ÉLEVÉE que `day_b`.
/// Aucune autre donnée (Nature, Color, Season, FeastID) n'influence l'ordre.
///
/// Cette implémentation interdit toute confusion avec une comparaison brute u32 :
/// `DayPacked` ne dérive pas `PartialOrd` automatiquement — le compilateur rejette
/// tout `<` sur u32 extrait sans passer par `as_u32()`.
impl PartialOrd for DayPacked {
    #[inline(always)]
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.precedence_cmp(other))
    }
}

impl Ord for DayPacked {
    #[inline(always)]
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.precedence_cmp(other)
    }
}

impl From<Day> for DayPacked {
    fn from(logic: Day) -> Self {
        Self(logic.into())
    }
}

impl From<Day> for u32 {
    fn from(day: Day) -> Self {
        ((day.precedence as u32) << 28)
            | ((day.nature as u32) << 25)
            | ((day.color as u32) << 22)
            | ((day.season as u32) << 19)
            | (day.feast_id & 0x3FFFF)
    }
}
```

**Justification de la Séparation** :

| Aspect             | `Day`                        | `DayPacked`                  |
| ------------------ | ---------------------------- | ---------------------------- |
| **Usage**          | Forge, Slow Path, calculs    | Runtime Fast Path uniquement |
| **Taille**         | ≥ 20 octets (struct riche)   | 4 octets (transparent)       |
| **Validation**     | Stricte à la construction    | Déjà validé par Forge        |
| **Conversions**    | Riches (JSON, display, etc.) | Minimales (u32 brut)         |
| **Évolution v2.x** | Extensible (nouveaux champs) | Figée (contrat binaire)      |

### 1.2 Color (3 bits)

Représentation des couleurs liturgiques selon les normes post-Vatican II.

```rust
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Color {
    Albus     = 0,  // Blanc
    Rubeus    = 1,  // Rouge
    Viridis   = 2,  // Vert
    Violaceus = 3,  // Violet
    Roseus    = 4,  // Rose
    Niger     = 5,  // Noir (défunts)
    // 6, 7 réservés pour extensions futures
}

impl Color {
    /// Construction sécurisée depuis u8 avec validation
    pub fn try_from_u8(val: u8) -> Result<Self, DomainError> {
        match val {
            0 => Ok(Self::Albus),
            1 => Ok(Self::Rubeus),
            2 => Ok(Self::Viridis),
            3 => Ok(Self::Violaceus),
            4 => Ok(Self::Roseus),
            5 => Ok(Self::Niger),
            _ => Err(DomainError::InvalidColor(val)),
        }
    }
}
```

> **Normalisation à la Forge** : `normalize_color(input: &str)` vit dans `liturgical-calendar-forge` (§5), pas dans `core`. Elle utilise `to_lowercase()` (allocation heap) et retourne `RegistryError`. Le crate `core` n'a pas cette dépendance.

### 1.3 Precedence (4 bits) et Nature (3 bits)

Le modèle v2.0 découple strictement deux axes orthogonaux.

**Axe ordinal : `Precedence` (4 bits)**

Force d'éviction. Comparaison purement numérique (`u32 >> 28`). Ordre total non cyclique. Une valeur numérique plus faible représente une force d'éviction plus élevée.

_Tabella dierum liturgicorum — NALC 1969. Ordre figé. Aucune modification autorisée après freeze v2.0._

| Valeur | Niveau Canonique                                            |
| ------ | ----------------------------------------------------------- |
| 0      | Triduum Sacrum                                              |
| 1      | Nativitas, Epiphania, Ascensio, Pentecostes                 |
| 2      | Dominicae Adventus, Quadragesimae, Paschales                |
| 3      | Feria IV Cinerum; Hebdomada Sancta                          |
| 4      | Sollemnitates Domini, BMV, Sanctorum in Calendario Generali |
| 5      | Sollemnitates propriae                                      |
| 6      | Festa Domini in Calendario Generali                         |
| 7      | Dominicae per annum                                         |
| 8      | Festa BMV et Sanctorum in Calendario Generali               |
| 9      | Festa propria                                               |
| 10     | Feriae Adventus (17–24 Dec), Octava Nativitatis             |
| 11     | Memoriae obligatoriae                                       |
| 12     | Feriae per annum; Memoriae ad libitum                       |

```rust
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Precedence {
    TriduumSacrum                      = 0,
    SollemnitatesFixaeMaior            = 1,
    DominicaePrivilegiataeMaior        = 2,
    FeriaePrivilegiataeMaior           = 3,
    SollemnitatesGenerales            = 4,
    SollemnitatesPropria               = 5,
    FestaDomini                        = 6,
    DominicaePerAnnum                  = 7,
    FestaBMVEtSanctorumGenerales       = 8,
    FestaPropria                       = 9,
    FeriaeAdventusEtOctavaNativitatis  = 10,
    MemoriaeObligatoriae               = 11,
    FeriaePerAnnumEtMemoriaeAdLibitum  = 12,
    // 13-15 réservés
}

impl Precedence {
    pub fn try_from_u8(val: u8) -> Result<Self, DomainError> {
        match val {
            0  => Ok(Self::TriduumSacrum),
            1  => Ok(Self::SollemnitatesFixaeMaior),
            2  => Ok(Self::DominicaePrivilegiataeMaior),
            3  => Ok(Self::FeriaePrivilegiataeMaior),
            4  => Ok(Self::SollemnitatesGenerales),
            5  => Ok(Self::SollemnitatesPropria),
            6  => Ok(Self::FestaDomini),
            7  => Ok(Self::DominicaePerAnnum),
            8  => Ok(Self::FestaBMVEtSanctorumGenerales),
            9  => Ok(Self::FestaPropria),
            10 => Ok(Self::FeriaeAdventusEtOctavaNativitatis),
            11 => Ok(Self::MemoriaeObligatoriae),
            12 => Ok(Self::FeriaePerAnnumEtMemoriaeAdLibitum),
            _  => Err(DomainError::InvalidPrecedence(val)),
        }
    }
}
```

**Axe sémantique : `Nature` (3 bits)**

Typologie rituelle de l'entité liturgique. La Nature ne dicte jamais la force d'éviction. Une Feria peut posséder une Precedence supérieure à une Memoria (ex : Feria IV Cinerum, Precedence=3, est supérieure à toute Memoria, Precedence=11 ou 12). Ce découplage est la justification structurelle du modèle 2D.

> **Dominica** : n'est pas une Nature. Dominica est une classe canonique de précédence. Sa Nature structurelle est `Feria`. Sa force d'éviction est encodée par `Precedence::DominicaePerAnnum` (7) ou `Precedence::DominicaePrivilegiataeMaior` (2) selon le temps liturgique.

```rust
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Nature {
    Sollemnitas    = 0,
    Festum        = 1,
    Memoria       = 2,
    Feria         = 3,
    Commemoratio  = 4,
    // 5-7 réservés
}

impl Nature {
    pub fn try_from_u8(val: u8) -> Result<Self, DomainError> {
        match val {
            0 => Ok(Self::Sollemnitas),
            1 => Ok(Self::Festum),
            2 => Ok(Self::Memoria),
            3 => Ok(Self::Feria),
            4 => Ok(Self::Commemoratio),
            _ => Err(DomainError::InvalidNature(val)),
        }
    }
}
```

### 1.4 Season (3 bits)

États liturgiques du calendrier. L'indice 0 représente l'état par défaut (Temps Ordinaire). Champ cache AOT — bits [21:19] du layout DayPacked v2.0.

```rust
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Season {
    TempusOrdinarium    = 0,  // Temps Ordinaire (défaut)
    TempusAdventus      = 1,  // Avent
    TempusNativitatis   = 2,  // Temps de Noël
    TempusQuadragesimae = 3,  // Carême
    TriduumPaschale     = 4,  // Triduum Pascal
    TempusPaschale      = 5,  // Temps Pascal
    DiesSancti          = 6,  // Semaine Sainte (Rameaux-Mercredi)
    // 7 réservé (3 bits, valeur max = 7)
}

impl Season {
    /// Construction sécurisée depuis u8 avec validation
    pub fn try_from_u8(val: u8) -> Result<Self, DomainError> {
        match val {
            0 => Ok(Self::TempusOrdinarium),
            1 => Ok(Self::TempusAdventus),
            2 => Ok(Self::TempusNativitatis),
            3 => Ok(Self::TempusQuadragesimae),
            4 => Ok(Self::TriduumPaschale),
            5 => Ok(Self::TempusPaschale),
            6 => Ok(Self::DiesSancti),
            _ => Err(DomainError::InvalidSeason(val)),
        }
    }
}
```

**Frontières Temporelles (Représentation DOD - Correction Audit Dates)** :

```rust
/// Représentation interne optimisée pour calculs CPU
/// IMPORTANT : Utilise u16 (DayOfYear 1-366) au lieu de structures Date complexes
/// pour éviter l'overhead de bibliothèques comme chrono
#[derive(Copy, Clone, Debug)]
pub struct SeasonBoundaries {
    pub advent_start: u16,        // Jour de l'année (1-366)
    pub christmas_start: u16,     // 25 déc : jour 359 (année commune) ou 360 (bissextile)
    pub epiphany_end: u16,        // Baptême du Seigneur
    pub ash_wednesday: u16,       // Pâques - 46 jours
    pub palm_sunday: u16,         // Pâques - 7 jours
    pub holy_thursday: u16,       // Pâques - 3 jours
    pub easter_sunday: u16,       // Comput de Pâques
    pub pentecost: u16,           // Pâques + 49 jours
}

impl SeasonBoundaries {
    /// Calcule les frontières pour une année donnée
    /// Retourne None si l'année est hors limites (< 1583 ou > 4099)
    pub fn compute(year: i32) -> Option<Self> {
        if year < 1583 || year > 4099 {
            return None;
        }

        let easter = compute_easter(year);

        Some(Self {
            advent_start: compute_advent_start(year),
            christmas_start: if is_leap_year(year) { 360 } else { 359 },  // 25 déc : j.359 ou j.360
            epiphany_end: compute_baptism_of_lord(year),
            ash_wednesday: easter.saturating_sub(46),
            palm_sunday: easter.saturating_sub(7),
            holy_thursday: easter.saturating_sub(3),
            easter_sunday: easter,
            pentecost: easter + 49,
        })
    }
}

/// Calcul de Pâques (Algorithme de Meeus/Jones/Butcher)
fn compute_easter(year: i32) -> u16 {
    let a = year % 19;
    let b = year / 100;
    let c = year % 100;
    let d = b / 4;
    let e = b % 4;
    let f = (b + 8) / 25;
    let g = (b - f + 1) / 3;
    let h = (19 * a + b - d - g + 15) % 30;
    let i = c / 4;
    let k = c % 4;
    let l = (32 + 2 * e + 2 * i - h - k) % 7;
    let m = (a + 11 * h + 22 * l) / 451;
    let month = (h + l - 7 * m + 114) / 31;
    let day = ((h + l - 7 * m + 114) % 31) + 1;

    // Conversion mois/jour → jour de l'année.
    //
    // INVARIANT (Meeus) : month ∈ {3, 4} — mathématiquement garanti par l'algorithme.
    // Branchless panic-free : aucun unreachable!(), aucun match exhaustif nécessaire.
    //
    //   mars  (month=3) : 31 (jan) + 28/29 (fév) = 59 + leap_bonus
    //   avril (month=4) : 31 (jan) + 28/29 (fév) + 31 (mar) = 90 + leap_bonus
    //
    // (month == 4) as i32 vaut 0 pour mars, 1 pour avril — pas de branche conditionnelle.
    let leap_bonus = is_leap_year(year) as i32;
    let is_april   = (month == 4) as i32;
    let days_before_month = 59 + leap_bonus + is_april * 31;

    (days_before_month + day) as u16
}

/// Détermine si une année est bissextile (calendrier grégorien)
#[inline]
fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0) && (year % 100 != 0 || year % 400 == 0)
}

/// Calcule le premier dimanche de l'Avent pour une année donnée
///
/// L'Avent commence le dimanche le plus proche du 30 novembre.
/// Retourne le jour de l'année (1-366).
fn compute_advent_start(year: i32) -> u16 {
    // Implémentation : roadmap section 1.2
    // Principe : trouver le dimanche le plus proche du 30 novembre (j.334)
    // puis reculer de 3 semaines (4e dimanche avant Noël)
    todo!("roadmap §1.2")
}

/// Calcule le jour de la Fête du Baptême du Seigneur (fin du Temps de Noël)
///
/// Retourne le jour de l'année (1-366).
fn compute_baptism_of_lord(year: i32) -> u16 {
    // Implémentation : roadmap section 1.2
    // Principe : dimanche après le 6 janvier (Épiphanie), ou lundi 7 si le
    // dimanche serait le 7 ou 8 janvier
    todo!("roadmap §1.2")
}
```

---

## 2. Format Binaire (.kald)

### 2.1 Structure Header (16 octets - Modifié avec Flags)

**Représentation Logique** :

```rust
/// Représentation logique du header (pas de layout mémoire direct)
///
/// ANNOTATION FFI OBLIGATOIRE (v2.1) : `#[repr(C)]` garantit un layout
/// prévisible à la frontière `extern "C"`. Requis pour que Header soit
/// passable en out-param FFI sans comportement indéfini.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub magic: [u8; 4],      // "KALD" (0x4B414C44)
    pub version: u16,        // Version du format (actuellement 1)
    pub start_year: i16,     // Année de départ (2025 pour france.kald)
    pub year_count: u16,     // Nombre d'années couvertes (300 pour france.kald)
    pub flags: u16,          // Flags d'extension
    pub _padding: [u8; 4],   // Strict 0x00
}

impl Header {
    /// Désérialise un header depuis 16 octets bruts
    ///
    /// ENDIANNESS CANONIQUE : Little-Endian (from_le_bytes).
    /// Les fichiers `.kald` utilisent LE indépendamment de l'architecture de build.
    /// Sur x86_64/ARM LE, le compilateur élimine le bswap — coût nul.
    /// Garantit un SHA-256 cross-platform déterministe (CI matrix 4 architectures).
    ///
    /// SÉCURITÉ : Pas de comportement indéfini (UB) lié à l'alignement.
    /// Portable sur toutes les architectures (ARM, RISC-V, x86, etc.).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, HeaderError> {
        if bytes.len() < 16 {
            return Err(HeaderError::FileTooSmall);
        }

        let magic = [bytes[0], bytes[1], bytes[2], bytes[3]];
        let version    = u16::from_le_bytes([bytes[4],  bytes[5]]);
        let start_year = i16::from_le_bytes([bytes[6],  bytes[7]]);
        let year_count = u16::from_le_bytes([bytes[8],  bytes[9]]);
        let flags      = u16::from_le_bytes([bytes[10], bytes[11]]);
        let padding = [bytes[12], bytes[13], bytes[14], bytes[15]];

        Ok(Header {
            magic,
            version,
            start_year,
            year_count,
            flags,
            _padding: padding,
        })
    }

    /// Sérialise le header en 16 octets bruts (Little-Endian canonique)
    pub fn to_bytes(&self) -> [u8; 16] {
        let mut bytes = [0u8; 16];

        bytes[0..4].copy_from_slice(&self.magic);
        bytes[4..6].copy_from_slice(&self.version.to_le_bytes());
        bytes[6..8].copy_from_slice(&self.start_year.to_le_bytes());
        bytes[8..10].copy_from_slice(&self.year_count.to_le_bytes());
        bytes[10..12].copy_from_slice(&self.flags.to_le_bytes());
        bytes[12..16].copy_from_slice(&self._padding);

        bytes
    }
}
```

**Politique Endianness** :

- **Little-Endian Canonique** : Les fichiers `.kald` utilisent LE indépendamment de l'architecture de build.
- **Justification** : SHA-256 cross-platform déterministe — un même `.kald` produit le même hash sur toutes les architectures.
- **Coût nul** : sur x86_64 et ARM (LE natif), le compilateur élimine le bswap. Sur une architecture BE (MIPS-BE, SPARC) : un seul bswap par champ lors de la sérialisation AOT — non critique.
- **`PossibleEndiannessMismatch`** : filet de sécurité résiduel pour les fichiers legacy produits avec l'ancienne politique `from_ne_bytes`. Non le mécanisme principal.

```

**Flags d'Extension (bits 0-15)** :

```

Bit 0 : Compression activée (0 = non, 1 = ZSTD) [réservé v2.2]
Bit 1 : Checksums inclus (0 = non, 1 = CRC32) [réservé v2.2]
Bit 2-3 : Réservé pour rites (00 = Ordinaire, 01 = Extraordinaire) [réservé v2.3]
Bit 4-15 : Réservé pour extensions futures

````

> **v1 : tous les flags sont refusés.** `KNOWN_FLAGS_V1 = 0x0000` — tout fichier présentant un flag non nul est rejeté au chargement (`UnsupportedFlags`). Les bits ci-dessus documentent les extensions planifiées pour v2+, non des fonctionnalités actives.

> **Invariant ZSTD / `no_alloc` (v2.2)** : lorsque le flag de compression sera activé, la décompression ZSTD est une responsabilité de la **couche appelante** (Runtime `std`). L'Engine (`liturgical-calendar-core`) reçoit toujours un buffer `&[u8]` pré-décompressé — il ne voit jamais ZSTD. Cet invariant sanctuarise `no_alloc` sur l'Engine quelle que soit la version du format.

**Validation Stricte** :

```rust
/// Valide et désérialise un header depuis un mmap
///
/// SÉCURITÉ :
/// - Pas d'UB lié à l'alignement (désérialisation explicite)
/// - Validation stricte de tous les champs
/// - Détection des corruptions et mismatches
///
/// PARAMÈTRE : `&[u8]` générique — pas de dépendance à memmap2 dans ce crate.
/// L'appelant extrait le slice depuis son Mmap via `&mmap[..]` avant d'appeler.
/// VISIBILITÉ : `pub` — requis pour le fuzz target et les tests d'intégration
/// (`use liturgical_calendar_core::validate_header`).
pub fn validate_header(bytes: &[u8]) -> Result<Header, HeaderError> {
    if bytes.len() < 16 {
        return Err(HeaderError::FileTooSmall);
    }

    // Désérialisation sans UB (pas de cast de pointeur)
    let header = Header::from_bytes(&bytes[0..16])?;

    // Validation magic
    if &header.magic != b"KALD" {
        return Err(HeaderError::InvalidMagic(header.magic));
    }

    // Validation version
    if header.version != 1 {
        return Err(HeaderError::UnsupportedVersion(header.version));
    }

    // Validation flags (rejet strict des bits inconnus)
    const KNOWN_FLAGS_V1: u16 = 0x0000;
    if (header.flags & !KNOWN_FLAGS_V1) != 0 {
        return Err(HeaderError::UnsupportedFlags {
            found: header.flags,
            known: KNOWN_FLAGS_V1,
            unknown_bits: header.flags & !KNOWN_FLAGS_V1,
        });
    }

    // Validation padding (doit être strictement 0x00)
    if header._padding != [0, 0, 0, 0] {
        return Err(HeaderError::InvalidPadding(header._padding));
    }

    // Validation range années
    if header.start_year < 1583 || header.start_year > 4099 {
        return Err(HeaderError::YearOutOfBounds(header.start_year));
    }

    if header.year_count == 0 || header.year_count > 2516 {
        return Err(HeaderError::InvalidYearCount(header.year_count));
    }

    // Détection heuristique de mismatch endianness.
    // INV-W5 (v2.1) : l'Engine ne produit aucun output — `eprintln!` est INTERDIT ici.
    // Le mismatch endianness est signalé via un variant dédié de HeaderError,
    // remontant à la couche appelante (Runtime, std) qui gère le diagnostic.
    if header.start_year < 1000 || header.start_year > 5000 {
        return Err(HeaderError::PossibleEndiannessMismatch(header.start_year));
    }

    Ok(header)
}

#[derive(Debug, Clone)]
pub enum HeaderError {
    FileTooSmall,
    InvalidMagic([u8; 4]),
    UnsupportedVersion(u16),
    UnsupportedFlags {
        found: u16,
        known: u16,
        unknown_bits: u16,
    },
    InvalidPadding([u8; 4]),
    YearOutOfBounds(i16),
    InvalidYearCount(u16),
    /// Heuristique endianness : start_year hors de la plage [1000, 5000].
    /// Signalé via ce variant plutôt que eprintln! (INV-W5 : Engine sans output).
    /// La couche appelante (Runtime, std) est responsable du diagnostic.
    PossibleEndiannessMismatch(i16),
}
````

### 2.2 Data Body (366 u32 × N années)

**Layout Strict** :

```
Offset   : Contenu
──────────────────────────────────
0x0000   : Header (16 octets)
0x0010   : Année[0], Jour 1 (u32)
0x0014   : Année[0], Jour 2 (u32)
...
0x05C4   : Année[0], Jour 366 (u32)
0x05C8   : Année[1], Jour 1 (u32)
...
```

**Taille Fichier** :

```rust
const HEADER_SIZE: usize = 16;
const YEAR_SIZE: usize = 366 * 4;  // 366 u32 = 1464 octets

fn compute_file_size(year_count: u16) -> usize {
    HEADER_SIZE + (year_count as usize * YEAR_SIZE)
}

// Exemple : france.kald (2025-2324, 300 ans)
// = 16 + (300 * 1464) = 439,216 octets
```

**Endianness (Documentation Runtime)** :

```rust
/// ENDIANNESS CANONIQUE : Little-Endian universel.
///
/// Les fichiers `.kald` utilisent LE indépendamment de l'architecture de build.
/// Cette décision garantit un SHA-256 identique sur toutes les cibles CI
/// (x86_64-linux, aarch64-linux, x86_64-darwin, aarch64-darwin — toutes LE).
///
/// Implémentation :
/// - Header : désérialisé avec from_le_bytes() (pas de cast de pointeur)
/// - Data Body : u32 lus avec from_le_bytes()
/// - Sérialisation : to_le_bytes() partout
///
/// Sur x86_64/ARM (LE natif) : le compilateur élimine le bswap. Coût = 0.
/// Sur architecture BE hypothétique : bswap au moment de la sérialisation AOT (Forge uniquement).
///
/// Détection heuristique résiduelle (pour fichiers legacy ne_bytes) :
pub fn detect_endianness_mismatch(header: &Header) -> bool {
    // Si start_year est aberrant après décodage LE, probable fichier legacy ne_bytes-BE.
    header.start_year < 1000 || header.start_year > 5000
}

/// Utilitaire de diagnostic
pub fn diagnose_file(path: &str) -> Result<DiagnosticReport, IoError> {
    let file = File::open(path)?;
    let mmap = unsafe { Mmap::map(&file)? };

    // Désérialisation sans UB
    let header = Header::from_bytes(&mmap[0..16])?;

    let report = DiagnosticReport {
        file_size: mmap.len(),
        magic: header.magic,
        version: header.version,
        start_year: header.start_year,
        year_count: header.year_count,
        flags: header.flags,
        endianness_ok: !detect_endianness_mismatch(&header),
        system_endian: if cfg!(target_endian = "little") { "little" } else { "big" },
    };

    Ok(report)
}
```

**Convention de Build** :

```toml
# Cargo.toml - Spécification des targets
[package.metadata.kald-build]
targets = [
    "x86_64-unknown-linux-gnu",      # Little-endian
    "aarch64-unknown-linux-gnu",     # Little-endian
    "x86_64-apple-darwin",           # Little-endian
    "aarch64-apple-darwin",          # Little-endian
    # Pour big-endian, utiliser des builds séparés
]
```

### 2.3 Bitpacking Layout — DayPacked (u32)

**Layout Normatif (v2.0 — figé)** :

| Bits     | Champ      | Taille  | Description                                                         |
| -------- | ---------- | ------- | ------------------------------------------------------------------- |
| [31..28] | Precedence | 4 bits  | Axe ordinal (0–12). Z-Index strict. Comparaison purement entière.   |
| [27..25] | Nature     | 3 bits  | Axe sémantique (Sollemnitas, Festum, Memoria, Feria, Commemoratio). |
| [24..22] | Color      | 3 bits  | Couleur liturgique finale résolue en Forge.                         |
| [21..19] | Season     | 3 bits  | Cache AOT — rendu O(1). Valeurs 0–6, 7 réservé.                     |
| [18]     | Reserved   | 1 bit   | Inactif en v2.0. Positionné à 0 par la Forge.                       |
| [17..0]  | FeastID    | 18 bits | Identifiant de fête (0–262 143).                                    |

**Extraction (Runtime)** :

```rust
impl Day {
    pub fn try_from_u32(packed: u32) -> Result<Self, DomainError> {
        let precedence_bits = ((packed >> 28) & 0xF) as u8;
        let nature_bits     = ((packed >> 25) & 0x7) as u8;
        let color_bits      = ((packed >> 22) & 0x7) as u8;
        let season_bits     = ((packed >> 19) & 0x7) as u8;
        // Bit [18] : Reserved — doit être 0
        let reserved_bit    = (packed >> 18) & 0x1;
        let feast_id        = packed & 0x3FFFF;
        if reserved_bit != 0 {
            return Err(DomainError::ReservedBitSet);
        }

        Ok(Self {
            precedence: Precedence::try_from_u8(precedence_bits)?,
            nature:     Nature::try_from_u8(nature_bits)?,
            color:      Color::try_from_u8(color_bits)?,
            season:     Season::try_from_u8(season_bits)?,
            feast_id,
        })
    }
}
```

---

### 2.4 Invariants Structurels (v2.0 — Freeze)

### INV-1 : Comparaison Ordinale

- La Precedence est l'unique axe de résolution des collisions.
- Comparaison purement entière : `(packed_a >> 28) < (packed_b >> 28)`.
- Ordre total, non cyclique.
- Valeur numérique plus faible = force d'éviction plus élevée.
- Aucune logique sémantique n'intervient dans la collision.

### INV-2 : Immutabilité du Z-Index

- La Tabella (13 niveaux, §1.3) est figée après freeze v2.0.
- Aucune modification de l'ordre 0–12 n'est autorisée.
- Toute extension future doit utiliser des valeurs hors de la plage 0–12, via migration majeure.

### INV-3 : Séparation des Axes

- `Nature ≠ Precedence`. Les deux axes sont orthogonaux.
- La Nature ne dicte jamais la force d'éviction.
- Cas normatif : `Feria IV Cinerum` possède `Precedence=3` (FeriaePrivilegiataeMaior), supérieure à toute `Memoria` (`Precedence=11`), bien que sa Nature soit `Feria`.
- `Dominica` n'est pas une Nature. Sa Nature est `Feria`. Sa force d'éviction est encodée par `Precedence::DominicaePerAnnum` (7) ou `Precedence::DominicaePrivilegiataeMaior` (2).

### INV-4 : Forge comme Producteur Unique

- Le fichier `.kald` est généré exclusivement par la Forge (Slow Path, AOT).
- Le runtime (Fast Path) est strictement en lecture.
- Aucune mutation, recalcul liturgique, de saison, de couleur ou de précédence n'est autorisé au runtime.

### INV-5 : Unicité des Commémorations

- La Forge garantit au maximum une seule Commemoratio par jour.
- Le runtime ne gère aucune liste de collisions.
- Toute collision complexe est résolue en AOT.

### INV-6 : Redondance Contrôlée — Season

- Le champ `Season` (bits [21:19]) est une matérialisation AOT volontaire.
- Il garantit un rendu O(1) sans calcul de frontières temporelles au runtime.
- Il est un cache structurel, non une donnée dérivable au runtime.

### INV-7 : Bit Reserved

- Le bit [18] est inactif en v2.0.
- La Forge le positionne à 0.
- Aucun comportement ne dépend de ce bit en v2.0.
- `try_from_u32` retourne `DomainError::ReservedBitSet` si ce bit est à 1.

---

## 3. FeastID Registry (Correction Audit #1 - Collisions)

### 3.1 Espace d'Allocation Hiérarchique

**Structure des FeastID (18 bits)** :

```
Bits 17-16 (2 bits) : Scope       (0=Universal, 1=Regional, 2=National, 3=Local)
Bits 15-12 (4 bits) : Category    (0=Temporal, 1=Sanctoral, 2=Marian, etc.)
Bits 11-0  (12 bits): Sequential  (0-4095 par scope/category)
```

Capacité totale : 262 144 FeastID (valeurs 0 à 262 143). Largement suffisant pour tout sanctoral universel, régional et local prévisible.

**Exemple d'Allocation** :

```
Universal/Temporal  : 0x00000 - 0x00FFF
Universal/Sanctoral : 0x01000 - 0x01FFF
Regional/Sanctoral  : 0x09000 - 0x09FFF
National/Sanctoral  : 0x11000 - 0x11FFF
Local/Sanctoral     : 0x19000 - 0x19FFF
```

### 3.2 Registry Canonique

```rust
use std::collections::BTreeMap;
use std::fs::File;
use std::io::Write;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeastRegistry {
    /// Mappage FeastID → Nom canonique
    pub allocations: BTreeMap<u32, String>,

    /// Compteurs par scope/category pour allocation séquentielle
    pub next_id: BTreeMap<(u8, u8), u16>,
}

impl FeastRegistry {
    /// Charge le registry depuis un fichier JSON canonique
    pub fn load(path: &str) -> Result<Self, IoError> {
        let file = File::open(path)?;
        let registry: Self = serde_json::from_reader(file)?;
        Ok(registry)
    }

    /// Sauvegarde le registry (déterministe via BTreeMap)
    pub fn save(&self, path: &str) -> Result<(), RegistryError> {
        let file = File::create(path)?;
        serde_json::to_writer_pretty(file, self)?;
        Ok(())
    }

    /// Alloue le prochain ID disponible pour un scope/category
    pub fn allocate_next(&mut self, scope: u8, category: u8) -> Result<u32, RegistryError> {
        if scope > 3 || category > 15 {
            return Err(RegistryError::InvalidScopeCategory { scope, category });
        }

        let key = (scope, category);
        let next = self.next_id.entry(key).or_insert(0);

        if *next == 0x1000 {
            return Err(RegistryError::FeastIDExhausted { scope, category });
        }

        let feast_id = ((scope as u32) << 16)
            | ((category as u32) << 12)
            | (*next as u32);

        *next += 1;

        Ok(feast_id)
    }

    /// Enregistre une allocation avec nom canonique
    pub fn register(&mut self, feast_id: u32, name: String) -> Result<(), RegistryError> {
        if self.allocations.contains_key(&feast_id) {
            return Err(RegistryError::FeastIDCollision(feast_id));
        }

        self.allocations.insert(feast_id, name);
        Ok(())
    }
}
```

### 3.3 Import/Export pour Interopérabilité

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct RegistryExport {
    pub scope: u8,
    pub category: u8,
    pub version: u16,              // Version du format d'export (actuellement 1)
    pub allocations: Vec<(u32, String)>,
}

/// Rapport d'importation retourné par FeastRegistry::import()
#[derive(Debug, Clone)]
pub struct ImportReport {
    pub imported: usize,           // Entrées nouvellement importées
    pub skipped: usize,            // Entrées ignorées (déjà présentes, identiques)
    pub collisions: Vec<CollisionInfo>,  // Conflits de noms pour un même FeastID
}

/// Détail d'un conflit de FeastID lors de l'import
#[derive(Debug, Clone)]
pub struct CollisionInfo {
    pub feast_id: u32,
    pub existing: String,          // Nom déjà enregistré localement
    pub incoming: String,          // Nom du fichier importé
}

impl FeastRegistry {
    /// Export d'un scope/category pour partage entre forges
    pub fn export_scope(&self, scope: u8, category: u8) -> RegistryExport {
        let prefix = ((scope as u32) << 16) | ((category as u32) << 12);
        let mask = 0x3F000u32;  // Bits [17:12] : Scope (2 bits) + Category (4 bits)

        let allocations: Vec<(u32, String)> = self
            .allocations
            .iter()
            .filter(|(id, _)| (**id & mask) == prefix)
            .map(|(id, name)| (*id, name.clone()))
            .collect();

        RegistryExport {
            scope,
            category,
            version: 1,
            allocations,
        }
    }

    /// Import avec détection de collision
    ///
    /// Retourne ImportReport si tout s'est bien passé (collisions incluses dans le rapport).
    /// Retourne Err uniquement pour les erreurs I/O ou d'intégrité structurelle.
    pub fn import(&mut self, export: RegistryExport) -> Result<ImportReport, RegistryError> {
        let mut report = ImportReport {
            imported: 0,
            skipped: 0,
            collisions: Vec::new(),
        };

        for (feast_id, name) in export.allocations {
            // Vérification collision
            if let Some(existing) = self.allocations.get(&feast_id) {
                if existing != &name {
                    report.collisions.push(CollisionInfo {
                        feast_id,
                        existing: existing.clone(),
                        incoming: name.clone(),
                    });
                    report.skipped += 1;
                } else {
                    report.skipped += 1;  // Déjà présent, identique — pas d'erreur
                }
            } else {
                self.allocations.insert(feast_id, name);
                report.imported += 1;
            }
        }

        // Mise à jour du compteur next_id
        let key = (export.scope, export.category);
        let max_seq = self.allocations
            .keys()
            .filter(|id| (**id & 0x3F000u32) == ((export.scope as u32) << 16 | (export.category as u32) << 12))
            .map(|id| (id & 0xFFF) as u16)
            .max()
            .unwrap_or(0);

        self.next_id.insert(key, max_seq + 1);

        Ok(report)
    }
}
```

**Workflow de Partage** :

```bash
# Forge France : Export des saints nationaux
$ liturgical-calendar-forge registry export --scope 2 --category 1 --output france_sanctoral.json

# Forge Allemagne : Import pour éviter collisions
$ liturgical-calendar-forge registry import --file france_sanctoral.json

# Allocation allemande (commencera à partir du dernier ID français + 1)
$ liturgical-calendar-forge registry allocate --scope 2 --category 1 --name "St. Boniface"
```

---

## 4. Slow Path (Calcul Algorithmique Complet)

### Statut Architectural

Le Slow Path est le **cœur algorithmique complet** du système, capable de calculer n'importe quelle date liturgique sans dépendances externes. Il n'est pas un "fallback" ou un "plan B" : c'est le système de référence canonique.

**Caractéristiques** :

- **Complétude** : Couvre 1583-4099 (calendrier grégorien complet)
- **Autonomie** : Aucune donnée pré-calculée requise
- **Déterminisme** : Résultats identiques au Fast Path (validé par tests)
- **Performance** : <10µs par jour (acceptable pour la plupart des usages)

**Relation Fast/Slow** :

Le Fast Path est une **optimisation pré-calculée** d'une fenêtre du Slow Path. Leur identité est garantie par construction (la Forge utilise le Slow Path pour générer le `.kald`). Le choix entre les deux est une **décision de performance**, pas de correction fonctionnelle.

### 4.1 Architecture Stratifiée

Le Slow Path est organisé en couches hiérarchiques pour gérer les règles liturgiques complexes.

```rust
pub struct SlowPath {
    /// Règles temporelles (calendrier civil → calendrier liturgique)
    temporal: TemporalLayer,

    /// Règles sanctorales (fêtes fixes des saints)
    sanctoral: SanctoralLayer,

    /// Règles de précédence (résolution des conflits)
    precedence: PrecedenceResolver,
}

impl SlowPath {
    /// Calcule la liturgie d'un jour donné
    pub fn compute(&self, year: i16, day_of_year: u16) -> Result<Day, DomainError> {
        // 1. Calcul des frontières de saisons
        let boundaries = SeasonBoundaries::compute(year as i32)
            .ok_or(DomainError::YearOutOfBounds(year))?;

        // 2. Détermination de la saison
        let season = self.temporal.determine_season(day_of_year, &boundaries)?;

        // 3. Recherche des fêtes candidates
        let temporal_feast = self.temporal.get_feast(year, day_of_year, &boundaries);
        let sanctoral_feast = self.sanctoral.get_feast(year, day_of_year);

        // 4. Résolution de précédence
        let (precedence, nature, color, feast_id) = self.precedence.resolve(
            year,
            &season,
            temporal_feast,
            sanctoral_feast,
            day_of_year,
            &boundaries,
        )?;

        Ok(Day {
            precedence,
            nature,
            season,
            color,
            feast_id,
        })
    }
}
```

### 4.2 Temporal Layer (Règles Calendrier Liturgique)

```rust
/// Données d'une fête liturgique dans le hot path du Slow Path
///
/// TYPE COPY intentionnel : pas d'allocation dans get_feast/get_day.
/// Le nom canonique n'est PAS ici — il est dans le StringProvider (.lits).
/// Le moteur n'a besoin que de l'identifiant, de la précédence, de la nature et de la couleur.
///
/// `#[repr(C)]` : layout stable à la frontière FFI (§0.4).
/// Layout résultant avec repr(C) et align(u32)=4 :
///   Offset 0 : id: u32          (4 octets)
///   Offset 4 : precedence: u8   (1 octet)
///   Offset 5 : nature: u8       (1 octet)
///   Offset 6 : color: u8        (1 octet)
///   Offset 7 : [padding: u8]    (1 octet, imposé par align(u32)=4)
///   Total : 8 octets | 1 octet de padding compilateur (12.5%)
///
/// NOTE : `repr(packed)` est interdit — génère des lectures non-alignées, UB en FFI.
/// Pour les layers internes (TemporalLayer, SanctoralLayer), `FeastDefinitionPacked`
/// (`NonZeroU32`) est utilisé depuis v2.3 — zéro padding, zéro cache line split (§4.2).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FeastDefinition {
    pub id: u32,               // FeastID (18 bits, voir section 3)
    pub precedence: Precedence,
    pub nature: Nature,
    pub color: Color,
}

/// Représentation bit-packed d'une `FeastDefinition` pour les layers internes.
///
/// TYPE ALIAS : `NonZeroU32` — garantit la niche optimization du compilateur Rust :
///   `Option<FeastDefinitionPacked>` occupe exactement **4 octets** (la valeur `0`
///   servant de discriminant `None`, jamais produite par un FeastID valide ≥ 1).
///
/// LAYOUT des bits (32 bits) :
///   [31:18]  ID         (14 bits) — 16 384 slots, suffisant pour le sanctoral universel
///   [17:14]  Precedence (4 bits)  — 16 valeurs d'éviction
///   [13:11]  Nature     (3 bits)  — 8 valeurs
///   [10:8]   Color      (3 bits)  — 8 valeurs
///   [7:0]    Réservés   (8 bits)  — toujours 0x00 ; extensibilité future
///
/// INVARIANT NonZero : tout FeastID ≥ 1 garantit u32 ≠ 0 (bits [31:18] ≠ 0).
///   ID = 0 est explicitement réservé "aucune fête" (§3.1) — NonZeroU32::new() refusera.
///
/// BILAN MÉMOIRE vs. FeastDefinition [repr(C)] :
///   Option<FeastDefinitionPacked>          =  4 octets (niche, zéro discriminant externe)
///   Option<FeastDefinition>                = 12 octets (discriminant 1B + padding 3B + 8B)
///   [Option<FeastDefinitionPacked>; 367]   =  1 468 octets  (TemporalLayer)
///   [Option<FeastDefinition>;       367]   =  4 404 octets  (−67%)
///   align_of(FeastDefinitionPacked) = 4 → 4 ∣ 64 → zéro cache line split garanti.
pub type FeastDefinitionPacked = core::num::NonZeroU32;

/// Encode une `FeastDefinition` en `FeastDefinitionPacked`.
///
/// PRÉCONDITION : `def.id` ∈ [1, 16 383].
///   La Forge garantit cette précondition via l'invariant registry (§3.1 — ID 0 réservé).
///
/// # Panics (debug uniquement)
///   `unwrap()` panic en debug si `id == 0` ET tous les autres champs nuls (u32 = 0).
///   En release : `NonZeroU32::new_unchecked` serait utilisable si l'invariant est prouvé ;
///   on conserve `unwrap()` pour la détection de régression en CI.
#[inline]
pub fn feast_definition_pack(def: FeastDefinition) -> FeastDefinitionPacked {
    let bits: u32 = ((def.id            & 0x3FFF) << 18)
                  | (((def.precedence as u32) & 0xF)  << 14)
                  | (((def.nature      as u32) & 0x7)  << 11)
                  | (((def.color       as u32) & 0x7)  <<  8);
    // SAFETY : id ≥ 1 garantit bits ≠ 0 (cf. INVARIANT NonZero ci-dessus).
    core::num::NonZeroU32::new(bits)
        .expect("feast_definition_pack: id=0 produit un NonZeroU32 invalide (§3.1 violation)")
}

/// Décode un `FeastDefinitionPacked` en `FeastDefinition`.
///
/// Appelé au point de retour de `get_feast()` — chemin froid, pas de pression SIMD ici.
/// Le décodage est O(1) sur 4 instructions (shift + and × 4), sans branchement.
#[inline]
pub fn feast_definition_unpack(packed: FeastDefinitionPacked) -> FeastDefinition {
    let bits = packed.get();
    FeastDefinition {
        id:         (bits >> 18) & 0x3FFF,
        precedence: Precedence::from_u8(((bits >> 14) & 0xF) as u8),
        nature:     Nature::from_u8    (((bits >> 11) & 0x7) as u8),
        color:      Color::from_u8     (((bits >>  8) & 0x7) as u8),
    }
}

/// Représentation DOD de la couche temporelle.
///
/// INVARIANT de layout :
/// - Tableau fixe de 367 slots (indices 1–366, slot 0 inutilisé).
/// - Accès O(1) garanti : `moveable_feasts[day_of_year as usize]`.
/// - Zéro pointer-chasing : le tableau est contigu en mémoire.
/// - Zéro allocation : taille connue à la compilation.
///
/// Remplacement délibéré du BTreeMap<u16, FeastDefinition> (O(log N), pointer-chasing)
/// par un tableau plat. Le domaine est borné à 366 jours — un BTreeMap est inadapté
/// à un domaine aussi petit et prévisible.
///
/// BILAN MÉMOIRE v2.3 (niche optimization via FeastDefinitionPacked) :
///   `[Option<FeastDefinitionPacked>; 367]` = 367 × 4 =  1 468 octets
///   `[Option<FeastDefinition>;       367]` = 367 × 12 = 4 404 octets (référence v2.2)
///   Gain : −67% | align_of = 4 → 4 ∣ 64 → zéro cache line split.
pub struct TemporalLayer {
    /// Fêtes mobiles indexées par day_of_year (1–366).
    /// Slot 0 inactif. Jamais de Vec, jamais de pointeur heap.
    moveable_feasts: [Option<FeastDefinitionPacked>; 367],
}

impl TemporalLayer {
    /// Construit la TemporalLayer depuis une slice de paires (FeastDefinition, day_of_year).
    ///
    /// INVARIANT : les FeastDefinition et leurs day_of_year sont résolus par la Forge (AOT)
    /// avant d'atteindre ici. Le core ne reçoit jamais de TemporalRule ni de String.
    /// Appelé une seule fois (AOT). Le tableau résultant est en lecture seule au runtime.
    ///
    /// CONTRAT : `day_of_year` est le jour de l'année 1-based (1–366).
    ///   Le slot 0 de `moveable_feasts` est inactif ; les indices hors [1, 366] sont ignorés.
    ///
    /// INTERFACE CORRECTE vs. v2.0 :
    ///   v2.0 (incorrect) : `defs: &[FeastDefinition]`  — accès à `def.resolved_day_of_year`,
    ///                       champ fantôme absent de `FeastDefinition`.
    ///   v2.1 (corrigé)   : `defs: &[(FeastDefinition, u16)]` — la Forge passe explicitement
    ///                       le jour résolu comme second élément de la paire.
    pub fn new(defs: &[(FeastDefinition, u16)]) -> Self {
        let mut layer = Self {
            moveable_feasts: [None; 367],
        };
        // L'insertion se fait à la Forge ; si deux définitions tombent le même jour,
        // la précédence est résolue en amont (AOT) avant cet appel.
        for &(def, day) in defs {
            let day = day as usize;
            if day > 0 && day < 367 {
                // Encodage AOT : zéro coût au runtime Engine.
                layer.moveable_feasts[day] = Some(feast_definition_pack(def));
            }
        }
        layer
    }

    pub fn determine_season(
        &self,
        day_of_year: u16,
        boundaries: &SeasonBoundaries,
    ) -> Result<Season, DomainError> {
        // NOTE : SeasonBoundaries est par année civile. Le wrap Avent/Noël
        // (les jours post-24 déc d'une année étant en Avent de la suivante)
        // doit être géré en amont par le Provider, pas ici.
        if day_of_year >= boundaries.advent_start && day_of_year < boundaries.christmas_start {
            Ok(Season::TempusAdventus)
        } else if day_of_year >= boundaries.christmas_start && day_of_year <= boundaries.epiphany_end {
            Ok(Season::TempusNativitatis)
        } else if day_of_year >= boundaries.ash_wednesday && day_of_year < boundaries.palm_sunday {
            Ok(Season::TempusQuadragesimae)
        } else if day_of_year >= boundaries.palm_sunday && day_of_year < boundaries.holy_thursday {
            Ok(Season::DiesSancti)
        } else if day_of_year >= boundaries.holy_thursday && day_of_year < boundaries.easter_sunday {
            // Triduum : Jeudi Saint → Samedi Saint (easter-3 à easter-1 inclus)
            Ok(Season::TriduumPaschale)
        } else if day_of_year >= boundaries.easter_sunday && day_of_year <= boundaries.pentecost {
            // Temps Pascal : Pâques (inclus) → Pentecôte (incluse)
            Ok(Season::TempusPaschale)
        } else {
            Ok(Season::TempusOrdinarium)
        }
    }

    /// Accès O(1) garanti. Zéro allocation. Zéro pointer-chasing.
    #[inline(always)]
    pub fn get_feast(
        &self,
        _year: i16,
        day_of_year: u16,
        boundaries: &SeasonBoundaries,
    ) -> Option<FeastDefinition> {
        // Priorité : fêtes fixes pré-résolues dans le tableau plat.
        // Cas spéciaux de Pâques et Pentecôte : comparaison entière directe.
        if day_of_year == boundaries.easter_sunday {
            return Some(FeastDefinition {
                id: 0x00001,
                precedence: Precedence::TriduumSacrum,
                nature: Nature::Sollemnitas,
                color: Color::Albus,
            });
        }
        if day_of_year == boundaries.pentecost {
            return Some(FeastDefinition {
                id: 0x00002,
                precedence: Precedence::SollemnitatesFixaeMaior,
                nature: Nature::Sollemnitas,
                color: Color::Rubeus,
            });
        }
        // Toutes les autres fêtes mobiles : accès tableau O(1) + décodage inline.
        if day_of_year < 367 {
            self.moveable_feasts[day_of_year as usize].map(feast_definition_unpack)
        } else {
            None
        }
    }
}
```

### 4.3 Sanctoral Layer (Fêtes Fixes)

```rust
/// Représentation DOD de la couche sanctorale.
///
/// INVARIANT de layout :
/// - Tableau fixe de 366 slots, indexé par day_of_year - 1 (0-based).
/// - Chaque slot contient au maximum 2 fêtes (INV-5 : max 1 Commemoratio).
///   Le second slot est réservé aux cas de co-occurrence garantis AOT.
///   `[Option<FeastDefinitionPacked>; 2]` est inline, sans allocation heap.
/// - Accès O(1) : `fixed_feasts[(day_of_year - 1) as usize]`.
/// - Zéro Vec, zéro pointer-chasing, zéro indirection.
///
/// Remplacement délibéré du BTreeMap<(u8,u8), Vec<FeastDefinition>> :
/// - BTreeMap : O(log N) + pointer-chasing à chaque nœud.
/// - Vec : indirection heap systématique même pour 1 élément.
/// Le domaine est strictement borné (366 jours) — le tableau plat est l'invariant optimal.
///
/// BILAN MÉMOIRE v2.3 (niche optimization via FeastDefinitionPacked) :
///   `[Option<[Option<FeastDefinitionPacked>; 2]>; 366]` = 366 × 8 =  2 928 octets
///   `[Option<[Option<FeastDefinition>;       2]>; 366]` = 366 × 28 = 10 248 octets (ref v2.2)
///   Gain : −71% | align_of = 4 → 4 ∣ 64 → zéro cache line split garanti.
pub struct SanctoralLayer {
    /// Indexé par (day_of_year - 1). Slot [0] = jour 1. Slot [365] = jour 366.
    fixed_feasts: [Option<[Option<FeastDefinitionPacked>; 2]>; 366],
}

impl SanctoralLayer {
    /// Construit la SanctoralLayer depuis une slice de paires (FeastDefinition, day_of_year).
    ///
    /// INVARIANT : les FeastDefinition et leurs day_of_year sont résolus par la Forge (AOT)
    /// avant d'atteindre ici. Le core ne reçoit jamais de SanctoralFeast ni de String.
    /// Appelé une seule fois (AOT). La Forge garantit l'unicité des Commemorationes (INV-5).
    /// Au plus 2 entrées par jour sont insérées — le second slot est pour les co-occurrences
    /// résolues à la compilation.
    ///
    /// INTERFACE CORRECTE vs. v2.0 :
    ///   v2.0 (incorrect) : `defs: &[FeastDefinition]` — accès à `def.date`, champ fantôme.
    ///   v2.1 (corrigé)   : `defs: &[(FeastDefinition, u16)]` — day_of_year 1-based fourni
    ///                       par la Forge comme second élément de la paire.
    ///
    /// RETOUR :
    ///   Ok(Self) — construction réussie.
    ///   Err(u16) — day_of_year (1-based) du slot qui a débordé (3e fête pour le même jour).
    ///              La Forge DOIT rejeter ce cas avec `RegistryError::SanctoralSlotOverflow`
    ///              avant d'appeler cette fonction (V1 garantit max 2 fêtes par jour).
    ///              En debug : `debug_assert!` échoue. En release : l'erreur est remontée
    ///              sans corruption silencieuse.
    pub fn new(defs: &[(FeastDefinition, u16)]) -> Result<Self, u16> {
        let mut layer = Self {
            fixed_feasts: [None; 366],
        };
        for &(def, day_of_year) in defs {
            // Conversion 1-based day_of_year → index 0-based
            let doy = day_of_year as usize;
            if doy == 0 || doy > 366 { continue; }
            let idx = doy - 1;

            let slot = &mut layer.fixed_feasts[idx];
            // Encodage AOT : FeastDefinition → FeastDefinitionPacked avant stockage.
            let packed = feast_definition_pack(def);
            match slot {
                None => *slot = Some([Some(packed), None]),
                Some(ref mut pair) if pair[1].is_none() => pair[1] = Some(packed),
                _ => {
                    // 3e fête ce jour : la Forge n'a pas validé correctement (V1 / INV-5).
                    // Remontée sans corruption silencieuse — ni eprintln!, ni unreachable!().
                    debug_assert!(false, "SanctoralLayer: 3e fête pour le jour {}", day_of_year);
                    return Err(day_of_year);
                }
            }
        }
        Ok(layer)
    }

    /// Accès O(1) garanti. Zéro allocation. Zéro pointer-chasing.
    ///
    /// Retourne la fête de précédence minimale (force d'éviction la plus haute)
    /// parmi les candidats du slot, déjà triée à la Forge.
    /// La comparaison de précédence s'opère directement sur les bits [17:14] du packed
    /// (zéro décodage intermédiaire si un seul candidat est présent).
    #[inline(always)]
    pub fn get_feast(&self, year: i16, day_of_year: u16) -> Option<FeastDefinition> {
        let doy_idx = (day_of_year as usize).saturating_sub(1);
        if doy_idx >= 366 {
            return None;
        }

        // Vérification bissextile pour le slot 366
        if day_of_year == 366 && !is_leap_year(year as i32) {
            return None;
        }

        self.fixed_feasts[doy_idx].and_then(|pair| {
            // Comparaison de précédence sur bits [17:14] — zéro décodage complet si un seul slot.
            match (pair[0], pair[1]) {
                (Some(a), Some(b)) => {
                    // Précédence = bits [17:14]. Valeur minimale = force d'éviction maximale.
                    let prec_a = (a.get() >> 14) & 0xF;
                    let prec_b = (b.get() >> 14) & 0xF;
                    if prec_a <= prec_b {
                        Some(feast_definition_unpack(a))
                    } else {
                        Some(feast_definition_unpack(b))
                    }
                }
                (Some(a), None) => Some(feast_definition_unpack(a)),
                (None, Some(b)) => Some(feast_definition_unpack(b)),
                (None, None)    => None,
            }
        })
    }
}

/// Convertit (mois, jour) en index 0-based dans le tableau fixed_feasts.
///
/// Utilise l'année commune (28 jours en février) pour l'indexation.
/// Les fêtes du 29 février sont gérées par le slot du 29 février (index 59).
#[inline]
fn month_day_to_slot(month: u8, day: u8) -> usize {
    let days_before = [0u16, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    if month == 0 || month > 12 || day == 0 { return 366; }  // invalide
    days_before[(month - 1) as usize] as usize + (day - 1) as usize
}

fn day_of_year_to_month_day(day_of_year: u16, is_leap: bool) -> (u8, u8) {
    let days_per_month = if is_leap {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut remaining = day_of_year;
    for (month_idx, &days) in days_per_month.iter().enumerate() {
        if remaining <= days {
            return ((month_idx + 1) as u8, remaining as u8);
        }
        remaining -= days;
    }

    // SAFETY : Cette branche est mathématiquement inatteignable.
    //
    // Preuve :
    //   sum(days_per_month, année commune)   = 365
    //   sum(days_per_month, année bissextile) = 366
    //
    //   Tous les appelants de cette fonction valident `day_of_year` ∈ [1, 366]
    //   AVANT l'appel (voir index_day Option<usize> — correction P2, et les
    //   guards doy ∈ [1, 366] dans kal_read_day / kal_compute_day).
    //
    //   Si day_of_year ∈ [1, 366], la boucle retourne nécessairement lors d'une
    //   des 12 itérations avant d'épuiser le tableau. `remaining` décroît
    //   strictement à chaque tour (days ≥ 28 > 0), et
    //   sum(days_per_month) ≥ day_of_year garantit qu'un slot est atteint.
    //
    //   `unreachable!()` (P1 context) est ici remplacé par `unreachable_unchecked`
    //   (R1 — v2.3) pour éliminer le panic en release tout en préservant la
    //   détection en debug via debug_assert en amont (non nécessaire ici car
    //   l'inatteignabilité est prouvée par le type système + les invariants P2).
    unsafe { core::hint::unreachable_unchecked() }
}
```

### 4.4 Precedence Resolver

**Règle d'Éviction — Hiérarchie Numérique Inverse**

La `Precedence` est une échelle d'éviction à lecture inverse : **une valeur numérique plus faible représente une priorité liturgique plus haute**.

```
Precedence 0 (TriduumSacrum)  →  priorité maximale  →  évince toute autre fête
Precedence 12 (FeriaePerAnnum) →  priorité minimale  →  évincé par toute autre fête
```

En cas de collision entre deux candidats sur le même jour :

```
winner = argmin(precedence as u8)   ← valeur numérique minimale
```

**Cas critique documenté — Ioannes Paulus II (canonisation 2014) :**

Après canonisation, `ioannis_pauli_ii` obtient `Precedence=12` (Memoria ad libitum, Calendarium Generale). Une Feria ordinaire du Temps Ordinaire a aussi `Precedence=12`. Il n'y a donc **pas de collision** : les deux candidats ont la même précédence, et c'est le candidat temporal qui prime par convention (§4.4 résolution d'égalité). Ce comportement est attendu pour une Memoria facultative : l'office local peut librement la célébrer ou non.

Ce cas illustre que `Precedence=12` n'est pas un "rang bas" dans le sens péjoratif — c'est la valeur correcte pour une Memoria ad libitum. La confusion vient de l'opposition intuitive canonisation → élévation de rang. En réalité, une Memoria universelle facultative peut coexister avec une Memoria nationale obligatoire (Precedence=11) dans le même calendrier sans contradiction.

**Règles de Résolution (Ordre Strict)** :

La résolution est une comparaison purement numérique sur l'axe `Precedence`. Valeur plus faible = force d'éviction plus élevée. Aucune logique sémantique n'intervient dans la collision.

```rust
pub struct PrecedenceResolver;

impl PrecedenceResolver {
    /// Résout les conflits entre fêtes selon la Tabella dierum liturgicorum
    pub fn resolve(
        &self,
        year: i16,
        season: &Season,
        temporal: Option<FeastDefinition>,
        sanctoral: Option<FeastDefinition>,
        day_of_year: u16,
        boundaries: &SeasonBoundaries,
    ) -> Result<(Precedence, Nature, Color, u32), DomainError> {
        // Résolution : sélection du candidat à Precedence numérique minimale.
        // En cas d'égalité : le candidat temporal prime sur le sanctoral.
        let winner = match (temporal, sanctoral) {
            (Some(t), Some(s)) => {
                if (t.precedence as u8) <= (s.precedence as u8) { t } else { s }
            }
            (Some(t), None) => t,
            (None, Some(s)) => s,
            (None, None) => {
                // Feria par défaut : Precedence selon saison
                let prec = default_precedence(season, is_sunday(year as i32, day_of_year));
                return Ok((prec, Nature::Feria, season_default_color(season), 0));
            }
        };

        Ok((winner.precedence, winner.nature, winner.color, winner.id))
    }
}

fn default_precedence(season: &Season, is_sunday: bool) -> Precedence {
    if is_sunday {
        match season {
            Season::TempusAdventus
            | Season::TempusQuadragesimae
            | Season::TempusPaschale => Precedence::DominicaePrivilegiataeMaior,
            _ => Precedence::DominicaePerAnnum,
        }
    } else {
        match season {
            Season::TempusAdventus => Precedence::FeriaeAdventusEtOctavaNativitatis,
            Season::TempusNativitatis => Precedence::FeriaeAdventusEtOctavaNativitatis,
            Season::TempusQuadragesimae | Season::DiesSancti => Precedence::FeriaePrivilegiataeMaior,
            Season::TriduumPaschale => Precedence::TriduumSacrum,
            _ => Precedence::FeriaePerAnnumEtMemoriaeAdLibitum,
        }
    }
}

fn season_default_color(season: &Season) -> Color {
    match season {
        Season::TempusOrdinarium => Color::Viridis,
        Season::TempusAdventus => Color::Violaceus,
        Season::TempusNativitatis => Color::Albus,
        Season::TempusQuadragesimae => Color::Violaceus,
        Season::TriduumPaschale => Color::Albus,
        Season::TempusPaschale => Color::Albus,
        Season::DiesSancti => Color::Rubeus,
    }
}

fn is_sunday(year: i32, day_of_year: u16) -> bool {
    // Implémentation complète : algorithme de Tomohiko Sakamoto
    // Voir roadmap section 1.2 pour le code détaillé et les tests.
    // Signature : (year: i32, day_of_year: u16) — l'année est requise par l'algorithme.
    todo!("roadmap §1.2")
}
```

---

## 5. Forge (Génération AOT de Fenêtre Optimisée)

### Philosophie

La Forge génère un **Kalendarium** (fichier `.kald`) pour une **fenêtre temporelle choisie par l'utilisateur**. Ce choix est stratégique : il détermine quelles années bénéficient de l'optimisation Fast Path (<100ns) vs Slow Path (<10µs).

**Paramètres de Fenêtre** :

```toml
# config.toml
[calendar]
start_year = 2025      # Début de la fenêtre optimisée
year_count = 300       # Durée de la fenêtre (2025-2324)

# Exemples de stratégies :
# - Application mobile contemporaine : 2000-2100 (100 ans)
# - Serveur avec fenêtre glissante : année_courante-50 à +250
# - Archive historique complète : 1583-2025 (442 ans)
# - Calendrier perpétuel moderne : 1900-2200 (300 ans)
```

**Contraintes** :

- Fenêtre dans [1583, 4099] (calendrier grégorien valide)
- Taille fichier : 16 octets + (year_count × 1464 octets)
- Génération : ~10s pour 300 ans sur machine standard

**Hors fenêtre** : Le runtime bascule automatiquement sur le Slow Path, transparent pour l'utilisateur.

### 5.1 Configuration des Règles Liturgiques

**IMPORTANT** : La spécification technique v2.0 définit l'**architecture et les contrats** du système, mais **ne spécifie pas le contenu liturgique exhaustif**.

**Responsabilité de l'Opérateur** :

L'opérateur doit fournir la configuration complète des règles liturgiques via fichiers de configuration (TOML, JSON, ou YAML) comprenant :

1. **Règles Temporelles** :
   - Fêtes mobiles en relation avec Pâques (Ascension, Pentecôte, etc.)
   - Règles de déplacement (Saint-Joseph, Annonciation sur dimanche, etc.)
   - Semaines liturgiques (Carême, Avent, etc.)

2. **Règles Sanctorales** :
   - Fêtes fixes des saints (sanctoral universel, national, diocésain)
   - Patronages et célébrations locales
   - Mémoires obligatoires et facultatives

3. **Règles de Précédence** :
   - Ordre de priorité entre célébrations concurrentes
   - Exceptions liturgiques (Triduum Pascal, etc.)

4. **Fêtes Votives** (facultatif) :
   - Messes votives selon les circonstances
   - Communs des saints

**Format de Configuration** (exemple schématique) :

```toml
# config.toml
[calendar]
start_year = 2025
year_count = 300

# Règles temporelles
[[temporal_rules]]
type = "relative_to_easter"
name = "Ascensio Domini"
offset_days = 39
precedence = 1
nature = "sollemnitas"
color = "albus"

[[temporal_rules]]
type = "displacement"
name = "Sanctus Ioseph"
base_date = { month = 3, day = 19 }
displaced_if = ["palm_sunday", "holy_week"]
displaced_to = "next_available"

# Règles sanctorales
[[sanctoral_feasts]]
date = { month = 1, day = 1 }
name = "Sancta Maria, Mater Dei"
precedence = 4
nature = "sollemnitas"
color = "albus"
scope = "universal"

[[sanctoral_feasts]]
date = { month = 7, day = 14 }
name = "Fête nationale de la France"
precedence = 9
nature = "festum"
color = "albus"
scope = "national"
region = "FR"
```

**Implémentation** :

Le `CalendarBuilder` charge cette configuration et construit les couches `TemporalLayer` et `SanctoralLayer` :

```rust
impl CalendarBuilder {
    pub fn new(config: Config) -> Result<Self, RuntimeError> {
        let feast_registry = FeastRegistry::load(&config.registry_path)?;

        // Construction du Slow Path depuis la configuration
        let slow_path = SlowPath::from_config(&config)?;

        Ok(Self {
            config,
            feast_registry,
            slow_path,
            cache: BTreeMap::new(),
        })
    }
}

impl SlowPath {
    /// Construit le Slow Path depuis des slices pré-résolues (appelé par la Forge).
    ///
    /// Config est une structure std (Forge) — ce bloc vit dans liturgical-calendar-forge, pas dans core.
    pub fn from_config(config: &Config) -> Result<Self, RuntimeError> {
        let temporal = TemporalLayer::new(&config.temporal_defs)?;     // &[FeastDefinition]
        let sanctoral = SanctoralLayer::new(&config.sanctoral_defs)?;  // &[FeastDefinition]
        let precedence = PrecedenceResolver::new(&config.precedence)?;

        Ok(Self {
            temporal,
            sanctoral,
            precedence,
        })
    }
}
```

**Fonctions de normalisation (Forge uniquement — std requis)**

Ces fonctions vivent dans `liturgical-calendar-forge/src/config.rs`. Elles opèrent sur des `&str` issus de fichiers TOML/YAML et sont exclues du crate `core` (`no_std`). Le type d'erreur est `RegistryError` (couche forge), pas `DomainError`.

```rust
// liturgical-calendar-forge/src/config.rs
// std disponible ici — alloc autorisée

/// Convertit une chaîne de configuration en Color liturgique.
///
/// Accepte les variantes latines, anglaises et françaises pour
/// faciliter la saisie par les opérateurs.
///
/// NOTE ARCHITECTURALE : cette fonction est dans forge, pas dans core.
/// `to_lowercase()` alloue. `core` (no_std) n'en a pas besoin :
/// il n'opère que sur des u8 via Color::try_from_u8.
fn normalize_color(input: &str) -> Result<Color, RegistryError> {
    match input.to_lowercase().as_str() {
        "albus"    | "white" | "blanc"  => Ok(Color::Albus),
        "rubeus"   | "red"   | "rouge"  => Ok(Color::Rubeus),
        "viridis"  | "green" | "vert"   => Ok(Color::Viridis),
        "violaceus"| "violet"           => Ok(Color::Violaceus),
        "roseus"   | "rose"             => Ok(Color::Roseus),
        "niger"    | "black" | "noir"   => Ok(Color::Niger),
        _ => Err(RegistryError::UnknownColorString(input.to_string())),
    }
}

/// Convertit une chaîne de configuration en Precedence liturgique.
fn normalize_precedence(input: u8) -> Result<Precedence, RegistryError> {
    Precedence::try_from_u8(input)
        .map_err(|_| RegistryError::InvalidPrecedenceValue(input))
}

/// Convertit une chaîne de configuration en Nature liturgique.
fn normalize_nature(input: Option<&str>) -> Result<Nature, RegistryError> {
    match input {
        Some("sollemnitas")   => Ok(Nature::Sollemnitas),
        Some("festum")       => Ok(Nature::Festum),
        Some("memoria")      => Ok(Nature::Memoria),
        Some("feria") | None => Ok(Nature::Feria),
        Some("commemoratio") => Ok(Nature::Commemoratio),
        Some(other) => Err(RegistryError::UnknownNatureString(other.to_string())),
    }
}
```

**Hors Scope v1.0** :

La spécification v2.0 se concentre sur :

- Architecture du système
- Format binaire `.kald`
- Contrats des APIs
- Pipeline de génération

Le contenu liturgique exhaustif (toutes les fêtes votives, règles de déplacement complexes, etc.) sera fourni dans des **fichiers de configuration séparés** maintenus par l'opérateur ou la communauté.

**Extensions Futures** :

- v2.x : Bibliothèque de configurations pré-établies (Rite Romain Ordinaire, Extraordinaire, etc.)
- v3.x : Éditeur visuel de règles liturgiques
- v4.x : Validation automatique contre le Calendrier Romain Général

#### 5.1.1 Architecture des Règles — Pivot DOD (v2.1)

**Invariant Fondamental** : `liturgical-calendar-core` est une **feuille pure** (leaf crate). Il ne dépend d'aucune crate de règles. Son seul type de données de règles est `FeastDefinition` (Copy, Pod, no_std).

**Partition des Types** :

| Type                    | Crate                                       | Raison                                               |
| ----------------------- | ------------------------------------------- | ---------------------------------------------------- |
| `FeastDefinition`       | `liturgical-calendar-core`                  | Copy, Pod, stack uniquement — interface du SlowPath  |
| `PrecedenceData`        | `liturgical-calendar-core`                  | Tableau statique `[u8; 13]` — figé après freeze v2.0 |
| `TemporalRule`          | `liturgical-calendar-forge` / `rules-roman` | Contient `String`, `Vec` — allocation std            |
| `SanctoralFeast`        | `liturgical-calendar-forge` / `rules-roman` | Contient `String` — allocation std                   |
| `RuleProvider` (trait)  | `liturgical-calendar-forge`                 | Abstraction Forge uniquement — jamais dans le core   |
| `HardcodedRuleProvider` | `liturgical-calendar-rules-roman`           | `Vec<TemporalRule>`, fournisseur std pour la Forge   |

**Interface du SlowPath dans `liturgical-calendar-core`** :

```rust
// liturgical-calendar-core/src/slow_path.rs — no_std, no_alloc

impl SlowPath {
    /// Construit le SlowPath depuis des slices de paires (FeastDefinition, day_of_year) pré-résolues.
    ///
    /// INVARIANT : l'appelant (Forge ou rules-roman) résout les TemporalRule → (FeastDefinition, u16)
    /// AVANT d'appeler cette fonction. Le core ne voit jamais String ni Vec.
    ///
    /// `temporal`  : paires (fête mobile, day_of_year 1-based) résolues pour une année-type (AOT)
    /// `sanctoral` : paires (fête fixe, day_of_year 1-based) indexées par jour de l'année (AOT)
    pub fn new(
        temporal: &[(FeastDefinition, u16)],
        sanctoral: &[(FeastDefinition, u16)],
        precedence: &PrecedenceData,
    ) -> Self {
        Self {
            temporal:  TemporalLayer::new(temporal),
            sanctoral: SanctoralLayer::new(sanctoral).expect("Forge invariant V1: max 2 fêtes/jour"),
            precedence: PrecedenceResolver::new(precedence),
        }
    }
}
```

**Données Statiques pour le FFI (`compute_day_static`)** :

`liturgical-calendar-core` embarque les `FeastDefinition` du Rite Romain Ordinaire comme constantes statiques compilées. Ces tableaux sont produits une fois par la Forge lors de la phase d'initialisation, puis gelés dans le binaire. Zéro allocation à l'exécution.

```rust
// liturgical-calendar-core/src/static_rules.rs — no_std, no_alloc
//
// Tableaux statiques des FeastDefinition du Rite Romain Ordinaire.
// Produits par la Forge (compile-rules) — jamais modifiés après freeze.
// Référencés uniquement par compute_day_static() (point d'entrée FFI sans état).

pub(crate) static STATIC_TEMPORAL_DEFS: &[FeastDefinition] = &[
    FeastDefinition { id: 0x00001, precedence: Precedence::SollemnitatesFixaeMaior,
                      nature: Nature::Sollemnitas, color: Color::Albus },  // Ascensio Domini
    FeastDefinition { id: 0x00002, precedence: Precedence::SollemnitatesFixaeMaior,
                      nature: Nature::Sollemnitas, color: Color::Rubeus }, // Pentecostes
    // ... ~50 définitions au total
];

pub(crate) static STATIC_SANCTORAL_DEFS: &[FeastDefinition] = &[
    FeastDefinition { id: 0x01001, precedence: Precedence::SollemnitatesGenerales,
                      nature: Nature::Sollemnitas, color: Color::Albus },  // Sancta Maria, Mater Dei (1 ian.)
    // ... sanctoral universel
];

pub(crate) static STATIC_PRECEDENCE: PrecedenceData = PrecedenceData::roman_ordinary();

/// Point d'entrée du Slow Path pour le FFI (`kal_compute_day`).
///
/// Utilise les tableaux statiques compilés du Rite Romain Ordinaire.
/// Zéro allocation au runtime. Zéro état mutable exposé.
///
/// CORRECTION v2.1 (audit P5) :
///   v2.0 reconstruisait `SlowPath` à chaque appel FFI : deux boucles d'initialisation
///   (~450 itérations chacune) par appel — coût de performance non nul.
///   v2.1 : `core::sync::OnceLock<SlowPath>` — initialisation paresseuse à la première
///   invocation, lecture lock-free ensuite. Disponible `no_std` depuis Rust 1.80.
static STATIC_SLOW_PATH: core::sync::OnceLock<SlowPath> = core::sync::OnceLock::new();

pub(crate) fn compute_day_static(year: i16, day_of_year: u16) -> Result<DayPacked, DomainError> {
    let slow = STATIC_SLOW_PATH.get_or_init(|| {
        SlowPath::new(STATIC_TEMPORAL_DEFS, STATIC_SANCTORAL_DEFS, &STATIC_PRECEDENCE)
    });
    slow.compute(year, day_of_year).map(DayPacked::from)
}
```

**`HardcodedRuleProvider` dans `liturgical-calendar-rules-roman`** :

Ce provider est la couche std de la Forge. Il convertit `TemporalRule` → `FeastDefinition` lors du build AOT, et transmet le résultat à `SlowPath::new`.

```rust
// liturgical-calendar-rules-roman/src/hardcoded.rs — std (Forge uniquement)

pub struct HardcodedRuleProvider {
    temporal: Vec<TemporalRule>,    // std — résolution AOT → FeastDefinition
    sanctoral: Vec<SanctoralFeast>, // std — résolution AOT → FeastDefinition
}

impl HardcodedRuleProvider {
    pub fn new_roman_rite_ordinary() -> Self {
        Self {
            temporal: vec![
                TemporalRule {
                    id: 0x00001,
                    name: "Ascensio Domini".to_string(),  // Latin canonique
                    rule_type: TemporalRuleType::RelativeToEaster { offset_days: 39 },
                    precedence: Precedence::SollemnitatesFixaeMaior,
                    nature: Nature::Sollemnitas,
                    color: Color::Albus,
                },
                TemporalRule {
                    id: 0x00002,
                    name: "Pentecostes".to_string(),  // Latin canonique
                    rule_type: TemporalRuleType::RelativeToEaster { offset_days: 49 },
                    precedence: Precedence::SollemnitatesFixaeMaior,
                    nature: Nature::Sollemnitas,
                    color: Color::Rubeus,
                },
                // ... ~50 règles au total
            ],
            sanctoral: vec![
                SanctoralFeast {
                    date: (1, 1),
                    name: "Sancta Maria, Mater Dei".to_string(),  // Latin canonique
                    precedence: Precedence::SollemnitatesGenerales,
                    nature: Nature::Sollemnitas,
                    color: Color::Albus,
                    scope: FeastScope::Universal,
                },
                // ... sanctoral complet
            ],
        }
    }

    /// Résout les TemporalRule en FeastDefinition (AOT — appelé par la Forge uniquement).
    pub fn resolve_temporal(&self) -> Vec<FeastDefinition> {
        self.temporal.iter().map(|r| FeastDefinition {
            id: r.id,
            precedence: r.precedence,
            nature: r.nature,
            color: r.color,
        }).collect()
    }

    /// Résout les SanctoralFeast en FeastDefinition (AOT — appelé par la Forge uniquement).
    pub fn resolve_sanctoral(&self) -> Vec<FeastDefinition> {
        self.sanctoral.iter().map(|f| FeastDefinition {
            id: f.id,
            precedence: f.precedence,
            nature: f.nature,
            color: f.color,
        }).collect()
    }
}

// Usage Forge — construction du SlowPath sans fuite vers le core :
let provider = HardcodedRuleProvider::new_roman_rite_ordinary();
let temporal_defs = provider.resolve_temporal();
let sanctoral_defs = provider.resolve_sanctoral();
let slow_path = SlowPath::new(&temporal_defs, &sanctoral_defs, &PrecedenceData::roman_ordinary());
```

**Pipeline AOT** :

```yaml
# config/roman-rite-ordinary.yaml
temporal_rules:
  - id: 0x00001
    name: "Ascensio Domini"
    type: relative_to_easter
    offset_days: 39
    precedence: 1
    nature: sollemnitas
    color: albus

  - id: 0x00002
    name: "Pentecostes"
    type: relative_to_easter
    offset_days: 49
    precedence: 1
    nature: sollemnitas
    color: rubeus

sanctoral_feasts:
  - date: [1, 1]
    name: "Sancta Maria, Mater Dei"
    precedence: 4
    nature: sollemnitas
    color: albus
    scope: universal
```

```bash
# Compilation AOT (intégrée dans liturgical-calendar-forge)
$ liturgical-calendar-forge compile-rules \
    --input config/roman-rite-ordinary.yaml \
    --output src/rules_generated.rs

# Validation stricte
✓ 50 règles temporelles validées (Latin canonique vérifié)
✓ 365 fêtes sanctorales validées
✓ Aucune collision de FeastID
✓ Précédence cohérente
→ Code Rust généré : src/rules_generated.rs (10,234 lignes)
```

**Garantie Structurelle** :

> _"Le moteur calcule sur des `FeastDefinition` (Copy, Pod). Les règles riches (`TemporalRule`, `String`) sont réservées à la Forge. La frontière entre les deux ne se traverse qu'à la résolution AOT."_

`cargo tree -p liturgical-calendar-core` doit produire un arbre **strictement vide** (zéro dépendance externe). Toute dépendance entrante dans `liturgical-calendar-core` constitue une violation d'INV-W4.

### 5.2 Architecture Pipeline

```rust
pub struct CalendarBuilder {
    /// Configuration source (années, couches, règles)
    config: Config,

    /// Registry des FeastID (évite collisions)
    feast_registry: FeastRegistry,

    /// Slow Path pour génération
    slow_path: SlowPath,

    /// Cache des jours calculés.
    /// IMPORTANT : BTreeMap pour déterminisme (ordre de sérialisation garanti).
    /// Type : DayPacked — cohérent avec le Data Body du .kald (u32 par entrée).
    /// La Forge calcule via Day (SlowPath) puis convertit immédiatement en DayPacked.
    /// Conforme roadmap §2.2 (correction B4).
    cache: BTreeMap<(i16, u16), DayPacked>,
}

impl CalendarBuilder {
    pub fn new(config: Config) -> Result<Self, RuntimeError> {
        let feast_registry = FeastRegistry::load(&config.registry_path)?;
        let slow_path = SlowPath::from_config(&config)?;

        Ok(Self {
            config,
            feast_registry,
            slow_path,
            cache: BTreeMap::new(),
        })
    }

    /// Génère le calendrier complet.
    pub fn build(mut self) -> Result<Calendar, RuntimeError> {
        let start_year = self.config.start_year;
        let end_year = start_year + self.config.year_count as i16;

        // Validation des bornes
        // ERREUR : DomainError::YearOutOfBounds — une borne hors du domaine grégorien
        // canonique est une violation de domaine, pas une erreur d'I/O.
        // Conforme roadmap §2.2 (correction B3).
        if start_year < 1583 || end_year > 4099 {
            return Err(RuntimeError::Domain(DomainError::YearOutOfBounds(start_year)));
        }

        for year in start_year..end_year {
            let max_day = if is_leap_year(year as i32) { 366 } else { 365 };

            for day in 1..=max_day {
                // SlowPath produit Day (logique) → converti immédiatement en DayPacked
                let liturgical_day: DayPacked = self.slow_path.compute(year, day)
                    .map(DayPacked::from)
                    .map_err(RuntimeError::Domain)?;
                self.cache.insert((year, day), liturgical_day);
            }
            // Jour 366 des années non-bissextiles : absent du cache.
            // write_kald écrit 0xFFFFFFFF (DayPacked::invalid) pour les entrées manquantes.
            // get_day() retourne DayPacked::invalid() avant même de lire le fichier.
        }

        Ok(Calendar {
            start_year,
            year_count: self.config.year_count,
            data: self.cache,
        })
    }
}
```

### 5.3 Sérialisation Binaire

```rust
pub struct Calendar {
    pub start_year: i16,
    pub year_count: u16,
    /// Type : DayPacked — compact, cohérent avec le Data Body du .kald.
    /// Conforme roadmap §2.2 (correction B4/C1).
    pub data: BTreeMap<(i16, u16), DayPacked>,
}

impl Calendar {
    /// Écrit le fichier .kald (déterministe, sans UB)
    pub fn write_kald(&self, path: &str) -> Result<(), IoError> {
        let mut file = File::create(path)?;

        // Construction du header
        let header = Header {
            magic: *b"KALD",
            version: 1,
            start_year: self.start_year,
            year_count: self.year_count,
            flags: 0,  // Pas de compression/checksum pour v1
            _padding: [0, 0, 0, 0],
        };

        // Sérialisation du header (endianness native, sans UB)
        file.write_all(&header.to_bytes())?;

        // Data Body (ordre strict : années puis jours)
        for year in self.start_year..(self.start_year + self.year_count as i16) {
            for day in 1..=366 {
                let packed: u32 = self
                    .data
                    .get(&(year, day))
                    // DayPacked : extraction zero-cost via as_u32()
                    .map(|dp| dp.as_u32())
                    // Padding jour 366 pour années non-bissextiles : 0xFFFFFFFF
                    // (DayPacked::invalid — Precedence=15 hors domaine, non décodable)
                    .unwrap_or(0xFFFFFFFF_u32);
                // Little-Endian canonique (P3/R5) : cohérent avec from_le_bytes() dans kal_read_day.
                file.write_all(&packed.to_le_bytes())?;
            }
        }

        Ok(())
    }
}
```

### 5.4 Test de Déterminisme Cross-Platform

```yaml
# .github/workflows/determinism.yml
name: Intra-LE Determinism

on: [push, pull_request]

jobs:
  build-linux:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - run: cargo build --release --bin liturgical-calendar-forge
      - run: ./target/release/liturgical-calendar-forge build --config test.toml
      - run: sha256sum france.kald > linux-hash.txt
      - uses: actions/upload-artifact@v3
        with:
          name: linux-build
          path: |
            france.kald
            linux-hash.txt

  build-macos:
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v3
      - run: cargo build --release --bin liturgical-calendar-forge
      - run: ./target/release/liturgical-calendar-forge build --config test.toml
      - run: shasum -a 256 france.kald > macos-hash.txt
      - uses: actions/upload-artifact@v3
        with:
          name: macos-build
          path: |
            france.kald
            macos-hash.txt

  build-windows:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v3
      - run: cargo build --release --bin liturgical-calendar-forge
      - run: ./target/release/liturgical-calendar-forge.exe build --config test.toml
      - run: certutil -hashfile france.kald SHA256 > windows-hash.txt
      - uses: actions/upload-artifact@v3
        with:
          name: windows-build
          path: |
            france.kald
            windows-hash.txt

  compare:
    needs: [build-linux, build-macos, build-windows]
    runs-on: ubuntu-latest
    steps:
      - uses: actions/download-artifact@v3
      - name: Compare SHA-256
        run: |
          LINUX_HASH=$(cat linux-build/linux-hash.txt | awk '{print $1}')
          MACOS_HASH=$(cat macos-build/macos-hash.txt | awk '{print $1}')
          WINDOWS_HASH=$(cat windows-build/windows-hash.txt | awk '{print $1}')

          if [ "$LINUX_HASH" = "$MACOS_HASH" ] && [ "$MACOS_HASH" = "$WINDOWS_HASH" ]; then
            echo "✓ Determinism verified across platforms"
            exit 0
          else
            echo "✗ Hash mismatch detected"
            echo "Linux:   $LINUX_HASH"
            echo "macOS:   $MACOS_HASH"
            echo "Windows: $WINDOWS_HASH"
            exit 1
          fi
```

---

## 6. Format Strings (.lits)

### 6.1 Structure Multi-Langue

**Layout Fichier** :

```
[Header - 12 octets]
magic: [u8; 4]      // "LITS"
version: u16        // 1
lang_code: [u8; 2]  // "fr", "en", "la"
entry_count: u32    // Nombre d'entrées

[Index Section - N × 12 octets]
struct IndexEntry {
    feast_id: u32,         // FeastID correspondant
    offset: u32,           // Offset vers le texte (depuis début Data Section)
    length: u32,           // Longueur du texte UTF-8
}

[Data Section - Textes UTF-8]
// Strings encodés en UTF-8, concaténés séquentiellement
```

### 6.2 Provider de Strings

```rust
use memmap2::Mmap;
use std::collections::HashMap;

pub struct StringProvider {
    /// Memory-mapped .lits
    mmap: Mmap,

    /// Index FeastID → (offset, length)
    index: HashMap<u32, (usize, usize)>,
}

impl StringProvider {
    pub fn load(path: &str) -> Result<Self, IoError> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };

        // Validation header
        let magic = &mmap[0..4];
        if magic != b"LITS" {
            return Err(IoError::InvalidMagic(*b"LITS"));
        }

        let entry_count = u32::from_le_bytes([mmap[8], mmap[9], mmap[10], mmap[11]]) as usize;

        // Construction de l'index
        let mut index = HashMap::with_capacity(entry_count);
        let index_start = 12;

        for i in 0..entry_count {
            let entry_offset = index_start + (i * 12);
            // Little-Endian canonique (R5 — v2.3) : SHA-256 reproductible cross-platform.
            let feast_id = u32::from_le_bytes([
                mmap[entry_offset],
                mmap[entry_offset + 1],
                mmap[entry_offset + 2],
                mmap[entry_offset + 3],
            ]);

            let offset = u32::from_le_bytes([
                mmap[entry_offset + 4],
                mmap[entry_offset + 5],
                mmap[entry_offset + 6],
                mmap[entry_offset + 7],
            ]) as usize;

            let length = u32::from_le_bytes([
                mmap[entry_offset + 8],
                mmap[entry_offset + 9],
                mmap[entry_offset + 10],
                mmap[entry_offset + 11],
            ]) as usize;

            index.insert(feast_id, (offset, length));
        }

        Ok(Self { mmap, index })
    }

    /// Récupère le nom d'une fête (zero-copy)
    pub fn get_feast_name(&self, feast_id: u32) -> Option<&str> {
        self.index.get(&feast_id).and_then(|(offset, length)| {
            let data_section_start = 12 + (self.index.len() * 12);
            let start = data_section_start + offset;
            let end = start + length;

            std::str::from_utf8(&self.mmap[start..end]).ok()
        })
    }
}
```

---

## 6.3 API Vectorielle — `kal_scan_precedence` (SIMD Readiness)

Le Data Body du `.kald` est un tableau de `u32` LE contigus. Sa structure autorise un traitement
vectoriel direct (SIMD) sans transformation préalable. La fonction `kal_scan_precedence` expose
ce chemin en C et en Rust, utilisable depuis n'importe quel appelant (Zig, WASM, C embarqué).

### Signature C (`kal_engine.h`)

```c
/// Scanne le Data Body et collecte les indices des jours dont la Precedence
/// (bits [31:28] du DayPacked) est inférieure ou égale à `max_precedence`.
///
/// Opère directement sur le buffer u32 LE fourni par l'appelant.
/// Aucune allocation interne. Compatible no_std / WASM / C embarqué.
///
/// @param data_body       Pointeur vers le Data Body (après Header 16 octets).
///                        Chaque élément est un DayPacked u32 Little-Endian.
/// @param data_len        Longueur du Data Body en octets (multiple de 4 imposé).
/// @param max_precedence  Seuil d'éviction inclusif [0, 15].
///                        Un jour est sélectionné si son champ Precedence ≤ max_precedence.
///                        Valeur 0 = seuls les jours TriduumSacrum.
///                        Valeur 15 = tous les jours ayant une fête enregistrée.
/// @param out_indices     Buffer de sortie (indices u32 dans le Data Body).
///                        L'appelant alloue ce buffer ; taille recommandée = data_len / 4.
/// @param out_count       Out-param : nombre d'indices écrits dans out_indices.
/// @return                KAL_ENGINE_OK, KAL_ERR_NULL_PTR, ou KAL_ERR_INDEX_OOB
///                        si data_len % 4 != 0.
///
/// COMPLEXITÉ : O(N) scalaire, O(N/8) AVX2 (8 × u32 par registre ymm).
/// SIMD LAYOUT : DayPacked est u32 LE → chargement direct `VMOVDQU ymm, [ptr]`.
///   Precedence = bits [31:28] → `VPSRLD ymm, 28` extrait le champ pour 8 jours simultanément.
int32_t kal_scan_precedence(
    const uint32_t* data_body,
    uintptr_t       data_len,
    uint8_t         max_precedence,
    uint32_t*       out_indices,
    uintptr_t*      out_count
);
```

### Implémentation Rust (scalaire, portable)

```rust
/// Implémentation scalaire de référence.
/// Sur les cibles disposant d'AVX2 (x86_64) ou NEON (aarch64), le compilateur Rust
/// auto-vectorise cette boucle si la crate est compilée avec `target-cpu=native`
/// ou les flags `RUSTFLAGS="-C target-feature=+avx2"`.
///
/// Pour une vectorisation explicite (garantie de débit), une implémentation
/// via `std::simd` (nightly) ou `wide` (stable) est planifiée en Phase 6.
#[no_mangle]
pub unsafe extern "C" fn kal_scan_precedence(
    data_body:      *const u32,
    data_len:       usize,
    max_precedence: u8,
    out_indices:    *mut u32,
    out_count:      *mut usize,
) -> i32 {
    if data_body.is_null() || out_indices.is_null() || out_count.is_null() {
        return KAL_ERR_NULL_PTR;
    }
    if data_len % 4 != 0 {
        return KAL_ERR_INDEX_OOB;  // data_len doit être un multiple de 4 (u32 × N)
    }

    let n = data_len / 4;
    let data = core::slice::from_raw_parts(data_body, n);
    let out  = core::slice::from_raw_parts_mut(out_indices, n);  // borne max = n

    let mut count: usize = 0;
    let threshold = (max_precedence as u32) & 0xF;

    for (i, &packed) in data.iter().enumerate() {
        // DayPacked layout : Precedence = bits [31:28].
        // 0xFFFFFFFF = sentinelle padding (slot vide) — toujours filtré (Precedence = 0xF = 15).
        let prec = packed >> 28;
        if prec <= threshold && packed != 0xFFFF_FFFF {
            // SAFETY : out a été alloué pour n éléments par l'appelant.
            // L'invariant count < n est garanti car count ≤ i < n.
            *out.get_unchecked_mut(count) = i as u32;
            count += 1;
        }
    }

    *out_count = count;
    KAL_ENGINE_OK
}
```

### Exemple d'utilisation — scan d'un siècle

```c
// Trouver tous les jours de Precedence ≤ 2 (TriduumSacrum + Solemnités majeures) sur 100 ans.
// Data Body d'un siècle : 100 × 366 × 4 = 146 400 octets.

uint32_t* results = malloc(100 * 366 * sizeof(uint32_t));
uintptr_t count   = 0;

int32_t rc = kal_scan_precedence(
    (const uint32_t*)data_body,
    data_body_len,
    2,          // max_precedence : Solemnités majeures et supérieures
    results,
    &count
);

// count : nombre de jours sélectionnés (~400–600 sur un siècle selon le comput pascal)
// results[0..count] : indices dans le Data Body → convertibles en (year, doy) via kal_index_day
```

---

## 7. Runtime Provider

### 7.1 Architecture Fast-Slow Path

**Principe de Sélection** :

Le Provider maintient deux chemins de calcul **fonctionnellement équivalents** :

1. **Fast Path** : Lecture directe dans le fichier `.kald` si l'année est dans la fenêtre optimisée
2. **Slow Path** : Calcul algorithmique pour toutes les années [1583, 4099]

La sélection est une **optimisation de performance**, pas une correction d'erreur. Les deux chemins produisent des résultats identiques (validé par tests d'identité).

````rust
use memmap2::{Mmap, MmapOptions};
use std::fs::File;

pub struct Provider {
    /// Fast Path : données mmap (fenêtre optimisée)
    /// None = système fonctionne en Slow Path uniquement
    fast_path: Option<FastPath>,

    /// Slow Path : calcul algorithmique (système complet)
    /// Toujours présent, couvre 1583-4099
    slow_path: SlowPath,

    /// Fenêtre couverte par Fast Path (si présent)
    range: (i16, u16),  // (start_year, year_count) — u16 natif, évite le cast silencieux

    /// Provider de strings localisés
    string_provider: StringProvider,

    /// Télémétrie
    telemetry: Telemetry,
}

struct FastPath {
    /// Memory-mapped .kald
    mmap: Mmap,

    /// Slice vers le Data Body (référence statique validée)
    data: &'static [u32],

    /// Année de départ du fichier
    start_year: i16,
}

#[derive(Default)]
pub struct Telemetry {
    fast_path_hits: AtomicU64,
    slow_path_hits: AtomicU64,
    invalid_returns: AtomicU64,
    corrupted_entries: AtomicU64,
    out_of_bounds_queries: AtomicU64,
}

impl Provider {
    /// Charge le Provider avec fenêtre Fast Path
    ///
    /// Exemple :
    /// ```rust
    /// let rules = HardcodedRuleProvider::new_roman_rite_ordinary();
    /// let slow_path = SlowPath::new(rules);
    /// let provider = Provider::new("france.kald", "france.lits", slow_path)?;
    /// ```
    pub fn new(data_path: &str, lang_path: &str, slow_path: SlowPath) -> Result<Self, RuntimeError> {
        Self::new_with_strategy(data_path, lang_path, LoadStrategy::Preload, slow_path)
    }

    /// Crée un Provider sans Fast Path (Slow Path uniquement)
    ///
    /// Utile pour :
    /// - Recherche historique (années < 1900)
    /// - Systèmes contraints en mémoire
    /// - Calculs ponctuels sans optimisation
    pub fn slow_only() -> Self {
        // SlowPath::new() prend des &[FeastDefinition] pré-résolus (voir §5.1.1).
        // En production, construire via HardcodedRuleProvider::resolve_temporal/sanctoral
        // puis appeler SlowPath::new(&temporal_defs, &sanctoral_defs, &precedence).
        // Cette méthode est un placeholder — l'appelant doit construire le SlowPath
        // via Provider::with_slow_path(slow_path) ou une factory dédiée.
        unimplemented!(
            "slow_only() requiert des FeastDefinition résolues. \
             Utiliser Provider::from_slow_path(SlowPath::new(&defs, &sanctoral, &prec))."
        )
    }

    /// Charge avec stratégie mémoire explicite
    ///
    /// Le SlowPath est passé en argument car il requiert un RuleProvider
    /// (voir section 5.1.1). L'appelant construit le SlowPath via :
    ///   `SlowPath::new(HardcodedRuleProvider::new_roman_rite_ordinary())`
    pub fn new_with_strategy(
        data_path: &str,
        lang_path: &str,
        strategy: LoadStrategy,
        slow_path: SlowPath,
    ) -> Result<Self, RuntimeError> {
        // Chargement du fichier .kald
        let file = File::open(data_path)?;
        let mmap = unsafe { MmapOptions::new().map(&file)? };

        // Application de la stratégie
        match strategy {
            LoadStrategy::Lazy => {
                // Rien à faire, accès à la demande (page faults possibles)
            }
            LoadStrategy::Preload => {
                // Hint au kernel pour prefetch
                unsafe {
                    libc::madvise(
                        mmap.as_ptr() as *mut _,
                        mmap.len(),
                        libc::MADV_WILLNEED,
                    );
                }
            }
            LoadStrategy::Locked => {
                // Lock en RAM pour hard real-time (évite page faults)
                unsafe {
                    libc::mlock(
                        mmap.as_ptr() as *const _,
                        mmap.len(),
                    );
                }
            }
        }

        // Validation et construction
        // &mmap[..] : extrait un &[u8] depuis le Mmap — conforme à la signature pub fn validate_header(bytes: &[u8])
        let header = validate_header(&mmap[..])?;
        let data_body = parse_data_body(&mmap, &header)?;
        let string_provider = StringProvider::load(lang_path)?;

        Ok(Self {
            fast_path: Some(FastPath {
                mmap,
                data: data_body,
                start_year: header.start_year
            }),
            slow_path,
            range: (header.start_year, header.year_count),
            string_provider,
            telemetry: Telemetry::default(),
        })
    }

    /// Récupère un jour liturgique
    ///
    /// Sélection automatique Fast/Slow selon la fenêtre optimisée :
    /// - Si année dans [range.0, range.0+range.1) ET Fast Path disponible → Fast Path
    /// - Sinon → Slow Path
    ///
    /// Les deux chemins sont fonctionnellement équivalents.
    pub fn get_day(&self, year: i16, day_of_year: u16) -> DayPacked {
        // Validation stricte
        if day_of_year == 0 || day_of_year > 366 {
            self.telemetry.invalid_returns.fetch_add(1, Ordering::Relaxed);
            return DayPacked::invalid();
        }

        // Validation année bissextile
        if day_of_year == 366 && !is_leap_year(year as i32) {
            self.telemetry.invalid_returns.fetch_add(1, Ordering::Relaxed);
            return DayPacked::invalid();
        }

        // Tentative Fast Path (fenêtre optimisée)
        if let Some(ref fast) = self.fast_path {
            if year >= self.range.0 && year < self.range.0 + self.range.1 as i16 {
                self.telemetry.fast_path_hits.fetch_add(1, Ordering::Relaxed);

                let idx = match index_day(year, day_of_year, fast.start_year) {
                    Some(i) => i,
                    None => {
                        self.telemetry.out_of_bounds_queries.fetch_add(1, Ordering::Relaxed);
                        return DayPacked::invalid();
                    }
                };
                let packed = fast.data[idx];

                match DayPacked::try_from_u32(packed) {
                    Ok(day) => return day,
                    Err(_) => {
                        // Corruption détectée — INV-W5 : pas d'eprintln! ici.
                        // CorruptionInfo remonte à la couche appelante (Runtime).
                        self.telemetry.corrupted_entries.fetch_add(1, Ordering::Relaxed);
                        let _info = self.build_corruption_info(year, day_of_year, packed);
                        // Le Runtime (std) est responsable du diagnostic via RuntimeError::CorruptedEntry(_info)
                        return DayPacked::invalid();
                    }
                }
            }
        }

        // Slow Path (calcul algorithmique complet)
        if year >= 1583 && year <= 4099 {
            self.telemetry.slow_path_hits.fetch_add(1, Ordering::Relaxed);
            return self.slow_path.compute(year, day_of_year)
                .map(|logic| DayPacked::from(logic))
                .unwrap_or_else(|_| DayPacked::invalid());
        }

        // Hors limites calendrier grégorien canonique
        self.telemetry.out_of_bounds_queries.fetch_add(1, Ordering::Relaxed);
        DayPacked::invalid()
    }

    /// Récupère les métriques de télémétrie
    pub fn get_telemetry(&self) -> TelemetrySnapshot {
        TelemetrySnapshot {
            fast_path_hits: self.telemetry.fast_path_hits.load(Ordering::Relaxed),
            slow_path_hits: self.telemetry.slow_path_hits.load(Ordering::Relaxed),
            invalid_returns: self.telemetry.invalid_returns.load(Ordering::Relaxed),
            corrupted_entries: self.telemetry.corrupted_entries.load(Ordering::Relaxed),
            out_of_bounds_queries: self.telemetry.out_of_bounds_queries.load(Ordering::Relaxed),
        }
    }

    /// Calcul direct via Slow Path (pour tests d'identité Fast vs Slow)
    ///
    /// Exposé publiquement pour les suites de tests uniquement.
    /// Ne doit pas être utilisé dans le code de production (préférer get_day).
    pub fn compute_slow(&self, year: i16, day_of_year: u16) -> Result<Day, DomainError> {
        self.slow_path.compute(year, day_of_year)
    }

    /// Construit un CorruptionInfo structuré depuis un packed invalide.
    ///
    /// INV-W5 (v2.1) : l'Engine ne produit aucun output — `eprintln!` est INTERDIT.
    /// Cette fonction retourne un `CorruptionInfo` au lieu d'imprimer.
    /// Le diagnostic (eprintln!, log::*, etc.) est délégué à la couche Runtime (std)
    /// qui reçoit ce CorruptionInfo via le variant RuntimeError::CorruptedEntry.
    fn build_corruption_info(&self, year: i16, day_of_year: u16, packed: u32) -> CorruptionInfo {
        // index_day retourne Option — unwrap_or(0) pour les cas hors-bornes (déjà filtrés en amont).
        let offset = index_day(year, day_of_year, self.range.0);
        match DayPacked::try_from_u32(packed) {
            Ok(_) => CorruptionInfo {
                packed_value: packed,
                invalid_field: "none",
                invalid_value: 0,
                offset,
            },
            Err(mut info) => {
                info.offset = offset;
                info
            }
        }
    }
}

/// Calcule l'index dans le Data Body
///
/// CONTRAT :
///   - Retourne `Some(idx)` si (year, day_of_year, start_year) sont valides.
///   - Retourne `None` si year < start_year, ou day_of_year == 0, ou day_of_year > 366.
///
/// CORRECTIONS v2.1 (audit P2) :
///   - Arithmétique i32 : évite le wrap silencieux de `(year - start_year) as usize`
///     lorsque `year < start_year` (i16 - i16 < 0 cast en usize = underflow UB en debug,
///     valeur erronée en release).
///   - Guard `day_of_year == 0` : évite `(0u16 as usize - 1)` = underflow.
///   - Retour `Option` : l'appelant (`kal_read_day`, `build_corruption_info`) propagent
///     `KAL_ERR_INDEX_OOB` au lieu d'accéder hors bornes.
#[inline(always)]
fn index_day(year: i16, day_of_year: u16, start_year: i16) -> Option<usize> {
    if year < start_year || day_of_year == 0 || day_of_year > 366 {
        return None;
    }
    // i32 élimine tout risque de wrap : (i16 as i32 - i16 as i32) est toujours ≥ 0 ici.
    let year_offset = (year as i32 - start_year as i32) as usize;
    Some(year_offset * 366 + (day_of_year as usize - 1))
}

pub enum LoadStrategy {
    /// Lazy loading (page faults possibles)
    Lazy,

    /// Preload avec madvise WILLNEED
    Preload,

    /// Lock en RAM (hard real-time)
    Locked,
}
````

### 7.2 Sécurité Mémoire (Invariant Lifetime Documenté - Audit #5)

#### Pourquoi `&'static [u32]` et non un accès par méthode

**Problème structurel : la self-referential struct**

La forme naïve serait de stocker directement la slice dans `FastPath` :

```rust
// ❌ REFUSÉ PAR RUSTC — self-referential struct
struct FastPath {
    mmap: Mmap,
    data: &[u32],  // référence vers mmap, qui est dans la même struct
}
```

Rust refuse cette construction : `data` est une référence vers des données
_possédées_ par la même struct (`mmap`). Il est impossible d'annoter le
lifetime de `data` sans faire référence à `FastPath` elle-même, ce que Rust
interdit formellement. Le compilateur produira une erreur de lifetime à
ce stade, sans solution directe en Rust safe.

**Deux solutions idiomatiques :**

1. **Accès par méthode** (zero-cost, pas d'`unsafe`) :

   ```rust
   impl FastPath {
       fn data(&self) -> &[u32] {
           // recalcul du pointeur à chaque appel — inliné par le compilateur
           let bytes = &self.mmap[16..];
           bytemuck::cast_slice(bytes)  // ou slice::from_raw_parts en unsafe
       }
   }
   ```

   Avantage : Rust safe, aucun lifetime artificiel.
   Inconvénient : la spec requiert un accès indexé direct `data[idx]` depuis
   `get_day` — cette forme impose un appel de méthode intermédiaire.

2. **`&'static [u32]`** (choix retenu) : étendre le lifetime à `'static`
   via `unsafe`, en garantissant par invariant que le `Mmap` vit aussi
   longtemps que la référence.

   Le compilateur **accepte** `&'static [u32]` comme champ de struct sans
   restriction de lifetime sur `FastPath`. C'est le contrat `unsafe` que
   `parse_data_body` établit explicitement.

**Comportement compilateur attendu :**

Si vous tentez d'annoter un lifetime non-`'static` sur ce champ
(ex: `data: &'a [u32]`), le compilateur demandera un paramètre de lifetime
sur `FastPath<'a>`, puis sur `Provider<'a>`, et enfin sur toute fonction
qui les construit — cascade qui rend l'API publique inutilisable.
Le choix `'static` + `unsafe` coupe cette cascade.

```rust
/// Parse le Data Body et retourne une slice &'static [u32]
///
/// INVARIANT CRITIQUE (Audit #5) :
/// La référence &'static retournée est valide TANT QUE le Mmap vit.
/// Le Mmap est stocké dans FastPath et doit vivre au moins aussi longtemps
/// que toute référence extraite.
///
/// RÈGLES DE SÉCURITÉ :
/// 1. Ne jamais remap/reload le Mmap sans détruire le Provider
/// 2. Ne jamais extraire la slice hors du contexte du Provider
/// 3. Le FastPath possède le Mmap (ownership), garantissant le lifetime
fn parse_data_body(mmap: &Mmap, header: &Header) -> Result<&'static [u32], IoError> {
    let expected_size = 16 + (header.year_count as usize * 1464);

    if mmap.len() != expected_size {
        return Err(IoError::CorruptedFile {
            expected: expected_size,
            actual: mmap.len(),
        });
    }

    // Vérification alignement u32
    let data_ptr = unsafe { mmap.as_ptr().add(16) };
    if (data_ptr as usize) % 4 != 0 {
        return Err(IoError::MisalignedData);
    }

    // Conversion sécurisée
    // SAFETY :
    // - Alignement vérifié (% 4 == 0)
    // - Taille validée (len == expected_size)
    // - Lifetime 'static justifié par ownership du Mmap dans FastPath
    let len = header.year_count as usize * 366;
    let slice = unsafe {
        std::slice::from_raw_parts(data_ptr as *const u32, len)
    };

    Ok(slice)
}
```

---

## 8. Versioning Binaire et Migration

### 8.1 Politique de Versioning

**Stratégie de Migration** :

```rust
pub enum MigrationStrategy {
    /// Refuser le chargement si version différente
    Strict,

    /// Tenter la migration automatique (v1 → v2)
    AutoMigrate,

    /// Charger en mode dégradé (features v2 ignorées)
    BestEffort,
}

impl Provider {
    pub fn load_with_migration(
        path: &str,
        strategy: MigrationStrategy,
    ) -> Result<Self, RuntimeError> {
        let header = validate_header_versioned(path)?;

        match (header.version, strategy) {
            (1, _) => {
                // Version actuelle, chargement normal
                Self::load_v1(path)
            }
            (2, MigrationStrategy::AutoMigrate) => {
                // Migration automatique
                Self::migrate_v1_to_v2_and_load(path)
            }
            (2, MigrationStrategy::BestEffort) => {
                // Charge v2 en ignorant features v2
                Self::load_v2_best_effort(path)
            }
            (v, MigrationStrategy::Strict) => {
                Err(RuntimeError::Io(IoError::UnsupportedVersion(v)))
            }
        }
    }
}
```

### 8.2 Utilitaire d'Inspection

```bash
# Utilitaire CLI pour diagnostics
$ kald-inspect france.kald

Format: KALD v1
Start Year: 2025
Year Count: 300
File Size: 439,216 bytes (expected: 439,216 ✓)
Checksum: None
Compression: None
Flags: 0x0000

First 10 entries:
  2025-001: 0x40100042 (Prec=4/SollemnitatesGenerales, Nat=Sollemnitas, Albus, TempusNativitatis, #0x00042)
  2025-002: 0xA6100000 (Prec=10/FeriaeAdventusEtOctavaNativitatis, Nat=Feria, Albus, TempusNativitatis, #0x00000)
  ...

Validation: ✓ All entries decodable
Corruption: 0 invalid entries detected

Compatibility:
  ✓ Can be loaded by liturgical-calendar-runtime v1.x
  ✓ Can be migrated to v2 format
```

**Implémentation** :

```rust
// kald-inspect/src/main.rs
fn inspect_file(path: &str) -> Result<Report, IoError> {
    let file = File::open(path)?;
    let mmap = unsafe { Mmap::map(&file)? };

    let header = validate_header(&mmap[..])?;

    let mut report = Report {
        version: header.version,
        start_year: header.start_year,
        year_count: header.year_count,
        file_size: mmap.len(),
        expected_size: 16 + (header.year_count as usize * 1464),
        corruption_count: 0,
        sentinel_count: 0,
        entries: Vec::new(),
    };

    // Scan de tous les u32 pour détecter corruptions.
    // IMPORTANT : différencier les sentinelles structurelles (0xFFFFFFFF sur slot jour-366
    // non-bissextile, attendu) des corruptions réelles (valeur illégale à un slot valide).
    let data = parse_data_body(&mmap, &header)?;
    for (i, &packed) in data.iter().enumerate() {
        let year_offset = i / 366;
        let day_of_year = (i % 366) + 1;
        let year = header.start_year as i32 + year_offset as i32;

        // Sentinelle structurelle : slot jour-366 d'une année non-bissextile.
        // Ce padding est écrit par la Forge intentionnellement (INV-7).
        if packed == 0xFFFF_FFFF && day_of_year == 366 && !is_leap_year(year) {
            report.sentinel_count += 1;
            continue;
        }

        match Day::try_from_u32(packed) {
            Ok(day) => {
                if i < 10 {
                    report.entries.push(day);
                }
            }
            Err(_) => {
                report.corruption_count += 1;
                // INV-W5 note : eprintln! est AUTORISÉ ici — kald-inspect est un binaire std,
                // pas un composant de l'Engine. Cette frontière est structurellement correcte.
                eprintln!("Corruption at offset {}: 0x{:08X}", i, packed);
            }
        }
    }

    Ok(report)
}
```

---

## 9. Gestion des Corruptions et Diagnostics

### 9.1 Hiérarchie d'Erreurs par Couche

**Principe** : chaque crate expose son propre type d'erreur. Les couches supérieures agrègent via `From`. Zéro couplage entre `core` et l'infrastructure I/O.

```
DomainError      ← liturgical-calendar-core   (validation bitfields, bornes)
IoError          ← liturgical-calendar-io     (format .kald, I/O fichier)
RegistryError    ← liturgical-calendar-forge  (FeastID allocation/collision)
RuntimeError     ← liturgical-calendar-runtime (agrège tout + corruption)
```

````rust
// ─────────────────────────────────────────────
// liturgical-calendar-core/src/error.rs
// ─────────────────────────────────────────────
//
// INVARIANT no_std : ce fichier ne peut importer que core::*.
// Pas de std::error::Error, pas de alloc, pas de String.
// Le crate racine déclare : #![no_std]
// Les crates consommateurs (io, forge, runtime) ont accès à std.

/// Erreurs du domaine liturgique pur.
///
/// Produites par : Season::try_from_u8, Color::try_from_u8,
/// Precedence::try_from_u8, Nature::try_from_u8, SlowPath::compute,
/// SeasonBoundaries::compute, Day::try_from_u32.
///
/// GARANTIES no_std :
/// - Aucun variant ne contient String ni Box<dyn _>
/// - #[derive(Debug)] utilise core::fmt::Debug (disponible no_std)
/// - impl Display utilise core::fmt::Display (disponible no_std)
/// - Pas d'impl std::error::Error (std uniquement)
///   → les crates std peuvent l'intégrer via RuntimeError qui, lui, est std
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DomainError {
    InvalidSeason(u8),
    InvalidColor(u8),
    InvalidPrecedence(u8),
    InvalidNature(u8),
    ReservedBitSet,
    YearOutOfBounds(i16),
}

impl core::fmt::Display for DomainError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidSeason(v)    => write!(f, "invalid season: {}", v),
            Self::InvalidColor(v)     => write!(f, "invalid color: {}", v),
            Self::InvalidPrecedence(v) => write!(f, "invalid precedence: {}", v),
            Self::InvalidNature(v)    => write!(f, "invalid nature: {}", v),
            Self::ReservedBitSet      => write!(f, "reserved bit [18] must be 0"),
            Self::YearOutOfBounds(y)  => write!(f, "year out of bounds: {}", y),
        }
    }
}

// impl std::error::Error est DÉLIBÉRÉMENT absent du crate core.
// Il est fourni par le crate runtime (std) via le wrapper RuntimeError :
//
//   #[cfg(feature = "std")]
//   impl std::error::Error for RuntimeError {}
//
// Cela préserve la compilabilité no_std de core tout en exposant
// l'interface std::error::Error aux consommateurs qui en ont besoin.

impl DomainError {
    /// Nom du champ invalide — utilisé par DayPacked::try_from_u32
    /// pour construire un CorruptionInfo sans allocation.
    pub fn field_name(&self) -> &'static str {
        match self {
            Self::InvalidSeason(_)    => "season",
            Self::InvalidColor(_)     => "color",
            Self::InvalidPrecedence(_) => "precedence",
            Self::InvalidNature(_)    => "nature",
            Self::ReservedBitSet      => "reserved",
            Self::YearOutOfBounds(_)  => "year",
        }
    }

    /// Valeur numérique hors domaine (0 pour YearOutOfBounds et ReservedBitSet).
    pub fn field_value(&self) -> u8 {
        match self {
            Self::InvalidSeason(v) | Self::InvalidColor(v)
            | Self::InvalidPrecedence(v) | Self::InvalidNature(v) => *v,
            Self::ReservedBitSet | Self::YearOutOfBounds(_) => 0,
        }
    }
}

### 9.2 Frontières `no_std` / `alloc` / `std` par Crate

**Tableau normatif des dépendances runtime par crate :**

| Crate | `#![no_std]` | `extern crate alloc` | `std` disponible | Justification |
|---|---|---|---|---|
| `liturgical-calendar-core` | ✅ Oui | ❌ Non | ❌ Non | Types purs, aucune allocation. `DomainError`, `DayPacked`, `Precedence`, `Nature`, `Color`, `Season` : stack uniquement. |
| `liturgical-calendar-io` | ❌ Non | N/A | ✅ Oui | Accès fichiers, `memmap2`, `Mmap`. `std` requis. |
| `liturgical-calendar-forge` | ❌ Non | N/A | ✅ Oui | `BTreeMap`, `Vec`, `serde`, `String` pour parsing config. `std` requis. |
| `liturgical-calendar-runtime` | ❌ Non | N/A | ✅ Oui | `AtomicU64`, `eprintln`, FFI CString. `std` requis. |
| `liturgical-calendar-rules-roman` | ❌ Non | ✅ Si tableau statique | ❌ Non | `HardcodedRuleProvider` utilise des tableaux `&'static [T]` en v2.0. `String` est interdit dans ce crate. Noms des fêtes : `&'static str` uniquement. |

**Règles impératives :**

1. `liturgical-calendar-core` ne dépend d'aucune crate d'allocation. Tout type contenant `String`, `Vec`, ou `Box` est interdit dans ce crate.
2. `TemporalRule::name`, `SanctoralFeast::name`, `RuleProvider` (trait) vivent dans `liturgical-calendar-forge` ou `liturgical-calendar-rules-roman` — jamais dans `liturgical-calendar-core`.
3. La séparation `FeastDefinition` (core, Copy, no_std) / `TemporalRule` (forge/rules-roman, String, std) est structurelle et non négociable. `FeastDefinition` est la seule représentation de règle que le core connaît.
4. `HardcodedRuleProvider` vit dans `liturgical-calendar-rules-roman` (std). Il expose `.resolve_temporal() → Vec<FeastDefinition>` et `.resolve_sanctoral() → Vec<FeastDefinition>` pour la Forge. Les `String` n'apparaissent que dans `TemporalRule` (Forge) — jamais dans `FeastDefinition` (core).
5. `compute_day_static` (point d'entrée FFI) utilise `STATIC_TEMPORAL_DEFS` et `STATIC_SANCTORAL_DEFS` : des `&'static [FeastDefinition]` compilées dans `liturgical-calendar-core`. Zéro allocation, zéro dépendance externe.

**Conséquence pour l'implémentation :**

```rust
// ✅ CORRECT — vit dans liturgical-calendar-core (no_std, no alloc)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FeastDefinition {
    pub id: u32,
    pub precedence: Precedence,
    pub nature: Nature,
    pub color: Color,
    // Pas de name — les noms sont dans le StringProvider (.lits)
}

// ✅ CORRECT — vit dans liturgical-calendar-forge (std)
#[derive(Debug, Clone)]
pub struct TemporalRule {
    pub id: u32,
    pub name: String,          // std — Forge uniquement
    pub rule_type: TemporalRuleType,
    pub precedence: Precedence,
    pub nature: Nature,
    pub color: Color,
}

// ❌ INTERDIT — String dans un crate no_std
// pub struct FeastDefinition { pub name: String, ... }
````

// ─────────────────────────────────────────────
// liturgical-calendar-io/src/error.rs
// ─────────────────────────────────────────────

/// Erreurs de format et d'I/O fichier.
///
/// Produites par : validate_header, parse_data_body,
/// Calendar::write_kald, StringProvider::load. #[derive(Debug)]
pub enum IoError {
Io(std::io::Error),
FileTooSmall,
InvalidMagic([u8; 4]),
UnsupportedVersion(u16),
UnsupportedFlags { found: u16, known: u16, unknown_bits: u16 },
InvalidPadding([u8; 4]),
// NOTE : InvalidYearRange supprimée — les bornes hors domaine grégorien sont
// désormais routées vers DomainError::YearOutOfBounds (correction C2/B3).
// Une borne invalide est une violation de domaine, pas une erreur d'I/O.
InvalidYearCount(u16),
MisalignedData,
CorruptedFile { expected: usize, actual: usize },
}

impl From<std::io::Error> for IoError {
fn from(e: std::io::Error) -> Self { Self::Io(e) }
}

// ─────────────────────────────────────────────
// liturgical-calendar-forge/src/error.rs
// ─────────────────────────────────────────────

/// Erreurs d'allocation et d'interopérabilité du FeastID Registry.
///
/// Produites par : FeastRegistry::allocate_next, register, import,
/// et les fonctions de normalisation de config (normalize_color, normalize_nature). #[derive(Debug)]
pub enum RegistryError {
/// Deux forges ont alloué le même FeastID avec des noms différents.
FeastIDCollision(u32),
/// L'espace séquentiel 12 bits d'un scope/category est épuisé (max 4096 entrées).
FeastIDExhausted { scope: u8, category: u8 },
/// Scope > 3 ou category > 15.
InvalidScopeCategory { scope: u8, category: u8 },
/// Chaîne de couleur inconnue dans un fichier de configuration TOML/YAML.
/// Contient la valeur originale pour le message d'erreur à l'opérateur.
UnknownColorString(String),
/// Chaîne de nature inconnue dans un fichier de configuration TOML/YAML.
UnknownNatureString(String),
/// Valeur de précédence hors domaine (0–12) dans un fichier de configuration.
InvalidPrecedenceValue(u8),
// NOTE : import() retourne Ok(ImportReport) même en présence de collisions.
// Les collisions de noms sont rapportées via ImportReport::collisions, non via Err.
// Voir §3.3 pour le comportement canonique.
}

// ─────────────────────────────────────────────
// liturgical-calendar-runtime/src/error.rs
// ─────────────────────────────────────────────

/// Erreur applicative du runtime — agrège toutes les couches inférieures.
///
/// C'est le type exposé aux consommateurs de la bibliothèque et au FFI.
/// Le mapping From garantit la propagation sans perte d'information. #[derive(Debug)]
pub enum RuntimeError {
/// Erreur de domaine liturgique (validation bitfields).
Domain(DomainError),
/// Erreur de format ou d'I/O fichier.
Io(IoError),
/// Entrée corrompue détectée dans le Data Body.
CorruptedEntry(CorruptionInfo),
/// Multiples corruptions détectées (scan complet).
MultipleCorruptions(Vec<CorruptionInfo>),
}

impl From<DomainError> for RuntimeError {
fn from(e: DomainError) -> Self { Self::Domain(e) }
}

impl From<IoError> for RuntimeError {
fn from(e: IoError) -> Self { Self::Io(e) }
}

impl From<std::io::Error> for RuntimeError {
fn from(e: std::io::Error) -> Self { Self::Io(IoError::Io(e)) }
}

````

**Règle de mapping crate ↔ type** :

| Crate     | Type d'erreur exposé       | Consomme                 |
| --------- | -------------------------- | ------------------------ |
| `core`    | `DomainError`              | —                        |
| `io`      | `IoError`                  | `std::io::Error`         |
| `forge`   | `RegistryError`, `IoError` | `DomainError` via `?`    |
| `runtime` | `RuntimeError`             | `DomainError`, `IoError` |

**Note** : `HeaderError` (défini §2.1) reste le type interne de `validate_header`. Il est converti en `IoError` par la couche `io` avant d'être exposé.

#### Conversions `From` requises — tableau exhaustif

**Contexte compilateur** : l'opérateur `?` propage automatiquement une erreur
en appelant `From::from(e)`. Si la conversion `From<X> for Y` n'est pas
implémentée, `?` produit une erreur de compilation au niveau de l'appel,
pas au niveau de la définition du type. Ces erreurs apparaissent tard dans
le développement, lorsque les crates sont assemblées.

Le tableau suivant liste toutes les implémentations `From` qui **doivent
exister** pour que `?` fonctionne à travers les frontières de crates. Toute
conversion manquante provoquera une erreur `the trait From<X> is not
implemented for Y` à la compilation.

| `From` (source)     | `For` (cible)  | Crate d'implémentation        | Usage                                           |
| ------------------- | -------------- | ----------------------------- | ----------------------------------------------- |
| `std::io::Error`    | `IoError`      | `liturgical-calendar-io`      | `File::open()?`, `file.write_all()?`            |
| `std::io::Error`    | `RuntimeError` | `liturgical-calendar-runtime` | `File::open()?` dans le Provider                |
| `IoError`           | `RuntimeError` | `liturgical-calendar-runtime` | propagation depuis les fonctions io             |
| `DomainError`       | `RuntimeError` | `liturgical-calendar-runtime` | `slow_path.compute()?` dans le runtime          |
| `HeaderError`       | `IoError`      | `liturgical-calendar-io`      | `validate_header()?` interne                    |
| `serde_json::Error` | `IoError`      | `liturgical-calendar-io`      | `serde_json::from_reader()?` dans FeastRegistry |

**Conversions absentes volontairement** :

- `DomainError → IoError` : non implémenté. Une erreur de domaine n'est pas
  une erreur I/O. Les sites qui produisent les deux utilisent `RuntimeError`
  comme type de retour commun.
- `RegistryError → RuntimeError` : non implémenté. La Forge et le Runtime
  sont des binaires distincts. `RegistryError` ne traverse pas cette frontière.

**Ordre d'implémentation recommandé** : implémenter dans l'ordre du tableau,
du plus bas (`std::io::Error → IoError`) vers le plus haut
(`DomainError → RuntimeError`). Le compilateur signalera les manques dans
cet ordre lors de la construction du workspace complet (`cargo build --workspace`).

// CorruptionInfo est défini canoniquement en section 1.1.
// Rappel : { packed_value: u32, invalid_field: &'static str,
// invalid_value: u8, offset: Option<usize> }

### 9.2 Télémétrie Structurée

```rust
#[derive(Debug, Clone, Copy)]
pub struct TelemetrySnapshot {
    pub fast_path_hits: u64,
    pub slow_path_hits: u64,
    pub invalid_returns: u64,
    pub corrupted_entries: u64,
    pub out_of_bounds_queries: u64,
}

impl TelemetrySnapshot {
    pub fn hit_rate(&self) -> f64 {
        let total = self.fast_path_hits + self.slow_path_hits;
        if total == 0 {
            0.0
        } else {
            (self.fast_path_hits as f64) / (total as f64)
        }
    }

    pub fn corruption_rate(&self) -> f64 {
        let total = self.fast_path_hits + self.slow_path_hits;
        if total == 0 {
            0.0
        } else {
            (self.corrupted_entries as f64) / (total as f64)
        }
    }
}
````

---

## 10. Bindings FFI (C/C++ Interop)

### 10.1 API C

```c
// kal.h
// Note: The "kal_" prefix derives from "Kalendarium", the compiled annual artifact
//       used to keep function names concise while avoiding namespace
//       collisions in C.

#ifndef KAL_H
#define KAL_H

#include <stdint.h>
#include <stdbool.h>

typedef struct KalProvider KalProvider;

typedef struct {
    uint32_t value;
    uint32_t error_code;
} KalResult;

typedef struct {
    uint64_t fast_path_hits;
    uint64_t slow_path_hits;
    uint64_t invalid_returns;
    uint64_t corrupted_entries;
    uint64_t out_of_bounds_queries;
} KalTelemetry;

// Error codes
#define KAL_OK 0
#define KAL_INVALID_HANDLE 1
#define KAL_FILE_ERROR 2
#define KAL_INVALID_DAY 3
#define KAL_CORRUPTED_ENTRY 4
#define KAL_OUT_OF_BOUNDS 5

// Lifecycle
KalProvider* kal_new(const char* data_path, const char* lang_path);
void kal_free(KalProvider* handle);

// Queries
KalResult kal_get_day_checked(
    const KalProvider* handle,
    int16_t year,
    uint16_t day_of_year
);

uint32_t kal_get_day(
    const KalProvider* handle,
    int16_t year,
    uint16_t day_of_year
);

// Telemetry
KalTelemetry kal_get_telemetry(
    const KalProvider* handle
);

// Error handling
const char* kal_get_last_error(
    const KalProvider* handle
);

#endif // KAL_H
```

### 10.2 Implémentation Rust

**Principe général des blocs `unsafe` FFI :**

Chaque fonction FFI qui déréférence un pointeur C est déclarée `unsafe extern "C"`.
Le compilateur Rust ne peut pas vérifier les préconditions côté C — elles sont
documentées dans les sections `# Safety` ci-dessous et constituent le **contrat
que l'appelant C doit respecter**. Toute violation est un comportement indéfini (UB).

```rust
#[repr(C)]
pub struct KalResult {
    pub value: u32,
    pub error_code: u32,
}

/// Interroge un jour liturgique avec code d'erreur explicite.
///
/// # Safety
///
/// - `handle` doit être un pointeur non-nul obtenu exclusivement via `kal_new()`.
/// - `handle` ne doit pas avoir été passé à `kal_free()`.
/// - `handle` ne doit pas être accédé simultanément depuis plusieurs threads
///   (le Provider n'est pas `Sync` pour les accès en écriture de télémétrie).
/// - `handle` doit pointer vers une zone mémoire valide et alignée
///   (garanti si obtenu via `kal_new()`).
///
/// Un `handle` NULL est géré sans UB (retourne `KAL_INVALID_HANDLE`).
/// Tout autre pointeur invalide est UB non récupérable.
#[no_mangle]
pub unsafe extern "C" fn kal_get_day_checked(
    handle: *const Provider,
    year: i16,
    day_of_year: u16,
) -> KalResult {
    if handle.is_null() {
        return KalResult {
            value: 0,
            error_code: 1,  // INVALID_HANDLE
        };
    }

    if day_of_year == 0 || day_of_year > 366 {
        let provider = unsafe { &*handle };
        provider.set_last_error("Invalid day_of_year: must be 1-366");
        return KalResult {
            value: 0,
            error_code: 3,  // INVALID_DAY
        };
    }

    let provider = unsafe { &*handle };
    let day = provider.get_day(year, day_of_year);

    if day.is_invalid() {
        let error_code = if year < 1583 || year > 4099 {
            provider.set_last_error(&format!("Year {} out of bounds", year));
            5  // OUT_OF_BOUNDS
        } else {
            provider.set_last_error("Corrupted entry or invalid day");
            4  // CORRUPTED_ENTRY
        };

        KalResult {
            value: 0,
            error_code,
        }
    } else {
        KalResult {
            value: day.as_u32(),
            error_code: 0,  // OK
        }
    }
}

/// Retourne la télémétrie courante du Provider.
///
/// # Safety
///
/// - `handle` doit être un pointeur valide obtenu via `kal_new()`, ou NULL.
/// - Un `handle` NULL retourne une `KalTelemetry` zéro sans UB.
/// - `handle` ne doit pas avoir été libéré par `kal_free()`.
#[no_mangle]
pub unsafe extern "C" fn kal_get_telemetry(
    handle: *const Provider,
) -> KalTelemetry {
    if handle.is_null() {
        return KalTelemetry::default();
    }

    let provider = unsafe { &*handle };
    let snapshot = provider.get_telemetry();

    KalTelemetry {
        fast_path_hits: snapshot.fast_path_hits,
        slow_path_hits: snapshot.slow_path_hits,
        invalid_returns: snapshot.invalid_returns,
        corrupted_entries: snapshot.corrupted_entries,
        out_of_bounds_queries: snapshot.out_of_bounds_queries,
    }
}

/// Crée un nouveau Provider et retourne un handle opaque.
///
/// # Safety
///
/// - `data_path` et `lang_path` doivent être des pointeurs vers des chaînes
///   C valides (null-terminées, encodées UTF-8).
/// - Les pointeurs `data_path` et `lang_path` doivent rester valides pour
///   toute la durée de l'appel (ils ne sont pas retenus après retour).
/// - Retourne NULL en cas d'erreur (fichier introuvable, header invalide, etc.).
/// - Le handle retourné doit être libéré par `kal_free()` exactement une fois.
///
/// # Ownership
///
/// Le handle retourné est alloué sur le heap Rust via `Box::into_raw()`.
/// Il appartient à l'appelant C jusqu'à l'appel de `kal_free()`.
#[no_mangle]
pub unsafe extern "C" fn kal_new(
    data_path: *const std::ffi::c_char,
    lang_path: *const std::ffi::c_char,
) -> *mut Provider {
    // Conversion sécurisée des chaînes C
    let data_path = match unsafe { std::ffi::CStr::from_ptr(data_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let lang_path = match unsafe { std::ffi::CStr::from_ptr(lang_path) }.to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let rules = HardcodedRuleProvider::new_roman_rite_ordinary();
    let slow_path = SlowPath::new(rules);

    match Provider::new(data_path, lang_path, slow_path) {
        Ok(p) => Box::into_raw(Box::new(p)),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Libère un handle créé par `kal_new()`.
///
/// # Safety
///
/// - `handle` doit être un pointeur obtenu via `kal_new()`, ou NULL.
/// - `handle` ne doit pas avoir déjà été passé à `kal_free()` (double-free = UB).
/// - Après appel, `handle` est invalide — ne pas le déréférencer.
/// - Un `handle` NULL est une no-op (pas d'erreur, pas d'UB).
#[no_mangle]
pub unsafe extern "C" fn kal_free(handle: *mut Provider) {
    if !handle.is_null() {
        // SAFETY : handle est non-nul, obtenu via Box::into_raw(), appelé une seule fois.
        drop(unsafe { Box::from_raw(handle) });
    }
}
```

---

## 10.3 Interface de Communication Forge ↔ Engine

### 10.3.1 Principe

La communication entre la Forge (Producer) et l'Engine (Consumer) est **unidirectionnelle et asynchrone** : la Forge produit un artefact binaire `.kald`, l'Engine le consomme en lecture seule via un buffer fourni par l'appelant. Il n'existe aucun appel direct Forge → Engine à l'exécution — uniquement pendant la phase AOT de compilation.

```
┌─────────────────────────────────┐
│  THE FORGE (liturgical-calendar-forge)   │  std — Producer
│  Ingestion YAML → Validation    │
│  → SlowPath (Engine) → .kald    │
└──────────────────┬──────────────┘
                   │  Artefact binaire : .kald (Data Body : 366 u32 × N années)
                   │  Artefact strings  : .lits (Index FeastID → UTF-8)
                   ▼
┌─────────────────────────────────┐
│  THE ENGINE (liturgical-calendar-core)   │  no_std, no_alloc — Consumer
│  validate_header(&[u8])         │
│  SlowPath::compute(year, doy)   │
│  DayPacked bit-ops              │
│  extern "C" FFI                 │
└─────────────────────────────────┘
```

### 10.3.2 Contrat Binaire Forge → Engine

Le `.kald` est le **contrat de données entre les deux composants**. Sa stabilité est une garantie système.

**Invariant de Conformité Binaire :**

> Pour toute année `y` dans la plage couverte par un `.kald`, et pour tout jour `d` de `y` : le `DayPacked` lu en position `index_day(y, d, start_year)` dans le Data Body est **bit-for-bit identique** au résultat de `SlowPath::compute(y, d).map(DayPacked::from)` exécuté par l'Engine.

Ce invariant est la définition opérationnelle du déterminisme. Il est vérifiable par le test de conformité binaire décrit au §10.3.4.

**Mapping mémoire du Data Body :**

```
Offset dans le Data Body = index_day(year, day_of_year, start_year) × 4
index_day(year, doy, start_year) = (year - start_year) × 366 + (doy - 1)
```

**Valeur sentinelle :**

Le slot jour 366 pour les années non-bissextiles contient `0xFFFFFFFF` (`DayPacked::invalid()`). Ce n'est pas une corruption — c'est un padding structurel posé par la Forge, détectable par `DayPacked::is_invalid()`.

### 10.3.3 Fonctions FFI de l'Engine exposant le Contrat

Les fonctions suivantes constituent la surface publique de l'Engine. Elles respectent les contraintes FFI définies en §0.3 et §0.4.

```c
// kal_engine.h — Surface publique de liturgical-calendar-core (Engine uniquement)
// Fonctions pures sur buffers fournis par l'appelant.
// Aucune allocation interne. Aucun output (stdout/stderr).

#include <stdint.h>

// Codes de retour Engine (i32) — mapping exhaustif des variantes d'erreur
#define KAL_ENGINE_OK             0  // Succès
#define KAL_ERR_MAGIC             1  // Magic != "KALD"
#define KAL_ERR_VERSION           2  // Version non supportée (récupérable par migration)
#define KAL_ERR_FLAGS             3  // Flags inconnus (rejet strict)
#define KAL_ERR_PADDING           4  // Padding non nul
#define KAL_ERR_YEAR_OOB          5  // start_year hors [1583, 4099]
#define KAL_ERR_YEAR_COUNT        6  // year_count = 0 ou > 2516
#define KAL_ERR_ENDIAN            7  // Probable mismatch endianness (heuristique)
#define KAL_ERR_BUF_TOO_SMALL     8  // Buffer < 16 octets
#define KAL_ERR_NULL_PTR          9  // Pointeur null
#define KAL_ERR_DAY_OOB          10  // day_of_year hors [1, 366]
#define KAL_ERR_CORRUPT_ENTRY    11  // DayPacked invalide dans le Data Body
#define KAL_ERR_INDEX_OOB        12  // Accès hors du Data Body

/// Valide un header .kald depuis un buffer fourni par l'appelant.
/// L'Engine n'ouvre aucun fichier. L'appelant mappe ou charge le buffer.
///
/// @param buf      Pointeur vers les 16 premiers octets du .kald
/// @param buf_len  Longueur du buffer (doit être ≥ 16)
/// @param out      Out-param : Header validé si retour == KAL_ENGINE_OK
/// @return         KAL_ENGINE_OK ou code d'erreur distinct par variante
int32_t kal_validate_header(
    const uint8_t* buf,
    uintptr_t buf_len,
    KalHeader* out
);

/// Calcule la liturgie d'un jour via le Slow Path (arithmétique pure).
/// Zéro allocation. Zéro I/O. Compatible no_std.
///
/// @param year        Année grégorienne [1583, 4099]
/// @param day_of_year Jour de l'année [1, 366]
/// @param out_packed  Out-param : DayPacked u32 si retour == KAL_ENGINE_OK
/// @return            KAL_ENGINE_OK ou code d'erreur
int32_t kal_compute_day(
    int16_t year,
    uint16_t day_of_year,
    uint32_t* out_packed
);

/// Lit un DayPacked depuis un buffer Data Body fourni par l'appelant (Fast Path).
/// L'Engine lit à l'offset calculé par kal_index_day — pas de mmap interne.
///
/// @param data_body   Pointeur vers le Data Body (après le Header de 16 octets)
/// @param data_len    Longueur du Data Body en octets
/// @param year        Année cible
/// @param day_of_year Jour cible [1, 366]
/// @param start_year  start_year extrait du Header
/// @param out_packed  Out-param : DayPacked u32 si retour == KAL_ENGINE_OK
/// @return            KAL_ENGINE_OK ou code d'erreur
int32_t kal_read_day(
    const uint8_t* data_body,
    uintptr_t data_len,
    int16_t year,
    uint16_t day_of_year,
    int16_t start_year,
    uint32_t* out_packed
);

/// Calcule l'index brut dans le Data Body (exposé pour tests de conformité).
///
/// @return  Index en octets de l'entrée (usize) si l'entrée est dans les bornes.
///          `SIZE_MAX` (`usize::MAX`) si year, day_of_year ou start_year sont hors-bornes.
///          L'appelant DOIT tester `result == SIZE_MAX` avant d'utiliser l'index.
///          Un retour 0 est un index valide (année=start_year, jour=1) — ne PAS utiliser
///          0 comme sentinelle d'erreur.
uintptr_t kal_index_day(int16_t year, uint16_t day_of_year, int16_t start_year);
```

**Implémentation Rust correspondante :**

```rust
// liturgical-calendar-core/src/ffi.rs
// no_std — aucun import std::* ici.

#[no_mangle]
pub unsafe extern "C" fn kal_validate_header(
    buf: *const u8,
    buf_len: usize,
    out: *mut Header,
) -> i32 {
    if buf.is_null() || out.is_null() {
        return KAL_ERR_NULL_PTR;
    }
    if buf_len < 16 {
        return KAL_ERR_BUF_TOO_SMALL;
    }
    let bytes = core::slice::from_raw_parts(buf, buf_len);
    match validate_header(bytes) {
        Ok(h) => { *out = h; KAL_ENGINE_OK }
        Err(HeaderError::InvalidMagic(_))              => KAL_ERR_MAGIC,
        Err(HeaderError::UnsupportedVersion(_))        => KAL_ERR_VERSION,
        Err(HeaderError::UnsupportedFlags { .. })      => KAL_ERR_FLAGS,
        Err(HeaderError::InvalidPadding(_))            => KAL_ERR_PADDING,
        Err(HeaderError::YearOutOfBounds(_))           => KAL_ERR_YEAR_OOB,
        Err(HeaderError::InvalidYearCount(_))          => KAL_ERR_YEAR_COUNT,
        Err(HeaderError::PossibleEndiannessMismatch(_))=> KAL_ERR_ENDIAN,
        Err(HeaderError::FileTooSmall)                 => KAL_ERR_BUF_TOO_SMALL,
    }
}

#[no_mangle]
pub unsafe extern "C" fn kal_compute_day(
    year: i16,
    day_of_year: u16,
    out_packed: *mut u32,
) -> i32 {
    if out_packed.is_null() { return KAL_ERR_NULL_PTR; }
    if year < 1583 || year > 4099 { return KAL_ERR_YEAR_OOB; }
    if day_of_year == 0 || day_of_year > 366 { return KAL_ERR_DAY_OOB; }

    // compute_day_static : Slow Path sur les FeastDefinition statiques compilées.
    // Utilise STATIC_TEMPORAL_DEFS et STATIC_SANCTORAL_DEFS (liturgical-calendar-core).
    // Zéro allocation — SlowPath construit sur la stack depuis &[FeastDefinition].
    match compute_day_static(year, day_of_year) {
        Ok(packed) => { *out_packed = packed.as_u32(); KAL_ENGINE_OK }
        Err(_)     => KAL_ERR_CORRUPT_ENTRY,
    }
}

#[no_mangle]
pub extern "C" fn kal_index_day(year: i16, day_of_year: u16, start_year: i16) -> usize {
    // R2 (v2.3) : retourne usize::MAX pour les entrées hors-bornes.
    // L'appelant DOIT tester `result == usize::MAX` avant utilisation.
    // Retour 0 = index valide (année=start_year, jour=1) — jamais une sentinelle d'erreur.
    index_day(year, day_of_year, start_year).unwrap_or(usize::MAX)
}

/// Lit un DayPacked depuis un buffer Data Body fourni par l'appelant (Fast Path).
///
/// CONTRAT SENTINELLE (audit P6) :
///   `0xFFFFFFFF` dans un slot est un padding structurel valide (jour 366 d'une année
///   non-bissextile), posé par la Forge. Ce n'est PAS une corruption.
///   Cette fonction retourne `KAL_ENGINE_OK` avec `*out_packed = 0xFFFFFFFF`.
///   L'appelant DOIT tester `*out_packed == 0xFFFFFFFF` (ou `DayPacked::is_invalid()`)
///   pour distinguer le cas "slot vide" d'une entrée liturgique valide.
///   `KAL_ERR_CORRUPT_ENTRY` est réservé aux valeurs hors domaine ≠ 0xFFFFFFFF.
///
/// ENDIANNESS : le buffer `data_body` est en Little-Endian canonique.
#[no_mangle]
pub unsafe extern "C" fn kal_read_day(
    data_body:   *const u8,
    data_len:    usize,
    year:        i16,
    day_of_year: u16,
    start_year:  i16,
    out_packed:  *mut u32,
) -> i32 {
    if data_body.is_null() || out_packed.is_null() { return KAL_ERR_NULL_PTR; }
    if day_of_year == 0 || day_of_year > 366       { return KAL_ERR_DAY_OOB;  }

    let idx = match index_day(year, day_of_year, start_year) {
        Some(i) => i,
        None    => return KAL_ERR_INDEX_OOB,
    };

    let byte_offset = idx * 4;
    if byte_offset + 4 > data_len { return KAL_ERR_INDEX_OOB; }

    let bytes = core::slice::from_raw_parts(data_body.add(byte_offset), 4);
    // Little-Endian canonique — sur x86_64/ARM le compilateur élimine le bswap.
    let packed = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);

    *out_packed = packed;
    KAL_ENGINE_OK  // 0xFFFFFFFF inclus — voir CONTRAT SENTINELLE ci-dessus.
}
```

### 10.3.4 Test de Conformité Binaire (Jalon "Sanctuarisation")

Ce test est le **critère d'acceptation du jalon Sanctuarisation du Core** (§ Roadmap). Il valide que la Forge et l'Engine produisent et lisent des données bit-for-bit identiques.

```rust
// tests/conformity.rs — test d'intégration inter-crates
#[test]
fn test_forge_engine_binary_conformity() {
    // 1. La Forge génère un .kald pour une plage réduite (tests rapides)
    let config = Config { start_year: 2025, year_count: 5, ..Default::default() };
    let calendar = CalendarBuilder::new(config).unwrap().build().unwrap();
    let kald_bytes = calendar.to_bytes();  // sérialisation en mémoire pour le test

    // 2. L'Engine lit les mêmes données via son interface FFI (buffer caller-provided)
    let header_bytes = &kald_bytes[..16];
    let data_body = &kald_bytes[16..];

    let mut header = KalHeader::default();
    let rc = unsafe { kal_validate_header(header_bytes.as_ptr(), header_bytes.len(), &mut header) };
    assert_eq!(rc, KAL_ENGINE_OK, "Header invalide");

    // 3. Vérification bit-for-bit sur l'intégralité de la plage
    for year in 2025_i16..2030 {
        let max_day: u16 = if is_leap_year(year as i32) { 366 } else { 365 };
        for doy in 1..=max_day {
            // Fast Path (lecture buffer)
            let mut fast_packed: u32 = 0;
            let rc = unsafe {
                kal_read_day(data_body.as_ptr(), data_body.len(), year, doy, header.start_year, &mut fast_packed)
            };
            assert_eq!(rc, KAL_ENGINE_OK);

            // Slow Path (calcul Engine)
            let mut slow_packed: u32 = 0;
            let rc = unsafe { kal_compute_day(year, doy, &mut slow_packed) };
            assert_eq!(rc, KAL_ENGINE_OK);

            // Invariant de Conformité Binaire
            assert_eq!(
                fast_packed, slow_packed,
                "VIOLATION de conformité binaire : année={}, doy={}, fast=0x{:08X}, slow=0x{:08X}",
                year, doy, fast_packed, slow_packed
            );
        }
    }
}
```

---

/// Construit un SlowPath de test avec les règles romaines hardcodées
fn make_slow_path() -> SlowPath {
SlowPath::new(HardcodedRuleProvider::new_roman_rite_ordinary())
}

## 11. Tests de Validation Canoniques

### 11.1 Test de Bitpacking Roundtrip

```rust
#[test]
fn test_bitpack_all_combinations() {
    use itertools::iproduct;

    for (prec, nat, color, season) in iproduct!(0..=12u8, 0..=4u8, 0..=5u8, 0..=6u8) {
        let original = Day {
            precedence: Precedence::try_from_u8(prec).unwrap(),
            nature:     Nature::try_from_u8(nat).unwrap(),
            color:      Color::try_from_u8(color).unwrap(),
            season:     Season::try_from_u8(season).unwrap(),
            feast_id:   0x12345,
        };

        let packed: u32 = original.clone().into();
        let unpacked = Day::try_from_u32(packed).unwrap();

        assert_eq!(original, unpacked);
    }
}
```

### 11.2 Test d'Identité Fast-Slow Path

```rust
#[test]
fn test_path_identity_comprehensive() {
    let provider = Provider::new("test.kald", "test.lits", make_slow_path()).unwrap();

    for year in 2025..2100 {
        let max_day: u16 = if is_leap_year(year as i32) { 366 } else { 365 };
        for day in 1..=max_day {
            let fast = provider.get_day(year, day);

            let slow = provider.compute_slow(year, day)
                .map(|logic| DayPacked::from(logic))
                .unwrap_or_else(|_| DayPacked::invalid());

            assert_eq!(
                fast.as_u32(), slow.as_u32(),
                "Divergence à {}-{:03}: fast={:08X}, slow={:08X}",
                year, day, fast.as_u32(), slow.as_u32()
            );
        }
    }
}
```

### 11.3 Test de Déterminisme de la Forge

```rust
#[test]
fn test_forge_determinism() {
    let config = Config::load("test_config.toml").unwrap();

    let output1 = CalendarBuilder::build(config.clone())
        .unwrap()
        .serialize();

    let output2 = CalendarBuilder::build(config.clone())
        .unwrap()
        .serialize();

    assert_eq!(
        output1, output2,
        "La Forge n'est pas déterministe ! Vérifier les BTreeMap."
    );
}
```

### 11.4 Test Forge → Runtime Loop (Nouveau - Audit #7)

```rust
#[test]
fn test_forge_runtime_identity() {
    // Génération
    let config = Config {
        start_year: 2025,
        year_count: 5,
        layers: vec![/* ... */],
    };

    let builder = CalendarBuilder::build(config).unwrap();
    builder.write_kald("test_loop.kald").unwrap();
    builder.write_lits("test_loop.lits", "fr").unwrap();

    // Chargement
    let provider = Provider::new("test_loop.kald", "test_loop.lits", make_slow_path()).unwrap();

    // Vérification sur 100 dates réparties
    for year in 2025..2030 {
        for day in [1, 50, 100, 150, 200, 250, 300, 365] {
            let runtime_result = provider.get_day(year, day);

            let slow_result = provider.compute_slow(year, day)
                .map(|logic| DayPacked::from(logic))
                .unwrap_or_else(|_| DayPacked::invalid());

            assert_eq!(
                runtime_result.as_u32(),
                slow_result.as_u32(),
                "Divergence Forge/Runtime pour {}-{:03}", year, day
            );
        }
    }
}
```

### 11.5 Tests de Fuzzing

```rust
// fuzz/fuzz_targets/litu_header.rs
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() < 16 {
        return;
    }

    // Tentative de validation header
    let _ = validate_header(data);

    // Ne doit JAMAIS paniquer, même avec données aléatoires
});

// fuzz/fuzz_targets/litu_data.rs
fuzz_target!(|data: &[u8]| {
    if data.len() < 1464 + 16 {
        return;
    }

    // Création d'un fichier temporaire
    let temp = create_temp_file(data);

    // Tentative de chargement
    let _ = Provider::load(&temp);

    // Vérification : pas de panic, erreurs contrôlées
});
```

### 11.6 Tests d'Interopérabilité FeastID

```rust
#[test]
fn test_feast_id_interop() {
    // Forge 1 : France
    let mut registry_fr = FeastRegistry::load("registry_france.json").unwrap();
    let mut builder_fr = CalendarBuilder::new(2025, 10);

    // Allocation locale
    for _ in 0..100 {
        let id = registry_fr.allocate_next(2, 1).unwrap();  // National/Sanctoral
        builder_fr.add_feast_with_id(id, "Test Feast");
    }

    // Export
    export_allocations(&builder_fr, "export_france.json").unwrap();

    // Forge 2 : Allemagne
    let mut registry_de = FeastRegistry::load("registry_germany.json").unwrap();

    // Import des allocations françaises
    let result = import_allocations(&mut registry_de, "export_france.json");

    // Vérification : pas de collision
    assert!(result.is_ok(), "FeastID collision detected between France and Germany");

    // Allocation allemande après import
    for _ in 0..100 {
        let id = registry_de.allocate_next(2, 1).unwrap();
        // Les IDs ne doivent pas chevaucher les allocations françaises
        assert!(!builder_fr.has_feast_id(id));
    }
}
```

### 11.7 Tests de Télémétrie

```rust
#[test]
fn test_telemetry_corruption_tracking() {
    // Création d'un fichier avec corruption intentionnelle
    let mut data = create_valid_litu(2025, 1);

    // Injection d'un packed invalide (season = 15, hors limites)
    data[16] = 0xFF;
    data[17] = 0xFF;
    data[18] = 0xFF;
    data[19] = 0xFF;

    write_to_file("corrupted.kald", &data);

    // Chargement (doit réussir malgré corruption)
    let provider = Provider::new("corrupted.kald", "corrupted.lits", make_slow_path()).unwrap();

    // Requête sur l'entrée corrompue
    let result = provider.get_day(2025, 1);

    // Vérification : DayPacked::invalid() = 0xFFFFFFFF
    assert_eq!(result.as_u32(), 0xFFFFFFFF);

    // Vérification télémétrie
    // La corruption incrémente corrupted_entries, pas invalid_returns.
    // invalid_returns est réservé aux requêtes avec day_of_year hors [1,366].
    let telemetry = provider.get_telemetry();
    assert_eq!(telemetry.corrupted_entries, 1);
    assert_eq!(telemetry.invalid_returns, 0);
}
```

---

## 12. Annexe : Layout Hexadécimal Complet

**Fichier** : `france.kald` (2025-2324, 300 ans)

```
[Header - 16 octets]
00000000: 4B 41 4C 44 01 00 E9 07  2C 01 00 00 00 00 00 00  |KALD....,.......|
          └─────┬─────┘ │  └──┬──┘  └──┬──┘  └──────┬──────┘
             Magic    Ver  Start   Count      Flags + Padding

[Data Body — Année 2025, Jour 1 (1er janvier, Sainte Marie Mère de Dieu)]
00000010: XX XX XX XX ...
          DayPacked layout v2.0 :
            Precedence [31:28] = 4  (SollemnitatesGenerales)
            Nature     [27:25] = 0  (Sollemnitas)
            Color      [24:22] = 0  (Albus)
            Season     [21:19] = 2  (TempusNativitatis)
            Reserved   [18]    = 0
            FeastID    [17:0]  = 0x00042

[Année 2025, Jour 110 (20 avril — Dominica Resurrectionis)]
000001C4: XX XX XX XX ...
          DayPacked layout v2.0 :
            Precedence [31:28] = 0  (TriduumSacrum — niveau 0 inclut Pâques, cf. NALC 1969)
            Nature     [27:25] = 0  (Sollemnitas)
            Color      [24:22] = 0  (Albus)
            Season     [21:19] = 5  (TempusPaschale)
            Reserved   [18]    = 0
            FeastID    [17:0]  = 0x00001

[Année 2025, Jour 366 (padding année non-bissextile)]
000005C4: FF FF FF FF                                       |....|
          └───┬───┘
            0xFFFFFFFF (DayPacked::invalid())
            Precedence [31:28] = 15 → hors domaine (max=12), rejeté
            Nature     [27:25] = 7  → hors domaine (max=4),  rejeté
```

---

## 13. Résumé des Corrections et Hardening

| #   | Risque Identifié                          | Correction                                                                        | Criticité   | Section     |
| --- | ----------------------------------------- | --------------------------------------------------------------------------------- | ----------- | ----------- |
| 1   | Collisions FeastID                        | Registry canonique + import/export                                                | **Moyenne** | 3.1-3.3     |
| 2   | Séparation Logic/Packed                   | Types distincts Logic/Packed                                                      | **Haute**   | 1.1         |
| 3   | Précédence non-stricte                    | PrecedenceResolver documenté                                                      | **Moyenne** | 4.4         |
| 4   | HashMap non-déterministe                  | BTreeMap partout dans Forge                                                       | **Haute**   | 5.1         |
| 5   | Lifetime 'static non-justifié             | Documentation invariants Mmap                                                     | **Haute**   | 7.2         |
| 6   | Bornes années non-validées                | Validation 1583-4099 stricte                                                      | **Moyenne** | 5.1         |
| 7   | Identité Forge/Runtime                    | Test loop complet                                                                 | **Haute**   | 11.4        |
| 8   | Versioning binaire                        | Header flags + migration strategy                                                 | **Haute**   | 8.1-8.2     |
| 9   | Corruptions silencieuses                  | Telemetry + logs structurés                                                       | **Haute**   | 9.1-9.2     |
| 10  | Endianness implicite                      | Documentation + détection runtime                                                 | **Moyenne** | 2.2         |
| 11  | FFI error reporting                       | KalResult + last_error                                                            | **Haute**   | 10.1        |
| 12  | Couverture tests                          | Fuzzing + cross-build + interop                                                   | **Haute**   | 11.5-11     |
| 13  | `eprintln!` dans l'Engine                 | Suppression + HeaderError::PossibleEndiannessMismatch + build_corruption_info     | **Haute**   | §0, 2.1, 7  |
| 14  | Engine sans `#[repr(C)]` sur Header       | `#[repr(C)]` ajouté, FFI-safe                                                     | **Haute**   | 2.1, 10.3   |
| 15  | Absence d'interface FFI Engine formalisée | Section §10.3 + kal_engine.h + test conformité                                    | **Haute**   | 10.3        |
| 16  | Partition Workspace non contractuelle     | §0 : INV-W1 à INV-W5 + nommage liturgical-calendar-forge/liturgical-calendar-core | **Haute**   | §0, A.1-A.4 |

---

## Annexe A : Architecture du Projet

### A.1 Structure Workspace

Le projet Liturgical Calendar est organisé en **workspace multi-crates Cargo** selon la partition **Forge (std) / Engine (no_std)** définie en §0.

**Workspace root** : `liturgical-calendar/`

**Composants principaux** :

1. **`liturgical-calendar-core`** — The Engine (**`#![no_std]`, no alloc**)
   - Types de domaine canoniques (§1) : `DayPacked`, `Day`, `Precedence`, `Nature`, `Color`, `Season`
   - Algorithmes purs : `compute_easter`, `SeasonBoundaries::compute` (§4)
   - Slow Path complet : `SlowPath::compute` (§4)
   - Validation header : `validate_header(&[u8])` (§2.1)
   - Interface FFI C-ABI `extern "C"` : `kal_validate_header`, `kal_compute_day`, `kal_read_day` (§10.3)
   - **Zéro dépendance externe. Zéro allocation. Zéro output.**
   - Compatible WASM, ARM, embarqué.

2. **`liturgical-calendar-forge`** — The Forge (**`std`**)
   - Ingestion YAML, validation canonique V1–V6 (§B.11)
   - `FeastRegistry` (§3) — BTreeMap, allocation, import/export JSON
   - Fonctions de normalisation : `normalize_color`, `normalize_nature` (§5.1) — requièrent `String`
   - Pipeline AOT : `CalendarBuilder` → `Calendar::write_kald` (§5.2–5.3)
   - Appel du Slow Path de l'Engine pour la génération du Data Body
   - CLI de génération de `.kald` et `.lits`

3. **`liturgical-calendar-io`** — Sérialisation binaire (`std`)
   - Lecture/écriture format `.kald` (§2)
   - Provider de strings `.lits` (§6)
   - Validation stricte des formats, gestion mmap

4. **`liturgical-calendar-runtime`** — Runtime library (`std`)
   - Provider Fast/Slow Path (§7)
   - Télémétrie et observabilité (§9)
   - Bindings FFI de haut niveau C/C++ (§10, feature optionnelle)
   - Gestion des corruptions et diagnostics (`eprintln!` autorisé ici)

5. **`kald-inspect`** — Diagnostic tool (binary, `std`)
   - Inspection de fichiers `.kald` (§8.2)
   - Détection de corruptions, validation formats et endianness

### A.2 Graphe de Dépendances

```
liturgical-calendar-core (0 dépendances externes — no_std, no_alloc)
          │
          ├─→ liturgical-calendar-io          (std — consomme validate_header)
          │         │
          │         └─→ liturgical-calendar-runtime   (std — Provider Fast/Slow)
          │                   │
          │                   └─→ (runtime utilisateurs)
          │
          ├─→ liturgical-calendar-forge               (std — Producer, consomme SlowPath)
          │
          └─→ kald-inspect                   (std — outil diagnostic)
```

**Propriétés** :

- Graphe acyclique (pas de dépendances circulaires)
- `liturgical-calendar-core` est la racine : aucune dépendance vers `liturgical-calendar-forge` ou `liturgical-calendar-runtime`
- `liturgical-calendar-forge` dépend de `liturgical-calendar-core` (consomme le Slow Path pour la génération AOT)
- Dépendances minimales : chaque crate tire uniquement ce qu'il nécessite

### A.3 Structure de Fichiers

```
liturgical-calendar/
├── Cargo.toml                          # Workspace root
├── README.md
├── LICENSE
│
├── crates/
│   ├── liturgical-calendar-core/       # Crate 1
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── types.rs
│   │   │   ├── easter.rs
│   │   │   ├── seasons.rs
│   │   │   ├── precedence.rs
│   │   │   └── error.rs
│   │   └── tests/
│   │
│   ├── liturgical-calendar-io/         # Crate 2
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── litu/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── header.rs
│   │   │   │   ├── reader.rs
│   │   │   │   └── writer.rs
│   │   │   └── lits/
│   │   │       ├── mod.rs
│   │   │       └── provider.rs
│   │   └── tests/
│   │
│   ├── liturgical-calendar-runtime/    # Crate 3
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── provider.rs
│   │   │   ├── telemetry.rs
│   │   │   └── ffi.rs              # feature = "ffi"
│   │   └── tests/
│   │
│   ├── liturgical-calendar-forge/      # Crate 4
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── main.rs
│   │   │   ├── builder.rs
│   │   │   ├── registry.rs
│   │   │   └── config.rs
│   │   └── tests/
│   │
│   └── kald-inspect/                   # Crate 5
│       ├── Cargo.toml
│       └── src/
│           └── main.rs
│
├── examples/                           # Exemples d'usage
│   ├── basic_usage.rs
│   ├── custom_config.rs
│   └── ffi_example.c
│
└── docs/
    └── architecture.md
```

### A.4 Cargo.toml Workspace (Racine)

```toml
[workspace]
resolver = "2"
members = [
    "crates/liturgical-calendar-core",           # Engine : no_std, no_alloc
    "crates/liturgical-calendar-forge",          # Forge  : std, Producer
    "crates/liturgical-calendar-io",
    "crates/liturgical-calendar-runtime",
    "crates/kald-inspect",
]

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "MIT OR Apache-2.0"
authors = ["Liturgical Calendar Contributors"]
repository = "https://github.com/user/liturgical-calendar"

[workspace.dependencies]
# Dépendances partagées (versions unifiées)
memmap2 = "0.9"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
clap = { version = "4.5", features = ["derive"] }

# Crates internes
liturgical-calendar-core = { path = "crates/liturgical-calendar-core" }
liturgical-calendar-forge = { path = "crates/liturgical-calendar-forge" }
liturgical-calendar-io = { path = "crates/liturgical-calendar-io" }
liturgical-calendar-runtime = { path = "crates/liturgical-calendar-runtime" }
```

> **`thiserror` retiré des workspace.dependencies** : le crate `core` est `no_std` et ne peut pas utiliser `thiserror` (qui dépend de `std::error::Error`). Les crates `io`, `forge` et `runtime` peuvent l'adopter localement si souhaité, mais la hiérarchie définie en §9.1 est suffisamment simple pour s'en passer.

**Cargo.toml du crate `liturgical-calendar-core` (Engine — no_std)**

```toml
# crates/liturgical-calendar-core/Cargo.toml
[package]
name = "liturgical-calendar-core"
version.workspace = true
edition.workspace = true

[features]
# Par défaut : no_std pur, zéro dépendance, zéro alloc
default = []
# Activer pour exposer impl std::error::Error sur DomainError (consommateurs std)
std = []

[dependencies]
# Aucune dépendance externe — zéro transitive closure
```

```rust
// crates/liturgical-calendar-core/src/lib.rs
#![no_std]
// INV-W1 : pas de extern crate alloc — zéro allocation autorisée dans ce crate.
// INV-W5 : aucun eprintln!/println! — tout diagnostic remonte via DomainError/HeaderError.
// core::fmt est disponible sans std (fourni par le compilateur Rust).

// Tout le code du crate n'accède qu'à core::*
// Pas de extern crate std, pas de alloc, pas de String, pas de Vec
```

### A.5 Principes Directeurs

**1. Core Stable**

- Le crate `core` a une API stable et minimaliste
- Versioning sémantique strict (1.x → 2.x rare)
- Rupture uniquement si les règles liturgiques canoniques changent

**2. Isolation I/O**

- La sérialisation est strictement séparée du calcul
- Formats binaires évolutifs sans impact sur le core
- Migration de formats documentée et testée

**3. Tools Optionnels**

- Les binaires CLI ne sont pas des dépendances obligatoires
- Installation séparée via `cargo install`
- Utilisables indépendamment du code

**4. FFI Feature**

- Les bindings C sont optionnels (feature flag)
- Activation : `liturgical-calendar-runtime = { features = ["ffi"] }`
- Isolation des dépendances système (libc)

**5. WASM-Ready**

- Le core compile en WebAssembly sans modification
- Zéro dépendance sur I/O système
- `#![no_std]` compatible

### A.6 Utilisation

**Calcul uniquement (Slow Path pur)** :

```toml
[dependencies]
liturgical-calendar-core = "0.1"
```

**Runtime complet (Fast/Slow Path + fichiers .kald)** :

```toml
[dependencies]
liturgical-calendar-runtime = "0.1"
```

**Avec bindings C/C++** :

```toml
[dependencies]
liturgical-calendar-runtime = { version = "0.1", features = ["ffi"] }
```

**Génération de .kald** :

```bash
$ cargo install liturgical-calendar-forge
$ liturgical-calendar-forge build --config france.toml --output france.kald
```

**Inspection de .kald** :

```bash
$ cargo install kald-inspect
$ kald-inspect france.kald --check
```

### A.7 Justification Multi-Crate

**Séparation des Responsabilités** :

- **Core** : Algorithmes purs, zéro I/O
- **I/O** : Formats binaires, mmap
- **Runtime** : Composition core + I/O + télémétrie
- **Tools** : CLI utilisateur final

**Évolutivité** :

- Ajout de nouveaux crates (serveur HTTP, export ICS) sans impact
- Feature flags limités (pas de matrice explosive)
- Dépendances ciblées (pas de bloat)

**Testabilité** :

- Tests du core : rapides, zéro I/O, déterministes
- Tests du runtime : avec fixtures .kald
- Isolation complète des suites de tests

**Contrôle des Dépendances** :

- Audit de sécurité facile (`cargo audit -p liturgical-calendar-core`)
- Core à zéro dépendance externe (auditable manuellement)
- Dépendances lourdes isolées (clap, serde dans tools uniquement)

**Coût Initial vs Long Terme** :

- Setup initial : +1 semaine
- Maintenance : -30% (compilation incrémentale, tests ciblés)
- Évolution : +50% facilité (ajout de features isolées)

### A.8 Séparation Moteur / Règles Liturgiques

**Principe** : L'architecture du projet doit strictement découpler le **moteur de calcul** de la **source des règles liturgiques**.

**Implémentation dans le Workspace** :

```
liturgical-calendar-core/
├── src/
│   ├── engine/              # Moteur de calcul (stable, no_std)
│   │   ├── slow_path.rs     # SlowPath::new(temporal, sanctoral, precedence)
│   │   ├── temporal.rs      # TemporalLayer — tableau [Option<FeastDefinition>; 367]
│   │   └── sanctoral.rs     # SanctoralLayer — tableau [Option<[Option<FeastDefinition>; 2]>; 366]
│   ├── static_rules.rs      # STATIC_TEMPORAL_DEFS, STATIC_SANCTORAL_DEFS, compute_day_static
│   └── lib.rs

liturgical-calendar-rules-roman/  # Crate std — Forge uniquement
├── src/
│   ├── hardcoded.rs         # HardcodedRuleProvider (TemporalRule + Vec — std)
│   │                        # .resolve_temporal() → Vec<FeastDefinition> pour SlowPath
│   └── lib.rs

liturgical-calendar-rules-aot/    # Crate std — Forge uniquement (v2.0+, optionnel)
├── src/
│   ├── generated.rs         # FeastDefinition statiques générées depuis YAML
│   └── lib.rs
```

**Graphe de Dépendances** :

```
liturgical-calendar-forge (std)
    ├─→ liturgical-calendar-core (no_std, no_alloc) — leaf, zéro dépendance externe
    └─→ liturgical-calendar-rules-roman (std)
            └─→ liturgical-calendar-core (types : FeastDefinition, Precedence, Nature, Color)
```

**`cargo tree -p liturgical-calendar-core` doit produire un arbre strictement vide.**
Toute dépendance sortante de `liturgical-calendar-core` est une violation d'INV-W4 bloquante pour le jalon Sanctuarisation.

**Contrat** :

`liturgical-calendar-core` n'expose aucun trait de provider. Le moteur ne connaît que `FeastDefinition` — la structure minimale Copy/Pod suffisante au calcul :

```rust
// liturgical-calendar-core/src/engine/slow_path.rs
// no_std, no_alloc — aucun import std::* ici

impl SlowPath {
    /// Construction depuis des slices de FeastDefinition pré-résolues.
    /// Appelé par la Forge (via rules-roman) ou depuis compute_day_static.
    pub fn new(
        temporal: &[FeastDefinition],
        sanctoral: &[FeastDefinition],
        precedence: &PrecedenceData,
    ) -> Self {
        Self {
            temporal: TemporalLayer::new(temporal),
            sanctoral: SanctoralLayer::new(sanctoral),
            precedence: PrecedenceResolver::new(precedence),
        }
    }
}
```

**Stratégie Évolutive** :

| Phase     | Implémentation    | Crate fournisseur                   | Entrée SlowPath                                        |
| --------- | ----------------- | ----------------------------------- | ------------------------------------------------------ |
| **v1.0**  | Hardcodée Rust    | `rules-roman` (std)                 | `Vec<FeastDefinition>` résolu par `resolve_temporal()` |
| **v2.0+** | AOT depuis YAML   | `rules-aot` (std)                   | `&'static [FeastDefinition]` générés                   |
| **FFI**   | Statique compilée | `liturgical-calendar-core` (no_std) | `STATIC_TEMPORAL_DEFS` (const)                         |

**Critère de Conformité** :

Le code du moteur (`liturgical-calendar-core/src/engine/`) ne doit **jamais** contenir :

- De types alloués sur le heap (`String`, `Vec`, `Box`)
- De traits de provider (`RuleProvider`, `TemporalRule`)
- De constantes liturgiques informelles (`if name == "Ascension"`)

Toute la connaissance liturgique allouée transite par `liturgical-calendar-forge` et est convertie en `FeastDefinition` avant d'atteindre le core.

---

## Annexe B : Conventions de Nommage

### B.1 Principes Généraux

Le projet Liturgical Calendar respecte strictement les conventions Rust idiomatiques :

1. **`snake_case`** : Fonctions, variables, modules
   - Exemple : `compute_easter()`, `day_of_year`, `slow_path`

2. **`PascalCase`** : Types (structs, enums, traits)
   - Exemple : `Day`, `Header`, `Provider`

3. **`SCREAMING_SNAKE_CASE`** : Constantes
   - Exemple : `KNOWN_FLAGS_V1`, `HEADER_SIZE`

**Principe Fondamental** : Les modules portent le contexte, pas les noms de types.

### B.2 Vocabulaire du Domaine

#### B.2.1 Latin Canonique

Les enums du domaine liturgique utilisent le latin, vocabulaire canonique de l'Église Catholique Romaine :

```rust
pub enum Color {
    Albus,      // Blanc
    Rubeus,     // Rouge
    Viridis,    // Vert
    Violaceus,  // Violet
    Roseus,     // Rose
    Niger,      // Noir
}

pub enum Precedence {
    TriduumSacrum                     = 0,
    SollemnitatesFixaeMaior           = 1,
    // ... 13 valeurs, voir §1.3
}

pub enum Nature {
    Sollemnitas,   // Solennité
    Festum,       // Fête
    Memoria,      // Mémoire
    Feria,        // Férie / Dimanche
    Commemoratio, // Commémoration
}

pub enum Season {
    TempusOrdinarium,   // Temps Ordinaire
    TempusAdventus,     // Avent
    TempusNativitatis,  // Temps de Noël
    TempusQuadragesimae,// Carême
    // ...
}
```

**Justification** :

- Évite les ambiguïtés de traduction (ex: "Ordinary Time" vs "Temps Ordinaire" vs "Tiempo Ordinario")
- Reste fidèle au vocabulaire liturgique officiel
- Facilite l'interopérabilité internationale

### B.3 Absence de Préfixes Redondants

**Antipattern : Préfixes C-like** ❌

```rust
// ❌ ÉVITER : Préfixes redondants
pub struct LiturgicalCalendarSlowPath { /* ... */ }
pub struct LiturgicalCalendarProvider { /* ... */ }
pub fn liturgical_calendar_compute_easter(year: i32) -> u16 { /* ... */ }
```

**Pattern Idiomatique : Contexte via Modules** ✅

```rust
// ✅ PRÉFÉRER : Modules portent le contexte
pub struct SlowPath { /* ... */ }
pub struct Provider { /* ... */ }
pub fn compute_easter(year: i32) -> u16 { /* ... */ }

// Usage avec chemin complet
use liturgical_calendar_core::SlowPath;
use liturgical_calendar_runtime::Provider;

let slow_path = SlowPath::new(rules);
let provider = Provider::new("data.kald", "lang.lits")?;
```

**Justification** :

- Le nom du crate (`liturgical_calendar_core`) fournit déjà le contexte complet
- Les types restent concis et lisibles
- Pattern standard Rust (cf. `std::io::Error` pas `std::io::StdIoError`)

### B.4 FFI C : Préfixe `kal_`

**Contexte** : Le C n'a pas de namespaces. Un préfixe est nécessaire pour éviter les collisions dans l'espace de noms global.

**Choix** : `kal_` (abréviation de "Kalendarium", l'artefact AOT produit par le moteur)

```c
// kal.h
// Note: The "kal_" prefix derives from "Kalendarium", the compiled annual
//       artifact produced by this engine. It keeps function names concise
//       while avoiding namespace collisions in C.

typedef struct KalProvider KalProvider;

KalProvider* kal_new(const char* data_path, const char* lang_path);
void kal_free(KalProvider* handle);
uint32_t kal_get_day(const KalProvider* h, int16_t year, uint16_t day);
```

**Justification** :

- Plus court que `liturgical_calendar_*` (24 caractères)
- Cohérent avec des pratiques courantes :
  - libgit2 → `git_*`
  - libssh2 → `ssh2_*`
  - libcurl → `curl_*`
- Dérive de "Kalendarium" (artefact AOT), terme officiel du projet
- Abréviation explicitement documentée

### B.5 Alias de Crates Recommandés

Pour réduire la verbosité des imports dans le code utilisateur :

```rust
// Sans alias (verbeux)
use liturgical_calendar_core::SlowPath;
use liturgical_calendar_core::types::Day;
use liturgical_calendar_runtime::Provider;
use liturgical_calendar_rules_roman::HardcodedRuleProvider;

// Avec alias (recommandé)
use liturgical_calendar_core as core;
use liturgical_calendar_runtime as runtime;
use liturgical_calendar_rules_roman as rules;

let provider = rules::HardcodedRuleProvider::new();
let slow_path = core::SlowPath::new(provider);
let runtime_provider = runtime::Provider::new("data.kald", "lang.lits")?;
```

**Alternative : Crate "Façade" (Optionnel)**

Un crate racine peut réexporter les APIs principales :

```rust
// liturgical-calendar/src/lib.rs (crate façade optionnel)
pub use liturgical_calendar_core::*;
pub use liturgical_calendar_runtime::Provider;

// Usage simplifié
use liturgical_calendar::SlowPath;
use liturgical_calendar::Provider;
```

### B.6 Variables et Noms Locaux

**Principe** : Le type et le contexte suffisent, pas besoin de répéter l'information.

**Antipattern** ❌ :

```rust
let liturgical_day_logic = provider.get_day(2025, 1);
let liturgical_season_boundaries = SeasonBoundaries::compute(2025)?;
```

**Pattern Idiomatique** ✅ :

```rust
let day = provider.get_day(2025, 1);
let boundaries = SeasonBoundaries::compute(2025)?;
```

**Justification** : Le type est déjà explicite via l'annotation ou l'inférence.

### B.7 Noms de Modules

**Structure Recommandée** :

```
liturgical-calendar-core/src/
├── types.rs           // Types de domaine
├── easter.rs          // Algorithmes Pâques
├── seasons.rs         // Frontières temporelles
├── rules/
│   ├── mod.rs
│   ├── provider.rs    // Trait RuleProvider
│   └── types.rs       // TemporalRule, SanctoralFeast
└── engine/
    ├── mod.rs
    ├── slow_path.rs
    ├── temporal.rs
    └── sanctoral.rs
```

**Imports** :

```rust
use liturgical_calendar_core::types::Day;
use liturgical_calendar_core::rules::RuleProvider;
use liturgical_calendar_core::engine::SlowPath;
```

### B.8 Cohérence Terminologique

**Principe** : Utiliser un vocabulaire consistant dans tout le projet.

| Concept                | Terme Utilisé      | Éviter                   |
| ---------------------- | ------------------ | ------------------------ |
| Fournisseur de données | `Provider`         | `Supplier`, `Source`     |
| Règles liturgiques     | `Rule`             | `Policy`, `Regulation`   |
| Couche de calcul       | `Layer`            | `Level`, `Tier`          |
| Chemin de calcul       | `Path` (Fast/Slow) | `Route`, `Mode`          |
| Fête liturgique        | `Feast`            | `Holiday`, `Celebration` |

### B.9 Exemples Conformes

**✅ Excellent** :

```rust
pub struct Provider { /* ... */ }
pub fn compute_easter(year: i32) -> Option<(u8, u8)> { /* ... */ }
pub enum Color { Albus, Rubeus, /* ... */ }
const KNOWN_FLAGS_V1: u16 = 0x0000;
```

**❌ À Éviter** :

```rust
pub struct LiturgicalCalendarRuntimeProvider { /* ... */ }  // Redondant
pub fn LiturgicalComputeEaster(year: i32) -> (u8, u8) { /* ... */ }  // PascalCase fonction
pub enum Color { White, Red, /* ... */ }  // Perd le contexte latin
const KnownFlagsV1: u16 = 0x0000;  // Pas SCREAMING_SNAKE_CASE
```

### B.10 Checklist de Conformité

Avant l'implémentation, vérifier :

- [ ] Aucun préfixe `liturgical_calendar_*` dans les noms de types Rust
- [ ] FFI utilise `kal_*` avec note explicative
- [ ] Enums liturgiques en latin (sauf justification)
- [ ] `Rank` absent du codebase — modèle 2D uniquement (`Precedence` + `Nature`)
- [ ] `snake_case` pour fonctions et variables
- [ ] `PascalCase` pour types et enums
- [ ] `SCREAMING_SNAKE_CASE` pour constantes
- [ ] Modules utilisés pour porter le contexte
- [ ] Variables locales concises (pas de répétition du type)

---

### B.11 Schéma de Configuration AOT (Contrat YAML)

#### B.11.1 Philosophie

Le fichier YAML est le **frontend de la Forge**. Il constitue la surface d'entrée humaine du pipeline AOT.

Flux de transformation :

```
YAML (slug, precedence, valid_from…)
  → Forge (validation, résolution slug→FeastID, tri temporel)
    → .kald (DayPacked u32, lecture seule au runtime)
```

**Invariants absolus :**

- Toute entrée YAML est **validée à la compilation** (AOT). Aucune erreur de configuration ne peut atteindre le runtime.
- Le `slug` est la clé de déduplication humaine. La Forge le transforme en `FeastID` (18 bits) via la `FeastRegistry`. Le slug n'existe pas dans le binaire.
- La Forge rejette tout fichier YAML contenant des slugs en collision, des plages temporelles incompatibles, ou des valeurs hors domaine.
- Les champs `valid_from` / `valid_to` expriment des années grégoriennes entières. Bornes inclusives. Plage maximale : [1583, 4099].

**Convention de nommage des slugs — neutralité obligatoire :**

Le slug identifie la **personne ou l'événement**, pas son statut liturgique courant.

```
✅  ioannis_pauli_ii        ← stable dans le temps, indépendant du statut
❌  s_ioannis_pauli_ii      ← encode "Sanctus" — la clé change à la canonisation
❌  b_caroli_de_foucauld    ← encode "Beatus" — invalide si canonisé ultérieurement
```

**Justification structurelle :** le `slug` est la clé primaire du `registry.lock`. Si le statut (Beatus → Sanctus) est encodé dans le slug, la canonisation force un changement de clé → un nouveau `FeastID` → une rupture de continuité historique dans toutes les forges qui ont déjà alloué l'ancien identifiant.

L'évolution du titre (Saint, Bienheureux, Vénérable) est portée par le champ `history` dans le bloc YAML — pas par le slug. Le slug est **immuable après première allocation**.

```
slug : ioannis_pauli_ii   ← ne change jamais
  history:
    - valid_from: 2011     ← Béatification
      title: "B. Ioannes Paulus II"
      precedence: 11
    - valid_from: 2014     ← Canonisation
      title: "S. Ioannes Paulus II"
      precedence: 12
```

---

#### B.11.2 Mapping YAML ↔ Types Rust

| Clé YAML               | Type YAML       | Type Rust (`core`)        | Contraintes                                                                                                                                                              |
| ---------------------- | --------------- | ------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `slug`                 | String          | `&'static str` (post-AOT) | Latin snake_case. **Neutre** : identifie la personne/événement, pas le statut liturgique. Immuable après première allocation dans le `registry.lock`. Absent du binaire. |
| `date`                 | `[month, day]`  | `(u8, u8)`                | Fêtes fixes sanctorales uniquement. Absent pour les fêtes mobiles (`offset_days`).                                                                                       |
| `offset_days`          | Integer         | `i16`                     | Offset relatif à Pâques. Absent pour les fêtes fixes.                                                                                                                    |
| `scope`                | String (enum)   | `FeastScope`              | `universal`, `regional`, `national`, `local`. Détermine les bits [17:16] du `FeastID`.                                                                                   |
| `category`             | Integer         | u8 (4 bits)               | Bits [15:12] du `FeastID`.                                                                                                                                               |
| `id`                   | Integer (hex)   | `u32` (18 bits)           | Optionnel. Si absent, alloué par la `FeastRegistry`. Si présent, vérifié contre le registry pour détecter les collisions.                                                |
| `history`              | Séquence        | `Vec<FeastVersion>`       | Liste ordonnée des versions temporelles de la fête. Chaque entrée couvre une plage `[valid_from, valid_to]`. La Forge valide l'absence de chevauchement.                 |
| `history[].valid_from` | Integer         | `i16`                     | Défaut : `1969`. Borne inclusive.                                                                                                                                        |
| `history[].valid_to`   | Integer \| null | `Option<i16>`             | Défaut : `None` (version toujours active). Borne inclusive.                                                                                                              |
| `history[].title`      | String          | `&'static str` (post-AOT) | Nom canonique displayable pour cette période (ex: `"B. Ioannes Paulus II"`). Stocké dans le `.lits`, absent du `.kald`.                                                  |
| `history[].precedence` | Integer         | `Precedence` (u8, 4 bits) | Domaine strict : [0, 12]. Valeurs 13–15 réservées système. **Hiérarchie inverse : valeur plus faible = priorité plus haute** (voir §4.4).                                |
| `history[].nature`     | String (enum)   | `Nature`                  | Valeurs admises : `sollemnitas`, `festum`, `memoria`, `feria`, `commemoratio`. "Beatus/Beata" n'est pas une `Nature` — utiliser `memoria`.                               |
| `history[].color`      | String (enum)   | `Color`                   | Valeurs : `albus`, `rubeus`, `viridis`, `violaceus`, `roseus`, `niger`. Normalisé par `normalize_color()` (Forge uniquement, §5.1).                                      |

---

#### B.11.3 Exemple : Ioannes Paulus II — Slug Neutre et Versioning Temporel

Jean-Paul II illustre deux invariants simultanément : la **neutralité du slug** et le **versioning temporel via `history`**.

```yaml
# config/roman-rite-ordinary.yaml

sanctoral_feasts:
  - slug: ioannis_pauli_ii # ← neutre, stable, immuable après allocation
    date: [10, 22]
    scope: national
    region: PL
    category: 1

    history:
      # Version 1 : Béatification (2011)
      # "Beatus" est un statut canonique, pas une Nature → Nature::Memoria
      # precedence: 11 = MemoriaeObligatoriae dans le calendrier polonais
      - valid_from: 2011
        valid_to: 2013
        title: "B. Ioannes Paulus II"
        precedence: 11
        nature: memoria
        color: albus

      # Version 2 : Canonisation (2014)
      # Le slug ne change pas — seul le title et la precedence évoluent.
      # precedence: 12 = FeriaePerAnnumEtMemoriaeAdLibitum (Memoria facultative)
      #
      # LECTURE DE LA HIÉRARCHIE (voir §4.4) :
      # precedence 11 (obligation nationale) < precedence 12 (facultatif universel)
      # → 11 a une priorité plus haute que 12 dans le moteur d'éviction.
      # Les deux peuvent coexister : scopes distincts (national / universal),
      # plages temporelles disjointes (2011-2013 / 2014-présent).
      - valid_from: 2014
        valid_to: ~ # None : version toujours active
        title: "S. Ioannes Paulus II"
        precedence: 12
        nature: memoria
        color: albus
```

**Pourquoi `precedence` monte de 11 à 12 à la canonisation :**

La hiérarchie est **numérique inverse** (§4.4) : valeur plus faible = priorité plus haute. La canonisation inscrit la fête au Calendarium Generale avec le rang de Memoria ad libitum (facultative), valeur 12. La béatification dans un calendrier national avait le rang de Memoria obligatoria, valeur 11. La valeur 11 est **plus contraignante** que 12 — attendu : l'obligation nationale prime sur le facultatif universel.

**Résolution AOT par la Forge :**

Pour une année `y`, la Forge sélectionne l'entrée `history[]` dont `[valid_from, valid_to]` contient `y`. Si deux entrées d'un même slug couvrent la même année et le même scope, c'est une erreur de configuration — rejetée à la compilation.

```rust
// Pseudo-code Forge — résolution de version temporelle
fn resolve_feast_for_year(
    slug: &str,
    history: &[FeastVersion],
    year: i16,
) -> Result<Option<&FeastVersion>, RegistryError> {
    let candidates: Vec<_> = history
        .iter()
        .filter(|v| v.valid_from <= year)
        .filter(|v| v.valid_to.map_or(true, |to| year <= to))
        .collect();

    match candidates.len() {
        0 => Ok(None),                // Fête inexistante pour cette année
        1 => Ok(Some(candidates[0])), // Résolution unique — cas nominal
        _ => Err(RegistryError::TemporalOverlap {
            slug: slug.to_string(),
            year,
            conflicting_entries: candidates.len(),
        }),
    }
}
```

---

#### B.11.4 Invariants de Validation AOT

La Forge applique ces contrôles avant toute génération de `.kald`. Un seul échec interrompt la compilation.

**V1 — Unicité temporelle dans le bloc `history` (par scope)**

```
∀ slug s, ∀ scope sc, ∀ année y ∈ [1583, 4099] :
  |{ versions v ∈ history(s, sc) | v.valid_from ≤ y ≤ v.valid_to_or_max }| ≤ 1
```

Violation → `RegistryError::TemporalOverlap`.

**V2 — Domaine de Precedence**

```
∀ entrée e : e.precedence ∈ [0, 12]
```

Valeurs 13–15 : réservées système. Violation → `RegistryError::InvalidPrecedenceValue`.

**V3 — Capacité FeastID (18 bits)**

```
∀ (scope, category) : allocated_count(scope, category) ≤ 4095
```

Dépassement → `RegistryError::FeastIDExhausted`. Le registre alloue séquentiellement dans les bits [11:0].

**V4 — Cohérence des plages temporelles**

```
∀ entrée e : e.valid_from ≤ e.valid_to_or_max
∧ e.valid_from ≥ 1583
∧ e.valid_to_or_max ≤ 4099
```

Violation → `RegistryError::InvalidTemporalRange`.

**V5 — Nature conforme aux enums du crate `core`**

Seules les valeurs de `Nature` définies en §1.3 sont admises. Tout autre terme est rejeté avec le message :

```
RegistryError::UnknownNatureString("beatus")
→ hint: "Beatus/Beata est un statut canonique, pas une Nature. Utiliser Nature::Memoria."
```

**V6 — Slug syntaxiquement valide**

```
slug ∈ [a-z0-9_]+   (latin snake_case, sans accent, sans tiret, sans espace)
```

Le préfixe conventionnel (`s_`, `b_`, `t_`, `d_`) est recommandé mais non imposé par le validateur.

**Tableau récapitulatif des erreurs de validation :**

| Code | Variant `RegistryError`                               | Déclencheur                                               |
| ---- | ----------------------------------------------------- | --------------------------------------------------------- |
| V1   | `TemporalOverlap { slug, year, conflicting_entries }` | Deux entrées actives le même jour pour le même slug/scope |
| V2   | `InvalidPrecedenceValue(u8)`                          | Precedence > 12                                           |
| V3   | `FeastIDExhausted { scope, category }`                | Dépassement des 4095 FeastID par scope/category           |
| V4   | `InvalidTemporalRange { valid_from, valid_to }`       | valid_from > valid_to, ou hors [1583, 4099]               |
| V5   | `UnknownNatureString(String)`                         | Valeur de `nature` non reconnue                           |
| V6   | `InvalidSlugSyntax(String)`                           | Caractère illicite dans le slug                           |

---

#### B.11.5 Checklist YAML

Avant de soumettre un fichier de configuration à la Forge :

- [ ] Tous les slugs sont en latin snake*case **neutre** — aucun statut liturgique encodé (pas de `s*`, `b\_` comme préfixe porteur de sens)
- [ ] Le slug est stable dans le temps : il identifie la personne/l'événement, pas son rang courant
- [ ] L'évolution du titre et de la `precedence` est portée par des entrées `history[]` distinctes, non par un changement de slug
- [ ] Les plages `[valid_from, valid_to]` du bloc `history` sont disjointes pour un même slug/scope
- [ ] `precedence` ∈ [0, 12] pour chaque entrée `history[]`
- [ ] `nature` est l'une des 5 valeurs admises (§1.3) — aucun terme canonique informel ("beatus", "venerabilis", etc.)
- [ ] `valid_from` est renseigné explicitement si différent de 1969
- [ ] `id` absent (allocation automatique) sauf besoin d'un identifiant stable documenté
- [ ] Les entrées `scope: national` portent un champ `region` (code ISO 3166-1 alpha-2)

---

---

## Annexe C : Jalon "Sanctuarisation du Core" — Référence Roadmap

Les étapes d'implémentation, checklists et tableau de suivi du jalon "Sanctuarisation du Core" sont définis exclusivement dans la **Roadmap v2.1** (document `roadmap.md`).

L'architecture résultante de ce jalon est capturée dans la présente spécification en :

- **§0** — Invariants structurels du workspace (INV-W1 à INV-W5)
- **§2.1** — `#[repr(C)]` sur `Header`, `HeaderError::PossibleEndiannessMismatch`, invariant ZSTD/`no_alloc`
- **§5.1.1** — Pivot DOD : `FeastDefinition` comme seul type de règle dans le core, `STATIC_TEMPORAL_DEFS`, `compute_day_static`
- **§10.3** — Interface FFI Engine (`kal_validate_header` avec mapping exhaustif, `kal_compute_day`, `kal_read_day`, `kal_index_day`)
- **§10.3.4** — Test de conformité binaire Forge↔Engine (critère de sortie du jalon)

**Critère de sortie non-négociable** :

```bash
$ cargo tree -p liturgical-calendar-core
liturgical-calendar-core v0.1.0
# Arbre strictement vide — zéro dépendance externe.
# Toute ligne supplémentaire invalide le jalon.
```

---

**Fin de la Spécification Technique v2.3 — Ready for Implementation**

_Document révisé le 2026-03-05. Intègre les corrections d'audit v2.3 R1–R5 : `day_of_year_to_month_day` `unreachable_unchecked` + bloc SAFETY (R1), `kal_index_day` sentinelle `usize::MAX` (R2), `FeastDefinitionPacked` (`NonZeroU32`) + niche optimization dans TemporalLayer/SanctoralLayer (R3), API `kal_scan_precedence` §6.3 SIMD-ready (R4), StringProvider `from_le_bytes` endianness LE canonique (R5). Basé sur v2.2 (2026-03-05)._
