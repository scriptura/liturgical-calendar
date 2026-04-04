# Liturgical Scheme v1.0 — Contrat de Données Amont

**Statut** : Canonique / Source de Vérité YAML  
**Scope** : `liturgical-calendar-forge` — Étapes 1 (Rule Parsing) et 2 (Canonicalization)  
**Référence** : `specification.md` v2.0  
**Date de Révision** : 2026-03-08  
**Version** : 1.0

---

## 0. Rôle de ce Document

Ce document est le **contrat de données amont** de la Forge. Il définit exhaustivement le format YAML utilisé pour décrire les calendriers liturgiques (universel, national, diocésain). Toute entrée conforme à ce schéma peut être ingérée sans ambiguïté par les Étapes 1 et 2 du pipeline Forge.

**Flux de transformation :**

```
YAML (slug, precedence, from…)
  → [Étape 1] Rule Parsing + Validation V1–V6
    → [Étape 2] Canonicalization (DOY, Pâques, dates mobiles)
      → [Étapes 3–5] Resolution → Materialization → Binary Packing
        → .kald (CalendarEntry, lecture seule par l'Engine)
```

**Invariants absolus :**

- Toute entrée YAML est **validée à la compilation** (AOT). Aucune erreur de configuration ne peut atteindre le runtime.
- Le `slug` est la clé de déduplication humaine. La Forge le transforme en `FeastID` (u16) via le `FeastRegistry`. Le slug n'existe pas dans le binaire `.kald`.
- La Forge rejette tout fichier YAML contenant des slugs en collision, des plages temporelles incompatibles, ou des valeurs hors domaine. Tout échec de validation est fatal — aucune sortie partielle.
- Les champs `from` / `to` expriment des années grégoriennes entières. Bornes inclusives. Plage admise : **[1969, 2399]**.

---

## 1. Structure d'un Fichier YAML

Un fichier YAML de configuration liturgique est identifié par son **scope** (`universal`, `national`, `diocesan`) et, pour les scopes non-universels, par un code de région ou d'entité.

```
config/
├── universal.yaml
├── national-FR.yaml
├── national-PL.yaml
└── diocesan-PARIS.yaml
```

### 1.1 En-tête de Fichier

```yaml
# En-tête obligatoire — tous les champs sont requis
scope: universal # universal | national | diocesan
region:
  ~ # null pour universal ; code ISO 3166-1 alpha-2 pour national (ex: FR) ;
  # identifiant diocésain pour diocesan (ex: PARIS)
from: 1969 # Année de validité minimale de ce fichier (inclusive)
to: ~ # null = indéfini ; integer = borne inclusive
format_version: 1 # Version du schéma YAML (ce document = version 1)

feasts: # Liste des définitions de fêtes — voir §2
  - ...
```

> **`format_version`** : permet à la Forge de détecter un fichier de schéma incompatible et de le rejeter avec `ParseError::UnsupportedSchemaVersion` avant toute tentative de traitement.

---

## 2. Définition d'une Fête (`feast`)

Chaque entrée dans la liste `feasts` définit une fête liturgique. Elle comporte deux niveaux :

1. **Identité** : champs stables dans le temps (`slug`, `id`, `date` ou `mobile`, `scope`, `region`, `category`)
2. **Historique** : tableau `history` — un ou plusieurs blocs décrivant les métadonnées pour des plages d'années distinctes

```yaml
feasts:
  - slug: <string> # Identifiant stable — voir §2.1
    id: <u16> # Optionnel — voir §2.2
    scope: <string> # universal | national | diocesan
    region: <string> # Requis si scope != universal ; code ISO 3166-1 alpha-2
    category: <0–3> # Sous-espace FeastID — bits [13:12], 4 valeurs — voir §2.2

    # Temporalité — exactement UN des deux blocs suivants doit être présent
    date: # Fête à date FIXE — voir §3.1
      month: <1–12>
      day: <1–31>
    mobile: # Fête à date MOBILE — voir §3.2
      anchor: <anchor_id>
      offset: <integer> # Peut être négatif (ex: -7 pour le dimanche avant Pâques)

    history: # Tableau ordonné des versions — voir §4
      - from: <year>
        to: <year|~>
        title: <string>
        precedence: <0–12>
        nature: <string>
        color: <string>
        season: <string> # Optionnel — voir §2.3
```

---

## 2.1 Identité : `slug`

Le `slug` est la **clé primaire immuable** d'une fête dans le `FeastRegistry`. Il est choisi une fois, lors de la première déclaration, et ne change plus.

**Règle de neutralité obligatoire :** le slug identifie la **personne ou l'événement**, pas son statut liturgique courant.

```
✅  ioannis_pauli_ii        ← stable, indépendant du statut (Beatus / Sanctus)
❌  s_ioannis_pauli_ii      ← encode "Sanctus" — cassé à la béatification
❌  b_caroli_de_foucauld    ← encode "Beatus" — cassé si canonisé ultérieurement
```

**Justification structurelle :** si le statut est encodé dans le slug, une canonisation force un changement de clé → un nouveau `FeastID` → une rupture de continuité historique. Le `FeastID` alloué par la Forge doit être **stable** sur toute la plage 1969–2399 pour un slug donné.

**Syntaxe** : `[a-z][a-z0-9_]*` — Latin snake_case, sans accent, sans tiret, sans espace. Le premier caractère doit être une lettre (pas un chiffre). Validation V6.

**Exemples valides :** `nativitas_domini`, `ioannis_pauli_ii`, `ascensio_domini`, `dominica_in_palmis`

---

## 2.2 Identité : `id` et `category`

**FeastID — Layout u16 officiel (spec §5.1) :**

```
 15  14  13  12  11  10   9   8   7   6   5   4   3   2   1   0
┌───┬───┬───┬───┬───────────────────────────────────────────────┐
│ S   S │ C   C │             Sequence (12 bits)                │
└───┴───┴───┴───┴───────────────────────────────────────────────┘
  [15:14]  [13:12]                [11:0]
   Scope   Category              Sequence
```

| Bits  | Champ      | Valeurs                                                                           |
| ----- | ---------- | --------------------------------------------------------------------------------- |
| 15–14 | `Scope`    | `00` = Universal · `01` = National · `10` = Diocesan · `11` = réservé             |
| 13–12 | `Category` | 0–3 (4 catégories par scope)                                                      |
| 11–0  | `Sequence` | 1–4095 par (Scope, Category) ; `0` non allouable (`0x0000` réservé Padding Entry) |

**`category` (bits [13:12] du FeastID) :**

| Valeur | Usage conventionnel                             |
| ------ | ----------------------------------------------- |
| 0      | Fêtes du Temporal (Dominicales, Pâques, Avent…) |
| 1      | Fêtes du Sanctoral universel                    |
| 2      | Fêtes propres nationales ou diocésaines         |
| 3      | Usage libre / extensions futures                |

**`id` (u16, optionnel) :**

Si absent : la Forge alloue le prochain FeastID libre dans `(scope, category)` selon INV-FORGE-3 — ordre lexicographique des slugs au premier build, puis stabilité garantie par `feast_registry.lock` aux builds suivants. L'ordre d'apparition dans le YAML n'influe pas sur le FeastID alloué.

Si présent : la Forge vérifie l'absence de collision dans le `FeastRegistry`. Une collision → `RegistryError::FeastIDConflict`. Ce mécanisme est réservé aux fêtes dont l'identifiant doit être stable et documenté.

**Validation V3 :** `allocated_count(scope, category) ≤ 4095` — violation → `RegistryError::FeastIDExhausted { scope, category }`.

---

## 2.3 Champ `season` dans `history`

Le champ `season` est **optionnel**. Quand il est absent, la Forge le calcule automatiquement depuis les `SeasonBoundaries` de l'année en cours (Étape 2, Canonicalization).

Il peut être fourni explicitement pour les fêtes dont la saison est fixe indépendamment du calendrier temporel (ex: une Solennité du Sanctoral tombe toujours en `TempusOrdinarium` sauf si elle coïncide avec une période privilégiée — la Forge résout ce conflit en Étape 3).

**Valeurs admises :** voir §6.4.

---

## 3. Temporalité

### 3.1 Dates Fixes

```yaml
date:
  month: <1–12> # Mois grégorien, 1-based
  day: <1–31> # Jour du mois, 1-based
```

**Conversion en DOY (0-based) par la Forge :**

```rust
// Table MONTH_STARTS (constante de compilation, §2.2 de la spec)
const MONTH_STARTS: [u16; 12] = [0, 31, 60, 91, 121, 152, 182, 213, 244, 274, 305, 335];

fn doy_from_date(month: u8, day: u8) -> u16 {
    MONTH_STARTS[(month - 1) as usize] + (day - 1) as u16
}
// Exemples :
// Nativitas Domini (25 déc) : MONTH_STARTS[11] + 24 = 335 + 24 = 359
// Assumptio BMV (15 août)   : MONTH_STARTS[7] + 14 = 213 + 14 = 227
```

**Cas particulier — le 29 février (`month: 2, day: 29`) :**

Le 29 février est déclarable en YAML et produit toujours `doy = 59`. Pour les années non-bissextiles, la Forge écrit une **Padding Entry** à `doy = 59` (`primary_id = 0`, `secondary_count = 0`, `flags = 0`). La fête n'est pas transférée — elle est simplement absente pour cette année. L'Engine reçoit la Padding Entry et retourne `KAL_ENGINE_OK` avec `primary_id = 0` ; l'interprétation est laissée à l'appelant.

```yaml
# Exemple : Fête fixée au 29 février
- slug: sancti_cassiani
  date:
    month: 2
    day: 29 # doy = 59 ; Padding Entry les années non-bissextiles
  scope: universal
  category: 1
  history:
    - from: 1969
      title: "S. Cassianus"
      precedence: 12
      nature: memoria
      color: rubeus
```

**Validation V3 :** la Forge vérifie que `day` est cohérent avec `month` (ex: `month: 2, day: 30` → `ParseError::InvalidDate`). Le 29 février est la seule date admise en dehors des limites standard du mois ; toutes les autres dates invalides sont rejetées.

---

### 3.2 Dates Mobiles

Les fêtes mobiles sont exprimées par un **ancre** et un **offset** en jours.

```yaml
mobile:
  anchor: <anchor_id> # Identifiant d'ancre — voir tableau ci-dessous
  offset:
    <integer> # Offset en jours, peut être négatif
    # Exemples : +39, -7, 0
```

**Ancres disponibles :**

| `anchor_id`   | Définition canonique        | Résolution par la Forge                           |
| ------------- | --------------------------- | ------------------------------------------------- |
| `pascha`      | Dimanche de Pâques          | Meeus/Jones/Butcher (Étape 2)                     |
| `adventus`    | Premier dimanche de l'Avent | Dimanche le plus proche du 30 novembre (Étape 2)  |
| `pentecostes` | Pentecôte                   | `pascha + 49` (dérivée, non déclarée directement) |

> **Note :** `pentecostes` est une ancre de convenance. Elle est strictement équivalente à `anchor: pascha, offset: +49`. Les deux formes sont admises et produisent le même DOY.

**Fêtes mobiles standard dérivées de Pâques :**

| Fête                     | Ancre    | Offset |
| ------------------------ | -------- | ------ |
| Jeudi Saint              | `pascha` | -3     |
| Vendredi Saint           | `pascha` | -2     |
| Samedi Saint             | `pascha` | -1     |
| Ascensio Domini          | `pascha` | +39    |
| Pentecostes              | `pascha` | +49    |
| Corpus Christi           | `pascha` | +60    |
| Sacratissimi Cordis Iesu | `pascha` | +68    |
| Dominica in Palmis       | `pascha` | -7     |
| Feria IV Cinerum         | `pascha` | -46    |

**Exemple complet — Ascension :**

```yaml
- slug: ascensio_domini
  mobile:
    anchor: pascha
    offset: +39
  scope: universal
  category: 0
  history:
    - from: 1969
      title: "Ascensio Domini"
      precedence: 1
      nature: sollemnitas
      color: albus
      season: tempus_paschale
```

**Validation V4 — Cycles et dépendances :**

Chaque ancre déclarée doit être résolvable sans cycle. La Forge construit un graphe de dépendances de toutes les dates mobiles avant de les calculer. Un cycle (ex: une fête dont l'offset référencerait une autre fête mobile) est rejeté avec `ParseError::CircularDependency { slug, anchor }`.

Dans la version actuelle, seules les ancres de la table ci-dessus sont admises. Les ancres personnalisées (référençant d'autres fêtes par slug) ne sont **pas** supportées en v1.0.

---

## 4. Logique de l'Attribut `history`

### 4.1 Structure

Le bloc `history` est un tableau ordonné. Chaque entrée couvre une plage d'années `[from, to]` et définit les métadonnées de la fête pour cette période.

```yaml
history:
  - from: <year> # Borne inférieure inclusive. Défaut : 1969 si omis.
    to: <year|~> # Borne supérieure inclusive. null (ou omis) = indéfini.
    title: <string> # Nom canonique pour cette période. Stocké dans le .lits.
    precedence: <0–12> # Rang liturgique effectif.
    nature: <string> # Voir §6.2.
    color: <string> # Voir §6.3.
    season: <string> # Optionnel — voir §2.3 et §6.4.
```

**Sémantique de `to: ~` (null) :**

L'entrée est active de `from` jusqu'à la fin de la plage supportée (2399). Elle reste active pour toutes les années `y ≥ from` tant qu'aucune entrée ultérieure dans le même bloc `history` ne prend le relais.

**Ordre d'évaluation :** la Forge trie les entrées `history` par `from` croissant avant traitement. L'ordre dans le YAML n'est pas significatif — mais la Forge rejette toute ambiguïté (voir Validation V1).

### 4.2 Algorithme de Résolution Temporelle

Pour une année `y` et un slug `s`, la Forge sélectionne l'entrée unique dont `[from, to]` contient `y` :

```rust
fn resolve_feast_for_year<'a>(
    slug: &str,
    history: &'a [FeastVersion],
    year: u16,
) -> Result<Option<&'a FeastVersion>, RegistryError> {
    let candidates: Vec<_> = history
        .iter()
        .filter(|v| v.from <= year)
        .filter(|v| v.to.map_or(true, |to| year <= to))
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

Si `resolve_feast_for_year` retourne `Ok(None)`, la fête est **absente** du dataset pour cette année — aucune `CalendarEntry` n'est produite pour ce slug. Ce n'est pas une erreur : la fête peut ne pas encore exister, ou avoir été supprimée du calendrier.

### 4.3 Exemple Complet — Jean-Paul II (Béatification → Canonisation)

```yaml
- slug: ioannis_pauli_ii # ← neutre, stable, immuable après première allocation
  date:
    month: 10
    day: 22
  scope: national
  region: PL
  category: 1

  history:
    # Version 1 : Béatification (2011)
    # "Beatus" est un statut canonique, pas une Nature → Nature::Memoria
    # precedence: 11 = MemoriaeObligatoriae dans le calendrier polonais
    - from: 2011
      to: 2013
      title: "B. Ioannes Paulus II"
      precedence: 11
      nature: memoria
      color: albus

    # Version 2 : Canonisation (2014 → indéfini)
    # Le slug ne change pas — seul title et precedence évoluent.
    # precedence: 12 = FeriaePerAnnumEtMemoriaeAdLibitum (Memoria facultative)
    # La canonisation inscrit la fête au calendarium generale (scope: universal),
    # mais l'entrée nationale PL reste distincte et peut coexister.
    - from: 2014
      to: ~
      title: "S. Ioannes Paulus II"
      precedence: 12
      nature: memoria
      color: albus
```

**Lecture de la hiérarchie :**

La `Precedence` est numérique inverse : valeur plus faible = priorité plus haute. La béatification (valeur 11, Memoria obligatoria nationale) avait une priorité **plus haute** que la version canonisée (valeur 12, Memoria ad libitum universelle). Les deux peuvent coexister dans le registre car leurs scopes et leurs plages temporelles sont disjoints.

---

## 5. Hiérarchie et Scopes

### 5.1 Définition des Scopes

| Scope       | Description                                               | Champ `region`                      | Bits FeastID        |
| ----------- | --------------------------------------------------------- | ----------------------------------- | ------------------- |
| `universal` | Calendarium Generale Romanum                              | `null`                              | bits [15:14] = `00` |
| `national`  | Calendrier national approuvé par la Conférence épiscopale | Code ISO 3166-1 alpha-2 (ex: `FR`)  | bits [15:14] = `01` |
| `diocesan`  | Propre diocésain                                          | Identifiant diocésain (ex: `PARIS`) | bits [15:14] = `10` |

Le bit [15:14] est encodé dans les 2 bits supérieurs du FeastID u16. Le bit [13:12] encode la `category`. Les bits [11:0] encodent la séquence (4096 valeurs par (scope, category)).

### 5.2 Règles de Fusion

La Forge fusionne les fichiers YAML dans l'ordre de priorité suivant, du moins prioritaire au plus prioritaire :

```
universal.yaml  <  national-<REGION>.yaml  <  diocesan-<ID>.yaml
```

**Règle de résolution des collisions (même slug) :**

Le scope le plus local l'emporte. Si `diocesan-PARIS.yaml` et `national-FR.yaml` définissent tous deux le slug `nativitas_domini`, la version diocésaine est retenue et la version nationale ignorée pour la génération du dataset Paris.

**Règle de résolution des collisions (même DOY, slugs différents) :**

Deux fêtes de scopes différents peuvent tomber le même jour. La Forge applique l'Étape 3 (Conflict Resolution) : la `Precedence` la plus haute (valeur numérique inférieure) détermine le `primary_id`. L'autre est placée en `secondary` si elle est une Memoria ou Commemoratio, sinon transférée ou supprimée selon les règles NALC 1969.

**Surcharge partielle :**

Un fichier national ou diocésain peut ne redéfinir que certains champs d'une fête universelle (ex: un titre localisé, une `precedence` relevée). La Forge fusionne les blocs `history` — les entrées du scope local écrasent, pour les années couvertes, les entrées du scope universel.

```yaml
# national-FR.yaml — surcharge du titre de la Nativité pour les années 1969-présent
- slug: nativitas_domini
  scope: national
  region: FR
  # Pas de date/mobile : héritée du universal.yaml
  history:
    - from: 1969
      to: ~
      title: "Nativité du Seigneur" # Titre localisé en français
      precedence: 1 # Inchangé
      nature: sollemnitas # Inchangé
      color: albus # Inchangé
```

### 5.3 Fichiers de Configuration Attendus par la Forge

La Forge accepte une liste ordonnée de fichiers YAML. L'ordre est celui de la fusion (du moins au plus local). Un fichier absent d'un scope intermédiaire n'est pas une erreur.

```rust
// Interface Forge — configuration de la session de compilation
struct ForgeSession {
    universal: PathBuf,              // Requis
    national: Option<PathBuf>,       // Optionnel
    diocesan: Option<PathBuf>,       // Optionnel
    output: PathBuf,                 // Chemin de sortie .kald
    range: RangeInclusive<u16>,      // Ex : 1969..=2399
}
```

---

## 6. Référentiel des Valeurs Admises

### 6.1 `precedence` — Degrés Liturgiques Effectifs

Valeurs admises : **0 à 12 inclus**. Valeurs 13–15 : réservées système — la Forge rejette toute occurrence avec `RegistryError::InvalidPrecedenceValue(u8)`.

**Hiérarchie inverse : valeur plus faible = priorité plus haute.**

| Valeur | Niveau canonique (NALC 1969)                                | Bits flags [3:0] |
| ------ | ----------------------------------------------------------- | ---------------- |
| 0      | Triduum Sacrum                                              | `0000`           |
| 1      | Nativitas, Epiphania, Ascensio, Pentecostes                 | `0001`           |
| 2      | Dominicae Adventus, Quadragesimae, Paschales                | `0010`           |
| 3      | Feria IV Cinerum ; Hebdomada Sancta                         | `0011`           |
| 4      | Sollemnitates Domini, BMV, Sanctorum in Calendario Generali | `0100`           |
| 5      | Sollemnitates propriae                                      | `0101`           |
| 6      | Festa Domini in Calendario Generali                         | `0110`           |
| 7      | Dominicae per annum                                         | `0111`           |
| 8      | Festa BMV et Sanctorum in Calendario Generali               | `1000`           |
| 9      | Festa propria                                               | `1001`           |
| 10     | Feriae Adventus (17–24 Dec) ; Octava Nativitatis            | `1010`           |
| 11     | Memoriae obligatoriae                                       | `1011`           |
| 12     | Feriae per annum ; Memoriae ad libitum                      | `1100`           |
| 13–15  | **Réservés système** — non admissibles en YAML              | —                |

### 6.2 `nature` — Type Liturgique

Valeurs YAML (insensibles à la casse, normalisées par la Forge) et leur correspondance en `CalendarEntry.flags` bits [13:11] :

| Valeur YAML    | `Nature` Rust          | Valeur numérique | Note                                                       |
| -------------- | ---------------------- | ---------------- | ---------------------------------------------------------- |
| `sollemnitas`  | `Nature::Sollemnitas`  | 0                |                                                            |
| `festum`       | `Nature::Festum`       | 1                |                                                            |
| `memoria`      | `Nature::Memoria`      | 2                | Couvre Beatus/Beata — "beatus" n'est pas une Nature        |
| `feria`        | `Nature::Feria`        | 3                | Inclut les Dominicae (classe de Precedence, pas de Nature) |
| `commemoratio` | `Nature::Commemoratio` | 4                |                                                            |

Toute autre valeur → `RegistryError::UnknownNatureString(String)` avec hint :

```
"beatus" n'est pas une Nature. Utiliser nature: memoria.
```

### 6.3 `color` — Couleur Liturgique

Valeurs YAML (insensibles à la casse) et correspondance en `CalendarEntry.flags` bits [7:4] :

| Valeur YAML | `Color` Rust       | Valeur numérique | Usage canonique                                  |
| ----------- | ------------------ | ---------------- | ------------------------------------------------ |
| `albus`     | `Color::Albus`     | 0                | Fêtes du Seigneur, Vierge, Confesseurs, Docteurs |
| `rubeus`    | `Color::Rubeus`    | 1                | Passion, Apôtres, Martyrs, Pentecôte             |
| `viridis`   | `Color::Viridis`   | 2                | Temps ordinaire                                  |
| `violaceus` | `Color::Violaceus` | 3                | Avent, Carême                                    |
| `roseus`    | `Color::Roseus`    | 4                | Gaudete (Avent III), Laetare (Carême IV)         |
| `niger`     | `Color::Niger`     | 5                | Messes des défunts                               |

### 6.4 `season` — Saison Liturgique (optionnel)

Valeurs YAML et correspondance en `CalendarEntry.flags` bits [10:8] :

| Valeur YAML            | `Season` Rust                 | Valeur numérique |
| ---------------------- | ----------------------------- | ---------------- |
| `tempus_ordinarium`    | `Season::TempusOrdinarium`    | 0                |
| `tempus_adventus`      | `Season::TempusAdventus`      | 1                |
| `tempus_nativitatis`   | `Season::TempusNativitatis`   | 2                |
| `tempus_quadragesimae` | `Season::TempusQuadragesimae` | 3                |
| `triduum_paschale`     | `Season::TriduumPaschale`     | 4                |
| `tempus_paschale`      | `Season::TempusPaschale`      | 5                |
| `dies_sancti`          | `Season::DiesSancti`          | 6                |

Si omis dans le YAML, la Forge calcule la saison depuis les `SeasonBoundaries` de l'année courante (Étape 2). Pour les fêtes du Sanctoral dont la saison est sans ambiguïté (ex: une Mémoire en juillet → `tempus_ordinarium`), fournir le champ évite le recalcul et garantit la valeur indépendamment du calendrier temporel.

---

## 7. Mapping YAML ↔ `CalendarEntry`

Tableau de correspondance complet entre les champs YAML et les champs binaires de `CalendarEntry` (spec §3.3–3.4) :

| Champ YAML                        | Type YAML           | Destination binaire                | Offset | Note                                             |
| --------------------------------- | ------------------- | ---------------------------------- | ------ | ------------------------------------------------ |
| `slug`                            | String              | —                                  | —      | Clé FeastRegistry uniquement. Absent du `.kald`. |
| `id`                              | u16 \| null         | `CalendarEntry.primary_id`         | 0      | Alloué par la Forge si absent.                   |
| `date.month` + `date.day`         | Integer             | DOY 0-based (formule §3.1)         | —      | Formule : `MONTH_STARTS[month-1] + day - 1`      |
| `mobile.anchor` + `mobile.offset` | String + Integer    | DOY 0-based (Étape 2)              | —      | Résolution Pâques + offset                       |
| —                                 | —                   | `CalendarEntry.secondary_index`    | 2      | Alimenté par Étape 4 (Materialization)           |
| `history[].precedence`            | Integer [0–12]      | `CalendarEntry.flags` bits [3:0]   | 4      |                                                  |
| `history[].color`                 | String enum         | `CalendarEntry.flags` bits [7:4]   | 4      |                                                  |
| `history[].season`                | String enum \| null | `CalendarEntry.flags` bits [10:8]  | 4      | Calculé si absent                                |
| `history[].nature`                | String enum         | `CalendarEntry.flags` bits [13:11] | 4      |                                                  |
| —                                 | —                   | `CalendarEntry.secondary_count`    | 6      | Alimenté par Étape 3 (Conflict Resolution)       |
| `history[].title`                 | String              | Fichier `.lits`                    | —      | Absent du `.kald`.                               |
| `scope`                           | String enum         | FeastID bits [15:14]               | —      | `universal=00`, `national=01`, `diocesan=10`     |
| `category`                        | Integer [0–3]       | FeastID bits [13:12]               | —      |                                                  |

> **Rappel layout `CalendarEntry` (spec §3.3) :** `primary_id (u16, off 0)` | `secondary_index (u16, off 2)` | `flags (u16, off 4)` | `secondary_count (u8, off 6)` | `_reserved (u8, off 7)`. Les trois `u16` sont aux offsets pairs — alignement naturel garanti.

**Encodage `flags` :**

```rust
fn encode_flags(p: Precedence, c: Color, s: Season, n: Nature) -> u16 {
    (p as u16)             // bits [3:0]
    | ((c as u16) << 4)    // bits [7:4]
    | ((s as u16) << 8)    // bits [10:8]
    | ((n as u16) << 11)   // bits [13:11]
    // bits [15:14] = 0 (réservés, doivent être nuls)
}
```

---

## 8. Validations Forge (V1–V6)

Ces validations sont appliquées durant l'**Étape 1 (Rule Parsing)**. Tout échec est fatal — la Forge n'émet aucun binaire partiel.

### Groupe A — Syntaxe et Structure (V1)

**V1 — Validité syntaxique YAML et conformité au schéma**

```
∀ fichier f :
  f est syntaxiquement valide YAML
  ∧ f.format_version == 1
  ∧ f.scope ∈ { "universal", "national", "diocesan" }
  ∧ si f.scope != "universal" alors f.region != null
  ∧ chaque feast ∈ f.feasts est conforme au schéma §2
  ∧ exactement un de {date, mobile} est présent par feast
```

Violations → `ParseError::MalformedYaml`, `ParseError::UnsupportedSchemaVersion`, `ParseError::MissingTemporalityField { slug }`, `ParseError::AmbiguousTemporalityField { slug }`

### Groupe B — Unicité des Identifiants (V2)

**V2a — Unicité des Slugs par scope**

```
∀ (scope, fichier) : aucun slug n'apparaît plus d'une fois dans ce couple
```

Violation → `RegistryError::DuplicateSlug { slug, scope }`

**V2b — Unicité des FeastIDs**

```
∀ id explicitement fourni : aucun autre feast ne revendique le même FeastID
```

Violation → `RegistryError::FeastIDConflict { id, slug_a, slug_b }`

**V2c — Capacité FeastID (ex-V3 spec §10)**

```
∀ (scope, category) : allocated_count ≤ 4095
```

Violation → `RegistryError::FeastIDExhausted { scope, category }`

**V2d — Unicité temporelle dans un bloc `history` (ex-V1 spec §10)**

```
∀ slug s, ∀ scope sc, ∀ année y ∈ [1969, 2399] :
  |{ entrées e ∈ history(s, sc) | e.from ≤ y ≤ e.to_or_max }| ≤ 1
```

Violation → `RegistryError::TemporalOverlap { slug, year, conflicting_entries }`

### Groupe C — Intégrité des Dates (V3)

**V3a — Validité des dates fixes**

```
∀ feast avec date :
  month ∈ [1, 12]
  ∧ day ∈ [1, days_in_month(month)]  (29 fév admis, traitement §3.1)
```

Violation → `ParseError::InvalidDate { slug, month, day }`

**V3b — Cohérence des plages temporelles (ex-V4 spec §10)**

```
∀ entrée e ∈ history :
  e.from ≤ e.to_or_max
  ∧ e.from ≥ 1969
  ∧ e.to_or_max ≤ 2399
```

Violation → `RegistryError::InvalidTemporalRange { from, to }`

**V3c — Padding Entry (29 février)**

Invariant de la Forge, pas une erreur utilisateur. La Forge génère automatiquement la Padding Entry à `doy = 59` pour les années non-bissextiles. Aucune validation YAML n'est requise ; la Forge ne délègue pas ce slot à une fête déclarée pour `month: 2, day: 29` sur les années non-bissextiles.

### Groupe D — Domaines et Cycles (V4, V5, V6)

**V4 — Résolution des cycles de dépendances (dates mobiles)**

```
∀ feast avec mobile : anchor ∈ { "pascha", "adventus", "pentecostes" }
∧ aucun cycle dans le graphe de dépendances des ancres
```

Violation → `ParseError::UnknownAnchor { slug, anchor }`, `ParseError::CircularDependency { slug, anchor }`

**V5 — Nature conforme aux enums (ex-V5 spec §10)**

```
∀ entrée e ∈ history : e.nature ∈ { "sollemnitas", "festum", "memoria", "feria", "commemoratio" }
```

Violation → `RegistryError::UnknownNatureString(String)` avec hint si valeur canonique informelle détectée.

**V6 — Slug syntaxiquement valide (ex-V6 spec §10)**

```
slug ∈ [a-z][a-z0-9_]*   (latin snake_case, premier char alphabétique)
```

Violation → `RegistryError::InvalidSlugSyntax(String)`

**V2-Bis — Domaine de Precedence (ex-V2 spec §10)**

```
∀ entrée e ∈ history : e.precedence ∈ [0, 12]
```

Violation → `RegistryError::InvalidPrecedenceValue(u8)` (valeurs 13–15 réservées système)

### Tableau de Correspondance Spec §10 ↔ Ce Document

Ce tableau est la clé de lecture bidirectionnelle entre les codes d'erreur Rust (spec §10) et les groupes de validation de ce document. Les codes V1–V6 sont les seuls identifiants à utiliser dans le code et les messages d'erreur produits par la Forge.

| Code spec §10 | Variant Rust (`RegistryError` / `ParseError`)         | Groupe §8      | Libellé                                                         |
| ------------- | ----------------------------------------------------- | -------------- | --------------------------------------------------------------- |
| **V1**        | `TemporalOverlap { slug, year, conflicting_entries }` | **B — V2d**    | Unicité temporelle dans `history` (même slug/scope, même année) |
| **V2**        | `InvalidPrecedenceValue(u8)`                          | **D — V2-Bis** | Domaine de Precedence — valeurs 13–15 interdites                |
| **V3**        | `FeastIDExhausted { scope, category }`                | **B — V2c**    | Capacité FeastID ≤ 4095 par (scope, category)                   |
| **V4**        | `InvalidTemporalRange { from, to }`                   | **C — V3b**    | Cohérence et bornes des plages `[from, to]` dans `[1969, 2399]` |
| **V5**        | `UnknownNatureString(String)`                         | **D — V5**     | Nature conforme aux 5 enums admis                               |
| **V6**        | `InvalidSlugSyntax(String)`                           | **D — V6**     | Slug : `[a-z][a-z0-9_]*` obligatoire                            |

**Validations §8 sans code V-numéroté dans la spec** (erreurs structurelles de parsing, pas d'erreurs de domaine) :

| Variant Rust                                             | Groupe §8   | Libellé                                        |
| -------------------------------------------------------- | ----------- | ---------------------------------------------- |
| `ParseError::MalformedYaml` / `UnsupportedSchemaVersion` | **A — V1**  | Syntaxe YAML invalide ou `format_version != 1` |
| `RegistryError::DuplicateSlug { slug, scope }`           | **B — V2a** | Slug déclaré deux fois dans le même scope      |
| `RegistryError::FeastIDConflict { id, slug_a, slug_b }`  | **B — V2b** | Collision sur `id` explicite                   |
| `ParseError::InvalidDate { slug, month, day }`           | **C — V3a** | Date fixe impossible (ex: 30 février)          |
| `ParseError::CircularDependency { slug, anchor }`        | **D — V4**  | Cycle dans le graphe des ancres mobiles        |

---

## 9. Exemples Complets

### 9.1 Solennité Fixe — Nativitas Domini

```yaml
# universal.yaml

scope: universal
region: ~
from: 1969
to: ~
format_version: 1

feasts:
  - slug: nativitas_domini
    date:
      month: 12
      day: 25 # doy = 335 + 24 = 359 (toujours — DOY 0-based)
    scope: universal
    category: 0

    history:
      - from: 1969
        to: ~
        title: "In Nativitate Domini"
        precedence: 1 # Rang le plus élevé après le Triduum — éviction maximale
        nature: sollemnitas
        color: albus
        season: tempus_nativitatis
```

**Résultat dans `CalendarEntry` pour le 25 décembre de n'importe quelle année :**

```
primary_id      : FeastID alloué pour nativitas_domini (ex: 0x0001)
secondary_count : 0  (aucune commémoration — Solennité évince tout)
flags           : 0x0201  (= 0b_0000_0010_0000_0001)
                  décomposé :
                  bits [3:0]   = 0001 → Precedence = 1 (SollemnitatesFixaeMaior)
                  bits [7:4]   = 0000 → Color      = 0 (Albus)
                  bits [10:8]  = 010  → Season     = 2 (TempusNativitatis)
                  bits [13:11] = 000  → Nature     = 0 (Sollemnitas)
                  bits [15:14] = 00   → réservés
```

Valeur `flags` numérique : `encode_flags(1, 0, 2, 0)` = `1 | (0 << 4) | (2 << 8) | (0 << 11)` = `0x0201`.

---

### 9.2 Fête Mobile — Ascensio Domini

```yaml
- slug: ascensio_domini
  mobile:
    anchor: pascha
    offset: +39
  scope: universal
  category: 0

  history:
    - from: 1969
      to: ~
      title: "Ascensio Domini"
      precedence: 1
      nature: sollemnitas
      color: albus
      season: tempus_paschale
```

**Résolution en 2025 :** Pâques 2025 = `doy 110` (20 avril, DOY 0-based). Ascension = `doy 110 + 39 = 149` (29 mai 2025).

---

### 9.3 Fête avec Historique — Ioannes Paulus II

Voir §4.3 pour l'exemple complet avec béatification (2011–2013) et canonisation (2014–présent).

**Résolution de la Forge pour l'année 2012 :** `from=2011, to=2013` contient 2012 → `title = "B. Ioannes Paulus II"`, `precedence = 11`, `nature = memoria`.

**Résolution pour l'année 2014 :** `from=2014, to=~` contient 2014 → `title = "S. Ioannes Paulus II"`, `precedence = 12`, `nature = memoria`.

**Résolution pour l'année 2009 :** aucune entrée ne contient 2009 → `Ok(None)` → fête absente du dataset 2009.

---

### 9.4 Surcharge Diocésaine

```yaml
# diocesan-PARIS.yaml — Fête du patron du diocèse de Paris
scope: diocesan
region: PARIS
from: 1969
to: ~
format_version: 1

feasts:
  - slug: dionysii_parisiensis
    date:
      month: 10
      day: 9 # doy = 274 + 8 = 282
    scope: diocesan
    region: PARIS
    category: 2

    history:
      - from: 1969
        to: ~
        title: "S. Dionysius, ep. et socii, mm."
        precedence: 4 # Solennité pour le diocèse de Paris (patron)
        nature: sollemnitas
        color: rubeus
```

**Comportement Forge :** lors de la génération d'un `.kald` pour le diocèse de Paris, la Forge charge `universal.yaml` puis `national-FR.yaml` puis `diocesan-PARIS.yaml`. Si le 9 octobre est également occupé par une fête nationale ou universelle de `Precedence` ≥ 4, la Forge applique Conflict Resolution (Étape 3) : la Solennité diocésaine (`Precedence = 4`) a la même valeur que les Solennités du calendrier général — la règle de scope local l'emporte à égalité de `Precedence`.

---

## 10. Checklist de Conformité YAML

Avant de soumettre un fichier à la Forge :

- [ ] `format_version: 1` présent en en-tête
- [ ] `scope` et `region` cohérents (null pour universal)
- [ ] Chaque fête a exactement un bloc `date` ou `mobile` — jamais les deux
- [ ] Tous les slugs sont en latin snake_case **neutre** — aucun statut liturgique encodé
- [ ] Le slug est stable dans le temps (immuable après première allocation)
- [ ] L'évolution du titre et de la `precedence` est portée par des entrées `history` distinctes
- [ ] Les plages `[from, to]` du bloc `history` sont disjointes pour un même slug/scope
- [ ] `precedence` ∈ [0, 12] pour chaque entrée `history` — jamais 13, 14 ou 15
- [ ] `nature` est l'une des 5 valeurs admises (§6.2) — aucun terme canonique informel
- [ ] `color` est l'une des 6 valeurs admises (§6.3)
- [ ] `from` ≥ 1969 et `to` ≤ 2399 pour toutes les entrées `history`
- [ ] `from` est renseigné explicitement si différent de 1969
- [ ] `id` absent sauf besoin documenté d'un identifiant stable
- [ ] Les entrées `scope: national` ou `scope: diocesan` portent un champ `region` non-null
- [ ] Les fêtes au 29 février (`date.month: 2, date.day: 29`) sont intentionnelles — la Forge génère une Padding Entry les années non-bissextiles

---

**Fin du Contrat de Données Amont v1.0**

_Document créé le 2026-03-07. Révisé le 2026-03-08. Contrat de données amont de la Forge (`liturgical-calendar-forge`). Définit le format YAML des calendriers liturgiques (universel, national, diocésain), les règles de validation V1–V6, et le mapping vers `CalendarEntry`. Référence : `specification.md` v2.0, §3–§5, §10._
