# Spécification Technique : Liturgical Calendar v2.2

**Statut** : Canonique / Ready for Implementation  
**Architecture** : AOT-Only / DOD / FFI-First  
**Workspace** : `liturgical-calendar-forge` (std) / `liturgical-calendar-core` (no_std, no_alloc)  
**Langage Domaine** : Latin (Strictement Canonique)  
**Déterminisme** : Bit-for-bit reproductible  
**Date de Révision** : 2026-04-10  
**Version** : 2.2.0

---

## 0. Séparation Workspace : Forge et Engine

### 0.1 Deux Composants Distincts

| Composant      | Crate Cargo                 | Modèle mémoire            | Rôle                                                                                                        |
| -------------- | --------------------------- | ------------------------- | ----------------------------------------------------------------------------------------------------------- |
| **The Forge**  | `liturgical-calendar-forge` | `std`                     | Producer : ingestion YAML, validation canonique (V1–V6), résolution des conflits, génération AOT du `.kald` |
| **The Engine** | `liturgical-calendar-core`  | `#![no_std]` sans `alloc` | Consumer : projecteur de mémoire O(1) sur `.kald`, interface FFI                                            |

Toute dépendance de l'Engine vers la Forge constitue une **violation architecturale**.

### 0.2 Invariants Structurels du Workspace

**INV-W1 — Engine `no_std` sans `alloc`**

Le pragma exact à utiliser dans `liturgical-calendar-core/src/lib.rs` est :

```rust
#![cfg_attr(not(test), no_std)]
```

Le mode `cargo test` compile avec `std` pour le harness — `#![no_std]` inconditionnel rend le harness inopérant. Hors test, `no_std` est actif sans exception. `Vec`, `String`, `Box`, `HashMap` restent **interdits dans le code de production** — le `cfg(test)` n'ouvre aucun droit d'utilisation dans les modules non-test. `extern crate alloc` reste interdit dans tous les contextes.

**Dépendances externes autorisées** : les crates certifiées `no_std` + `no_alloc` et auditées par des tiers sont admises sous condition d'approbation explicite dans ce document. Liste figée :

- `sha2` v0.10+ (RustCrypto) avec `default-features = false` — validation du checksum `.kald`

Critère de validation mécanisé : `cargo tree -p liturgical-calendar-core` ne doit contenir **aucune** dépendance vers `std`, `alloc`, ou toute crate non listée ci-dessus. Toute nouvelle dépendance requiert une révision de ce document et une approbation explicite.

**INV-W2 — Engine Stateless et Opaque**

L'Engine opère exclusivement sur des buffers fournis par l'appelant (`&[u8]`, out-params). Aucune allocation interne. Seuls états internes autorisés : tableaux statiques de taille fixe (`[T; N]`).

**INV-W3 — Interface C-ABI Obligatoire**

Toutes les fonctions publiques exportées utilisent `extern "C"`. Aucune signature publique ne retourne `Result<T, E>` à la frontière FFI — les erreurs transitent par un `i32` et des out-params. Interopérabilité garantie avec Zig, C, Wasm sans glue layer.

**INV-W4 — Flux unidirectionnel Forge → Engine**

La Forge peut dépendre de l'Engine (lecture/validation). L'Engine ne dépend jamais de la Forge. Graphe de dépendances acyclique et unidirectionnel.

**INV-W5 — Zéro diagnostic de l'Engine**

L'Engine ne produit aucun output (`eprintln!`, `println!`, `log::*`). Tout chemin d'erreur remonte via code de retour entier ou out-param. Le diagnostic est la responsabilité exclusive de la couche appelante (`std`).

---

**INV-W6 — `CalendarEntry::zeroed()` est `const fn` pub**

La méthode `CalendarEntry::zeroed()` est `const fn` et publique. Elle est la méthode canonique pour les pré-allocations déterministes en contexte `no_alloc`. Cet invariant est référencé ici pour éviter toute suppression accidentelle lors d'une refactorisation de `entry.rs` — sa spécification complète figure dans §0.3.

---

**INV-W7 — Politique de lint `missing_docs` par crate et par jalon**

| Crate                       | Jalon 2     | Jalon 3    |
| --------------------------- | ----------- | ---------- |
| `liturgical-calendar-core`  | `#![warn]`  | `#![warn]` |
| `liturgical-calendar-forge` | `#![allow]` | `#![warn]` |

Justification : le Core est une surface FFI publique certifiée — sa documentation est une contrainte de livraison dès Jalon 1. La Forge est un outil interne AOT — son API publique n'est stabilisée qu'en Jalon 3. Activer `warn(missing_docs)` sur la Forge en Jalon 2 génère plus de 100 warnings qui masquent les erreurs de compilation réelles.

**Pragma canonique pour `liturgical-calendar-forge/src/lib.rs` en Jalon 2 :**

```rust
#![allow(missing_docs)] // Activé en Jalon 3 — voir INV-W7
```

---

**INV-W8 — Ownership des données dans le pipeline Forge**

Les méthodes du pipeline interne (Étapes 1–5) **consomment** leurs entrées par move semantics. Aucune signature interne n'accepte `&[T]` sur le chemin chaud quand l'ownership est transféré de toute façon en aval.

Corollaire pour les tests unitaires : tout vecteur passé à une méthode qui consomme par move doit être `.clone()`-é **avant** l'appel si sa valeur est lue après (assertion, message d'erreur).

```rust
// Forme correcte dans les tests
let ids1 = vec![0x0001u16, 0x0002];
let idx1 = pool.insert(ids1.clone()).unwrap();
// ids1 encore valide ici pour l'assertion
assert_eq!(idx1, expected, "ids={:?}", ids1);
```

Exception : `PoolBuilder::insert` peut évoluer vers `&[u16]` en Jalon 3 si le profiling démontre un coût de clonage mesurable sur corpus complet. La décision est reportée à données réelles — pas anticipée.

---

**INV-W9 — Version Rust minimale : 1.77.0**

`offset_of!` est stabilisé en 1.77. La spec exige cette macro dans les tests de layout de l'Engine (§1.3 roadmap). Une toolchain antérieure rend ces tests non compilables.

```toml
# liturgical-calendar-core/Cargo.toml
rust-version = "1.77"
```

Conséquence CI (Jalon 3) : la matrice de cibles doit inclure `rust: "1.77"` comme version minimale testée, en plus de `stable` et `nightly`. Toute utilisation d'une feature stabilisée après 1.77 dans le Core requiert une révision de cet invariant.

### 0.3 Responsabilités par Composant

**Forge (`liturgical-calendar-forge`, std)** :

- Ingestion et validation des fichiers YAML de configuration liturgique
- Validations canoniques V1–V6 (§10)
- `FeastRegistry` (BTreeMap, allocation, import/export)
- Fonctions de normalisation chaînes (`normalize_color`, `normalize_nature`)
- Algorithme de Pâques (Meeus/Jones/Butcher) — exécuté dans la Forge, résultat figé
- `SeasonBoundaries::compute` — arithmétique pure, déplacée de l'Engine vers la Forge
- Résolution complète des préséances et transferts (Conflict Resolution)
- Calcul des DOY via la table `MONTH_STARTS` (§2.2)
- Placement de la Padding Entry (`doy = 59`, années non-bissextiles)
- Construction du Secondary Pool (avec déduplication)
- Calcul SHA-256 sur `[Data Body ∥ Secondary Pool]`
- Sérialisation binaire (`Calendar::write_kald`)
- Format `.lits` et `StringProvider` (production)

**Engine (`liturgical-calendar-core`, no_std, no_alloc)** :

- Types de domaine canoniques : `Precedence`, `Nature`, `Color`, `LiturgicalPeriod`
- `Header` (`#[repr(C)]`) — lecture et validation structurelle
- `CalendarEntry` (`#[repr(C)]`) — lecture et décodage des flags
- `kal_validate_header` — validation header + bounds check + SHA-256
- `kal_read_entry` — lecture O(1) par `(year, doy)`
- `kal_read_secondary` — lecture du Secondary Pool pour une entrée donnée
- `kal_scan_flags` — scan vectoriel du Data Body par masque (SIMD-ready)
- `StringProvider` (consommation du `.lits`)

**Production de la staticlib FFI :**

Le `crate-type` de `liturgical-calendar-core` est `"lib"` (rlib) uniquement en configuration standard. Déclarer `["lib", "staticlib"]` simultanément rend `cargo test` inopérant : le `staticlib` en `no_std` requiert un `#[panic_handler]` qui entre en conflit avec `std` dans le contexte harness (`duplicate lang item: panic_impl`).

La staticlib C est produite par invocation explicite :

```bash
cargo rustc -p liturgical-calendar-core --release -- --crate-type staticlib
```

Le `#[panic_handler]` requis par la staticlib est fourni par un shim dédié, **exclu de `cargo test`** via `cfg` :

```rust
// panic_shim.rs — exclu du harness test
#[cfg(not(test))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    extern "C" { fn abort() -> !; }
    unsafe { abort() }
}
```

Les profils `panic = "abort"` sont déclarés dans le `Cargo.toml` racine du workspace (les `Cargo.toml` des membres sont ignorés pour ce champ — comportement Cargo).

**Surface API publique de `CalendarEntry` (Engine) :**

```rust
impl CalendarEntry {
    /// Construit une entrée nulle. Tous les champs à zéro.
    /// `const fn` — utilisable en contexte statique et `no_alloc`.
    /// Requise par les tests d'intégration Forge (dev-dependency).
    pub const fn zeroed() -> Self;

    /// `true` si l'entrée est une Padding Entry ou un slot vide.
    pub fn is_padding(&self) -> bool;

    // Décodeurs flags — inchangés
    pub fn precedence(&self) -> Result<Precedence, DomainError>;
    pub fn color(&self)      -> Result<Color, DomainError>;
    pub fn liturgical_period(&self) -> Result<LiturgicalPeriod, DomainError>;
    pub fn nature(&self)     -> Result<Nature, DomainError>;
}

impl Default for CalendarEntry {
    /// Délègue à `zeroed()`. Permet `vec![CalendarEntry::default(); N]`.
    fn default() -> Self { Self::zeroed() }
}
```

**Invariant INV-W6 — `CalendarEntry::zeroed()` est `const fn` :**

La méthode doit être `const fn` pour rester compatible avec les contextes `no_alloc` et les
initialisateurs statiques. `Default::default()` ne peut pas être `const fn` en Rust stable —
`zeroed()` est donc la méthode canonique pour les pré-allocations déterministes.

### 0.4 Contrat de Types à la Frontière FFI

| Type                    | Représentation  | Statut FFI                 |
| ----------------------- | --------------- | -------------------------- |
| `Header`                | `#[repr(C)]`    | ✅ Compatible              |
| `CalendarEntry`         | `#[repr(C)]`    | ✅ Compatible              |
| `i32`                   | type C natif    | ✅ Code de retour standard |
| `*const u8` / `*mut u8` | pointeur C      | ✅ Buffer appelant         |
| `u8`, `u16`, `u32`      | types primitifs | ✅ Compatible              |
| `Result<T, E>`          | type Rust       | ❌ Non-représentable en C  |
| `String`, `Vec<T>`      | types alloués   | ❌ Interdits dans l'Engine |

---

## 1. Philosophie Architecturale

Le calendrier liturgique est un système de **droit positif** (réformes : 1582, 1914, 1969) — pas une constante physique calculable perpétuellement par un algorithme.

**Plage couverte : 1969–2399 (431 ans).** Baseline : réforme de Paul VI (Novus Ordo, 1969). Au-delà de 2399, l'évolution du droit liturgique invaliderait tout calcul.

Le pipeline correct est :

```
règles (époque-spécifiques) → compilation AOT (Forge) → dataset matérialisé (.kald)
```

**L'Engine ne connaît pas les règles. Il lit uniquement le dataset.**

La Forge est un **compilateur de droit liturgique**. L'Engine est un **projecteur de mémoire** O(1), sans aucune logique de domaine — plus aucun calcul de Pâques, plus aucune résolution de précédence au runtime.

---

## 2. Convention DOY (Day of Year)

### 2.1 Définition

`doy ∈ [0, 365]`. Janvier 1 = 0. Décembre 31 = 365. Convention **0-based**.

**Invariant central — index 59 fixe pour le 29 février :**

Le 29 février occupe toujours l'index `doy = 59`, qu'il soit réel (année bissextile) ou padding (année non-bissextile, `primary_id = 0`). Le 1er mars est toujours à l'index `doy = 60`. L'offset de toute date `(M, D)` est une **constante de compilation**, indépendante du caractère bissextile. L'Engine n'a jamais besoin de calculer `is_leap_year`.

### 2.2 Table MONTH_STARTS (constante pour toutes les années)

| Mois    | DOY de début | Mois      | DOY de début |
| ------- | ------------ | --------- | ------------ |
| Janvier | 0            | Juillet   | 182          |
| Février | 31           | Août      | 213          |
| Mars    | 60           | Septembre | 244          |
| Avril   | 91           | Octobre   | 274          |
| Mai     | 121          | Novembre  | 305          |
| Juin    | 152          | Décembre  | 335          |

### 2.3 Formule d'Index

```rust
let idx: u32 = (year as u32 - 1969) * 366 + doy as u32;
// Bounds check obligatoire avant tout accès :
// idx < header.entry_count, sinon KAL_ERR_INDEX_OOB
```

> **Ordre d'exécution obligatoire :** les guards sur `year` et `doy` doivent précéder ce calcul — voir §7.2 pour l'implémentation canonique. Sans guard préalable, `year as u32 - 1969` produit un wrap silencieux en release si `year < 1969`.

`idx` est un index dans le tableau de `CalendarEntry` du Data Body. Chaque `CalendarEntry` faisant 8 octets, l'offset en octets depuis le début du Data Body est `idx * 8`. L'offset en octets depuis le début du fichier est `64 + idx * 8`.

---

## 3. Format Binaire `.kald` v2.0

Toutes les valeurs numériques utilisent l'**endianness Little-Endian canonique** (`from_le_bytes` / `to_le_bytes`). Déterminisme SHA-256 cross-platform garanti.

### 3.1 Structure Générale du Fichier

```
[ Header : 64 octets ]
[ Data Body : entry_count × 8 octets ]
[ Secondary Pool : pool_size octets ]
```

Invariant de validation Engine :

```
file_size == 64 + (entry_count × 8) + pool_size
```

### 3.2 Header (64 octets, align 8)

`#[repr(C)]`

| Champ         | Type      | Offset | Valeur / Note                                                          |
| ------------- | --------- | ------ | ---------------------------------------------------------------------- |
| `magic`       | `[u8; 4]` | 0      | `"KALD"` (0x4B414C44)                                                  |
| `version`     | `u16`     | 4      | `4`                                                                    |
| `variant_id`  | `u16`     | 6      | `0` = Ordinaire ; extensible pour rites futurs                         |
| `epoch`       | `u16`     | 8      | `1969` (première année couverte)                                       |
| `range`       | `u16`     | 10     | `431` (nombre d'années couvertes)                                      |
| `entry_count` | `u32`     | 12     | `range × 366` — invariant de bounds checking                           |
| `pool_offset` | `u32`     | 16     | Offset en **octets** depuis le début du fichier vers le Secondary Pool |
| `pool_size`   | `u32`     | 20     | Taille en **octets** du Secondary Pool                                 |
| `checksum`    | `[u8;32]` | 24     | SHA-256 sur `[Data Body ∥ Secondary Pool]` (header exclu)              |
| `_reserved`   | `[u8; 8]` | 56     | `0x00 × 8` — doit être nul à la validation                             |

**Total : 64 octets.** `pool_offset` vaut `64 + entry_count * 8` pour un fichier sans padding.

**Build ID — valeur dérivée, non stockée :**

Le Build ID est défini comme `checksum[0..8]` (les 8 premiers octets du SHA-256). Il identifie de façon unique un build spécifique du `.kald` sans nécessiter de champ supplémentaire ni d'UUID non-déterministe. Deux Forges identiques compilant le même corpus YAML produisent le même Build ID — propriété de déterminisme bit-for-bit conservée.

Le `.lits` embarque ce Build ID dans son propre header (§9). L'Engine/client vérifie la cohérence avant toute utilisation conjointe des deux fichiers.

### 3.3 CalendarEntry (8 octets, stride constant)

`#[repr(C)]`

| Champ             | Type  | Offset | Note                                                                             |
| ----------------- | ----- | ------ | -------------------------------------------------------------------------------- |
| `primary_id`      | `u16` | 0      | FeastID de la célébration principale. `0` = Padding Entry                        |
| `secondary_index` | `u16` | 2      | Index dans le tableau `u16` du Secondary Pool (ignoré si `secondary_count == 0`) |
| `flags`           | `u16` | 4      | Voir layout §3.4                                                                 |
| `secondary_count` | `u8`  | 6      | Nombre de commémorations. `0` = aucune                                           |
| `_reserved`       | `u8`  | 7      | `0x00` — padding pour stride 64 bits                                             |

> **Justification du layout :** les trois champs `u16` (`primary_id`, `secondary_index`, `flags`) sont aux offsets 0, 2, 4 — alignement naturel sur 2 octets garanti en `#[repr(C)]`. Placer `secondary_count (u8)` avant `flags (u16)` aurait produit un offset impair (5) pour `flags` — accès non-aligné, UB potentiel sur architectures strictes, dégradation de performance sur x86.

> **Note de sémantique** : `secondary_index` est un index dans le **tableau de `u16`** du Secondary Pool (unité : éléments). `pool_offset` dans le Header est un offset en **octets** depuis le début du fichier. Ces deux champs ont des sémantiques intentionnellement différentes.

**Padding Entry** : entrée avec `primary_id = 0`, `secondary_count = 0`, `flags = 0`. Placée à `doy = 59` par la Forge pour les années non-bissextiles. L'Engine traite cette entrée comme un slot vide — il n'accède pas au Secondary Pool pour cette entrée.

### 3.4 Layout `flags` (u16) — figé

| Bits  | Champ              | Largeur | Description                                                    |
| ----- | ------------------ | ------- | -------------------------------------------------------------- |
| 0–3   | `Precedence`       | 4 bits  | Rang effectif 0–15, résolu définitivement par la Forge         |
| 4–7   | `Color`            | 4 bits  | Index couleur liturgique (valeurs 0–6 définies, 7–15 réservés) |
| 8–10  | `LiturgicalPeriod` | 3 bits  | Période opérationnelle résolue (cache AOT)                     |
| 11–13 | `Nature`           | 3 bits  | Type liturgique de la célébration                              |
| 14–15 | Reserved           | 2 bits  | `0` — doit être nul                                            |

Encodage : `flags = (Precedence as u16) | ((Color as u16) << 4) | ((LiturgicalPeriod as u16) << 8) | ((Nature as u16) << 11)`

### 3.5 Secondary Pool

Tableau de `u16` contigu, placé immédiatement après le Data Body à l'offset `pool_offset` (octets depuis le début du fichier).

- Si `secondary_count > 0` : l'Engine lit `secondary_count` FeastIDs consécutifs dans le pool à partir de `secondary_index` (index dans le tableau `u16`, pas un offset en octets).
- Si `secondary_count == 0` : la Forge écrit `secondary_index = 0`. L'Engine n'accède pas au pool.

Capacité maximale : 65 535 entrées `u16`, adressables par `secondary_index: u16`. Suffisant pour la plage 1969–2399 compte tenu de la rareté des commémorations dans le Novus Ordo.

---

## 4. Types de Domaine

Tous les enums sont `#[repr(u8)]` pour garantir la représentation binaire exacte. Les valeurs numériques sont **inchangées depuis v1.0**.

### 4.1 Precedence (4 bits, bits 0–3 de `flags`)

Axe ordinal de résolution de collision. Valeur numérique **inverse** : valeur plus faible = priorité plus haute. Comparaison entière pure — aucun match, aucune branche.

_Tabella dierum liturgicorum — NALC 1969. Ordre figé. Aucune modification autorisée._

```rust
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Precedence {
    TriduumSacrum                      = 0,
    SollemnitatesFixaeMaior            = 1,
    DominicaePrivilegiataeMaior        = 2,
    FeriaePrivilegiataeMaior           = 3,
    SollemnitatesGenerales             = 4,
    SollemnitatesPropria               = 5,
    FestaDomini                        = 6,
    DominicaePerAnnum                  = 7,
    FestaBMVEtSanctorumGenerales       = 8,
    FestaPropria                       = 9,
    FeriaeAdventusEtOctavaNativitatis  = 10,
    MemoriaeObligatoriae               = 11,
    FeriaePerAnnumEtMemoriaeAdLibitum  = 12,
    // 13–15 : réservés système — V2 interdit ces valeurs dans les entrées YAML
}
```

### 4.2 Nature (3 bits, bits 11–13 de `flags`)

Axe sémantique. **La Nature ne dicte jamais la force d'éviction** — seule `Precedence` est utilisée pour l'éviction. Ce découplage est la justification structurelle du modèle 2D.

> `Dominica` n'est pas une Nature. C'est une classe de précédence. Sa Nature structurelle est `Feria`. Sa force d'éviction est encodée par `Precedence::DominicaePerAnnum` (7) ou `Precedence::DominicaePrivilegiataeMaior` (2).

```rust
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Nature {
    Sollemnitas   = 0,
    Festum        = 1,
    Memoria       = 2,
    Feria         = 3,
    Commemoratio  = 4,
    // 5–7 : réservés
}
```

### 4.3 Color (4 bits, bits 4–7 de `flags`)

Couleurs liturgiques post-Vatican II. La largeur passe de 3 bits (v1.0) à 4 bits (v2.0) pour extensibilité. Valeurs 0–6 définies, 7–15 réservés.

```rust
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Color {
    Albus     = 0,  // Fêtes du Seigneur, Vierge, Confesseurs, Docteurs
    Rubeus    = 1,  // Passion, Apôtres, Martyrs, Pentecôte
    Viridis   = 2,  // Temps ordinaire
    Violaceus = 3,  // Avent, Carême
    Roseus    = 4,  // Gaudete (Avent III), Laetare (Carême IV)
    Niger     = 5,  // Messes des défunts
    // 6 : usage liturgique futur (or, argent — optionnel selon usages diocésains)
    // 7–15 : réservés
}
```

### 4.4 LiturgicalPeriod (3 bits, bits 8–10 de `flags`)

Période opérationnelle résolue. Champ cache AOT — calculé par la Forge, matérialisé dans le `.kald`, non recalculé par l'Engine. Ce type est une **projection technique** : il encode des segments mutuellement exclusifs indispensables au pipeline déterministe, pas une taxonomie canonique du Missel. `DiesSancti` y est un variant pleinement valide bien qu'hétérogène sur le plan liturgique strict.

```rust
/// Période opérationnelle résolue (cache AOT).
/// Projection technique matérialisée par la Forge — pas une taxonomie du Missel.
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LiturgicalPeriod {
    TempusOrdinarium    = 0,  // Temps ordinaire (état par défaut)
    TempusAdventus      = 1,  // Avent
    TempusNativitatis   = 2,  // Temps de Noël
    TempusQuadragesimae = 3,  // Carême
    TriduumPaschale     = 4,  // Triduum Pascal
    TempusPaschale      = 5,  // Temps pascal
    DiesSancti          = 6,  // Phase opérationnelle : Semaine Sainte (Rameaux–Mercredi Saint)
    // 7 : réservé
}
```

---

## 5. FeastID

Identifiant unique d'une fête liturgique. Représenté sur **u16** dans `CalendarEntry.primary_id` et dans le Secondary Pool.

**Valeur `0x0000` : réservée** — désigne la Padding Entry (slot 29 février des années non-bissextiles). Aucun slug ne peut se voir allouer ce FeastID.

### 5.1 Layout u16 — Officiel et Figé

```
 15  14  13  12  11  10   9   8   7   6   5   4   3   2   1   0
┌───┬───┬───┬───┬───────────────────────────────────────────────┐
│ S   S │ C   C │             Sequence (12 bits)                │
└───┴───┴───┴───┴───────────────────────────────────────────────┘
  [15:14]  [13:12]                [11:0]
   Scope   Category              Sequence
```

| Bits  | Champ      | Largeur | Valeurs                                                                     |
| ----- | ---------- | ------- | --------------------------------------------------------------------------- |
| 15–14 | `Scope`    | 2 bits  | `00` = Universal, `01` = National, `10` = Diocesan, `11` = réservé          |
| 13–12 | `Category` | 2 bits  | 0–3 (4 catégories par scope ; voir `liturgical-scheme.md` §2.2)             |
| 11–0  | `Sequence` | 12 bits | 0–4095 par (Scope, Category) ; `0` non allouable (`0x0000` réservé Padding) |

**Capacité effective :** `3 scopes × 4 catégories × 4095 séquences = 49 140 FeastIDs` adressables. Suffisant pour la plage 1969–2399 avec une marge confortable.

**Invariants :**

- Le FeastID est **stable** sur toute la plage 1969–2399 pour un slug donné. L'évolution d'une fête (béatification → canonisation, changement de precedence) est encodée dans les entrées `history[]` YAML, pas dans le FeastID.
- La Forge alloue les séquences par ordre lexicographique des slugs dans chaque `(scope, category)` — voir INV-FORGE-3 (§6, Étape 1). L'ordre d'apparition dans le YAML n'influe pas sur le FeastID attribué.
- **La stabilité inter-builds est garantie par le `feast_registry.lock`** — voir INV-FORGE-3 pour le mécanisme complet.
- **Validation V3 :** `allocated_count(scope, category) ≤ 4095` — violation → `RegistryError::FeastIDExhausted { scope, category }`.

---

## 6. Pipeline de la Forge

La Forge exécute **6 étapes ordonnées et séquentielles**. Un échec dans une étape interrompt le pipeline. Les étapes 4 et 5 opèrent en deux passes (résolution puis matérialisation) pour garantir la clôture transitive des transferts avant toute écriture binaire.

---

### Étape 1 — Rule Parsing

Ingestion des fichiers YAML, validation syntaxique, construction du `FeastRegistry`. Application des validations V1–V6, V-T1–V-T4 (§10). Toute violation est fatale — aucune sortie partielle.

Le YAML est traité comme un **graphe de données pur** : aucun champ textuel (`title`, `name`, …) n'est présent ni attendu. Les structs de désérialisation Rust rejettent tout champ inconnu via `#[serde(deny_unknown_fields)]` — la présence d'un `title:` produit `ParseError::MalformedYaml`.

**Desugaring de l'ancre `pentecostes` :**

L'ancre `pentecostes` est un **alias statique**. La Forge la résout à l'Étape 1, avant toute construction du graphe de dépendances, comme équivalence stricte :

```
anchor: pentecostes  →  anchor: pascha, offset: 49
```

Cette substitution est effectuée in-place sur la représentation intermédiaire. Aucune entrée `pentecostes` n'existe dans le graphe de dépendances — seules `pascha` et `adventus` sont des ancres primitives. La validation V4 (cycles, §8 scheme Groupe D) opère exclusivement sur les ancres désucrantées.

**INV-FORGE-1 — Ordre d'ingestion canonique et déterministe :**

Les fichiers YAML sont lus dans l'ordre suivant, sans exception :

1. `universal.yaml` (unique)
2. `national-<REGION>.yaml` triés lexicographiquement par nom de fichier
3. `diocesan-<ID>.yaml` triés lexicographiquement par nom de fichier

`fs::read_dir` n'est pas ordonné — la Forge doit collecter les chemins, les trier, puis les ingérer. Tout autre ordre invalide le déterminisme bit-for-bit.

**INV-FORGE-2 — Interdiction de `HashMap` dans les chemins de production :**

Toute structure de données dont le contenu influence le `.kald` utilise un type ordonné : `BTreeMap`, `BTreeSet`, ou `Vec` avec ordre d'insertion explicitement défini. `HashMap` et `HashSet` sont autorisés uniquement pour des accumulateurs locaux dont le résultat est trié avant toute utilisation en aval.

**INV-FORGE-3 — Stabilité des FeastIDs : `feast_registry.lock`**

Le tri lexicographique seul garantit le déterminisme au sein d'un build mais pas la **stabilité entre builds successifs** : ajouter un slug dont l'ordre alphabétique précède des slugs existants décalerait tous leurs FeastIDs, invalidant tout ID persisté par les clients.

La Forge maintient un fichier `feast_registry.lock` — analogue à `Cargo.lock` — comme source de vérité inter-builds. Ce fichier est **versionné avec le corpus YAML** et **jamais édité manuellement**.

**Algorithme d'allocation avec lock :**

```
PREMIER BUILD (feast_registry.lock absent) :
  → Allouer les FeastIDs par ordre lexicographique des slugs dans chaque (scope, category)
  → Écrire feast_registry.lock : { slug → FeastID } pour tous les slugs alloués

BUILD SUIVANT (feast_registry.lock présent) :
  → Pour chaque slug dans le YAML :
      SI slug présent dans le lock → utiliser le FeastID du lock (pinning implicite)
      SI slug absent du lock      → allouer le prochain FeastID libre dans (scope, category),
                                    l'inscrire dans le lock
  → Pour chaque slug présent dans le lock mais absent du YAML :
      → TOMBSTONE : marquer l'entrée comme supprimée, FeastID jamais réalloué
```

**Règles de conflit avec le champ `id` explicite du YAML :**

```
SI YAML.id présent ET YAML.id == lock[slug]  → OK, cohérent
SI YAML.id présent ET slug absent du lock    → inscrire dans le lock, traiter comme pinning
SI YAML.id présent ET YAML.id ≠ lock[slug]  → ForgeError::FeastIDLockConflict { slug, yaml_id, lock_id }
SI YAML.id absent  ET slug présent dans lock → utiliser lock[slug] silencieusement
```

**Invariants du lock file :**

- Un FeastID tombstoné n'est jamais réalloué à un autre slug — les IDs sont monotones croissants et définitifs.
- Le lock file est déterministe : deux builds avec le même corpus YAML et le même lock file produisent exactement le même `.kald`.
- En l'absence de lock file (bootstrap ou environnement CI isolé), l'allocation par tri lexicographique est le comportement par défaut. Le lock file produit à l'issue du build doit être sauvegardé et committé.

---

**Convention champs serde réservés :**

Tout champ serde désérialisé depuis le YAML mais non consommé dans le jalon courant est préfixé par `_` **dans le struct Rust**, avec un attribut `#[serde(rename = "nom_yaml")]` pour maintenir la compatibilité de désérialisation :

```rust
#[derive(Deserialize)]
struct YamlFile {
    version:  u32,           // "version" — remplace l'ancien "format_version" (supprimé v1.2)
    category: u8,
    #[serde(rename = "from")]
    _from:    Option<u16>,   // réservé Jalon 3 — versioning temporel
    #[serde(rename = "to")]
    _to:      Option<u16>,   // réservé Jalon 3 — versioning temporel
    // slug : absent — déduit du path.file_stem() avant désérialisation (§2.1 scheme)
    // scope, region : déduits du chemin — non désérialisés depuis le YAML
}
```

---

### Étape 1bis — i18n Resolution

Corrélation entre le `FeastRegistry` construit en Étape 1 et les dictionnaires `i18n/` externes. Application des validations V-I1 et V-I2 (§10). Toute violation est fatale.

**Entrées :** `FeastRegistry` (ensemble des slugs + plages `[from, to]` de chaque `history[]`), arborescence `i18n/` (§4.4 scheme).

**Algorithme :**

```
POUR chaque (slug, from) dans le FeastRegistry :
  SI i18n/la/{slug}.yaml absent         → ParseError::I18nMissingLatinKey { slug, from=*, field="title" }
  SI clé {from}.title absente dans la/  → ParseError::I18nMissingLatinKey { slug, from, field="title" }

POUR chaque lang ∈ langues_compilées :
  POUR chaque clé (from, field) dans i18n/{lang}/{slug}.yaml :
    SI from ∉ froms_connus(slug)        → ParseError::I18nOrphanKey { slug, lang, from, field }
```

**Fusion AOT (fallback latin) :**

La Forge construit pour chaque `(slug, from, field, lang)` la valeur résolue :

```rust
fn resolve_label<'a>(
    slug: &str, from: u16, field: &str, lang: &str,
    dicts: &'a DictStore,
) -> &'a str {
    dicts.get(lang, slug, from, field)
        .or_else(|| dicts.get("la", slug, from, field))
        .expect("V-I1 guarantees latin key exists") // invariant garanti Étape 1bis
}
```

Le résultat est un `LabelTable` plat : `BTreeMap<(FeastID, u16 from, Lang), String>`, consommé par l'Étape 6 pour produire le `.lits`.

**Artefact intermédiaire :** `LabelTable` — structure en mémoire, non persistée. Elle n'influence pas le `.kald`.

---

### Étape 3 — Canonicalization

- Résolution de toutes les dates mobiles : Pâques (Meeus/Jones/Butcher), `SeasonBoundaries::compute`
- Calcul des DOY 0-based via `MONTH_STARTS` pour toutes les dates fixes et mobiles
- Détermination du caractère bissextile (années ≡ 0 mod 400, ou ≡ 0 mod 4 et ≢ 0 mod 100)

#### Ordre de résolution des ancres (v2.2)

Les ancres sont résolues dans l'ordre suivant, garantissant l'absence de cycle et la disponibilité des dépendances :

```
1. nativitas          — O(1), indépendante
2. epiphania          — O(1), indépendante
3. adventus           — O(1), indépendante
4. tempus_ordinarium  — O(1), dépend de adventus (déjà résolu en 3)
5. pascha             — Meeus/Jones/Butcher
6. pentecostes        — dérivée : pascha + 49
```

`tempus_ordinarium` dépend uniquement de `adventus` (étape 3), déjà résolu quand elle est calculée. Acyclique par construction.

#### anchor: tempus_ordinarium (v2.2)

**Sémantique du champ `ordinal`** : index ordinal de la semaine dans le Temps Ordinaire, de 1 (premier dimanche) à 34 (Christ-Roi). Le champ `offset` est **absent** pour cette ancre — sa présence est une erreur fatale V4a.

**Algorithme de résolution (O(1)) :**

Le comptage est ancré sur le dernier dimanche du Temps Ordinaire (XXXIV = Christ-Roi), lui-même défini comme `adventus - 7`. Tous les dimanches ordinaires sont calculés par soustraction homogène :

```
DOY(tempus_ordinarium, ordinal) = DOY(adventus) − 7 × (35 − ordinal)
```

**Pseudo-code de référence :**

```rust
fn resolve_tempus_ordinarium(adventus_doy: u16, ordinal: u8) -> u16 {
    // ordinal ∈ [1, 34] — validé en V4a avant appel
    // adventus_doy déjà résolu (étape 3)
    let offset_from_adventus: u16 = 7 * (35 - ordinal as u16);
    adventus_doy.saturating_sub(offset_from_adventus)
    // saturating_sub : protection contre underflow théorique (impossible si ordinal ∈ [1,34]
    // et adventus ∈ [DOY 300, DOY 335])
}
```

**Vérification : table de correspondance pour 2025**
(Avent 2025 = 30 novembre = DOY 333)

| Ordinal | DOY calculé | Date grégorienne | Correct ? |
| ------- | ----------- | ---------------- | --------- |
| 34      | 333 − 7     = 326 | 23 nov 2025 | ✅ Christ-Roi |
| 33      | 333 − 14    = 319 | 16 nov 2025 | ✅         |
| 10      | 333 − 175   = 158 | 8 juin 2025 | ✅         |
| 1       | 333 − 238   = 95  | 6 avr 2025  | ⚠ absorbé (Pâques 2025 = 20 avr, TO commence après Pentecôte 8 juin) → Ok(None) |

**Comportement pour les slots absorbés :**

La Forge calcule le DOY sans condition. Elle ne décide pas si le dimanche ordinaire est « actif » : cette responsabilité appartient à l'**Étape 4 (Conflict Resolution)**. L'Étape 4 constate qu'un slug de Temps de Pâques ou de Noël occupe déjà ce DOY avec une `Precedence` ≤ 2, et supprime le dimanche ordinaire du dataset pour cette année.

La Forge retourne `Ok(None)` pour le slot ordinaire — ce n'est pas une erreur. L'Engine reçoit une Padding Entry (`primary_id = 0`).

**Invariant** : la Forge ne conditionne jamais la résolution d'un DOY à une logique saisonnière. La saison est un attribut calculé, pas un filtre de résolution.

**Fêtes enregistrées utilisant tempus_ordinarium (corpus universale) :**

| Slug                                    | Ordinal | DOY typique (2025) |
| --------------------------------------- | ------- | ------------------ |
| `dominica_x_temporis_ordinarii`         | 10      | 158 (8 juin)       |
| `dominica_xi_temporis_ordinarii`        | 11      | 165 (15 juin)      |
| … (ordinals 10–34 actifs typiquement)   |         |                    |
| `dominica_xxxiv_temporis_ordinarii`     | 34      | 326 (23 nov)       |

> Les ordinals 1–9 peuvent être partiellement ou totalement absorbés selon l'année (Pâques tardives). La Forge génère tous les slugs ; l'Étape 4 élimine les slots en conflit.

---

**Validation croisée de Pâques :**

L'algorithme est seul juge du calcul. Une table de référence partielle (années limites + cas connus) est vérifiée à l'exécution pour détecter toute régression de conversion DOY :

```rust
fn compute_and_validate_easter(year: u16) -> Result<u16, ForgeError> {
    // Meeus/Jones/Butcher → (mois, jour) grégorien
    let (month, day) = meeus_jones_butcher(year);
    // Conversion en DOY 0-based — cas sensible : mars suit immédiatement
    // l'index fixe 59 (29 fév). MONTH_STARTS[2] = 60.
    let doy = MONTH_STARTS[month as usize - 1] + day as u16 - 1;

    // Assertion de bornes astronomiques — invariant absolu.
    // Pâques grégorien : [22 mars, 25 avril] = DOY [81, 115].
    //   22 mars = MONTH_STARTS[2] + 21 = 60 + 21 = 81
    //   25 avril = MONTH_STARTS[3] + 24 = 91 + 24 = 115
    // Une valeur hors de cette plage indique un bug d'algorithme ou de conversion.
    debug_assert!(
        doy >= 81 && doy <= 115,
        "Easter DOY {} for year {} is outside valid range [81, 115]", doy, year
    );

    // Table de référence — sous-ensemble d'années vérifiées
    if let Some(&expected) = EASTER_REFERENCE.get(&year) {
        if doy != expected {
            return Err(ForgeError::EasterMismatch { year, computed: doy, expected });
        }
    }
    Ok(doy)
}

// Cas limites obligatoires dans EASTER_REFERENCE :
// - Première et dernière année de la plage (1969, 2399)
// - Pâques le plus tôt possible sur la plage (22 mars — vérifier existence)
// - Pâques le plus tard possible (25 avril — ex: 2038)
// - Années séculaires non-bissextiles (2100, 2200, 2300)
// - 2025 : doy = 110 (20 avril — MONTH_STARTS[3] + 19 = 91 + 19 = 110, vérifié)
```

**Stratégie de tests automatisés (Jalon 2) :**

Trois niveaux de vérification à implémenter dans la suite de tests de la Forge :

| Test                        | Couverture                                 | Méthode                                                                           |
| --------------------------- | ------------------------------------------ | --------------------------------------------------------------------------------- |
| **Assertion de bornes**     | Toutes les années 1969–2399 (431 appels)   | `assert!(doy >= 81 && doy <= 115)` pour chaque année                              |
| **Invariant index 59**      | Toutes les années 1969–2399                | `assert!(easter_doy != 59)` — Pâques ne peut jamais tomber le 29 février          |
| **Roundtrip DOY→(M,D)→DOY** | Toutes les dates fixes + toutes les Pâques | Reconstruire `(mois, jour)` depuis le DOY calculé et comparer à l'entrée de Meeus |
| **Table de référence**      | ~20 années critiques                       | Comparaison exacte avec `EASTER_REFERENCE`                                        |
| **Continuité de saison**    | Toutes les années                          | `SeasonBoundaries` : vérifier que chaque DOY appartient à exactement une saison   |

---

### Étape 4 — Conflict Resolution

Résolution définitive des préséances et transferts de fêtes. **Aucun conflit ne doit atteindre l'Engine.** Sortie : table `(year, doy) → (primary_feast, [secondary_feasts])` sans ambiguïté.

#### 3.0 `ResolutionKey` — Clé de Tri Canonique

Toute collision sur un slot DOY est traitée non comme une décision conditionnelle mais comme un **problème d'ordonnancement** : les fêtes candidates sont triées selon une clé totale et stable, et l'élément d'index 0 est élu `primary`. Aucun `if/else` métier dans le code de résolution.

**Définition — `liturgical-calendar-forge/src/resolution.rs` :**

```rust
/// Clé de tri canonique pour la désignation primary/secondary au sein d'un slot DOY.
/// Ordre lexicographique natif via derive(Ord). Valeur inférieure = priorité supérieure.
/// Scope : Forge uniquement. Absent du Core et du .kald.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ResolutionKey<'a> {
    pub precedence:  u8,          // [0, 12] — inférieur = priorité plus haute
    pub cycle:       Cycle,       // Temporal(0) < Sanctoral(1)
    pub temporality: Temporality, // Fixed(0) < Mobile(1)
    pub slug:        &'a str,     // tiebreaker final — ordre lexicographique ASCII
}

/// Cycle liturgique — dérivé de la présence du bloc `mobile:` ou `date:` dans le YAML.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Cycle {
    Temporal  = 0,  // fête déclarée avec `mobile:` (Proprium de Tempore)
    Sanctoral = 1,  // fête déclarée avec `date:`   (Sanctoral fixe)
}

/// Temporalité de la fête — fixe ou calculée depuis une ancre.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Temporality {
    Fixed  = 0,  // bloc `date:`   — DOY constant
    Mobile = 1,  // bloc `mobile:` — DOY calculé depuis Pâques/Avent
}
```

**Propriétés de la clé :**

| Critère | Type | Signification de la valeur inférieure |
|---------|------|---------------------------------------|
| `precedence` | `u8` | Rang liturgique plus élevé (Solennité < Mémoire) |
| `cycle` | `Cycle` | Temporal prime sur Sanctoral à égalité de Precedence |
| `temporality` | `Temporality` | Fête fixe prime sur fête mobile à égalité de Cycle |
| `slug` | `&str` | Tiebreaker final stable — ordre ASCII, déterministe cross-build |

**Invariant de `slug` comme tiebreaker :** le slug est dérivé du stem du nom de fichier — il est stable entre builds (contrairement au FeastID, qui dépend de l'ordre d'allocation du lock). Son usage comme tiebreaker ne produit **aucun `ConflictWarning`** : c'est un ordonnancement mécanique attendu, pas une anomalie de corpus.

**Relation avec `Cycle` / `Temporality` :** dans la pratique actuelle du schéma, `Cycle::Temporal` implique `Temporality::Mobile` et `Cycle::Sanctoral` implique `Temporality::Fixed`. Les deux champs sont conservés distincts par clarté sémantique et pour préserver la surface d'extension (ex: une fête sanctorale à date calculée en v2.x ne casserait pas la clé).

#### 3.1 Règle de surcharge par Scope

Pour un DOY donné, si plusieurs fêtes coexistent issues de scopes différents, la règle de fusion est :

```
diocesan > national > universal
```

La hiérarchie de scope est résolue **avant** le tri canonique (§3.0). Elle détermine quels candidats entrent dans le tri — elle n'est pas un critère de la `ResolutionKey`. Le scope le plus local fournit la fête principale (`primary_feast`). Les fêtes de scopes moins locaux dont la `Precedence` les rend commémorables rejoignent les candidats secondaires.

#### 3.2 Garde de Solennité (V7) — Pré-tri

**Cas Precedence ∈ [0, 3], tout scope :**

Les rangs 0 à 3 (TriduumSacrum, SollemnitatesFixaeMaior, DominicaePrivilegiataeMaior, FeriaePrivilegiataeMaior) sont structurellement uniques — deux fêtes légitimes de ce rang ne peuvent coexister sur le même DOY. Une telle collision indique un corpus fondamentalement incohérent. Détection **avant le tri**.

**Cas Precedence ∈ [4, 5], même scope :**

Deux Solennités de même scope et de même rang sur le même DOY sont irrésolubles mécaniquement. Détection **avant le tri**.

Dans les deux cas :

```
ForgeError::SolemnityCollision {
    slug_a, slug_b, precedence, scope_a, scope_b, doy, year
}
```

Compilation interrompue. Arbitrage humain requis dans le YAML.

**Cas Precedence ∈ [4, 5], scopes différents :** la hiérarchie de scope (§3.1) s'applique automatiquement — ce cas n'est pas fatal.

**Cas Precedence ≥ 6 :** résolu intégralement par le tri canonique `ResolutionKey` — aucune branche conditionnelle.

#### 3.3 Pipeline de Résolution — Structure en Cinq Passes

Certaines fêtes de haute priorité ne peuvent être supprimées quand elles entrent en conflit : elles sont **transférées** au prochain slot disponible. Ce mécanisme est **non local** et peut produire des cascades. Les cinq passes garantissent que chaque phase opère sur une table stable produite par la phase précédente.

```
POUR chaque année dans [1969, 2399] :

  PASSE 1 — Collecte et placement
    Précondition : table vide pour cette année.
    Placer toutes les fêtes (fixes et mobiles) dans leur slot DOY :
      — Fêtes fixes   : DOY via MONTH_STARTS (§2.2)
      — Fêtes mobiles : DOY via résolution Pâques/Avent (Étape 3)
    Résultat : BTreeMap<u16 (doy), Vec<PlacedFeast>> — liste non encore triée.
    Aucun conflit n'est résolu dans cette passe.

  PASSE 2 — Garde V7 + résolution de scope (§3.1, §3.2)
    Pour chaque slot DOY avec plusieurs fêtes candidates :
      a. Appliquer la hiérarchie de scope (§3.1) :
           SI deux fêtes de scopes différents et Precedence ∈ [4,5] :
             la fête du scope le plus local reste candidate, l'autre est retirée.
      b. Détecter et rejeter les collisions fatales (§3.2) :
           SI même scope + Precedence ≤ 3      → ForgeError::SolemnityCollision (fatal)
           SI même scope + Precedence ∈ [4, 5] → ForgeError::SolemnityCollision (fatal)
    Sortie : liste de candidats validée par slot — garantie sans collision fatale.

  PASSE 3 — Tri Canonique + Élection + Déclassement + Dispatch Transferts
    Pour chaque slot DOY :

    [TRI]
      slot.sort_unstable_by_key(|f| ResolutionKey {
          precedence:  f.precedence as u8,
          cycle:       f.cycle(),        // Cycle::Temporal si mobile:, Sanctoral si date:
          temporality: f.temporality(),  // Temporality::Fixed si date:, Mobile si mobile:
          slug:        f.slug.as_str(),
      });

    [ÉLECTION]
      primary               = slot[0]
      secondary_candidates  = slot[1..]

    [DÉCLASSEMENT SAISONNIER — §3.4]
      Calculer la saison du slot (SeasonBoundaries de l'année courante).
      Pour chaque fête dans secondary_candidates :
        SI should_demote_to_commemoratio(f, saison) → forcer en secondary_feasts
                                                       quelle que soit sa Precedence.

    [PARTITION]
      secondary_feasts ← secondary_candidates où Precedence ∈ [8, 12]
                         (commémorables — versés dans le Secondary Pool)
      to_transfer      ← secondary_candidates où Precedence ≤ 9
                         ET Nature ≠ Feria
                         ET non déjà en secondary_feasts
                         (transférables — ne peuvent être supprimés)
      suppressed       ← reste (évincés sans transfert ni commémoration)

    [DISPATCH TRANSFERTS — §2.4 scheme]
      Pour chaque fête dans to_transfer :
        SI bloc transfers présent ET entrée collides == primary.slug :
          Appliquer règle déclarative (offset ou date fixe).
          Résolution à un seul niveau — pas de réapplication récursive.
        SINON :
          Alimenter TransferQueue générique.

    Sortie : table (doy → ResolvedDay { primary, secondary_feasts }), TransferQueue.

  PASSE 4 — Exécution des transferts (clôture transitive)
    Précondition : TransferQueue issue de Passe 3.
    Structure : BTreeSet<(doy_courant, FeastID)> — ordre déterministe garanti.

    TANT QUE la TransferQueue n'est pas vide :
      Extraire le premier élément (doy_src, feast).
      Chercher doy_dst dans [doy_src+1, doy_src+7] :
        → premier DOY où l'occupant a Precedence > feast.precedence.
      SI doy_dst trouvé :
        Appliquer ResolutionKey au slot cible avec la fête insérée.
        SI l'occupant de doy_dst est lui-même transférable :
          L'enqueuer avec profondeur + 1 (BTreeSet — pas de doublon possible).
      SI doy_dst introuvable dans la fenêtre :
        → ForgeError::TransferFailed { slug, origin_doy, blocked_at, year }
        → Compilation interrompue immédiatement.

  PASSE 5 — Vérification de stabilité (invariant de clôture)
    Précondition : TransferQueue vide, table finale issue de Passe 4.
    Pour chaque DOY de l'année :
      — Exactement un primary_feast (ou Padding Entry à doy=59).
      — Aucun conflit de Precedence non résolu.
      — Aucune fête transférable laissée sans slot.
    Violation → ForgeError::ResolutionIncomplete { doy, year, detail }
    Succès → table (year, doy) → (primary_feast, [secondary_feasts]) gelée.
```

**Structure de contrôle — `TransferQueue` avec compteur de profondeur :**

```rust
/// File de transferts. Chaque entrée porte la fête à placer, son DOY de recherche
/// courant et sa profondeur de chaîne (nombre de déplacements successifs).
struct TransferQueue {
    // BTreeSet : ordre de traitement déterministe = (doy_courant, feast_id) croissants
    pending: BTreeSet<(u16, ResolvedFeast)>,
}

/// Profondeur maximale de chaîne : 7 jours = fenêtre de transfert canonique.
/// Au-delà, la Forge interrompt la compilation — aucune décision automatique.
const MAX_TRANSFER_DEPTH: u8 = 7;

impl TransferQueue {
    fn enqueue(
        &mut self,
        doy_src: u16,
        feast: ResolvedFeast,
        depth: u8,
    ) -> Result<(), ForgeError> {
        if depth > MAX_TRANSFER_DEPTH {
            return Err(ForgeError::TransferFailed {
                slug:       feast.slug.clone(),
                origin_doy: doy_src.saturating_sub(depth as u16),
                blocked_at: doy_src,
                year:       feast.year,
            });
        }
        self.pending.insert((doy_src, feast));
        Ok(())
    }
}
```

**Garantie de terminaison :** les transferts sont strictement vers l'avant (`doy_dst > doy_src`). La profondeur est un compteur monotone croissant borné à 7. Le `BTreeSet` empêche qu'une même fête soit enfilée deux fois pour le même DOY. La boucle termine en O(7N) avec N ≤ 366.

**`ForgeError::TransferFailed` — erreur fatale, aucune dérogation :**

```
ERREUR FORGE : Transfert impossible
  Fête    : annuntiatio_domini (Precedence=1)
  Origine : doy=86, year=2035
  Bloqué  : 7 jours consécutifs [87–93] tous de Precedence ≤ 1
  Action  : déclarer une règle de transfert explicite dans le YAML
             ou redéfinir la plage [from, to] de la fête concurrente
```

**Le déclassement automatique d'une Solennité en Mémoire n'est pas une option.** La Forge exécute des règles, elle n'invente pas de droit liturgique.

**Invariant de transfert :** une fête insérée par transfert est re-triée via `ResolutionKey` dans son slot cible. Elle ne bénéficie d'aucune priorité liée à son transfert.

**Cas limite documenté — cascade Semaine Sainte :** l'Annonciation (25 mars, Precedence = 1) tombant en Semaine Sainte est transférée au lundi après l'Octave de Pâques. Si ce lundi est occupé par une Mémoire obligatoire, la Mémoire est positionnée en `secondary_feasts` par le tri canonique. Conforme au Novus Ordo (GNLYC §60).

#### 3.4 Déclassement Saisonnier — Carême et Avent

Pendant certaines périodes liturgiques (`TempusQuadragesimae`, `TempusAdventus`), les Mémoires obligatoires (`Precedence = 11`) perdent leur caractère prescriptif et deviennent des **Commémorations facultatives**. Ce déclassement est appliqué dans la **Passe 3** (étape [DÉCLASSEMENT SAISONNIER]) du pipeline §3.3 — il ne modifie pas le FeastID ni les propriétés permanentes de la fête.

**Mécanisme de matérialisation :**

Le slot DOY est pris par la fête temporelle du jour (Feria de Carême, Dimanche d'Avent, etc.) comme `primary_feast`. La Mémoire déclassée est versée dans `secondary_feasts` avec son FeastID original, **inchangé**.

La nature "Commémoraison" est implicite par la position dans le Secondary Pool : toute fête présente dans `secondary_feasts` est par définition commémorée ce jour-là, quel que soit son rang habituel. L'Engine retourne le FeastID sans interprétation — c'est la couche appelante qui reconstruit la sémantique de commémoraison à partir de la période opérationnelle (`flags.LiturgicalPeriod` du primary).

**Aucune structure binaire supplémentaire n'est nécessaire.** Le Secondary Pool stocke des `u16` (FeastIDs) sans flags propres : la présence dans le pool est le signal, la période du jour est le contexte.

**Invariant de déclassement :** la Forge ne modifie jamais le FeastID d'une fête déclassée. Le FeastID est une clé stable d'identité. Seule sa position dans le slot (primary vs secondary) encode son statut ce jour-là.

**Détection en Étape 4 :**

```rust
fn should_demote_to_commemoratio(feast: &Feast, period: LiturgicalPeriod) -> bool {
    // Mémoires obligatoires (11) et facultatives (12) en période privilégiée
    feast.precedence >= 11
        && matches!(period,
            LiturgicalPeriod::TempusQuadragesimae |
            LiturgicalPeriod::TempusAdventus      |
            LiturgicalPeriod::TriduumPaschale     |
            LiturgicalPeriod::DiesSancti)
}
```

Si `should_demote_to_commemoratio` est vrai, la fête est ajoutée à `secondary_feasts` du slot au lieu de devenir `primary_feast`. Elle n'est **pas** ajoutée si le slot n'a pas de fête temporelle dominante — dans ce cas elle reste primary (Mémoire en Carême sans conflit avec le temporal).

---

### Étape 5 — Day Materialization

- Génération des 366 slots par an pour la plage 1969–2399, en itérant les années dans l'ordre croissant (1969 → 2399) et les DOY dans l'ordre croissant (0 → 365)
- Placement de la Padding Entry (`primary_id = 0`, `secondary_count = 0`, `flags = 0`) à `doy = 59` pour chaque année non-bissextile

**INV-FORGE-4 — Ordre du Secondary Pool : tri par FeastID croissant :**

L'ordre des FeastIDs dans chaque entrée du Secondary Pool est lexicographique sur les `u16` — tri numérique croissant, indépendant des slugs, de la locale système et de l'ordre de résolution de l'Étape 4. Le `sort_unstable()` dans `PoolBuilder::insert` est le seul mécanisme d'ordre admis. Aucune autre clé de tri (slug, date, Precedence) n'est acceptable — le binaire produit doit être identique sur toutes les machines de build.

**Construction du Secondary Pool — déduplication avec tri canonique et vérification de capacité :**

```rust
// Forge uniquement (std autorisé)
struct PoolBuilder {
    // Clé : séquence triée de FeastIDs — garantit {A,B} == {B,A}
    index: BTreeMap<Vec<u16>, u16>,
    data:  Vec<u16>,
}

impl PoolBuilder {
    fn insert(&mut self, mut ids: Vec<u16>) -> Result<u16, ForgeError> {
        ids.sort_unstable();  // tri canonique avant déduplication
        if let Some(&existing) = self.index.get(&ids) {
            return Ok(existing);  // réutilisation d'index — zéro duplication
        }
        // Capacité : secondary_index est u16 (max 65 535).
        // La déduplication est une contrainte d'implémentation, pas une optimisation.
        // Sans déduplication, le pool worst-case (~78 000 entrées) dépasse u16::MAX.
        if self.data.len() + ids.len() > u16::MAX as usize {
            return Err(ForgeError::SecondaryPoolOverflow {
                pool_len:     self.data.len() as u32,
                max_capacity: u16::MAX as u32,
            });
        }
        let idx = self.data.len() as u16;
        self.index.insert(ids.clone(), idx);
        self.data.extend_from_slice(&ids);
        Ok(idx)
    }
}
```

Le tri des FeastIDs avant insertion définit l'ordre des commémorations dans le `.kald` comme lexicographique sur les FeastIDs — indépendant de l'ordre de résolution en Étape 4 et donc déterministe.

La déduplication réduit la taille du pool en exploitant la répétition des combinaisons de commémorations entre années. L'impact cache est neutre pour l'Engine (accès aléatoire au pool) mais réduit la taille totale du fichier `.kald`.

---

### Étape 6 — Binary Packing

**Production `.kald` :**
- Encodage `flags` depuis les valeurs de `Precedence`, `Color`, `LiturgicalPeriod`, `Nature` résolus en Étape 4
- Sérialisation LE canonique de chaque `CalendarEntry`, dans l'ordre index croissant
- Calcul SHA-256 sur `[Data Body ∥ Secondary Pool]`
- Construction et écriture du `Header` (64 octets)
- Validation post-écriture : relecture via `kal_validate_header`

**Production `.lits` (une par langue compilée) :**
- Consomme le `LabelTable` produit en Étape 1bis
- Pour chaque langue, construit l'Entry Table triée par `(feast_id, from)` et le String Pool UTF-8
- Écrit le Header `.lits` avec `kald_build_id = kald_checksum[..8]`
- Produit un fichier autonome et complet par langue — aucun lien de dépendance inter-langues

**Invariant de production :** le `.kald` est produit **avant** les `.lits`. Le `kald_build_id` du header `.lits` est calculé depuis le SHA-256 du `.kald` finalisé.

#### 5.1 Diagramme d'état des `flags` pour une fête déclassée

Les `flags` encodent le **statut résolu au moment de la compilation**, pas le statut nominal du YAML. Ce diagramme illustre les transitions d'état pour une _Memoria Obligatoria_ (Precedence = 11) traversant les différents contextes de résolution.

```
┌─────────────────────────────────────────────────────────────────────┐
│  SOURCE YAML                                                        │
│  slug: "s_thomae_aquinatis"                                         │
│  history[].precedence : 11  (MemoriaeObligatoriae)                  │
│  history[].nature     : Memoria                                     │
│  history[].color      : Albus                                       │
│  date: { month: 1, day: 28 }                                        │
└────────────────────────┬────────────────────────────────────────────┘
                         │ Étape 4 — Conflict Resolution
                         │ La Forge évalue le contexte du slot (year, doy)
                         │
         ┌───────────────┼───────────────────────┐
         │               │                       │
         ▼               ▼                       ▼
  [CAS A]           [CAS B]               [CAS C]
  Pas de conflit    Conflit avec          Saison privilégiée
  (Temps ordinaire) fête de Precedence    (Carême, Avent,
                    supérieure (≤ 10)     Triduum, DiesSancti)
         │               │                       │
         ▼               ▼                       ▼
  primary_feast     primary_feast =       primary_feast =
  = cette fête      fête concurrente      fête temporelle du jour
                    cette fête →          cette fête →
                    secondary_feasts      secondary_feasts
                    (si Precedence ≥ 8)   (déclassement §3.4)
         │               │                       │
         ▼               ▼                       ▼
  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────────┐
  │ CalendarEntry│  │ CalendarEntry│  │ CalendarEntry (primary)  │
  │ primary_id   │  │ primary_id   │  │ primary_id  = FeastID    │
  │  = FeastID   │  │  = FeastID   │  │               temporel   │
  │              │  │   concurrent │  │                          │
  │ flags:       │  │ flags:       │  │ flags:                   │
  │  Precedence=11│  │  Precedence=N│  │  Precedence = fête temp. │
  │  Nature=Mem. │  │  Nature=...  │  │  Nature     = Feria      │
  │  Period=Ord. │  │  Period=...  │  │  Period     = Quadrages. │
  │  Color=Albus │  │  Color=...   │  │  Color      = Violaceus  │
  │              │  │              │  │                          │
  │ secondary_   │  │ secondary_   │  │ secondary_index → pool:  │
  │  count = 0   │  │  count ≥ 1   │  │  [FeastID_s_thomae, ...] │
  └──────────────┘  │  → pool:     │  │  secondary_count ≥ 1    │
                    │  [FeastID_st]│  └──────────────────────────┘
                    └──────────────┘
```

**Invariants à vérifier en Étape 6 :**

| Invariant                                             | Expression                                    | Erreur si violation               |
| ----------------------------------------------------- | --------------------------------------------- | --------------------------------- |
| FeastID jamais modifié                                | `entry.primary_id == feast_registry[slug].id` | `ForgeError::FeastIDMutated`      |
| `flags` bits 14–15 toujours nuls                      | `flags & 0xC000 == 0`                         | `ForgeError::FlagsReservedBitSet` |
| `secondary_count == 0` ⟹ `secondary_index` ignoré     | Vérifiable à la lecture                       | Avertissement log                 |
| Padding Entry à doy=59 sur toute année non-bissextile | `entry == { 0, 0, 0, 0, 0 }`                  | `ForgeError::PaddingEntryMissing` |
| Capacité `secondary_count` (u8 ≤ 255)                 | `secondaries.len() ≤ 255` pour tout slot DOY  | `ForgeError::SecondaryCountOverflow { doy, year, count }` — V12 |

---

## 7. API de l'Engine

L'Engine expose **exactement 4 fonctions publiques**. Zéro logique de domaine. Zéro calcul de dates.

### 7.0 Contrat de Sécurité FFI — Obligatoire pour Toute Fonction

**INV-FFI-1 — NULL check en première instruction.**

Toute fonction FFI vérifie la nullité de **chaque** pointeur reçu avant toute autre opération. L'ordre d'évaluation est : null-check → taille → bornes → contenu. Aucune exception.

```rust
// Patron obligatoire — à reproduire dans chaque fonction kal_*
pub unsafe extern "C" fn kal_exemple(
    data: *const u8,
    len: usize,
    out: *mut KalFoo,
) -> i32 {
    // 1. NULL checks — premiers, inconditionnels
    if data.is_null() || out.is_null() {
        return KAL_ERR_NULL_PTR; // -1
    }
    // 2. Taille minimale avant tout accès
    if len < MINIMUM_REQUIRED {
        return KAL_ERR_BUF_TOO_SMALL; // -2
    }
    // 3. Bornes logiques (year, doy, index…)
    // 4. Lecture du contenu
}
```

**INV-FFI-2 — Validation de la fenêtre mémoire avant tout accès.**

Pour toute lecture à l'offset `off` de `n` octets : `off + n ≤ len` doit être vérifié **avant** le déréférencement. Retourner `KAL_ERR_INDEX_OOB` ou `KAL_ERR_POOL_OOB` selon le contexte si la condition est violée. Aucun accès spéculatif au-delà du buffer.

**INV-FFI-3 — Pas de dereference sans validation préalable.**

Les out-params (`out_entry`, `out_header`, `out_ids`, `out_count`) sont écrits **uniquement** après que tous les checks ont réussi. En cas d'erreur, les out-params ne sont pas modifiés — l'appelant ne doit pas en lire la valeur.

**INV-FFI-4 — Surface `unsafe` minimale et documentée.**

Chaque bloc `unsafe` dans l'Engine porte un commentaire `// SAFETY:` justifiant les invariants garantis par les checks précédents. Exemple :

```rust
// SAFETY: null check passé (INV-FFI-1), off + 8 ≤ len vérifié (INV-FFI-2).
let entry = unsafe { &*(data.add(off) as *const CalendarEntry) };
```

### 7.1 `kal_validate_header`

```c
int32_t kal_validate_header(
    const uint8_t *data,    // buffer complet du fichier .kald
    size_t         len,     // taille du buffer
    KalHeader     *out_header  // out-param, peut être NULL
);
```

**Préconditions FFI (§7.0) :** `data` non-NULL. `out_header` peut être NULL (résultat ignoré). `len` ≥ 64 avant tout accès.

Validations séquentielles (arrêt au premier échec) :

1. `data != NULL` → `KAL_ERR_NULL_PTR`
2. `len >= 64` → `KAL_ERR_FILE_SIZE`
3. Magic == `"KALD"` → `KAL_ERR_MAGIC`
4. `version == 4` → `KAL_ERR_VERSION`
5. `len == 64 + (entry_count × 8) + pool_size` → `KAL_ERR_FILE_SIZE`
6. `_reserved == 0x00 × 8` → `KAL_ERR_RESERVED`
7. SHA-256(`[Data Body ∥ Secondary Pool]`) == `checksum` → `KAL_ERR_CHECKSUM`

**SHA-256 — Implémentation `no_std`, streaming, sans allocation :**

Le calcul du checksum porte sur `data[64..len]` (Data Body + Secondary Pool concaténés). L'implémentation **doit** être streaming : elle consomme le buffer par blocs de 64 octets sans jamais allouer de copie intermédiaire.

Implémentation de référence : crate `sha2` avec feature `no_std` activée (désactive `std` + `alloc`). Utilisation via l'interface `Digest` :

```rust
use sha2::{Sha256, Digest};

// SAFETY : null check + len >= 64 déjà vérifiés avant cet appel.
let payload = unsafe { core::slice::from_raw_parts(data.add(64), len - 64) };
let mut hasher = Sha256::new();
hasher.update(payload);            // Traitement en place — aucune allocation
let computed = hasher.finalize();  // [u8; 32] sur la pile
if computed.as_slice() != &header.checksum {
    return KAL_ERR_CHECKSUM;
}
```

**Contrainte `no_alloc` :** `Sha256::new()` et `hasher.finalize()` opèrent sur la pile uniquement. L'état interne du hasher (`~208 octets`) est alloué sur la pile de l'appelant FFI. Aucune dépendance vers `alloc`. Vérification au `cargo build` : `cargo tree -p liturgical-calendar-core` doit rester vide.

Retourne `KAL_ENGINE_OK` (0) si toutes les validations passent.

### 7.2 `kal_read_entry`

```c
int32_t kal_read_entry(
    const uint8_t    *data,
    size_t            len,
    uint16_t          year,   // [1969, 2399]
    uint16_t          doy,    // [0, 365]
    KalCalendarEntry *out_entry
);
```

**Implémentation Rust — ordre d'exécution obligatoire et figé :**

```rust
pub unsafe extern "C" fn kal_read_entry(
    data: *const u8,
    len: usize,
    year: u16,
    doy: u16,
    out_entry: *mut CalendarEntry,
) -> i32 {
    // INV-FFI-1 : NULL checks — premiers, inconditionnels
    if data.is_null() || out_entry.is_null() {
        return KAL_ERR_NULL_PTR;
    }

    // Guards domaine — OBLIGATOIREMENT avant tout calcul arithmétique.
    // Si year < 1969 et casté en u32 avant le guard, la soustraction
    // (year as u32 - 1969) wrappe silencieusement en release → idx invalide.
    if year < 1969 || year > 2399 {
        return KAL_ERR_INDEX_OOB;
    }
    if doy > 365 {
        return KAL_ERR_INDEX_OOB;
    }

    // Calcul en u32 — overflow impossible après les guards ci-dessus.
    // Valeur maximale : (2399 - 1969) × 366 + 365 = 157 645
    // u32::MAX = 4 294 967 295 — marge ×27 000. u64 non nécessaire.
    let idx: u32 = (year as u32 - 1969) * 366 + doy as u32;

    // Défense en profondeur contre un header corrompu (collision SHA-256 théorique
    // ou plage non standard). Les guards précédents garantissent idx ≤ 157 645 ;
    // entry_count nominal vaut 431 × 366 = 157 746. Ce check reste obligatoire.
    if idx >= header.entry_count {
        return KAL_ERR_INDEX_OOB;
    }

    // INV-FFI-2 : validation de la fenêtre mémoire avant déréférencement
    let offset: usize = 64 + idx as usize * 8;
    if offset + 8 > len {
        return KAL_ERR_INDEX_OOB;
    }

    // SAFETY : null check passé (INV-FFI-1). offset + 8 ≤ len vérifié (INV-FFI-2).
    // CalendarEntry est #[repr(C)], align 2. offset = 64 + idx*8 est pair — alignement garanti.
    let entry = unsafe { (data.add(offset) as *const CalendarEntry).read_unaligned() };
    unsafe { out_entry.write(entry) };
    KAL_ENGINE_OK
}
```

La Padding Entry (`primary_id = 0`) est retournée normalement — `KAL_ENGINE_OK` — l'interprétation est laissée à l'appelant.

**Lecture de `entry_count` dans `kal_read_entry` :**

`kal_read_entry` ne doit **pas** appeler `kal_validate_header` en interne — cela violerait le contrat O(1) (SHA-256 est O(N) sur la taille du fichier). `entry_count` est lu directement depuis les 4 octets à l'offset 12 du buffer, sans validation SHA-256 :

```rust
// SAFETY: len >= 64 vérifié (INV-FFI-2). Offset 12+4=16 ≤ len.
let entry_count = u32::from_le_bytes([
    *data.add(12), *data.add(13),
    *data.add(14), *data.add(15),
]);
```

Responsabilité de l'appelant : valider le fichier via `kal_validate_header` avant tout appel à `kal_read_entry`. L'Engine n'impose pas d'ordre d'appel — la défense en profondeur (`idx >= entry_count → KAL_ERR_INDEX_OOB`) couvre les cas de header corrompu ayant échappé à la validation.

### 7.3 `kal_read_secondary`

```c
int32_t kal_read_secondary(
    const uint8_t *data,
    size_t         len,
    uint16_t       secondary_index,
    uint8_t        secondary_count,
    uint16_t      *out_ids,      // buffer fourni par l'appelant
    uint8_t        out_capacity  // taille du buffer out_ids
);
```

Lit `secondary_count` FeastIDs consécutifs depuis le Secondary Pool à partir de `secondary_index`.

Si `secondary_count == 0` : retourne `KAL_ENGINE_OK` immédiatement, sans accès mémoire.

**Bounds check — promotion obligatoire avant addition :**

```rust
// secondary_index : u16 (max 65 535), secondary_count : u8 (max 255)
// Somme max = 65 790 — dépasse u16::MAX (65 535) → overflow si calcul en u16.
// Promotion en u32 obligatoire avant l'addition.
let end_idx: u32 = secondary_index as u32 + secondary_count as u32;

// pool_offset (u32) + end_idx * 2 peut approcher 2^32 sur un fichier pathologique.
// Promotion en u64 pour ce calcul final.
let byte_end: u64 = header.pool_offset as u64 + end_idx as u64 * 2;
if byte_end > len as u64 {
    return KAL_ERR_POOL_OOB;
}
```

### 7.4 `kal_scan_flags`

```c
int32_t kal_scan_flags(
    const uint8_t *data,
    size_t         len,
    uint16_t       flag_mask,
    uint16_t       flag_value,
    uint32_t      *out_indices,   // buffer d'indices fourni par l'appelant
    uint32_t       out_capacity,
    uint32_t      *out_count      // nombre d'entrées trouvées
);
```

Scan linéaire du Data Body. Retourne les `idx` des entrées pour lesquelles `(flags & flag_mask) == flag_value`. Complexité O(N).

**Propriétés de vectorisation :** le stride constant de 8 octets et l'offset fixe de `flags` à la position 4 de chaque entrée permettent une implémentation manuelle via intrinsics de gather (ex : `_mm256_i32gather_epi32` sur AVX2) ou une boucle scalaire que certains compilateurs auto-vectorisent. Les `flags` ne sont pas contigus en mémoire (stride > 1) — un load SIMD contigu n'est pas applicable directement. La performance réelle doit être mesurée par benchmark sur les cibles CI avant toute affirmation de gain. L'alignement pair de `flags` (offset 4) garantit l'absence d'accès non-aligné dans tous les cas.

---

## 8. Codes de Retour FFI

| Code                    | Valeur | Description                                                  |
| ----------------------- | ------ | ------------------------------------------------------------ |
| `KAL_ENGINE_OK`         | 0      | Succès                                                       |
| `KAL_ERR_NULL_PTR`      | -1     | Pointeur nul passé en argument                               |
| `KAL_ERR_BUF_TOO_SMALL` | -2     | Buffer out-param insuffisant                                 |
| `KAL_ERR_MAGIC`         | -3     | Magic invalide                                               |
| `KAL_ERR_VERSION`       | -4     | Version non supportée                                        |
| `KAL_ERR_CHECKSUM`      | -5     | SHA-256 invalide                                             |
| `KAL_ERR_FILE_SIZE`     | -6     | Taille de fichier incohérente avec `entry_count`/`pool_size` |
| `KAL_ERR_INDEX_OOB`     | -7     | Index `(year, doy)` hors bornes                              |
| `KAL_ERR_POOL_OOB`      | -8     | Accès Secondary Pool hors bornes                             |
| `KAL_ERR_RESERVED`      | -9     | Champ `_reserved` non nul                                    |

---

## 9. Format `.lits` — Language Index Table System

Le `.lits` est l'artefact compagnon du `.kald`. Il contient les labels textuels localisés pour une langue donnée, indexés par `(FeastID, année)`. Un `.lits` est produit par langue compilée. Le `.kald` est toujours produit en premier — le `.lits` est produit à l'**Étape 6**, après que les FeastIDs sont définitivement alloués.

### 9.1 Invariants de séparation

- Le `.kald` ne contient **aucune chaîne de caractères**. Topologie pure, 8 bytes par slot.
- Le `.lits` ne contient **aucune donnée de topologie**. Labels uniquement.
- Les deux artefacts sont liés par le champ `kald_build_id` du header `.lits` — vérification de cohérence obligatoire côté client avant tout accès conjoint.

### 9.2 Data Layout Binaire

```
[ Header   :  32 octets ]
[ Entry Table : entry_count × 10 octets ]
[ String Pool : pool_size octets, UTF-8, offsets en octets depuis le début du pool ]
```

**Header (32 octets, `#[repr(C)]`) :**

| Champ          | Type      | Offset | Valeur / Note                                      |
| -------------- | --------- | ------ | -------------------------------------------------- |
| `magic`        | `[u8; 4]` | 0      | `b"LITS"`                                          |
| `version`      | `u16 LE`  | 4      | `1`                                                |
| `lang`         | `[u8; 6]` | 6      | Code langue UTF-8, zéro-padded (ex: `b"la\0\0\0\0"`) |
| `kald_build_id`| `[u8; 8]` | 12     | 8 premiers octets du SHA-256 du `.kald` compagnon  |
| `entry_count`  | `u32 LE`  | 20     | Nombre d'entrées dans l'Entry Table               |
| `pool_offset`  | `u32 LE`  | 24     | Offset du String Pool depuis le début du fichier  |
| `pool_size`    | `u32 LE`  | 28     | Taille du String Pool en octets                   |

**Entry Table — chaque entrée (10 octets) :**

| Champ        | Type     | Note                                                       |
| ------------ | -------- | ---------------------------------------------------------- |
| `feast_id`   | `u16 LE` | FeastID alloué par la Forge                                |
| `from`       | `u16 LE` | Année de début du bloc `history[]` concerné                |
| `to`         | `u16 LE` | Année de fin (`0xFFFF` = indéfini)                        |
| `str_offset` | `u32 LE` | Offset de la chaîne dans le String Pool (octets)           |

La table est triée par `(feast_id ASC, from ASC)`. La recherche d'un label pour `(feast_id, year)` est une **recherche binaire** sur `feast_id`, puis scan linéaire de la plage `from ≤ year ≤ to`.

**String Pool :**

Chaînes UTF-8 concaténées. Chaque chaîne est terminée par `\0`. L'offset pointe vers le premier octet de la chaîne (pas le terminateur). Aucun alignement interne — accès séquentiel uniquement depuis l'Entry Table.

### 9.3 Interface Engine (`no_std`, `no_alloc`)

```rust
/// Projecteur de mémoire sur un buffer .lits fourni par l'appelant.
/// Zéro allocation. Zéro copie. Zéro état interne.
pub struct LitsProvider<'a> {
    data: &'a [u8],
}

impl<'a> LitsProvider<'a> {
    /// Construit le projecteur depuis un buffer brut.
    /// Valide uniquement le magic et la version — pas de SHA-256 (responsabilité client).
    pub fn new(data: &'a [u8]) -> Result<Self, LitsError>;

    /// Retourne le label pour (feast_id, year).
    /// Recherche binaire sur feast_id + scan plage [from, to].
    /// None si aucune entrée ne couvre l'année demandée (fête absente pour cet intervalle).
    pub fn get(&self, feast_id: u16, year: u16) -> Option<&'a str>;
}
```

**Complexité :** O(log N + K) avec N = `entry_count` et K = nombre d'entrées pour ce `feast_id` (K ≤ 10 en pratique — nombre de blocs `history[]` d'une fête).

**Contrat `None` :** un `None` indique que la fête n'avait pas de label pour cette année dans cette langue (ex: fête canonisée en 2014, requête en 1990). Ce n'est pas une erreur. Le client gère l'affichage (`"?"`, ID brut, fallback).

### 9.4 Vérification de Cohérence Client

```rust
// À effectuer par le client avant tout accès conjoint .kald + .lits
let kald_build_id = &kald_header.checksum[..8];
let lits_build_id = lits_provider.build_id();   // bytes 12–19 du header .lits
if kald_build_id != lits_build_id {
    return Err(ArtifactMismatch { kald: *kald_build_id, lits: *lits_build_id });
}
```

Cette vérification est de la responsabilité du **client** (`std`), pas de l'Engine — conformément à INV-W5.

### 9.5 Invariants `.lits`

- Toutes les lectures numériques utilisent `from_le_bytes` (LE canonique, déterminisme cross-platform).
- `LitsProvider` opère sur un `&[u8]` fourni par l'appelant — aucune allocation (INV-W1, INV-W2).
- La Forge produit le `.lits` à l'Étape 6, après l'allocation définitive des FeastIDs. L'ordre des entrées dans l'Entry Table est déterministe : tri par `(feast_id, from)` croissants.
- Un seul `.lits` par langue compilée. La Forge produit autant de `.lits` que de langues présentes dans `i18n/`.
- Le fallback latin est résolu **AOT** (Étape 1bis) — le `.lits` d'une langue ne contient jamais de référence vers le `.lits` latin. Chaque `.lits` est autonome et complet.

---

## 10. Validations Forge (V1–V6) et Erreurs de Résolution (V7–V12)

Les validations V1–V6 et V-T1–V-T3 sont appliquées lors de l'**Étape 1 (Rule Parsing)**. Les validations V-I1–V-I2 sont appliquées lors de l'**Étape 1bis (i18n Resolution)**. Les erreurs V7–V8 sont levées lors de l'**Étape 4 (Conflict Resolution)**. Les erreurs V9–V10 sont levées lors de l'**Étape 6 (Binary Packing)**. Un seul échec, à n'importe quelle étape, interrompt la compilation. Les codes V1–V12, V-T1–V-T4 et V-I1–V-I2 sont les identifiants canoniques dans les variants `RegistryError` / `ParseError` / `ForgeError` Rust.

La définition exhaustive de chaque validation (conditions formelles, hints d'erreur) est dans **`liturgical-scheme.md` §8**. Ce tableau est la clé de correspondance entre les deux documents.

**Validations Étape 1 (Rule Parsing) — codes numérotés :**

| Code spec | Variant Rust                                                         | Déclencheur                                                                               | Groupe `liturgical-scheme.md` §8 |
| --------- | -------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- | -------------------------------- |
| **V1**    | `RegistryError::TemporalOverlap { slug, year, conflicting_entries }` | Deux versions `history[]` actives la même année, même slug, même scope                    | **Groupe B — V2d**               |
| **V2**    | `RegistryError::InvalidPrecedenceValue(u8)`                          | `precedence > 12` dans une entrée YAML (valeurs 13–15 réservées système)                  | **Groupe D — V2-Bis**            |
| **V3**    | `RegistryError::FeastIDExhausted { scope, category }`                | Dépassement des 4095 séquences allouables par (Scope, Category) — voir §5.1               | **Groupe B — V2c**               |
| **V4**    | `RegistryError::InvalidTemporalRange { from, to }`                   | `from > to`, ou `from < 1969`, ou `to > 2399`                                             | **Groupe C — V3b**               |
| **V5**    | `RegistryError::UnknownNatureString(String)`                         | Valeur de `nature` non reconnue dans les enums §4.2                                       | **Groupe D — V5**                |
| **V6**    | `ParseError::InvalidSlugSyntax(String)`                              | Stem du nom de fichier ne satisfait pas `[a-z][a-z0-9_]*` — rejeté avant parsing YAML    | **Groupe D — V6**                |

**Validations additionnelles Étape 1** (sans code V-numéroté dans la spec) :

| Variant Rust                                                     | Déclencheur                                                                                                                                          | Groupe `liturgical-scheme.md` §8 |
| ---------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------- |
| `ParseError::MalformedYaml` / `UnsupportedSchemaVersion`         | Syntaxe YAML invalide ou `version != 1` (`format_version` est supprimé — sa présence produit `MalformedYaml`)                                        | **Groupe A — V1**                |
| `RegistryError::DuplicateSlug { slug, scope }`                   | Même stem de fichier déclaré deux fois dans le même scope                                                                                            | **Groupe B — V2a**               |
| `RegistryError::FeastIDLockConflict { slug, yaml_id, lock_id }`  | Collision entre l'`id` explicite du YAML et le FeastID enregistré dans `feast_registry.lock` pour ce slug — le lock a priorité (INV-FORGE-3)         | **Groupe B — V2b**               |
| `ParseError::InvalidDate { slug, month, day }`                   | Date fixe impossible (ex: 30 février)                                                                                                                | **Groupe C — V3a**               |
| `ParseError::CircularDependency { slug, anchor }`                | Cycle dans le graphe des ancres mobiles                                                                                                              | **Groupe D — V4**                |
| `ParseError::TransferAmbiguous { slug, collides }`               | Entrée `transfers` déclarant `offset` et `date` simultanément                                                                                        | **Groupe E — V-T1**              |
| `ParseError::TransferEmpty { slug, collides }`                   | Entrée `transfers` sans `offset` ni `date`                                                                                                           | **Groupe E — V-T1**              |
| `ParseError::UnknownCollidesTarget { slug, collides }`           | Slug déclaré dans `collides` absent du `FeastRegistry` au terme de l'Étape 1                                                                         | **Groupe E — V-T2**              |
| `ParseError::TransferDuplicateCollides { slug, collides }`       | Deux entrées `transfers` référencent le même concurrent                                                                                              | **Groupe E — V-T3**              |

**Validations Étape 1bis (i18n Resolution) :**

| Variant Rust                                                     | Déclencheur                                                                              | Groupe `liturgical-scheme.md` §8 |
| ---------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | -------------------------------- |
| `ParseError::I18nMissingLatinKey { slug, from, field }`          | Clé `{slug}.{from}.{field}` absente du dictionnaire `i18n/la/`                           | **Groupe F — V-I1**              |
| `ParseError::I18nOrphanKey { slug, lang, from, field }`          | Clé dictionnaire dont l'année `from` est absente du `history[]` YAML correspondant       | **Groupe F — V-I2**              |

**Validations Étape 3 — Contrainte offset/ordinal (V4a, v2.2) :**

Ces validations sont appliquées **avant** la désérialisation du bloc `history` — erreur fatale, aucune sortie partielle.

| Condition                                          | Erreur fatale                                          |
| -------------------------------------------------- | ------------------------------------------------------ |
| `anchor: tempus_ordinarium` + `offset` présent     | `ParseError::OffsetOnOrdinalAnchor { slug }`           |
| `anchor: tempus_ordinarium` + `ordinal` absent     | `ParseError::MissingOrdinal { slug }`                  |
| `anchor: tempus_ordinarium` + `ordinal` ∉ [1, 34]  | `ParseError::OrdinalOutOfRange { slug, ordinal }`      |
| `anchor: <autre>` + `ordinal` présent              | `ParseError::OrdinalOnNonOrdinalAnchor { slug, anchor }`|

**Erreurs Étape 4 (Conflict Resolution) :**

| Code spec | Variant Rust                                                               | Déclencheur                                                                                                                                     |
| --------- | -------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| **V7**    | `ForgeError::SolemnityCollision { slug_a, slug_b, precedence, doy, year }` | Deux Solennités (`Precedence ≤ 5`) de même scope et même rang sur le même DOY — arbitrage humain requis dans le YAML (§3.3)                     |
| **V8**    | `ForgeError::TransferFailed { slug, origin_doy, blocked_at, year }`        | Fête transférable (`Precedence ≤ 9`) sans slot libre dans `[doy+1, doy+7]`, après épuisement des règles déclaratives `transfers` (§3.3 pipeline + §2.4 scheme) |

**Erreurs Étape 4 (Day Materialization) :**

| Code spec | Variant Rust                                                   | Déclencheur                                                                                                                                                                                     |
| --------- | -------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **V11**   | `ForgeError::SecondaryPoolOverflow { pool_len, max_capacity }` | Le Secondary Pool dépasse 65 535 entrées `u16` après construction — `secondary_index (u16)` ne peut plus adresser la fin du pool. Corpus YAML anormalement dense ou déduplication insuffisante. |

**Note de capacité Secondary Pool :**

`secondary_index` est un `u16` (max 65 535). Cette limite est suffisante pour tout corpus Novus Ordo réaliste avec déduplication active : en pratique, ~30–50 jours par an portent un `secondary_count ≥ 1`, et les séquences dupliquées entre années partagent le même index. L'estimation worst-case sans déduplication (~78 000 entrées sur 431 ans) dépasse la capacité — la déduplication n'est donc pas une optimisation mais une **contrainte d'implémentation**.

`PoolBuilder::insert` doit vérifier la capacité résiduelle à chaque insertion :

```rust
impl PoolBuilder {
    fn insert(&mut self, mut ids: Vec<u16>) -> Result<u16, ForgeError> {
        ids.sort_unstable();
        if let Some(&existing) = self.index.get(&ids) {
            return Ok(existing);
        }
        // Vérification de capacité avant insertion
        if self.data.len() + ids.len() > u16::MAX as usize {
            return Err(ForgeError::SecondaryPoolOverflow {
                pool_len:     self.data.len() as u32,
                max_capacity: u16::MAX as u32,
            });
        }
        let idx = self.data.len() as u16;
        self.index.insert(ids.clone(), idx);
        self.data.extend_from_slice(&ids);
        Ok(idx)
    }
}
```

Si V11 se déclenche, le remède est d'abord de vérifier que INV-FORGE-2 est respecté (déduplication active) avant d'envisager toute extension du format binaire. Passer `secondary_index` à `u32` nécessiterait de porter `CalendarEntry` à 12 octets, brisant le stride de 8 octets — décision d'architecture non triviale à ne pas prendre sans données réelles sur le corpus.

**Erreurs Étape 6 (Binary Packing) :**

| Code spec | Variant Rust                                                            | Déclencheur                                                                                                                                                          |
| --------- | ----------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **V9**    | `ForgeError::FeastIDMutated { slug, expected_id, found_id, doy, year }` | Le `primary_id` d'une entrée ne correspond pas au FeastID enregistré dans le `FeastRegistry` pour ce slug — corruption interne du pipeline                           |
| **V10**   | `ForgeError::PaddingEntryMissing { year, doy }`                         | `doy = 59` d'une année non-bissextile ne contient pas la Padding Entry attendue (`primary_id = 0, flags = 0, secondary_count = 0`) — cohérence binaire violée        |
| **V12**   | `ForgeError::SecondaryCountOverflow { doy, year, count }`               | Le nombre de commémorations/mémoires pour un slot DOY dépasse 255 — capacité du champ `secondary_count: u8` dépassée ; corpus anormalement dense ou corpus malformé  |

**Règle d'interprétation :** tout déclencheur V1–V12, V-T1–V-T4 et V-I1–V-I2 produit un arrêt immédiat. La Forge n'émet aucun artefact partiel (ni `.kald`, ni `.lits`). Les avertissements (`ConflictWarning`) ne sont pas des erreurs — ils n'interrompent pas la compilation mais doivent être traités avant toute mise en production.

---

## 11. Outil de Diagnostic : `kald-inspect`

Binaire `std` indépendant (hors workspace Engine). Lien avec `liturgical-calendar-core` via l'ABI C.

Fonctionnalités :

- Validation header via `kal_validate_header`
- Dump d'entrées par `(year, doy)` via `kal_read_entry`
- Affichage des commémorations via `kal_read_secondary`
- Statistiques du Secondary Pool (densité, entrées avec commémorations)
- Vérification SHA-256 avec sortie hexadécimale

```
kald-inspect <fichier.kald> [--year <année>] [--doy <doy>] [--dump-pool]
```

---

## Annexe A : FeastRegistry et Format YAML

Le format YAML de saisie des fêtes liturgiques, la logique de versionnement (`history[]`), les champs `from`/`to`, la hiérarchie de scopes (universel / national / diocésain), les règles de nommage des slugs (dérivés du stem du nom de fichier), la structure des dictionnaires i18n et les algorithmes de résolution temporelle sont définis exhaustivement dans le document **`liturgical-scheme.md` v1.3** — Contrat de Données Amont.

Ce document est la **source de vérité unique** pour les Étapes 1 et 1bis du pipeline Forge. Toute référence à l'Annexe B.11 de la spécification v1.0 est obsolète.

**Adaptations v2.0 documentées dans `liturgical-scheme.md` :**

- Champs `valid_from`/`valid_to` (v1.0) renommés en `from`/`to`
- Plage temporelle admise : `[1969, 2399]` (validation V3b)
- FeastID : u16 (vs u32/18 bits en v1.0) — Scope sur bits [15:14], Category sur bits [13:12], Sequence sur bits [11:0]
- Support des dates mobiles via `mobile.anchor` + `mobile.offset`
- Hiérarchie de scopes étendue : `universal → national → diocesan`
- Commémorations alimentées par le Secondary Pool (Étape 6), pas directement par le YAML

**Adaptations v2.0.2 (scheme v1.2) :**

- `slug` supprimé du corps YAML — déduit du stem du nom de fichier (`path.file_stem()`)
- `format_version` remplacé par `version` — rupture de schéma sans compatibilité ascendante
- Bloc `transfers` introduit pour la résolution déclarative des collisions (Passe 3, Étape 4)
- Groupe E de validations (V-T1, V-T2, V-T3) — erreurs Étape 1

**Adaptations v2.0.4 (scheme v1.3) :**

- **Zéro String dans le YAML** — le corpus YAML est un graphe de données pur ; `title` et tout champ textuel sont supprimés
- **Dictionnaires i18n externes** — arborescence `i18n/{lang}/{slug}.yaml` ; clé composite implicite `{slug}.{from}.{field}`
- **Latin comme langue source obligatoire** — fallback AOT résolu en Étape 1bis ; le `.lits` produit est autonome par langue
- **Groupe F de validations (V-I1, V-I2)** — corrélation YAML ↔ dictionnaires, appliquée en Étape 1bis
- **Format `.lits` revu** — year-aware (`Entry Table` indexée par `(FeastID, from, to)`) ; `LitsProvider::get(feast_id, year)` en O(log N + K)

**Adaptations v2.1 (scheme v1.3.2) :**

- Ancres `nativitas` et `epiphania` ajoutées — O(1), indépendantes, résolution avant `adventus`
- Contrat de données amont : `liturgical-scheme.md` v1.3.2

**Adaptations v2.2 (scheme v1.3.3) :**

- Ancre `tempus_ordinarium` avec champ `ordinal` exclusif — O(1), dépend de `adventus`
- Validations V4a : contraintes `offset`/`ordinal` pour `anchor: tempus_ordinarium`
- Résolution `tempus_ordinarium` : O(1) via `DOY(adventus) − 7 × (35 − ordinal)`
- Invariant : `ordinal` exclusif à `anchor: tempus_ordinarium` — rejet fatal si combinaison incorrecte
- Décisions architecturales gelées : `transfers` interdit pour du calcul structurel
- Contrat de données amont : `liturgical-scheme.md` v1.3.3

---

## Annexe B : Note de Migration v1.0 → v2.0

### Éléments supprimés

| Élément                                | Emplacement v1.0 | Note                                             |
| -------------------------------------- | ---------------- | ------------------------------------------------ |
| `SlowPath`                             | Engine           | Remplacé par la Forge                            |
| `compute_easter`                       | Engine           | Migré dans la Forge (Étape 2)                    |
| `SeasonBoundaries::compute`            | Engine           | Migré dans la Forge (Étape 2)                    |
| `TemporalLayer`, `SanctoralLayer`      | Engine           | Absorbés dans la Forge (Étape 3)                 |
| `FeastDefinitionPacked` (`NonZeroU32`) | Engine           | N/A en architecture AOT-Only                     |
| `OnceLock<SlowPath>`                   | Engine           | Supprimé avec SlowPath                           |
| `kal_compute_day`                      | API FFI          | Supprimé — l'Engine ne calcule plus              |
| `kal_read_day`                         | API FFI          | Remplacé par `kal_read_entry`                    |
| `kal_index_day`                        | API FFI          | Logique internalisée dans `kal_read_entry`       |
| `kal_scan_precedence`                  | API FFI          | Remplacé par `kal_scan_flags` (masque générique) |
| `day_of_year_to_month_day`             | Engine           | Migré dans la Forge si nécessaire                |
| `DayPacked` (u32 bitfield)             | Format binaire   | Remplacé par `CalendarEntry` + `flags` u16       |
| `CorruptionInfo`                       | Engine           | Sans objet — l'Engine ne reconstruit plus de Day |
| Philosophie Fast/Slow Path             | Architecture     | Remplacée par AOT-Only                           |
| Plage algorithmique 1583–4099          | Architecture     | Remplacée par plage AOT 1969–2399                |
| Sentinelle `0xFFFFFFFF`                | Contrat binaire  | Remplacée par `primary_id = 0` (Padding Entry)   |
| `slug` (champ YAML)                    | Schéma YAML      | Supprimé v2.0.2 — déduit du stem du nom de fichier (§2.1 scheme) |
| `format_version` (champ YAML)          | Schéma YAML      | Supprimé v2.0.2 — remplacé par `version`                         |
| `title` (champ `history[]` YAML)       | Schéma YAML      | Supprimé v2.0.4 — externalisé dans `i18n/{lang}/{slug}.yaml`     |
| `StringProvider` (API v2.0 initiale)   | Engine/Forge     | Remplacé v2.0.4 par `LitsProvider` year-aware (§9)               |

### Éléments conservés intégralement

- Format `.lits` (endianness LE canonique) — structure révisée en v2.0.4 (year-aware)
- `FeastRegistry` (BTreeMap, Forge)
- Structure YAML et système de slugs/scopes/history
- Validations V1–V6 (adaptations mineures : V3 capacité 4095, V4 plage `[1969, 2399]`)
- Types `Precedence`, `Nature`, `LiturgicalPeriod` — valeurs numériques inchangées
- Type `Color` — valeurs 0–5 inchangées, largeur étendue à 4 bits dans `flags`
- Conventions FFI : préfixe `kal_*`, ABI `extern "C"`, codes `KAL_ERR_*`
- Invariants INV-W1 à INV-W5
- Politique Little-Endian canonique sur tout champ numérique
- `kald-inspect`
- Déterminisme bit-for-bit et SHA-256 cross-platform

### Changements de convention

| Aspect               | v1.0                                     | v2.0                                                            |
| -------------------- | ---------------------------------------- | --------------------------------------------------------------- |
| Convention DOY       | 1-based (`doy ∈ [1, 366]`)               | **0-based** (`doy ∈ [0, 365]`)                                  |
| Padding Entry        | Sentinelle `0xFFFFFFFF` dans `DayPacked` | `primary_id = 0` dans `CalendarEntry`                           |
| FeastID width        | 18 bits dans `DayPacked`                 | **u16** dans `CalendarEntry.primary_id`                         |
| Header size          | 16 octets                                | **64 octets** (+ SHA-256 32 octets, `pool_offset`, `pool_size`) |
| Format binaire entry | `DayPacked` u32 (4 octets)               | **`CalendarEntry`** 8 octets                                    |
| Commémorations       | N/A (v1.0 one-primary only)              | **Secondary Pool** (`secondary_index` + `secondary_count`)      |
| Plage couverte       | Algorithmique : 1583–4099                | **AOT uniquement : 1969–2399**                                  |
| Clé d'identité YAML  | Champ `slug` dans le corps YAML          | **Stem du nom de fichier** (`path.file_stem()`) — v2.0.2        |
| Version schéma YAML  | `format_version: 1`                      | **`version: 1`** — v2.0.2 — rupture sans compatibilité          |
| Gestion collisions   | Code impératif Étape 3                   | **Bloc `transfers` déclaratif** (scheme §2.4) — v2.0.2          |
| Résolution intra-slot | if/else sur Precedence + FeastID tiebreaker | **`ResolutionKey` tri canonique** (`sort_unstable_by_key`) — v2.0.3 |
| Labels textuels       | Champ `title` dans `history[]` YAML + `StringProvider(FeastID)` | **Dictionnaires i18n externes** + `LitsProvider::get(FeastID, year)` year-aware — v2.0.4 |
| Pipeline Forge        | 5 étapes                                   | **6 étapes** (Étape 1bis : i18n Resolution) — v2.0.4               |
| Résolution TO         | N/A                                        | **`anchor: tempus_ordinarium` + `ordinal`** O(1) — v2.2            |

---

**Fin de la Spécification Technique v2.2**

_Document révisé le 2026-04-10 (v2.2.0). Architecture AOT-Only : Engine (`liturgical-calendar-core`) projecteur de mémoire O(1), 4 fonctions FFI, `no_std`/`no_alloc`. Forge (`liturgical-calendar-forge`) compilateur AOT, pipeline en 6 étapes. Format binaire `.kald` v2.0 : Header 64 octets, `CalendarEntry` 8 octets, Secondary Pool. Format `.lits` year-aware : Header 32 octets, Entry Table `(FeastID, from, to, str_offset)`, String Pool UTF-8. Convention DOY 0-based. Plage 1969–2399 (431 ans). Modifications v2.0.2 : slug/version/transfers/V-T1–V-T3. Modifications v2.0.3 : `ResolutionKey`. Modifications v2.0.4 : zéro String YAML, dictionnaires i18n, Étape 1bis, `LitsProvider`, V-I1–V-I2. Corrections v2.0.5 : desugaring `pentecostes` (Étape 1) ; V12 `SecondaryCountOverflow` ; V-T1–V-T4 ; V3a étendue aux dates de transfert. Modifications v2.1 : ancres `nativitas`, `epiphania`. Modifications v2.2 : ancre `tempus_ordinarium` + champ `ordinal`, ordre de résolution des ancres, validations V4a (`OffsetOnOrdinalAnchor`, `MissingOrdinal`, `OrdinalOutOfRange`, `OrdinalOnNonOrdinalAnchor`). Contrat de données amont : `liturgical-scheme.md` v1.3.3._
