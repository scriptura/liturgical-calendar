# liturgical-calendar-core

Engine de lecture du calendrier liturgique catholique (Novus Ordo, 1969–2399).

Accès `O(1)` à n'importe quel jour de n'importe quelle année par simple offset arithmétique dans un buffer pré-compilé. Aucun calcul liturgique à l'exécution — toute l'intelligence est déportée dans la Forge ([`liturgical-calendar-forge`](https://crates.io/crates/liturgical-calendar-forge)).

## Caractéristiques

- `no_std`, `no_alloc` — embarquable partout
- Interface C-ABI (`extern "C"`) — iOS, Android, WebAssembly, systèmes embarqués
- Zéro dépendance runtime
- Accès O(1) par `(année, jour_de_l_année)` via offset fixe
- SHA-256 + discriminant de layout dans le header — intégrité vérifiable au chargement
- Validation structurelle O(1) séparée de la vérification SHA-256 O(N)

## Format binaire `.kald`

Un fichier `.kald` contient 431 années (1969–2399) organisées en trois segments :

```
Header          80 octets
Feast Registry  registry_count × 4 octets   — invariants des fêtes (AOT)
Timeline        157 746 × 8 octets           — occurrences journalières
Secondary Pool  variable                     — listes de fêtes secondaires (u16)
```

`FeastEntry` — 4 octets, stride constant. Stocke les **invariants** d'une fête, indépendamment du nombre d'années où elle est célébrée :

```
feast_id  u16   identifiant corpus (pour vérification croisée .lits)
flags     u16   precedence[3:0] | color[7:4] | period[10:8] | nature[13:11] | vigil[14]
```

`TimelineEntry` — 8 octets, stride constant. Encode l'**occurrence** d'une fête pour un slot donné :

```
primary_index    u16   registry_index de la fête principale (0 = Padding Entry)
secondary_offset u16   offset dans le Secondary Pool (en unités de u16)
occurrence_flags u8    bit 0 = vesperae_i, bit 1 = vigilia
secondary_count  u8    nombre de fêtes secondaires
_reserved        u16   0x0000
```

Le `registry_index` est 1-based : 0 est le sentinel Padding Entry, les fêtes réelles occupent les indices 1..=registry_count.

## Format binaire `.lits`

Fichier compagnon du `.kald`, un par langue (BCP 47) :

```
Header       32 octets   magic, version, lang (6B), kald_build_id (8B), entry_count, pool_offset, pool_size
Entry Table  entry_count × 14 octets   (feast_id, from, to, label_offset, annotation_offset)
String Pool  variable    UTF-8 null-terminé
```

L'Engine rejette tout `.lits` dont le `kald_build_id` ne correspond pas au `.kald` chargé.

## API

```rust
use liturgical_calendar_core::{
    kal_validate_header, kal_read_entry, kal_read_feast,
    TimelineEntry, FeastEntry, KAL_ENGINE_OK,
};

// 1. Validation au chargement — O(payload) — appeler une seule fois.
//    Vérifie magic, version, offsets, discriminant de layout, et SHA-256.
let rc = unsafe { kal_validate_header(kald.as_ptr(), kald.len(), std::ptr::null_mut()) };
assert_eq!(rc, KAL_ENGINE_OK);

// 2. Lecture d'une occurrence journalière — O(1).
//    Validation structurelle rapide (sans SHA-256) en interne.
let mut entry = TimelineEntry::zeroed();
let rc = unsafe { kal_read_entry(kald.as_ptr(), kald.len(), 2025, 109, &mut entry) };
assert_eq!(rc, KAL_ENGINE_OK);

// 3. Résolution des invariants de la fête — O(1).
if !entry.is_padding() {
    let mut feast = FeastEntry::zeroed();
    let rc = unsafe {
        kal_read_feast(kald.as_ptr(), kald.len(), entry.primary_index, &mut feast)
    };
    assert_eq!(rc, KAL_ENGINE_OK);

    let precedence = feast.precedence()?;
    let nature     = feast.nature()?;
    let color      = feast.color()?;
    let feast_id   = feast.feast_id;  // pour lookup .lits
}
```

```rust
// Labels via LitsProvider (zero-copy)
use liturgical_calendar_core::lits_provider::LitsProvider;

let provider = LitsProvider::new(&lits_bytes)?;
if let Some(lits_entry) = provider.get(feast.feast_id, 2025) {
    println!("{}", lits_entry.label);
    if let Some(ann) = lits_entry.annotation { println!("{}", ann); }
}
```

### Note sur la performance

`kal_validate_header` calcule le SHA-256 sur l'intégralité du payload (~1.3 Mo) : à appeler **une seule fois** au chargement. Les fonctions de lecture (`kal_read_entry`, `kal_read_feast`, etc.) utilisent `kal_validate_header_fast` en interne — vérification structurelle O(1) uniquement, sans recompute SHA-256.

Pour les sources de confiance (ROM, stockage pré-vérifié), `kal_validate_header_fast` est également exposée comme fonction publique.

## Codes de retour FFI

| Constante | Valeur | Signification |
|---|---|---|
| `KAL_ENGINE_OK` | 0 | Succès |
| `KAL_ERR_NULL_PTR` | -1 | Pointeur null |
| `KAL_ERR_BUF_TOO_SMALL` | -2 | Buffer insuffisant |
| `KAL_ERR_MAGIC` | -3 | Magic bytes invalides |
| `KAL_ERR_VERSION` | -4 | Version de format non supportée |
| `KAL_ERR_CHECKSUM` | -5 | SHA-256 incorrect — fichier corrompu |
| `KAL_ERR_FILE_SIZE` | -6 | Taille incohérente avec le header |
| `KAL_ERR_INDEX_OOB` | -7 | Année, DOY ou registry_index hors plage |
| `KAL_ERR_POOL_OOB` | -8 | Offset Secondary Pool hors limites |
| `KAL_ERR_RESERVED` | -9 | Réservé (compatibilité ABI) |
| `KAL_ERR_SCHEMA` | -10 | Discriminant de layout incompatible |

## Intégration C / FFI

```c
#include "liturgical_calendar_core.h"

// Valider une fois à l'ouverture
kal_validate_header(kald_data, kald_len, NULL);

// Lire une occurrence
TimelineEntry entry;
int rc = kal_read_entry(kald_data, kald_len, 2025, 109, &entry);

// Résoudre ses invariants
if (rc == KAL_ENGINE_OK && entry.primary_index != 0) {
    FeastEntry feast;
    kal_read_feast(kald_data, kald_len, entry.primary_index, &feast);
    uint8_t precedence = feast.flags & 0x000F;
    uint16_t feast_id  = feast.feast_id;
}
```

Le header C est généré par `cbindgen` via le feature `gen-headers`.

## Génération des fichiers `.kald` et `.lits`

Les artefacts consommés par cet Engine sont produits par [`liturgical-calendar-forge`](https://crates.io/crates/liturgical-calendar-forge).
