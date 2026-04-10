# ADR-003 — Champ `ordinal` distinct de `offset` pour l'ancre `tempus_ordinarium`

**Statut** : Accepté  
**Date** : 2026-04-10  
**Contexte** : `liturgical-calendar-forge` — §3.2 du contrat / Validation V4a

---

## Contexte

L'introduction de l'ancre `tempus_ordinarium` pour modéliser les dimanches du Temps Ordinaire nécessite de transmettre à la Forge un paramètre supplémentaire : l'index ordinal de la semaine (de 1 à 34).

Le champ `offset` existant dans le bloc `mobile` est un entier de jours, signé, homogène pour toutes les ancres. La proposition initiale était de réutiliser ce champ avec la convention que, pour `tempus_ordinarium` uniquement, `offset` représenterait un index ordinal et non un nombre de jours.

---

## Décision

**Un champ distinct `ordinal` est introduit dans le bloc `mobile`, exclusivement pour l'ancre `tempus_ordinarium`. Le champ `offset` reste réservé aux jours, pour toutes les autres ancres. Les deux champs sont mutuellement exclusifs selon l'ancre — toute combinaison incorrecte est une erreur fatale V4a.**

```yaml
# Forme valide — ancre ordinaire
mobile:
  anchor: pascha
  offset: +39

# Forme valide — ancre ordinale
mobile:
  anchor: tempus_ordinarium
  ordinal: 10

# Formes invalides — rejetées V4a
mobile:
  anchor: tempus_ordinarium
  offset: 10        # ParseError::OffsetOnOrdinalAnchor

mobile:
  anchor: pascha
  ordinal: 10       # ParseError::OrdinalOnNonOrdinalAnchor
```

---

## Justification

### 1. Surcharge sémantique = branchement implicite dans la Forge

Si `offset` porte deux sémantiques selon l'ancre, la Forge doit brancher :

```rust
let doy = match anchor {
    Anchor::TempusOrdinarium => resolve_tempus_ordinarium(adventus, offset as u8),
    _ => base_doy(anchor) + offset,
};
```

Ce branchement encode de la logique conditionnelle dans une fonction qui devrait être une application uniforme d'un offset. C'est une violation du principe DOD : le type du champ détermine son interprétation, pas la valeur d'un autre champ.

### 2. Domaines de valeurs incompatibles

`offset` est signé et peut être négatif (Vendredi Saint : `-2`, Cendres : `-46`). `ordinal` est un entier non signé borné à [1, 34]. Partager un type `integer` pour deux domaines sémantiquement disjoints empêche la validation statique de domaine — la Forge ne peut pas rejeter `ordinal: -3` ou `ordinal: 200` sans logique conditionnelle supplémentaire.

Avec deux champs distincts, la validation est structurelle : `ordinal: u8` borné à [1, 34] est vérifiable sans condition sur l'ancre.

### 3. Lisibilité du corpus YAML

```yaml
# Avec surcharge — ambigu pour le lecteur humain
mobile:
  anchor: tempus_ordinarium
  offset: 10   # 10 jours ? 10e semaine ? Non évident.

# Avec champ distinct — non ambigu
mobile:
  anchor: tempus_ordinarium
  ordinal: 10  # 10e dimanche du Temps Ordinaire — sans ambiguïté
```

Le corpus YAML est une source de vérité lue par des humains (liturgistes, développeurs). La lisibilité sans documentation contextuelle est un critère de qualité du contrat.

### 4. Extensibilité future

D'autres ancres à sémantique ordinale pourraient être introduites (ex : semaines de l'Avent numérotées pour une granularité infradomnicale). Le pattern `ordinal` comme champ distinct est réutilisable sans modifier le type de `offset`.

---

## Alternatives rejetées

| Option | Description | Raison du rejet |
| ------ | ----------- | --------------- |
| Surcharge de `offset` | Réutiliser `offset` avec convention documentée | Branchement implicite, domaines incompatibles, lisibilité dégradée |
| Union typée en YAML | `value: { days: 39 }` ou `value: { ordinal: 10 }` | Complexité de désérialisation disproportionnée, rupture du contrat existant |
| Ancre paramétrique | `anchor: tempus_ordinarium_10` | Explosion du répertoire d'ancres, validation par regex plutôt que par type |

---

## Conséquences

**Positives :**
- Validation V4a : contrainte de champ vérifiable structurellement, sans logique conditionnelle.
- Désérialiseur Forge : `mobile.offset` et `mobile.ordinal` sont deux champs optionnels distincts — leur présence/absence est vérifiable avant tout calcul.
- Lisibilité corpus : intention sémantique explicite dans le YAML.

**Contraintes acceptées :**
- Rupture de schéma mineure : le bloc `mobile` accepte désormais deux champs optionnels mutuellement exclusifs. Le désérialiseur doit valider cette exclusivité (V4a). Coût : une règle de validation supplémentaire, localisée à l'Étape 1.

---

## Règle dérivée

> **V4a** : Pour tout bloc `mobile`, exactement l'un des deux champs suivants doit être présent selon l'ancre déclarée : `offset` (toutes ancres sauf `tempus_ordinarium`) ou `ordinal` (ancre `tempus_ordinarium` uniquement). Toute autre combinaison est une erreur fatale avant désérialisation du bloc `history`.

---

## Références

- `liturgical-scheme.md` v1.3.3 — §3.2 : table des ancres, contraintes V4a, champ `ordinal`
- `specification-patch-v2.2.md` — Étape 2 : pseudo-code `resolve_tempus_ordinarium`, table V4a
- ADR-001 — Indépendance de la résolution DOY vis-à-vis de la logique saisonnière
