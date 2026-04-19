# ADR-010 : Normalisation de la Frontière (Précédence 1-based to 0-based)

**Statut** : Accepté  
**Date** : 2026-04-18  
**Contexte** : Architecture AOT / Pipeline de la Forge

## 1. Contexte et Problématique

Le système de calendrier liturgique repose sur une hiérarchie de poids (précédence) déterminant la résolution des conflits de dates. Deux contraintes divergentes s'opposent :

1.  **Domaine Canonique (Input)** : Le droit positif (Normae Universales 1969) définit une échelle de 1 à 13. C'est le référentiel utilisé par les rédacteurs des fichiers YAML (souvent non-développeurs).
2.  **Domaine Machine (Engine)** : Pour garantir un accès O(1) et un layout binaire compact, l'Engine (`liturgical-calendar-core`) utilise un indexage naturel (0-based). Une valeur sur 4 bits (0-15) est allouée dans le pack binaire.

Jusqu'à présent, une "fuite d'abstraction" (leaky abstraction) imposait la plage 0-12 dans les fichiers YAML, créant une friction cognitive et un risque d'erreur de saisie significatif.

## 2. Décision Architecturale

Nous implémentons une **Normalisation à la Frontière** (Boundary Normalization) au sein de la Forge.

- **Interface de Saisie (YAML)** : Migration vers la plage **1 à 13**.
- **Point de Transformation** : La désérialisation (Phase 1 du pipeline : Rule Parsing) via un décorateur `serde` personnalisé.
- **Représentation Interne** : Maintien de la plage **0 à 12**.

## 3. Justification Technique

### 3.1 Séparation des Responsabilités (SoC)

La Forge est un compilateur AOT. Son rôle est d'absorber la complexité du domaine humain pour produire une donnée "prête à l'emploi" pour l'Engine. Déporter le `shift -1` dans la Forge garantit que l'Engine reste `stateless` et purement orienté données (Data-Oriented).

### 3.2 Invariant de Performance

Le passage de 1-based à 0-based est effectué une seule fois lors de la compilation des données (.kald). À l'exécution (runtime), l'Engine effectue ses comparaisons et accès mémoire sans aucune opération arithmétique de correction.

### 3.3 Intégrité du Pipeline

L'utilisation d'un désérialiseur personnalisé permet de valider le domaine de valeur (`1 <= x <= 13`) avant même que la donnée ne pénètre dans les structures logiques de la Forge. Cela renforce la robustesse du typage dès l'entrée.

## 4. Conséquences

| Domaine              | Impact                                                                                              |
| :------------------- | :-------------------------------------------------------------------------------------------------- |
| **Maintenance YAML** | Amélioration de l'ergonomie. Alignement strict sur les documents officiels.                         |
| **Code Forge**       | Ajout d'une fonction `deserialize_precedence`. Mise à jour des tests unitaires (+1 sur les inputs). |
| **Engine (Core)**    | **Zéro impact.** Conservation du layout binaire et des performances actuelles.                      |
| **Documentation**    | Mise à jour de `liturgical-scheme.md` (v1.7.1) pour refléter la plage 1-13.                         |

## 5. Modèle d'Implémentation (AOT Shift)

```rust
// Dans la Forge (liturgical-calendar-forge)
fn normalize_precedence(val: u8) -> u8 {
    // Validation stricte du domaine canonique
    assert!(val >= 1 && val <= 13);
    // Normalisation vers l'invariant machine
    val - 1
}
```

---

_Ce document fait autorité pour toute future modification du schéma d'ingestion des données._
