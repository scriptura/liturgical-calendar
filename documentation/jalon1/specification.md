# Spécification Technique : Liturgical Calendar v2.0

**Statut** : Canonique / Ready for Implementation  
**Architecture** : AOT-Only / DOD / FFI-First  
**Workspace** : `liturgical-calendar-forge` (std) / `liturgical-calendar-core` (no_std, no_alloc)  
**Langage Domaine** : Latin (Strictement Canonique)  
**Déterminisme** : Bit-for-bit reproductible  
**Date de Révision** : 2026-04-05  
**Version** : 2.0.1

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

[partie expurgée pour éviter surcharge cognitive de Claude Sonnet]

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

## 9. StringProvider / Format `.lits`

Le format `.lits` fournit les chaînes localisées associées aux FeastIDs. `StringProvider` mappe `FeastID (u16) → Option<&str>`.

**Header `.lits` (ajout v2.0) :**

Le fichier `.lits` embarque un champ `kald_build_id : [u8; 8]` dans son header, contenant les 8 premiers octets du SHA-256 du `.kald` avec lequel il a été compilé. Ce champ permet la vérification de cohérence croisée avant toute utilisation conjointe des deux fichiers.

```rust
// Vérification de cohérence — à effectuer par le client avant tout accès
let kald_build_id = &kald_header.checksum[..8];
if lits_header.kald_build_id != kald_build_id {
    return Err(ArtifactMismatch {
        kald_build_id: *kald_build_id,
        lits_build_id: lits_header.kald_build_id,
    });
}
```

Cette vérification est de la responsabilité du **client** (couche `std`), pas de l'Engine — conformément à INV-W5 (zéro diagnostic dans l'Engine).

**Contrat `StringProvider::get` :**

```rust
impl StringProvider<'_> {
    /// Retourne le nom localisé associé au FeastID, ou None si absent.
    /// Ne panique jamais. Ne retourne jamais de référence invalide.
    /// Un FeastID absent indique une désynchronisation entre le .kald et le .lits
    /// (corpus partiellement mis à jour) — le client décide du rendu (ID brut, "?", etc.).
    pub fn get(&self, id: u16) -> Option<&str>;
}
```

**`None` n'est pas une erreur fatale.** Le client traite `None` selon sa politique d'affichage : ID brut hexadécimal (`0x05A1`), chaîne `"?"`, ou valeur de fallback localisée. L'Engine ne prend jamais cette décision.

**Invariants `.lits` :**

- Toutes les lectures numériques utilisent `from_le_bytes` (LE canonique, déterminisme cross-platform)
- Aucune allocation dans `StringProvider` — opère sur un `&[u8]` fourni par l'appelant (INV-W1, INV-W2)
- La Forge produit le `.lits` ; l'Engine le consomme en lecture seule
- Le `kald_build_id` est vérifié par le client avant le premier appel à `StringProvider::get`

---

## 10. Validations Forge (V1–V6) et Erreurs de Résolution (V7–V10)

Les validations V1–V6 sont appliquées lors de l'**Étape 1 (Rule Parsing)**. Les erreurs V7–V8 sont levées lors de l'**Étape 3 (Conflict Resolution)**. Les erreurs V9–V10 sont levées lors de l'**Étape 5 (Binary Packing)**. Un seul échec, à n'importe quelle étape, interrompt la compilation. Les codes V1–V10 sont les identifiants canoniques dans les variants `RegistryError` / `ParseError` / `ForgeError` Rust.

La définition exhaustive de chaque validation V1–V6 (conditions formelles, exemples, hints d'erreur) est dans **`liturgical-scheme.md` §8**. Ce tableau est la clé de correspondance entre les deux documents.

**Validations Étape 1 (Rule Parsing) :**

| Code spec | Variant Rust                                                         | Déclencheur                                                                 | Groupe `liturgical-scheme.md` §8 |
| --------- | -------------------------------------------------------------------- | --------------------------------------------------------------------------- | -------------------------------- |
| **V1**    | `RegistryError::TemporalOverlap { slug, year, conflicting_entries }` | Deux versions `history[]` actives la même année, même slug, même scope      | **Groupe B — V2d**               |
| **V2**    | `RegistryError::InvalidPrecedenceValue(u8)`                          | `precedence > 12` dans une entrée YAML (valeurs 13–15 réservées système)    | **Groupe D — V2-Bis**            |
| **V3**    | `RegistryError::FeastIDExhausted { scope, category }`                | Dépassement des 4095 séquences allouables par (Scope, Category) — voir §5.1 | **Groupe B — V2c**               |
| **V4**    | `RegistryError::InvalidTemporalRange { from, to }`                   | `from > to`, ou `from < 1969`, ou `to > 2399`                               | **Groupe C — V3b**               |
| **V5**    | `RegistryError::UnknownNatureString(String)`                         | Valeur de `nature` non reconnue dans les enums §4.2                         | **Groupe D — V5**                |
| **V6**    | `RegistryError::InvalidSlugSyntax(String)`                           | Caractère illicite dans le slug (`[a-z][a-z0-9_]*` exigé)                   | **Groupe D — V6**                |

**Erreurs Étape 3 (Conflict Resolution) :**

| Code spec | Variant Rust                                                               | Déclencheur                                                                                                                               |
| --------- | -------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| **V7**    | `ForgeError::SolemnityCollision { slug_a, slug_b, precedence, doy, year }` | Deux Solennités (`Precedence ≤ 5`) de même scope et même rang sur le même DOY — arbitrage humain requis dans le YAML (§3.3)               |
| **V8**    | `ForgeError::TransferFailed { slug, origin_doy, blocked_at, year }`        | Fête transférable (`Precedence ≤ 9`) sans slot libre dans `[doy+1, doy+7]` — aucun déclassement automatique, YAML source à réviser (§3.4) |

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

**Erreurs Étape 5 (Binary Packing) :**

| Code spec | Variant Rust                                                            | Déclencheur                                                                                                                                                          |
| --------- | ----------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **V9**    | `ForgeError::FeastIDMutated { slug, expected_id, found_id, doy, year }` | Le `primary_id` d'une entrée ne correspond pas au FeastID enregistré dans le `FeastRegistry` pour ce slug — corruption interne du pipeline (§5.1)                    |
| **V10**   | `ForgeError::PaddingEntryMissing { year, doy }`                         | `doy = 59` d'une année non-bissextile ne contient pas la Padding Entry attendue (`primary_id = 0, flags = 0, secondary_count = 0`) — cohérence binaire violée (§5.1) |

**Validations additionnelles Étape 1** (sans code V-numéroté dans la spec) :

| Variant Rust                                                    | Déclencheur                                                                                                                                  | Groupe `liturgical-scheme.md` §8 |
| --------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------- |
| `ParseError::MalformedYaml` / `UnsupportedSchemaVersion`        | Syntaxe YAML invalide ou `format_version != 1`                                                                                               | **Groupe A — V1**                |
| `RegistryError::DuplicateSlug { slug, scope }`                  | Même slug déclaré deux fois dans le même scope                                                                                               | **Groupe B — V2a**               |
| `RegistryError::FeastIDLockConflict { slug, yaml_id, lock_id }` | Collision entre l'`id` explicite du YAML et le FeastID enregistré dans `feast_registry.lock` pour ce slug — le lock a priorité (INV-FORGE-3) | **Groupe B — V2b**               |
| `ParseError::InvalidDate { slug, month, day }`                  | Date fixe impossible (ex: 30 février)                                                                                                        | **Groupe C — V3a**               |
| `ParseError::CircularDependency { slug, anchor }`               | Cycle dans le graphe des ancres mobiles                                                                                                      | **Groupe D — V4**                |

**Règle d'interprétation :** tout déclencheur V1–V10 produit un arrêt immédiat. La Forge n'émet aucun `.kald` partiel. Les avertissements (`ConflictWarning`) ne sont pas des erreurs — ils n'interrompent pas la compilation mais doivent être traités avant toute mise en production.

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

**Fin de la Spécification Technique v2.0 — Ready for Implementation**
