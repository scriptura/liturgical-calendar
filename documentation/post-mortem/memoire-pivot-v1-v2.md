# Mémoire du pivot architectural — v1 → v2

**Nature du document** : trace historique du basculement architectural  
**Périmètre** : mémoire de projet, post-mortem, contexte de migration  
**Statut** : rétrospectif

---

## 1. Pourquoi ce document existe

Ce document ne remplace pas l’ADR. L’ADR documente la décision telle qu’elle s’applique au présent du système : ce qu’il faut faire maintenant, et pourquoi le code a cette forme.

Cette note, elle, conserve la **mémoire du pivot** : le moment où le projet a cessé d’être pensé comme un moteur de calcul liturgique dynamique pour devenir un système compilé, à la frontière entre **Forge** et **runtime de lecture**.

L’objectif est simple : éviter qu’avec le temps, la structure actuelle soit relue comme une évidence technique alors qu’elle est le résultat d’un basculement conceptuel précis.

---

## 2. Contexte initial

La première architecture, **v1**, cherchait à porter une grande partie de la logique au runtime.  
Elle combinait calculs, résolution de règles et mécanismes de sélection de chemin d’exécution.

Ce choix a fini par rencontrer une limite structurelle : le domaine n’est pas une mécanique naturelle stable, mais un **corpus de droit liturgique historique**, évolutif, localisé dans le temps, et résolu par compilation amont plutôt que par calcul perpétuel.

Autrement dit, l’erreur initiale n’était pas seulement une question d’implémentation.  
Elle était **ontologique** : traiter un système de règles historiques comme un problème d’algorithme continu.

---

## 3. Le point de rupture

Le pivot a été déclenché quand il est devenu clair que :

1. le runtime ne devait plus porter l’intelligence métier ;
2. la logique de domaine devait être extraite et figée en amont ;
3. la stabilité du système dépendait davantage de la matérialisation des données que de la sophistication du calcul ;
4. toute tentative de conserver une voie hybride créait de l’indirection, des branches et des risques de divergence.

Le changement décisif a donc consisté à abandonner le modèle **hybride Fast/Slow Path** au profit d’un modèle **AOT-only** :  
la Forge calcule, résout et matérialise ; le runtime lit.

---

## 4. Ce que le pivot a changé

### 4.1 Déplacement de l’intelligence

Avant le pivot, le runtime devait encore connaître des éléments comme :

- les calculs liés aux dates mobiles ;
- les règles de préséance ;
- les mécanismes de résolution de conflits ;
- des couches logiques séparées pour les saisons, les temporalités et le sanctoral.

Après le pivot, cette intelligence a été déplacée dans la Forge.  
Le runtime n’a plus de logique de domaine : il devient un **projecteur de mémoire**.

### 4.2 Passage d’un calcul à une matérialisation

Le système n’est plus conçu pour “savoir” la liturgie au moment de l’exécution.  
Il lit un état déjà compilé, stocké dans un format binaire stable.

Le changement de perspective est majeur :

- **v1** : le runtime reconstitue ;
- **v2** : la Forge compile, le runtime consomme.

### 4.3 Réduction du risque de divergence

Le pivot a aussi été motivé par un risque pratique : multiplier les implémentations du calcul augmentait la probabilité de divergences entre langages, plateformes ou ports futurs.

En figant la sortie dans un artefact binaire unique, le projet a gagné :

- une source de vérité plus nette ;
- une reproductibilité supérieure ;
- une surface runtime plus petite ;
- une meilleure portabilité.

---

## 5. Ce qui a été conservé

Le pivot n’a pas consisté à tout jeter.  
Certaines structures devaient rester, car elles portent l’identité stable du projet.

Sont conservés :

- les types de domaine ;
- le système de `FeastID` ;
- le format d’entrée YAML ;
- le format `.lits` ;
- le `StringProvider` ;
- les conventions FFI et les codes d’erreur ;
- les invariants `no_std` et `no_alloc` ;
- la discipline de validation.

Ce qui a changé, ce n’est pas la présence d’un domaine ; c’est **l’endroit où il est résolu**.

---

## 6. La nouvelle architecture v2

La **v2** repose sur une séparation stricte :

### Forge

La Forge est le compilateur du domaine.  
Elle :

- ingère les données ;
- canonicalise ;
- résout les conflits ;
- matérialise les jours ;
- construit le pool secondaire ;
- écrit le binaire final ;
- garantit l’intégrité par checksum.

### Runtime

Le runtime ne fait que :

- valider ;
- adresser ;
- lire ;
- exposer.

Il ne calcule plus la liturgie.  
Il n’essaie plus de la déduire.  
Il ne reconstitue plus les règles.

Cette simplification n’est pas seulement un gain de performance.  
C’est une clarification du modèle mental du projet.

---

## 7. Conséquences historiques du pivot

### Conséquences positives

- baisse de la complexité runtime ;
- disparition d’une partie de l’indirection ;
- meilleure prédictibilité ;
- meilleure compatibilité avec les contraintes AOT ;
- portabilité accrue ;
- reproductibilité plus forte ;
- séparation plus nette entre construction et consultation.

### Coûts et contreparties

- le système dépend davantage d’un artefact compilé ;
- la Forge devient un composant critique ;
- la correction du runtime dépend de la qualité de la compilation amont ;
- la plage de validité est bornée par le corpus compilé.

Autrement dit, le pivot a déplacé la complexité au bon endroit, mais il ne l’a pas supprimée.  
Il l’a rendue **explicite**.

---

## 8. Ce qu’il faut retenir dans le futur

Si, plus tard, le code semble “naturellement” simple, il ne faut pas en déduire que cette simplicité a toujours existé.

Elle est le produit d’une rupture précise :

- abandon du calcul liturgique au runtime ;
- adoption d’un système compilé ;
- renforcement du rôle de la Forge ;
- réduction du runtime à une lecture déterministe.

La leçon historique à préserver est la suivante :

> le projet n’a pas été simplifié par hasard ; il a été redéfini pour que la logique de domaine vive au bon étage.

---

## 9. Résumé en une phrase

Le passage de **v1** à **v2** marque le moment où le projet a cessé d’être un moteur de calcul liturgique pour devenir un système compilé, déterministe et orienté données, où la Forge porte l’intelligence et le runtime ne fait que lire.

---

## 10. Référence interne

Ce document est à conserver avec les notes de post-mortem et l’ADR du pivot, afin de distinguer :

- la **justification présente** de l’architecture,
- et la **mémoire historique** de son basculement.
