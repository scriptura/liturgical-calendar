# Roadmap de Développement : Liturgical Calendar v2.0

**Version** : 2.0.1  
**Date de Révision** : 2026-04-05  
**Méthodologie** : 3 jalons, chacun produisant un livrable binaire validable indépendamment  
**Critères de Succès** : Conformité binaire Forge↔Engine · SHA-256 cross-platform · Fuzzing · CI 4 cibles

---

## Philosophie de la Roadmap

**Architecture :** deux crates dans un workspace Cargo. `liturgical-calendar-core` (`no_std`, `no_alloc`) est un projecteur de mémoire O(1) — 4 fonctions FFI, zéro logique de domaine. `liturgical-calendar-forge` (`std`) est le compilateur AOT qui produit les artefacts `.kald` consommés par l'Engine.

**Principe d'organisation :** chaque jalon produit un binaire ou ensemble de crates testables et validables en isolation. Aucune étape intermédiaire sans critère de sortie concret.

---

## Jalon 1 — Binary Foundation

**Périmètre :** Structure binaire `.kald` v2.0 côté Engine — Header, CalendarEntry, formule d'index, fonctions de lecture.

**Critère de sortie :** `kal_validate_header` + `kal_read_entry` compilent en `#![no_std]` sans `alloc` et passent les tests unitaires de layout. `cargo tree -p liturgical-calendar-core` ne retourne aucune dépendance autre que `sha2` (dérogation INV-W1 — RustCrypto, `default-features = false`, pas de `std`/`alloc` transitif).

---

### 1.1 Types de Domaine Engine

**Fichier :** `liturgical-calendar-core/src/types.rs`

Implémenter `Precedence`, `Nature`, `Color`, `LiturgicalPeriod` conformément aux §4.1–4.4 de la spec.

- `#[repr(u8)]` sur chaque enum
- `try_from_u8(val: u8) -> Result<Self, DomainError>` pour chaque type
- `DomainError` : `Copy`, pas de `String`, types primitifs uniquement
- Traits dérivés obligatoires par enum :

| Enum               | Traits dérivés obligatoires                                | Justification                        |
| ------------------ | ---------------------------------------------------------- | ------------------------------------ |
| `Precedence`       | `Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash` | Axe de tri principal, déjà dans spec |
| `Nature`           | `Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash` | Champ de ResolvedFeast (BTree\*)     |
| `Color`            | `Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash` | Champ de ResolvedFeast (BTree\*)     |
| `LiturgicalPeriod` | `Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash` | Champ de ResolvedFeast (BTree\*)     |

Note : `PartialOrd`/`Ord` sur `Nature`, `Color`, `LiturgicalPeriod` sont dérivés automatiquement
par discriminant (`repr(u8)`) et n'ont aucune signification liturgique — ils satisfont
uniquement la contrainte de typage des collections ordonnées.

**Tests :**

- Roundtrip `try_from_u8(v as u8) == Ok(v)` pour chaque variant
- `try_from_u8(13)` → `Err` pour Precedence (valeurs 13–15 réservées)

---

### 1.2 Header v2.0

**Fichier :** `liturgical-calendar-core/src/header.rs`

```rust
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub magic:       [u8; 4],
    pub version:     u16,
    pub variant_id:  u16,
    pub epoch:       u16,
    pub range:       u16,
    pub entry_count: u32,
    pub pool_offset: u32,
    pub pool_size:   u32,
    pub checksum:    [u8; 32],
    pub _reserved:   [u8; 8],
}

pub fn validate_header(bytes: &[u8]) -> Result<Header, HeaderError>;
```

Validations dans l'ordre :

1. `bytes.len() >= 64` → `HeaderError::FileTooSmall`
2. Magic == `b"KALD"` → `HeaderError::InvalidMagic`
3. `version == 4` → `HeaderError::UnsupportedVersion(version)`
4. `bytes.len() as u64 == 64 + entry_count as u64 * 8 + pool_size as u64` → `HeaderError::FileSizeMismatch`
5. `_reserved == [0u8; 8]` → `HeaderError::ReservedNotZero`
6. SHA-256 sur `bytes[64..]` == `checksum` → `HeaderError::ChecksumMismatch`

Désérialisation : `u16::from_le_bytes`, `u32::from_le_bytes` — LE canonique obligatoire.

**Tests :**

- `assert_eq!(size_of::<Header>(), 64)`
- `assert_eq!(align_of::<Header>(), 8)` (ou au moins 4)
- Offset de chaque champ via `offset_of!`
- Validation nominal : header construit → sérialisé → `validate_header` OK
- Chaque chemin d'erreur (magic invalide, version 3, taille incohérente, reserved non-nul, SHA erroné)

---

### 1.3 CalendarEntry v2.0

**Fichier :** `liturgical-calendar-core/src/entry.rs`

```rust
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CalendarEntry {
    pub primary_id:      u16,  // offset 0
    pub secondary_index: u16,  // offset 2
    pub flags:           u16,  // offset 4 — u16 aligné sur offset pair
    pub secondary_count: u8,   // offset 6
    pub _reserved:       u8,   // offset 7
}

impl CalendarEntry {
    /// Entrée nulle : tous les champs à zéro. `const fn` — no_alloc safe.
    pub const fn zeroed() -> Self;

    pub fn is_padding(&self) -> bool { self.primary_id == 0 }
    pub fn precedence(&self) -> Result<Precedence, DomainError>;
    pub fn color(&self)      -> Result<Color, DomainError>;
    pub fn liturgical_period(&self) -> Result<LiturgicalPeriod, DomainError>;
    pub fn nature(&self)     -> Result<Nature, DomainError>;
}

impl Default for CalendarEntry {
    fn default() -> Self { Self::zeroed() }
}
```

Extraction des champs depuis `flags` :

- `Precedence = flags & 0x000F`
- `Color = (flags >> 4) & 0x000F`
- `LiturgicalPeriod = (flags >> 8) & 0x0007`
- `Nature = (flags >> 11) & 0x0007`

**Tests :**

- `assert_eq!(size_of::<CalendarEntry>(), 8)` — stride constant, critique pour l'indexation
- `assert_eq!(offset_of!(CalendarEntry, flags), 4)` — alignement naturel u16 sur offset pair
- `assert_eq!(offset_of!(CalendarEntry, secondary_count), 6)`
- Roundtrip flags encode/decode pour toutes les combinaisons (Precedence × Nature × Color × LiturgicalPeriod)
- Padding Entry : `primary_id = 0`, `secondary_count = 0`, `is_padding() == true`
- Reserved non-nul : toléré en lecture (l'Engine ne valide pas `_reserved` au niveau entrée)

```rust
#[test]
fn zeroed_is_padding() {
    let e = CalendarEntry::zeroed();
    assert!(e.is_padding());
    assert_eq!(e.flags, 0);
    assert_eq!(e.secondary_count, 0);
    assert_eq!(e._reserved, 0);
}

#[test]
fn default_equals_zeroed() {
    assert_eq!(CalendarEntry::default(), CalendarEntry::zeroed());
}
```

---

### 1.4 Formule d'Index et `kal_read_entry`

**Fichier :** `liturgical-calendar-core/src/ffi.rs`

```rust
#[no_mangle]
pub unsafe extern "C" fn kal_read_entry(
    data: *const u8,
    len: usize,
    year: u16,
    doy: u16,
    out_entry: *mut CalendarEntry,
) -> i32 {
    // 1. Null check
    // 2. Parse header (validate_header ou lecture directe entry_count)
    // 3. Bounds : year ∈ [1969, 2399], doy ∈ [0, 365]
    // 4. idx = (year as u32 - 1969) * 366 + doy as u32
    // 5. idx < header.entry_count → KAL_ERR_INDEX_OOB
    // 6. offset = 64 + idx * 8
    // 7. Lire CalendarEntry en LE depuis data[offset..]
    // 8. *out_entry = entry ; retourner KAL_ENGINE_OK
}
```

**Tests :**

- Formule : `year=1969, doy=0` → `idx=0`, `year=2399, doy=365` → `idx = 430*366+365`
- Limites : `year=1968` → `KAL_ERR_INDEX_OOB`, `year=2400` → `KAL_ERR_INDEX_OOB`
- `doy=366` → `KAL_ERR_INDEX_OOB`
- Null `out_entry` → `KAL_ERR_NULL_PTR`
- Lecture Padding Entry `doy=59` sur une année non-bissextile → `KAL_ENGINE_OK`, `primary_id=0`

---

### 1.5 `cbindgen` — Génération `kal_engine.h`

**Fichier :** `liturgical-calendar-core/cbindgen.toml`

Générer `kal_engine.h` exposant :

- `KalHeader`, `KalCalendarEntry` (structs C)
- Constantes `KAL_ENGINE_OK`, `KAL_ERR_*`
- Prototypes `kal_validate_header`, `kal_read_entry`

Vérification : compilation d'un programme C minimal utilisant `kal_read_entry`.

**Feature `gen-headers` — déclaration obligatoire :**

`build.rs` conditionne l'invocation de cbindgen via `#[cfg(feature = "gen-headers")]`. Cette feature doit être déclarée explicitement dans `Cargo.toml`, même sans dépendance associée — Cargo 1.75+ traite `unexpected_cfg` comme erreur sous `-D warnings` :

```toml
# liturgical-calendar-core/Cargo.toml
[features]
gen-headers = []
```

cbindgen reste une `build-dependency` optionnelle, non installée par défaut. La génération de `kal_engine.h` est une étape **manuelle, hors CI standard** :

```bash
cargo build -p liturgical-calendar-core --features gen-headers
```

---

**Fin de Jalon 1**
