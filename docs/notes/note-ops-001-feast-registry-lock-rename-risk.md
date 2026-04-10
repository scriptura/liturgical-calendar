# 🛡️ Note Opérationnelle 001 : Gestion de l’Identité et Risques de Renommage (`feast_registry.lock`)

**Type** : Spécification Opérationnelle  
**Contexte** : `liturgical-calendar-forge` — Invariant d'Identité Stable (INV-FORGE-3)  
**Statut** : Canonique

---

## 🎯 1. Le Paradigme : Identité vs Projection

Dans une architecture **AOT/DOD**, il est crucial de distinguer le registre d'identité de la vue matérialisée :

- **Le Registre (`feast_registry.lock`)** : C'est la **Source de Vérité (SOT)** de l'identité. Il lie un nom de fichier (slug) à un `FeastID` unique. Il est persistant et cumulatif.
- **Le Binaire (`.kald`)** : C'est une **Projection** temporelle dense. Il est reforgé "à neuf" à chaque compilation, mais il puise ses IDs dans le registre.

**Le Risque** : Un renommage de fichier YAML est interprété par la Forge comme une suppression suivie d'une création. Cela génère un **Tombstone** (ID enterré) dans le registre.

```text
État N   : fete_a.yaml -> ID 0x001 (Actif)
État N+1 : fete_b.yaml (ex fete_a) -> ID 0x001 (Tombstone) | ID 0x002 (Actif)
```

---

## 🏗️ 2. Impact sur le Data Layout (DOD Focus)

Contrairement à une base de données classique, le renommage n'alourdit pas l'artefact de production.

- **Densité du `.kald`** : Le fichier binaire utilise un _stride_ fixe de 64 bits par jour. Que l'ID soit `0x001` ou `0x999`, l'empreinte mémoire reste strictement la même. Le `.kald` ne contient aucun "trou" lié aux anciens IDs.
- **Embonpoint de la Forge** : Seul le fichier `.lock` (utilisé uniquement au build-time) conserve la trace des IDs tombstonés. L'impact au runtime est **nul**.

---

## 🔗 3. Synchronisation Atomique : `.kald` & `.lits`

La cohérence du système repose sur la synchronisation entre la structure (`.kald`) et le dictionnaire (`.lits`).

- **Forge Atomique** : Les deux fichiers sont générés lors de la même passe de compilation.
- **Sync Hash** : La Forge injecte un hash d'intégrité identique dans les headers des deux fichiers.
- **Protection Runtime** : L'Engine refuse de charger un `.lits` dont le hash de synchronisation ne correspond pas au `.kald`. Cela garantit qu'un décalage d'IDs (suite à un reset du `.lock`) ne provoque jamais l'affichage d'un texte erroné.

---

## 🧪 4. Stratégie de Test : L'Approche "Architecte"

Pour maintenir la validité du système malgré les évolutions du registre, la stratégie de test doit privilégier le **Contrat** sur l'**État**.

- **Anti-Pattern (Option A)** : Coder des IDs en dur dans les tests (`assert_eq!(id, 0x1042)`). Ces tests brisent au moindre renommage ou nettoyage du registre.
- **Pattern Recommandé (Option B)** : Les tests interrogent le registre pour obtenir l'ID actuel d'un slug avant de valider le binaire.

```rust
// Approche DOD/Architecte
let expected_id = registry.get_id("nativitas");
let day_data = engine.get_day(2026, 359); // 25 Déc.
assert_eq!(day_data.feast_id, expected_id);
```

_Cette méthode rend les tests immuns aux refactors de noms de fichiers et aux resets de registres._

---

## 🛠️ 5. Mesures de Gouvernance

1.  **Phase de Design (Pré-v1.0)** : La suppression du `.lock` est autorisée pour "nettoyer" la réalité technique et repartir sur une indexation dense. C'est le moment idéal pour stabiliser les noms de fichiers.
2.  **Phase de Production (Post-v1.0)** : Le `.lock` devient immuable. Tout renommage doit être justifié car il brise la compatibilité avec les systèmes externes (favoris utilisateurs, logs, bases tierces) qui stockent les `FeastID`.
3.  **Validation AOT** : Utiliser le "Binary Diffing" pour vérifier que les changements dans le registre ne provoquent pas de modifications inattendues dans la topologie du `.kald`.

---

**Conclusion** : Le temps investi dans l'ajustement des noms de fichiers avant le "gel" de la v1.0 n'est pas une perte de temps, mais une action d'assainissement du **Contrat d'Identité** du système.
