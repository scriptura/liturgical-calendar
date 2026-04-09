# 🛡️ ADR : Encodage des phases opérationnelles via `LiturgicalPeriod` (Firewall Sémantique)

**Statut** : Adopté  
**Date** : 05 Avril 2026  
**Composants impactés** : `liturgical-calendar-core` (Engine), `liturgical-calendar-forge` (Forge)

---

### 1. 🎯 Contexte et Problématique

Dans le cadre de l'architecture AOT-Only (v2.0), le format binaire `.kald` impose un layout strict pour la structure `CalendarEntry` : un stride constant de 64 bits (8 octets). L'état d'un jour liturgique est compacté dans un champ `flags` (`u16`, offset 4), dont 3 bits (bits 8-10) sont alloués à l'axe temporel.

Initialement nommé `Season`, ce champ devait intégrer la valeur `DiesSancti` (Semaine Sainte). Or, `DiesSancti` n'est pas un "temps liturgique" (_Tempus_) canonique au sens du Missel, mais une subdivision opérationnelle du Carême.

Cette situation crée un risque de **"Domain Trap"** : un développeur futur, guidé par une volonté de pureté sémantique, pourrait être tenté de "purifier" l'enum `Season` au runtime. Une telle modification briserait l'invariant de performance $O(1)$ en réintroduisant de la logique de branchement complexe pour reconstituer les phases liturgiques.

---

### 2. ⚙️ Options Envisagées

- **Option A : Conservation du terme `Season`**
  - _Risque_ : Confusion persistante entre taxonomie canonique et projection technique. Tentation de refactoring "propre" impactant le layout.
- **Option B : Adoption de `LiturgicalPeriod` (Retenue)**
  - _Mécanique_ : Renommage complet du type et du champ pour marquer une rupture conceptuelle avec le domaine source.

---

### 3. ⚖️ Décision

Nous adoptons le terme **`LiturgicalPeriod`** pour désigner l'enum et le champ de bits 8-10. Ce type intègre explicitement des variants opérationnels tels que `DiesSancti` (valeur 6) aux côtés des temps classiques (`TempusAdventus`, `TempusQuadragesimae`, etc.).

---

### 4. 💎 Justification Architecturale

1.  **Firewall Sémantique et Verrouillage AOT** : Le passage de `Season` à `LiturgicalPeriod` agit comme une barrière de protection. Il clarifie que l'Engine ne manipule pas des concepts théologiques abstraits, mais des **segments techniques matérialisés**. Ce renommage verrouille la nature de "cache AOT" du champ : l'information est résolue une fois pour toutes par la Forge.
2.  **Interdiction de la "Purification" Runtime** : En dissociant explicitement le nom du champ de la terminologie stricte du Missel, on interdit toute tentative de refactorisation logique au runtime. L'Engine reste un simple projecteur de données ; toute intelligence de segmentation reste déportée dans la Forge.
3.  **Validité Opérationnelle de `DiesSancti`** : Dans ce cadre, `DiesSancti` est un variant parfaitement valide. Bien qu'hétérogène sur le plan canonique, il est homogène sur le plan technique : il représente un état de rendu et de préséance mutuellement exclusif, indispensable au pipeline déterministe.
4.  **Optimisation DOD** : Ce choix préserve le budget de 3 bits sans sacrifier la clarté du code. L'accès reste une opération bitwise unique (`(flags >> 8) & 0x0007`), garantissant la compatibilité SIMD et l'efficacité du cache L1.

---

### 5. ✅ Conséquences

- **Positives** :
  - L'invariant de performance $O(1)$ est sanctuarisé contre les dérives sémantiques.
  - Le contrat entre la Forge (Producteur) et l'Engine (Consommateur) est explicite : "Je te transmets une période opérationnelle résolue".
  - L'empreinte mémoire reste verrouillée au stride de 8 octets.
- **Négatives** :
  - Nécessité de mettre à jour la documentation technique et les fichiers de spécification pour refléter ce changement de nomenclature.

---

### 6. 📝 Implémentation (Jalon 1)

L'enum dans le crate `core` sera défini ainsi :

```rust
/// Représente une période opérationnelle résolue (Cache AOT).
/// Ce type est une projection technique matérialisée par la Forge.
#[repr(u8)]
pub enum LiturgicalPeriod {
    TempusOrdinarium = 0,
    TempusAdventus = 1,
    TempusNativitatis = 2,
    TempusQuadragesimae = 3,
    TriduumPaschale = 4,
    TempusPaschale = 5,
    /// Phase opérationnelle : Semaine Sainte (Rameaux au Mercredi Saint).
    DiesSancti = 6,
}
```
