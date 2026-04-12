# ADR-007 — Neutralité du Slug : Interdiction d'encoder le Statut Liturgique

**Statut** : Accepté  
**Date** : 2026-04-10  
**Contexte** : `liturgical-calendar-forge` — §2.1 du contrat / `FeastRegistry`

---

## Contexte

Le slug est la clé primaire immuable d'une fête dans le `FeastRegistry`. Il est déduit du stem du nom de fichier et transformé en `FeastID` (u16) par la Forge. Une fois alloué, un `FeastID` est stable sur toute la plage 1969–2399 pour un slug donné — le `feast_registry.lock` garantit cette stabilité inter-builds.

Le statut liturgique d'une personne peut évoluer dans le temps : un laïc est béatifié, un bienheureux est canonisé. Ces évolutions sont courantes dans le corpus (Jean-Paul II : béatification 2011, canonisation 2014 ; Teresa de Calcutta : canonisation 2016). La question architecturale est : le slug doit-il refléter le statut courant de la fête ?

---

## Décision

**Le slug identifie la personne ou l'événement, pas son statut liturgique courant. L'encodage de tout préfixe ou suffixe de statut dans le slug est interdit.**

```
✅  ioannis_pauli_ii          ← stable sur toute la plage 1969–2399
✅  teresiae_de_calcutta       ← stable, indépendant de la canonisation
❌  s_ioannis_pauli_ii         ← encode "Sanctus" — invalide à la béatification
❌  b_caroli_de_foucauld       ← encode "Beatus" — invalide à la canonisation
❌  sancti_ioannis_pauli_ii    ← idem
```

---

## Justification

### 1. Un rename de fichier est un changement d'identité, pas un refactor

Le `FeastID` est alloué par la Forge à partir du slug au premier build, puis stabilisé dans `feast_registry.lock`. Si le slug change (rename du fichier), la Forge alloue un nouveau `FeastID` et tombstonise l'ancien. L'ancien `FeastID` ne peut jamais être réalloué — il est perdu.

Conséquence : toute installation ayant compilé un `.kald` avec l'ancien `FeastID` est désynchronisée. Les références au `FeastID` dans des données persistantes tierces (agendas exportés, bases de données applicatives) pointent vers un identifiant tombstoné.

### 2. La fréquence des canonisations est non nulle sur la plage 1969–2399

Sur la plage de 430 ans couverte par la Forge, le nombre de canonisations futures est inconnu mais non nul. Tout saint canonisé après inscription au corpus (comme bienheureux) déclencherait un rename si le statut est encodé dans le slug. Ce n'est pas un cas théorique : Jean-Paul II, Thérèse de Calcutta, Charles de Foucauld illustrent ce pattern dans le corpus actuel.

### 3. Le statut est une propriété temporelle, pas une propriété d'identité

Le statut liturgique (bienheureux, saint) est une information qui varie dans le temps — c'est exactement ce que le bloc `history` est conçu pour capturer. Le label textuel (« B. Ioannes Paulus II » vs « S. Ioannes Paulus II ») est externalisé dans les dictionnaires `i18n/` et versionné par `from`. Le slug n'a pas à porter cette information.

```yaml
# Correct : le slug est neutre, le statut est dans l'i18n
# i18n/la/ioannis_pauli_ii.yaml
2011:
  title: "B. Ioannes Paulus II, pp."
2014:
  title: "S. Ioannes Paulus II, pp."
```

### 4. Validation syntaxique préalable

Le contrat impose `[a-z][a-z0-9_]*` comme syntaxe de slug. Cette règle exclut les préfixes de statut courants (`s_`, `b_`) uniquement si le caractère suivant le tiret-bas n'est pas alphabétique — ce n'est pas suffisant. La règle de neutralité est sémantique, pas syntaxique, et doit être documentée explicitement pour guider les contributeurs du corpus.

---

## Conséquences

**Positives :**

- Stabilité des `FeastID` garantie pour toute canonisation future — aucune intervention sur le corpus ou le `feast_registry.lock`.
- Cohérence du modèle : le slug est une clé technique, pas un label liturgique.
- Les dictionnaires `i18n/` sont le seul vecteur d'évolution du label textuel — séparation nette data/présentation.

**Contraintes acceptées :**

- Le slug ne communique pas le statut au lecteur humain. Un contributeur qui lit `ioannis_pauli_ii.yaml` ne sait pas si la fête est pour un bienheureux ou un saint sans consulter le bloc `history` ou l'`i18n`. C'est le coût de la neutralité — acceptable car le statut est dans les données, pas dans la clé.
- La règle est sémantique et ne peut pas être vérifiée mécaniquement par la Forge sans une liste noire de préfixes/suffixes interdits. La vérification est humaine (code review du corpus).

---

## Règle dérivée

> **INV-CORPUS-1** : Le stem d'un fichier YAML du corpus doit identifier la personne ou l'événement liturgique de manière neutre vis-à-vis de tout statut canonique (Beatus, Sanctus, Venerabilis, Servus Dei) et de tout rang liturgique (Martyr, Confessor, Virgo, Doctor). Tout slug contenant un préfixe ou suffixe de statut est refusé à la revue de corpus, indépendamment de la validation syntaxique V6.

---

## Références

- ADR-001 — Indépendance de la résolution DOY (contexte : stabilité des identifiants)
