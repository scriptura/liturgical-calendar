# Liturgical Scheme — Contrat de Données Amont

**Statut :** Canonique / Source de Vérité YAML  
**Scope :** `liturgical-calendar-forge` — Étapes 1 (Rule Parsing) et 1bis (i18n Resolution)  
**Version :** 2.0.0  
**Date de révision :** 2026-04-30

---

## 0. Rôle de ce Document

Ce document est le **contrat de données amont** de la Forge. Il définit exhaustivement le format YAML des fichiers corpus et des dictionnaires i18n. Toute entrée conforme à ce schéma peut être ingérée sans ambiguïté par le pipeline.

**Flux de transformation :**

```
YAML corpus (version, category, date|mobile, history…)
+ Dictionnaires i18n (la/, fr/, …)
  → [Étape 1]    Rule Parsing + Validations V1–V6, V-T1–V-T5
  → [Étape 1bis] i18n Resolution — corrélation YAML ↔ dicts, fallback latin AOT
  → [Étapes 2–5] Canonicalization → Resolution → Materialization → Vespers pass
  → [Étape 6]    Binary Packing
      → .kald  (CalendarEntry 8 octets/slot — topologie pure, zéro chaîne)
      → .lits  (labels indexés par FeastID + plage d'années)
```

**Invariants absolus :**

- Toute entrée YAML est **validée à la compilation** (AOT). Aucune erreur de configuration n'atteint le runtime.
- Le `slug` est la clé de déduplication humaine, **déduit du stem du nom de fichier** — jamais déclaré dans le corps YAML.
- **Zéro champ textuel** dans les fichiers corpus. Les labels sont externalisés dans les dictionnaires i18n (§4).
- Le `feast_registry.lock` et le `variant_registry.lock` sont versionnés — garantissent la stabilité des IDs inter-builds.
- Tout échec de validation est **fatal** — la Forge n'émet aucun binaire partiel.

---

## 1. Organisation du Corpus sur Disque

### 1.1 Arborescence

```
corpus/
├── romanus/
│   ├── feast_registry.lock
│   ├── variant_registry.lock
│   ├── universale/
│   │   ├── i18n/
│   │   │   ├── la/              ← langue source obligatoire
│   │   │   └── fr/
│   │   ├── temporale/           ← fêtes avec mobile:
│   │   └── sanctorale/          ← fêtes avec date:
│   │       ├── 01/
│   │       └── …/12/
│   ├── continentalia/
│   │   └── europae/
│   │       ├── i18n/
│   │       └── sanctorale/
│   ├── nationalia/
│   │   └── FR/
│   │       ├── i18n/
│   │       ├── sanctorale/
│   │       └── temporale/
│   ├── dioecesana/
│   │   └── PARIS/
│   │       └── sanctorale/
│   └── ordines/
│       └── praedicatorum/
│           └── sanctorale/
└── ambrosianus/
    ├── feast_registry.lock
    ├── variant_registry.lock
    └── universale/
        ├── i18n/
        ├── temporale/
        └── sanctorale/
```

**Règles structurelles :**

- Chaque rite est un **espace de nommage isolé** — slugs, FeastIDs et artefacts indépendants.
- `temporale/` accueille les fêtes déclarées avec `mobile:`. `sanctorale/` accueille celles déclarées avec `date:`.
- Un scope contenant un fichier `DRAFT` à sa racine est ignoré par `kal-forge` sauf option `-d`.
- Les dictionnaires i18n sont **colocalisés par scope** : `{scope}/i18n/{lang}/{slug}.yaml`.

### 1.2 Dérivation du Scope depuis le Chemin

Le scope est déduit du chemin — il n'est pas déclaré dans le YAML.

| Chemin                                          | Scope      | FeastID bits [15:14] |
|-------------------------------------------------|------------|----------------------|
| `corpus/{rite}/universale/**/{slug}.yaml`       | universal  | `00`                 |
| `corpus/{rite}/continentalia/{ID}/**/{slug}.yaml` | national | `01`                 |
| `corpus/{rite}/nationalia/{ISO}/**/{slug}.yaml` | national   | `01`                 |
| `corpus/{rite}/dioecesana/{ID}/**/{slug}.yaml`  | diocesan   | `10`                 |
| `corpus/{rite}/ordines/{ORDO}/**/{slug}.yaml`   | diocesan   | `10`                 |

### 1.3 Ordre d'Ingestion (INV-FORGE-1)

La Forge ingère dans l'ordre, tri lexicographique à chaque niveau :

```
1. universale/temporale/
2. universale/sanctorale/
3. continentalia/{ID}/temporale/  sanctorale/   (tri lex sur ID)
4. nationalia/{ISO}/temporale/    sanctorale/   (tri lex sur ISO)
5. dioecesana/{ID}/temporale/     sanctorale/   (tri lex sur ID)
6. ordines/{ORDO}/temporale/      sanctorale/   (tri lex sur ORDO)
```

Un répertoire absent est ignoré silencieusement.

---

## 2. Format d'un Fichier Corpus

### 2.1 Structure Générale

```yaml
version: 1          # Obligatoire
category: <0–3>     # Sous-espace FeastID — bits [13:12]
id: <u16>           # Optionnel — FeastID forcé (voir §2.2)

# Exactement UN des deux blocs suivants :
date:
  month: <1–12>
  day:   <1–31>
mobile:
  anchor:  <anchor_id>   # voir §3.2
  offset:  <integer>     # interdit si anchor = tempus_ordinarium
  ordinal: <1–34>        # exclusif à anchor: tempus_ordinarium

history:
  - from: <year>         # défaut : 1969 si omis
    to:   <year|~>       # null = indéfini (2399)
    precedence: <1–13>   # Tabella dierum, YAML 1-based → interne 0-based
    nature:    <string>  # voir §6.2
    color:     <string>  # voir §6.3
    period:    <string>  # optionnel — voir §6.4
    has_vigil_mass: <bool> # optionnel, défaut false
    transfers:           # optionnel, scoped à [from, to] — voir §2.4
      - collides: <slug>
        offset: <i32 ≠ 0>   # OU date: OU mobile: — mutuellement exclusifs
```

> **Surcharge partielle :** un fichier de scope national/diocésain peut omettre `date`/`mobile` si la temporalité est héritée du scope universel.

### 2.2 Slug et `category`

**Slug :** déduit du stem du nom de fichier (`path.file_stem()`). Syntaxe : `[a-z][a-z0-9_]*`. Un rename de fichier est un changement d'identité — il entraîne l'allocation d'un nouveau FeastID et la tombstonisation de l'ancien dans le lock.

```
ioannis_pauli_ii.yaml  →  slug = "ioannis_pauli_ii"   ✓
s_ioannis_pauli_ii.yaml                                ✗  encode le statut
```

**`category` — layout FeastID u16 :**

```
 15  14  13  12  11  ...  0
┌───┬───┬───┬───┬─────────┐
│ S   S │ C   C │Sequence │
└───┴───┴───┴───┴─────────┘
```

| `category` | Usage conventionnel                  |
|------------|--------------------------------------|
| 0          | Temporal (Dominicales, Pâques, Avent…) |
| 1          | Sanctoral universel                  |
| 2          | Propres nationaux/diocésains         |
| 3          | Extensions futures                   |

### 2.3 `history` — Tranches Temporelles

```yaml
history:
  - from: 1969     # borne inférieure inclusive (défaut 1969)
    to: 2013        # borne supérieure inclusive — null/~ = 2399
    precedence: 12
    nature: memoria
    color: albus
  - from: 2014
    precedence: 10
    nature: memoria
    color: albus
```

Les tranches doivent être **disjointes** (V2d). La Forge trie par `from` croissant avant traitement. Pour une année `y`, une seule entrée peut être active — zéro entrée active = fête absente (Padding Entry).

### 2.4 `transfers` — Résolution Déclarative des Collisions

Optionnel, déclaré **à l'intérieur** d'une entrée `history[]` — actif uniquement pour sa plage `[from, to]`.

```yaml
history:
  - from: 2008
    precedence: 4
    nature: sollemnitas
    color: albus
    transfers:
      - collides: sacratissimi_cordis_iesu   # collision → offset rétrograde (anticipation)
        offset: -1
      - collides: dominica_in_palmis         # collision → cible fixe
        date:
          month: 3
          day: 15
      - collides: feria_ii_hebdomadae_sanctae  # collision → offset avant
        offset: 2
      - collides: nativitas_domini           # collision → cible mobile
        mobile:
          anchor: pascha
          offset: -8

```

| Clé | Type | Sémantique |
| --- | --- | --- |
| `collides` | slug (String) | Fête dont la présence sur le même DOY déclenche le transfert |
| `offset` | entier signé (i32) | Décalage algébrique en jours depuis le DOY de collision (positif = postposition, négatif = anticipation/report rétrograde). |
| `date` | `{month, day}` | Date fixe de repli |
| `mobile` | `{anchor, offset}` | Cible calculée depuis une ancre primitive — offset signé admis |

`offset`, `date` et `mobile` sont mutuellement exclusifs (V-T1). `collides` doit référencer un slug du FeastRegistry (V-T2).

### 2.5 `has_vigil_mass`

Booléen optionnel (défaut `false`). Quand `true`, indique une **messe de vigile propre** — réservée aux Solennités (V-Vigilia). La Forge lève le bit `HAS_VIGILIA` sur la `CalendarEntry` du DOY et sur celle du DOY−1 (passe vespérale, Étape 5).

---

## 3. Temporalité

### 3.1 Dates Fixes

```yaml
date:
  month: 12
  day: 25
```

Conversion en DOY 0-based : `MONTH_STARTS[month-1] + day - 1`.  
Le 29 février produit `doy = 59`. Les années non-bissextiles, la Forge écrit une **Padding Entry** (`primary_id = 0`) — la fête n'est pas transférée.

### 3.2 Dates Mobiles

```yaml
mobile:
  anchor: pascha
  offset: +39
```

**Ancres disponibles :**

| `anchor_id`         | Champ requis | Définition                                              |
|---------------------|--------------|---------------------------------------------------------|
| `pascha`            | `offset`     | Dimanche de Pâques (algorithme Meeus/Jones/Butcher)     |
| `adventus`          | `offset`     | Premier dimanche de l'Avent (∼ 30 novembre)             |
| `pentecostes`       | `offset`     | Alias de `pascha + 49`                                  |
| `nativitas`         | `offset`     | Dimanche dans l'Octave de Noël [Dec 26–31]              |
| `epiphania`         | `offset`     | Premier dimanche strictement après le 6 janvier [7–13] |
| `tempus_ordinarium` | `ordinal`    | N-ième dimanche du Temps Ordinaire (1–34)               |

`tempus_ordinarium` utilise `ordinal` (pas `offset`). Les deux champs sont mutuellement exclusifs selon l'ancre (V4a). DOY : `DOY(adventus) − 7 × (35 − ordinal)`.

**Fêtes mobiles standards :**

| Fête                         | Ancre           | Offset |
|------------------------------|-----------------|--------|
| Dominica in Palmis           | `pascha`        | −7     |
| Feria IV Cinerum             | `pascha`        | −46    |
| Ascensio Domini              | `pascha`        | +39    |
| Pentecostes                  | `pascha`        | +49    |
| Corpus Christi               | `pascha`        | +60    |
| Sacratissimi Cordis Iesu     | `pascha`        | +68    |
| Sancta Familia               | `nativitas`     | 0      |
| In Baptismate Domini         | `epiphania`     | 0      |

---

## 4. Dictionnaires i18n

### 4.1 Format d'un Fichier i18n

```yaml
# corpus/romanus/universale/i18n/la/dominica_ii_paschae.yaml
version: 1
history:
  - from: 1969        # optionnel — défaut : 1969
    to: 2001          # optionnel — parsé pour lisibilité, non stocké
    label: "Dominica II Paschæ"
    annotation: "*In albis*"   # optionnel, Markdown admis
  - from: 2002
    label: "Dominica II Paschæ"
    annotation: "Dominica in octava paschæ seu de sacra misericordia, *In albis*"
```

**Règles :**

- `label` est obligatoire dans chaque bloc `history`.
- `from` est optionnel si la fête n'a qu'une seule tranche (défaut 1969). Si absent et que la fête démarre après 1969, la Forge remappage automatiquement.
- Si plusieurs tranches partagent le même label, seule la première doit le déclarer — la Forge propage le label vers les tranches suivantes sans entrée explicite.
- `annotation` est facultatif. Si absent, l'Engine reçoit `annotation_offset = 0xFFFF_FFFF`.
- Le formatage Markdown (`*italique*`) est transmis tel quel à l'UI cliente.

### 4.2 Clé Composite Implicite

L'identité d'un label est la clé `{slug}.{from}.label` — jamais déclarée, reconstruite par la Forge lors de la corrélation YAML ↔ dictionnaires.

### 4.3 Hiérarchie et Fallback

La Forge traverse les scopes dans l'ordre d'ingestion (§1.3). Pour chaque scope, elle charge `{scope}/i18n/{lang}/`. Les scopes plus spécifiques écrasent les universels (last-write-wins). Si une traduction est absente pour `lang=fr`, le label latin est injecté AOT dans le `.lits` français — l'Engine ne connaît pas de fallback.

### 4.4 Exemple Complet — Jean-Paul II

```yaml
# corpus/romanus/nationalia/PL/sanctorale/ioannis_pauli_ii.yaml
version: 1
category: 1
date:
  month: 10
  day: 22
history:
  - from: 2011
    to: 2013
    precedence: 12
    nature: memoria
    color: albus
  - from: 2014
    precedence: 12
    nature: memoria
    color: albus
```

```yaml
# corpus/romanus/nationalia/PL/i18n/la/ioannis_pauli_ii.yaml
version: 1
history:
  - from: 2011
    to: 2013
    label: "B. Ioannis Pauli II, papæ"
  - from: 2014
    label: "S. Ioannis Pauli II, papæ"
```

```yaml
# corpus/romanus/nationalia/PL/i18n/fr/ioannis_pauli_ii.yaml
version: 1
history:
  - from: 2014
    label: "Saint Jean-Paul II, pape"
    # 2011–2013 absent → Forge injecte le label latin en fallback AOT
```

---

## 5. Hiérarchie et Scopes

### 5.1 Scopes

| Scope       | Description                   | Champ `region`            | Bits FeastID |
|-------------|-------------------------------|---------------------------|--------------|
| `universal` | Proprium universale du rite   | —                         | `00`         |
| `national`  | Proprium national             | Code ISO 3166-1 (ex: `FR`) | `01`         |
| `diocesan`  | Propre diocésain ou ordo      | Identifiant (ex: `PARIS`) | `10`         |
| _(réservé)_ | Usage futur                   | —                         | `11`         |

### 5.2 Fusion des Scopes

Ordre de priorité croissante (le scope le plus local l'emporte) :

```
universale < continentalia < nationalia < dioecesana < ordines
```

Un fichier de scope local peut omettre `date`/`mobile` — la temporalité est héritée du scope universel. Les labels i18n du scope local écrasent les universels via le même mécanisme last-write-wins.

### 5.3 CompilationTarget et Artefacts

Chaque `CompilationTarget` = `rite × scope` produit un artefact autonome. La CLI `kal-forge` est le point d'entrée.

**Nommage des artefacts (chemin aplati avec `_`) :**

| Scope                    | `.kald`                        | `.lits`                           |
|--------------------------|--------------------------------|-----------------------------------|
| `romanus/universale`     | `romanus_universale.kald`      | `romanus_universale_la.lits`      |
| `romanus/nationalia/FR`  | `romanus_nationalia_FR.kald`   | `romanus_nationalia_FR_fr.lits`   |
| `ambrosianus/universale` | `ambrosianus_universale.kald`  | `ambrosianus_universale_la.lits`  |

---

## 6. Référentiel des Valeurs Admises

### 6.1 `precedence` — Degrés Liturgiques

Valeurs YAML : **1 à 13** (Tabella dierum, rangs I–XIII). La Forge normalise `precedence_yaml − 1 → interne [0–12]` au point Serde. Valeur plus faible = priorité plus haute.

| YAML | Interne | Niveau canonique                                              |
|------|---------|---------------------------------------------------------------|
| 1    | 0       | Triduum Sacrum                                                |
| 2    | 1       | Nativitas, Epiphania, Ascensio, Pentecostes                   |
| 3    | 2       | Dominicae Adventus, Quadragesimae, Paschales                  |
| 4    | 3       | Feria IV Cinerum ; Hebdomada Sancta                           |
| 5    | 4       | Sollemnitates Domini, BMV, Sanctorum (Cal. Generali)          |
| 6    | 5       | Sollemnitates propriae                                        |
| 7    | 6       | Festa Domini in Calendario Generali                           |
| 8    | 7       | Dominicae per annum                                           |
| 9    | 8       | Festa BMV et Sanctorum in Calendario Generali                 |
| 10   | 9       | Festa propria                                                 |
| 11   | 10      | Feriae Adventus (17–24 Dec) ; Octava Nativitatis              |
| 12   | 11      | Memoriae obligatoriae                                         |
| 13   | 12      | Feriae per annum ; Memoriae ad libitum                        |

### 6.2 `nature` — Type Liturgique (bits flags [13:11])

| Valeur YAML    | `Nature` Rust          | Valeur numérique |
|----------------|------------------------|------------------|
| `sollemnitas`  | `Nature::Sollemnitas`  | 0                |
| `festum`       | `Nature::Festum`       | 1                |
| `dominica`     | `Nature::Dominica`     | 2                |
| `memoria`      | `Nature::Memoria`      | 3                |
| `commemoratio` | `Nature::Commemoratio` | 4                |
| `feria`        | `Nature::Feria`        | 5                |

`natura: memoria` implique `precedence ∈ {12, 13}` (V-Natura-Memoria).

### 6.3 `color` — Couleur Liturgique (bits flags [7:4])

| Valeur YAML | `Color` Rust       | Valeur numérique | Usage canonique                      |
|-------------|--------------------|------------------|--------------------------------------|
| `albus`     | `Color::Albus`     | 0                | Fêtes du Seigneur, Vierge, Confesseurs |
| `rubeus`    | `Color::Rubeus`    | 1                | Passion, Apôtres, Martyrs, Pentecôte |
| `viridis`   | `Color::Viridis`   | 2                | Temps ordinaire                      |
| `violaceus` | `Color::Violaceus` | 3                | Avent, Carême                        |
| `rosaceus`  | `Color::Rosaceus`  | 4                | Gaudete (Avent III), Laetare (Carême IV) |
| `niger`     | `Color::Niger`     | 5                | Messes des défunts                   |

### 6.4 `period` — Saison Liturgique (bits flags [10:8], optionnel)

| Valeur YAML            | `LiturgicalPeriod` Rust       | Valeur numérique |
|------------------------|-------------------------------|------------------|
| `tempus_ordinarium`    | `TempusOrdinarium`            | 0 (défaut calculé) |
| `tempus_adventus`      | `TempusAdventus`              | 1                |
| `tempus_nativitatis`   | `TempusNativitatis`           | 2                |
| `tempus_quadragesimae` | `TempusQuadragesimae`         | 3                |
| `triduum_paschale`     | `TriduumPaschale`             | 4                |
| `tempus_paschale`      | `TempusPaschale`              | 5                |
| `dies_sancti`          | `DiesSancti`                  | 6                |

Si omis, la Forge calcule la saison depuis les `SeasonBoundaries` de l'année (Étape 2).

---

## 7. Mapping YAML ↔ `CalendarEntry`

| Champ YAML                   | Destination binaire                    | Note                                      |
|------------------------------|----------------------------------------|-------------------------------------------|
| _(stem fichier)_             | —                                      | Slug → FeastID. Absent du `.kald`.        |
| `id`                         | `CalendarEntry.primary_id` [off 0]     | Alloué par la Forge si absent.            |
| `date` / `mobile`            | DOY 0-based (Étape 2)                  | Absent du `.kald` — implicite par offset. |
| `transfers`                  | —                                      | Consommé à l'Étape 3 uniquement.          |
| `history[].precedence`       | `flags` bits [3:0]                     | Valeur interne (YAML − 1).                |
| `history[].color`            | `flags` bits [7:4]                     |                                           |
| `history[].period`           | `flags` bits [10:8]                    | Calculé si absent.                        |
| `history[].nature`           | `flags` bits [13:11]                   |                                           |
| `history[].has_vigil_mass`   | `flags` bit 15 + bit 15 du DOY−1      | Reporté par la passe vespérale (Étape 5). |
| _(clé i18n implicite)_       | Fichier `.lits`                        | Absent du `.kald`.                        |
| `scope`                      | FeastID bits [15:14]                   |                                           |
| `category`                   | FeastID bits [13:12]                   |                                           |
| —                            | `secondary_index` [off 2]              | Alimenté par l'Étape 4.                   |
| —                            | `secondary_count` [off 6]              | Alimenté par l'Étape 3.                   |

**Encodage `flags` :**

```rust
fn encode_flags(p: Precedence, c: Color, lp: LiturgicalPeriod, n: Nature) -> u16 {
    (p as u16) | ((c as u16) << 4) | ((lp as u16) << 8) | ((n as u16) << 11)
    // bits [15:14] = 0 (réservés hors vespers pass)
}
```

---

## 8. Validations Forge

Toute violation est fatale. Codes de validation par groupe :

### Groupe A — Syntaxe YAML (V1)

- **V1** — Syntaxe YAML valide, `version == 1`, exactement un de `{date, mobile}` présent.
- Violations : `ParseError::MalformedYaml`, `UnsupportedSchemaVersion`, `MissingTemporalityField`, `AmbiguousTemporalityField`.

### Groupe B — Unicité des Identifiants (V2)

- **V2a** — Unicité des slugs par scope. → `RegistryError::DuplicateSlug`
- **V2b** — Unicité des FeastIDs explicites. → `RegistryError::FeastIDConflict`
- **V2c** — Capacité FeastID ≤ 4095 par `(scope, category)`. → `RegistryError::FeastIDExhausted`
- **V2d** — Unicité temporelle dans `history[]` : au plus une entrée active par année. → `RegistryError::TemporalOverlap`

### Groupe C — Intégrité des Dates (V3)

- **V3a** — Cohérence `month`/`day` (dates fixes et cibles de transfert). → `ParseError::InvalidDate`
- **V3b** — Plages `[from, to]` ⊆ [1969, 2399], `from ≤ to`. → `RegistryError::InvalidTemporalRange`

### Groupe D — Domaines et Cycles (V4, V5, V6)

- **V4** — Ancres connues, pas de cycle dans le graphe de dépendances. → `ParseError::UnknownAnchor`, `CircularDependency`
- **V4a** — `offset` interdit si `anchor = tempus_ordinarium` ; `ordinal ∈ [1, 34]`. → `ParseError::OffsetOnOrdinalAnchor`, `MissingOrdinal`, `OrdinalOutOfRange`, `OrdinalOnNonOrdinalAnchor`
- **V5** — `nature` ∈ valeurs admises (§6.2). → `RegistryError::UnknownNatureString`
- **V-Natura-Memoria** — `nature = memoria` ⟹ `precedence_yaml ∈ {12, 13}`. → `ParseError::InvalidMemoriaPrecedence`
- **V-Vigilia** — `has_vigil_mass = true` ⟹ `nature = sollemnitas`. → `ParseError::VigiliaNonSollemnitas`
- **V6** — Stem fichier : `[a-z][a-z0-9_]*`. → `ParseError::InvalidSlugSyntax`
- **V2-Bis** — `precedence_yaml ∈ [1, 13]`. → `ParseError::MalformedYaml`

### Groupe E — Bloc `transfers` (V-T1–V-T5)

- **V-T1** — Exactement une option parmi `offset`, `date`, `mobile`. → `ParseError::TransferAmbiguous`, `TransferEmpty`
- **V-T2** — `collides` ∈ slugs du FeastRegistry. → `ParseError::UnknownCollidesTarget`
- **V-T3** — `collides` unique au sein d'un même bloc `transfers`. → `ParseError::TransferDuplicateCollides`
- **V-T4** — `offset` (direct) ≠ 0. → `ParseError::TransferOffsetIsZero`
- **V-T5** — `mobile.anchor` ∈ ancres primitives (hors `tempus_ordinarium`). → `ParseError::TransferMobileInvalidAnchor`

### Groupe F — i18n (V-I1, V-I2)

- **V-I1** — Pour chaque `(slug, from)` du registry, `i18n/la/{slug}.yaml` doit contenir un bloc avec ce `from`. → `ParseError::I18nMissingLatinKey`
- **V-I2** — Toute clé `from` d'un dictionnaire doit correspondre à un `from` du registry pour ce slug. → `ParseError::I18nOrphanKey`

---

## 9. Exemples

### 9.1 Fête Fixe — Nativité du Seigneur

```yaml
# universale/sanctorale/nativitas_domini.yaml
version: 1
category: 0
date:
  month: 12
  day: 25
history:
  - from: 1969
    precedence: 2
    nature: sollemnitas
    color: albus
    period: tempus_nativitatis
    has_vigil_mass: true
```

```yaml
# universale/i18n/la/nativitas_domini.yaml
version: 1
history:
  - from: 1969
    label: "In Nativitate Domini"
```

### 9.2 Fête Mobile — Ascension

```yaml
# universale/temporale/ascensio_domini.yaml
version: 1
category: 0
mobile:
  anchor: pascha
  offset: +39
history:
  - from: 1969
    precedence: 2
    nature: sollemnitas
    color: albus
    period: tempus_paschale
```

### 9.3 Transfers Scoped — Saint Joseph

```yaml
# universale/sanctorale/iosephi_sponsi_bmv.yaml
version: 1
category: 1
date:
  month: 3
  day: 19
history:
  - from: 1969
    to: 2007
    precedence: 5
    nature: sollemnitas
    color: albus
  - from: 2008
    precedence: 5
    nature: sollemnitas
    color: albus
    transfers:
      - collides: dominica_in_palmis
        mobile:
          anchor: pascha
          offset: -8
      - collides: feria_ii_hebdomadae_sanctae
        mobile:
          anchor: pascha
          offset: -8
      - collides: feria_iii_hebdomadae_sanctae
        mobile:
          anchor: pascha
          offset: -8
      - collides: feria_iv_in_hebdomada_sancta
        mobile:
          anchor: pascha
          offset: -8
```

### 9.4 Surcharge Nationale

```yaml
# nationalia/FR/sanctorale/nativitas_domini.yaml
# date héritée du scope universel
version: 1
category: 0
history:
  - from: 1969
    precedence: 1    # promue en rang I pour la France
    nature: sollemnitas
    color: albus
```

### 9.5 Dimanche du Temps Ordinaire

```yaml
# universale/temporale/dominica_x_temporis_ordinarii.yaml
version: 1
category: 0
mobile:
  anchor: tempus_ordinarium
  ordinal: 10
history:
  - from: 1969
    precedence: 8
    nature: dominica
    color: viridis
```

---

## 10. Checklist de Conformité

**Fichier YAML corpus**

- [ ] `version: 1` présent
- [ ] Placement correct : `temporale/` si `mobile:`, `sanctorale/` si `date:`
- [ ] Stem fichier : `[a-z][a-z0-9_]*`, neutre (sans statut liturgique encodé)
- [ ] Exactement un bloc `date:` ou `mobile:`
- [ ] **Aucun champ textuel** (`title`, `label`, `name`…) dans aucun bloc `history[]`
- [ ] Plages `[from, to]` disjointes, dans `[1969, 2399]`
- [ ] `precedence ∈ [1, 13]`
- [ ] Si `nature: memoria` : `precedence ∈ {12, 13}`
- [ ] Si `has_vigil_mass: true` : `nature: sollemnitas`
- [ ] `collides` dans `transfers` : slug existant dans le corpus

**Dictionnaire i18n**

- [ ] `version: 1` présent
- [ ] `label` présent dans chaque bloc `history`
- [ ] Un fichier `i18n/la/{slug}.yaml` existe pour chaque fichier YAML corpus soumis
- [ ] Chaque `from` du dictionnaire correspond à un `from` du YAML corpus (pas de clé orpheline)
- [ ] `from` peut être omis si la fête n'a qu'une seule tranche (la Forge remappage automatiquement)
- [ ] Si plusieurs tranches partagent le même label, seule la première doit le déclarer
- [ ] Les dictionnaires non-latins peuvent être partiels — le fallback est résolu AOT par la Forge
