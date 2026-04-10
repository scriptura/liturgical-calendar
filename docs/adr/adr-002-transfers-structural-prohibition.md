# ADR-002 — Interdiction du bloc `transfers` pour le calcul structurel

**Statut** : Accepté  
**Date** : 2026-04-10  
**Contexte** : `liturgical-calendar-forge` — Étape 3 (Conflict Resolution) / §2.4 du contrat

---

## Contexte

Le bloc `transfers` est un mécanisme déclaratif permettant à une fête de spécifier une règle de repli quand elle entre en collision avec une fête identifiée par son slug. Il est conçu pour les **exceptions liturgiques nommées** : "si cette fête rencontre telle autre fête, se déplacer de N jours ou à telle date fixe".

Lors de la modélisation des fêtes du Cycle de Noël (Sainte Famille, Baptême du Seigneur), une question s'est posée : peut-on exprimer "dimanche dans l'Octave de Noël" en déclarant `date: month: 12, day: 25` avec un bloc `transfers` qui déplace la fête au dimanche suivant si Noël n'est pas un dimanche ?

Cette approche a été envisagée et rejetée.

---

## Décision

**Le bloc `transfers` est réservé exclusivement à la résolution de collisions liturgiques nommées entre fêtes identifiées. Il est interdit de l'utiliser pour exprimer un calcul de date structurel.**

Sont considérés comme calcul structurel : toute règle dont le résultat dépend d'une propriété du calendrier (jour de la semaine, position dans le cycle pascal, relation à une date fixe) plutôt que de la présence d'une fête concurrente identifiée par un slug.

---

## Justification

### 1. Sémantique incompatible

`transfers` est une **table de dispatch** indexée par slug concurrent. Son sémantisme est : "si le slug X occupe mon DOY, je me déplace". Ce n'est pas une règle de calcul de DOY — c'est une règle de résolution post-calcul.

Utiliser `transfers` pour calculer "le dimanche suivant le 25 décembre" reviendrait à déclarer des collides sur des slugs fantômes (`nativitas_domini` si dimanche, absence de collision sinon). Ce pattern introduit une logique conditionnelle encodée dans des données censées être déclaratives.

### 2. Résolution à un seul niveau — invariant de la spec

Le contrat §2.4 stipule explicitement : "La Forge ne réapplique pas le bloc `transfers` de la fête déplacée sur sa nouvelle date." La résolution est à un seul niveau. Un calcul structurel exprimé via `transfers` qui atterrit sur un slot occupé produit un `ConflictWarning` — pas une nouvelle application de la règle. Le comportement devient non déterministe pour les années où le dimanche cible est lui-même occupé.

### 3. Testabilité dégradée

Un bloc `transfers` pour calcul structurel crée une dépendance entre l'Étape 3 et le calendrier grégorien (jour de la semaine) que l'Étape 3 n'est pas conçue pour consommer. La fonction de résolution de l'Étape 3 attend des slugs concurrents résolvables dans le `FeastRegistry` — pas des propriétés temporelles.

### 4. Coût AOT nul de l'alternative

L'alternative retenue — étendre la liste des ancres mobiles — encapsule le calcul structurel dans l'Étape 2 (Canonicalization), là où il est sémantiquement correct. Les ancres étant résolues AOT, leur ajout n'impacte pas l'empreinte mémoire du runtime ni la structure binaire `.kald`.

---

## Conséquences

**Positives :**
- `transfers` conserve un sémantisme unique et auditable : collision entre slugs identifiés, jamais calcul de position.
- L'Étape 3 reste indépendante du calendrier grégorien — elle opère sur des DOY résolus, pas sur des dates.
- Les règles de `transfers` sont exhaustivement vérifiables au build (V-T1 à V-T4) sans simulation calendaire.

**Contraintes acceptées :**
- Tout nouveau cas de date mobile structurelle requiert l'extension du répertoire d'ancres et un patch `specification.md`. C'est un coût de conception, pas un coût runtime.

---

## Règle dérivée

> **INV-FORGE-5** : Tout slug déclaré dans un champ `collides` doit être un slug existant dans le `FeastRegistry` au moment de la Passe 3. Une règle `transfers` dont la condition de déclenchement dépend d'une propriété calendaire (jour de semaine, DOY absolu, appartenance à une saison) est invalide par construction et doit être reexprimée comme une ancre mobile à l'Étape 2.

---

## Références

- `liturgical-scheme.md` v1.3.3 — §2.4 : sémantique et invariants du bloc `transfers`
- `specification.md` v2.2 — Étape 2 : répertoire des ancres
- ADR-001 — Indépendance de la résolution DOY vis-à-vis de la logique saisonnière
