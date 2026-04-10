# ADR-001 — Indépendance de la résolution DOY vis-à-vis de la logique saisonnière

**Statut** : Accepté  
**Date** : 2026-04-10  
**Contexte** : `liturgical-calendar-forge` — Étape 2 (Canonicalization) / Étape 3 (Conflict Resolution)

---

## Contexte

La Forge calcule, pour chaque année de la plage [1969, 2399], un DOY (Day Of Year, base 0) pour chaque fête déclarée dans le corpus YAML. Certaines fêtes sont mobiles — leur DOY varie d'une année à l'autre selon des règles liturgiques (Pâques, Avent, Noël, etc.).

Le Temps Ordinaire introduit un cas structurellement ambigu : certains dimanches ordinaires tombent, selon l'année, sur des slots déjà occupés par des périodes liturgiquement prioritaires (Temps de Noël, Temps de Pâques). La question architecturale est : **à quelle étape de la Forge cette situation est-elle détectée et résolue ?**

Deux approches ont été envisagées :

- **Option A (rejetée)** : la fonction de résolution du DOY (`resolve_tempus_ordinarium`) vérifie si le DOY calculé appartient à une période saisonnière prioritaire, et retourne conditionnellement `Ok(None)` si c'est le cas.
- **Option B (retenue)** : la fonction de résolution du DOY calcule toujours un DOY absolu, sans connaissance de la saison. La détection du conflit et la suppression du slot sont déléguées à l'Étape 3 (Conflict Resolution).

---

## Décision

**La Forge ne conditionne jamais la résolution d'un DOY à une logique saisonnière.**

Toute fonction de la forme `resolve_<anchor>(year, ...) -> u16` produit un DOY absolu inconditionnel. Elle n'inspecte pas les `SeasonBoundaries`, ne consulte pas d'autres slots résolus, et ne retourne pas `Ok(None)` sur critère saisonnier.

La responsabilité de détecter qu'un dimanche ordinaire est absorbé par une période prioritaire appartient **exclusivement à l'Étape 3**.

---

## Justification

### 1. Séparation stricte des responsabilités (invariant pipeline)

Le pipeline Forge est organisé en étapes séquentielles dont les responsabilités sont disjointes :

```
Étape 1 — Rule Parsing       : validation syntaxique et sémantique du corpus YAML
Étape 2 — Canonicalization   : résolution des DOY (dates mobiles → entiers)
Étape 3 — Conflict Resolution: arbitrage entre fêtes en collision sur un même DOY
```

Introduire une vérification saisonnière dans l'Étape 2 revient à faire remonter une responsabilité de l'Étape 3 vers l'Étape 2. Cela crée un couplage entre deux étapes qui doivent rester testables et auditables indépendamment.

### 2. La saison est un attribut calculé, pas un invariant de résolution

La saison d'un slot (Temps de Noël, Temps de Pâques, Temps Ordinaire…) est elle-même le résultat d'un calcul de l'Étape 2, produit par les `SeasonBoundaries`. Elle n'est pas un invariant disponible avant que la résolution soit complète. Une fonction de résolution qui consomme les `SeasonBoundaries` introduit une dépendance circulaire implicite dans l'ordre de résolution :

```
resolve_tempus_ordinarium() a besoin de → SeasonBoundaries
SeasonBoundaries dépend de → resolve_pascha(), resolve_adventus()
```

Ce n'est pas un cycle fatal, mais c'est un couplage de données qui rend l'ordre d'exécution sensible et difficile à vérifier formellement. L'Option B supprime ce couplage.

### 3. Cohérence du modèle de données en sortie d'Étape 2

L'Étape 2 produit une table de DOY résolus — un entier par slug par année. Cette table est le contrat d'interface entre l'Étape 2 et l'Étape 3. Si l'Étape 2 supprime conditionnellement certaines entrées sur critère saisonnier, la table devient incomplète de manière non déterministe : un audit de l'Étape 2 seule ne suffit plus à prédire quels slugs seront présents en entrée de l'Étape 3.

Avec l'Option B, la table de sortie de l'Étape 2 est complète et déterministe : **tous** les slugs du corpus ont un DOY résolu pour chaque année couverte par leur bloc `history`. La suppression est une transformation de l'Étape 3, traçable séparément.

### 4. Testabilité AOT

Chaque fonction de résolution d'ancre est une pure fonction `(year, params) -> u16`. Elle peut être testée exhaustivement sur la plage [1969, 2399] sans dépendance à l'état global du pipeline. Ce property est détruit dès qu'on introduit une consultation des `SeasonBoundaries` dans la fonction.

### 5. Uniformité du comportement `Ok(None)`

`Ok(None)` signifie exactement une chose dans le pipeline : **la fête n'existe pas pour cette année** (bloc `history` non couvert). Ce sémantisme est produit par `resolve_feast_for_year()` à l'Étape 1, pas par les fonctions de résolution de DOY à l'Étape 2. Utiliser `Ok(None)` pour signifier "absorbé par la saison" introduirait une ambiguïté sémantique dans un type de retour qui doit rester non ambigu.

---

## Conséquences

**Positives :**
- Étapes 2 et 3 testables et auditables indépendamment.
- Fonctions de résolution d'ancre : pures, exhaustivement testables sur [1969, 2399].
- Table de sortie Étape 2 : complète, déterministe, sans suppression conditionnelle.
- `Ok(None)` conserve un sémantisme unique et non ambigu dans le pipeline.

**Négatives / contraintes acceptées :**
- L'Étape 3 reçoit des slots ordinaires "fantômes" certaines années (DOY calculé mais liturgiquement inactif). Elle doit les éliminer par Conflict Resolution. C'est un travail supplémentaire pour l'Étape 3, acceptable car la logique d'élimination est triviale (Precedence ≤ 2 sur le slot concurrent → suppression du dimanche ordinaire).
- Le corpus YAML déclare 34 slugs `dominica_*_temporis_ordinarii` dont certains ne produiront jamais de `CalendarEntry` active certaines années. C'est un invariant documenté, pas une anomalie.

---

## Règle dérivée (à intégrer dans specification.md)

> **INV-FORGE-4** : Aucune fonction de résolution de DOY (Étape 2) ne consulte les `SeasonBoundaries` ni n'inspecte les slots résolus d'autres ancres. La résolution d'un DOY est une fonction pure de l'année et des paramètres de l'ancre. Toute logique conditionnelle sur la saison est une responsabilité exclusive de l'Étape 3.

---

## Références

- `liturgical-scheme.md` v1.3.3 — §3.2 : règle de résolution `tempus_ordinarium`, comportement `Ok(None)`
- `specification.md` v2.2 — Étape 2 : ordre de résolution des ancres, invariant saison
- `specification-patch-v2.2.md` — pseudo-code `resolve_tempus_ordinarium`, table de vérification 2025
