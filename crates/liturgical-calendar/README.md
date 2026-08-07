# liturgical-calendar

Façade du calendrier liturgique catholique (Novus Ordo, 1969–2399).

Point d'entrée unique pour les intégrateurs Rust. Ré-exporte l'intégralité de la surface publique de [`liturgical-calendar-core`](https://crates.io/crates/liturgical-calendar-core) — moteur `no_std`, `no_alloc`, accès O(1) par `(année, jour_de_l_année)`.

## Utilisation

```toml
[dependencies]
liturgical-calendar = "0.1.4"
```

```rust
use liturgical_calendar::{
    kal_read_entry, kal_read_feast,
    TimelineEntry, FeastEntry, KAL_ENGINE_OK,
};

let kald: Vec<u8> = std::fs::read("romanus_universale.kald")?;

// 1. Valider le fichier à l'ouverture (SHA-256 + invariants structurels)
let rc = unsafe { kal_validate_header(kald.as_ptr(), kald.len(), std::ptr::null_mut()) };
assert_eq!(rc, KAL_ENGINE_OK);

// 2. Lire l'occurrence journalière — O(1)
let mut entry = TimelineEntry::zeroed();
let rc = unsafe { kal_read_entry(kald.as_ptr(), kald.len(), 2025, 109, &mut entry) };
assert_eq!(rc, KAL_ENGINE_OK);

// 3. Résoudre les invariants de la fête — O(1)
if !entry.is_padding() {
    let mut feast = FeastEntry::zeroed();
    let rc = unsafe {
        kal_read_feast(kald.as_ptr(), kald.len(), entry.primary_index, &mut feast)
    };
    assert_eq!(rc, KAL_ENGINE_OK);

    let nature     = feast.nature()?;
    let precedence = feast.precedence()?;
    let color      = feast.color()?;
}
```

```rust
// Labels i18n (zero-copy)
use liturgical_calendar::lits_provider::LitsProvider;

let lits: Vec<u8> = std::fs::read("romanus_universale_la.lits")?;
let provider = LitsProvider::new(&lits)?;

if let Some(lits_entry) = provider.get(feast.feast_id, 2025) {
    println!("{}", lits_entry.label);
    if let Some(ann) = lits_entry.annotation { println!("{}", ann); }
}
```

## Intégration C / FFI

La génération du header C (`liturgical_calendar_core.h`) s'effectue directement depuis [`liturgical-calendar-core`](https://crates.io/crates/liturgical-calendar-core), là où résident les symboles `#[no_mangle] extern "C"` :

```bash
cargo build -p liturgical-calendar-core --features gen-headers
```

Les intégrateurs FFI ciblent `liturgical-calendar-core` pour compiler la bibliothèque (`.a` / `.so`) et récupérer l'interface C-ABI. La façade n'intervient pas dans ce flux.

```c
#include "liturgical_calendar_core.h"

// Valider une fois à l'ouverture
kal_validate_header(kald_data, kald_len, NULL);

// Lire et résoudre
TimelineEntry entry;
kal_read_entry(kald_data, kald_len, 2025, 109, &entry);

FeastEntry feast;
kal_read_feast(kald_data, kald_len, entry.primary_index, &feast);
```

## Architecture

```
YAML corpus
  → kal-forge  (build-time, liturgical-calendar-forge)
    → romanus_universale.kald
        Header 80B | Feast Registry | Timeline | Secondary Pool
    → romanus_universale_la.lits
      → liturgical_calendar::kal_read_entry   (runtime, O(1))
      → liturgical_calendar::kal_read_feast   (runtime, O(1))
      → liturgical_calendar::lits_provider    (zero-copy)
```

Les artefacts `.kald` et `.lits` sont produits par [`liturgical-calendar-forge`](https://crates.io/crates/liturgical-calendar-forge). Ce crate ne contient aucune règle liturgique — il consomme uniquement les artefacts pré-compilés.

## Crates du Projet

| Crate | Rôle |
|---|---|
| `liturgical-calendar` | Façade — point d'entrée utilisateur (ce crate) |
| [`liturgical-calendar-core`](https://crates.io/crates/liturgical-calendar-core) | Engine `no_std` — lecture des artefacts binaires |
| [`liturgical-calendar-forge`](https://crates.io/crates/liturgical-calendar-forge) | Compilateur YAML → `.kald` + `.lits` |
