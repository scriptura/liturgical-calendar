/// Erreur retournée par les conversions de types de domaine.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DomainError {
    /// Valeur numérique hors plage des variants définis.
    InvalidDiscriminant(u8),
}

// ── Precedence ───────────────────────────────────────────────────────────────

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
/// Définit la hiérarchie de préséance pour la résolution des occurrences liturgiques.
/// L'ordre numérique (0..=12) correspond à la priorité décroissante (0 = priorité absolue).
pub enum Precedence {
    /// 1. Triduum pascal.
    TriduumSacrum = 0,
    /// 2. Solennités majeures : Nativité, Épiphanie, Ascension, Pentecôte.
    ///    Inclut les dimanches d'Avent, Carême, Temps Pascal, Mercredi des Cendres et Semaine Sainte.
    SollemnitatesMaiores = 1,
    /// 3. Solennités du Seigneur, de la Vierge et des saints inscrits au calendrier général.
    ///    Inclut la Commémoration des fidèles défunts.
    SollemnitatesGenerales = 2,
    /// 4. Solennités propres (patron du lieu, dédicace de l'église, titulaire de l'ordre).
    SollemnitatesPropria = 3,
    /// 5. Fêtes du Seigneur inscrites au calendrier général.
    FestaDomini = 4,
    /// 6. Dimanches du temps de Noël et dimanches du temps ordinaire.
    DominicaePerAnnum = 5,
    /// 7. Fêtes de la Vierge Marie et des saints du calendrier général.
    FestaBMVEtSanctorumGenerales = 6,
    /// 8. Fêtes propres (patron du diocèse, anniversaire dédicace cathédrale, etc.).
    FestaPropria = 7,
    /// 9. Féries privilégiées : Avent (17-24 déc.), Octave de Noël, féries de Carême.
    FeriaePrivilegiatae = 8,
    /// 10. Mémoires obligatoires du calendrier général.
    MemoriaeObligatoriaGenerales = 9,
    /// 11. Mémoires obligatoires propres (patron du lieu, diocèse, ou ordre).
    MemoriaeObligatoriaePropria = 10,
    /// 12. Mémoires facultatives.
    MemoriaeAdLibitum = 11,
    /// 13. Féries communes : Avent (jusqu'au 16 déc.), Noël, Temps Pascal et Temps Ordinaire.
    FeriaePerAnnum = 12,
    // 13–15 : réservés système — V2 interdit ces valeurs dans les entrées YAML.
}

impl Precedence {
    /// Convertit un `u8` en `Precedence`. Variants 13–15 → `Err`.
    pub fn try_from_u8(val: u8) -> Result<Self, DomainError> {
        match val {
            0 => Ok(Self::TriduumSacrum),
            1 => Ok(Self::SollemnitatesMaiores),
            2 => Ok(Self::SollemnitatesGenerales),
            3 => Ok(Self::SollemnitatesPropria),
            4 => Ok(Self::FestaDomini),
            5 => Ok(Self::DominicaePerAnnum),
            6 => Ok(Self::FestaBMVEtSanctorumGenerales),
            7 => Ok(Self::FestaPropria),
            8 => Ok(Self::FeriaePrivilegiatae),
            9 => Ok(Self::MemoriaeObligatoriaGenerales),
            10 => Ok(Self::MemoriaeObligatoriaePropria),
            11 => Ok(Self::MemoriaeAdLibitum),
            12 => Ok(Self::FeriaePerAnnum),
            v => Err(DomainError::InvalidDiscriminant(v)),
        }
    }
}

// ── Nature ───────────────────────────────────────────────────────────────────

/// Nature de la célébration liturgique.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Nature {
    /// Solennité.
    Sollemnitas = 0,
    /// Fête.
    Festum = 1,
    /// Dimanche du temps ordinaire.
    Dominica = 2,
    /// Mémoire (obligatoire ou facultative).
    Memoria = 3,
    /// Commémoration, "trace" d'une célébration déclassée.
    Commemoratio = 4,
    /// Férie.
    Feria = 5,
    // 6–7 : réservés.
}

impl Nature {
    /// Variants 5–7 → `Err`.
    pub fn try_from_u8(val: u8) -> Result<Self, DomainError> {
        match val {
            0 => Ok(Self::Sollemnitas),
            1 => Ok(Self::Festum),
            2 => Ok(Self::Dominica),
            3 => Ok(Self::Memoria),
            4 => Ok(Self::Commemoratio),
            5 => Ok(Self::Feria),
            v => Err(DomainError::InvalidDiscriminant(v)),
        }
    }
}

// ── Color ────────────────────────────────────────────────────────────────────

/// Couleur liturgique.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Color {
    /// Blanc — fêtes du Seigneur, Vierge, Confesseurs, Docteurs.
    Albus = 0,
    /// Rouge — Passion, Apôtres, Martyrs, Pentecôte.
    Rubeus = 1,
    /// Vert — temps ordinaire.
    Viridis = 2,
    /// Violet — Avent, Carême.
    Violaceus = 3,
    /// Rose — Gaudete (Avent III), Laetare (Carême IV).
    Rosaceus = 4,
    /// Noir — messes des défunts.
    Niger = 5,
    // 6 : usage liturgique futur (or, argent — optionnel selon usages diocésains).
    // 7–15 : réservés.
}

impl Color {
    /// Variants 6–15 → `Err`.
    pub fn try_from_u8(val: u8) -> Result<Self, DomainError> {
        match val {
            0 => Ok(Self::Albus),
            1 => Ok(Self::Rubeus),
            2 => Ok(Self::Viridis),
            3 => Ok(Self::Violaceus),
            4 => Ok(Self::Rosaceus),
            5 => Ok(Self::Niger),
            v => Err(DomainError::InvalidDiscriminant(v)),
        }
    }
}

// ── LiturgicalPeriod ─────────────────────────────────────────────────────────

/// Période du calendrier liturgique.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum LiturgicalPeriod {
    /// Temps ordinaire (état par défaut, valeur 0).
    TempusOrdinarium = 0,
    /// Avent.
    TempusAdventus = 1,
    /// Temps de Noël.
    TempusNativitatis = 2,
    /// Carême.
    TempusQuadragesimae = 3,
    /// Triduum Pascal.
    TriduumPaschale = 4,
    /// Temps pascal.
    TempusPaschale = 5,
    /// Semaine Sainte (Rameaux inclus → Mercredi Saint inclus).
    /// Variant opérationnel — subdivision du Carême, non du Missel strict.
    DiesSancti = 6,
    // 7 : réservé.
}

impl LiturgicalPeriod {
    /// Variant 7 → `Err`.
    pub fn try_from_u8(val: u8) -> Result<Self, DomainError> {
        match val {
            0 => Ok(Self::TempusOrdinarium),
            1 => Ok(Self::TempusAdventus),
            2 => Ok(Self::TempusNativitatis),
            3 => Ok(Self::TempusQuadragesimae),
            4 => Ok(Self::TriduumPaschale),
            5 => Ok(Self::TempusPaschale),
            6 => Ok(Self::DiesSancti),
            v => Err(DomainError::InvalidDiscriminant(v)),
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precedence_roundtrip() {
        let variants = [
            Precedence::TriduumSacrum,
            Precedence::SollemnitatesMaiores,
            Precedence::SollemnitatesGenerales,
            Precedence::SollemnitatesPropria,
            Precedence::FestaDomini,
            Precedence::DominicaePerAnnum,
            Precedence::FestaBMVEtSanctorumGenerales,
            Precedence::FestaPropria,
            Precedence::FeriaePrivilegiatae,
            Precedence::MemoriaeObligatoriaGenerales,
            Precedence::MemoriaeObligatoriaePropria,
            Precedence::MemoriaeAdLibitum,
            Precedence::FeriaePerAnnum,
        ];
        for v in variants {
            assert_eq!(Precedence::try_from_u8(v as u8), Ok(v));
        }
    }

    #[test]
    fn precedence_yaml_to_internal_spot_checks() {
        assert_eq!(Precedence::try_from_u8(0), Ok(Precedence::TriduumSacrum));
        assert_eq!(
            Precedence::try_from_u8(9),
            Ok(Precedence::MemoriaeObligatoriaGenerales)
        );
        assert_eq!(
            Precedence::try_from_u8(10),
            Ok(Precedence::MemoriaeObligatoriaePropria)
        );
        assert_eq!(
            Precedence::try_from_u8(11),
            Ok(Precedence::MemoriaeAdLibitum)
        );
        assert_eq!(Precedence::try_from_u8(12), Ok(Precedence::FeriaePerAnnum));
    }

    #[test]
    fn precedence_reserved() {
        for v in [13u8, 14, 15] {
            assert_eq!(
                Precedence::try_from_u8(v),
                Err(DomainError::InvalidDiscriminant(v))
            );
        }
    }

    #[test]
    fn precedence_ordering() {
        assert!(Precedence::TriduumSacrum < Precedence::SollemnitatesMaiores);
        assert!(Precedence::MemoriaeObligatoriaGenerales < Precedence::MemoriaeObligatoriaePropria);
        assert!(Precedence::MemoriaeObligatoriaePropria < Precedence::MemoriaeAdLibitum);
        assert!(Precedence::MemoriaeAdLibitum < Precedence::FeriaePerAnnum);
    }

    #[test]
    fn nature_roundtrip() {
        for v in [
            Nature::Sollemnitas,
            Nature::Festum,
            Nature::Memoria,
            Nature::Feria,
            Nature::Commemoratio,
        ] {
            assert_eq!(Nature::try_from_u8(v as u8), Ok(v));
        }
    }

    #[test]
    fn color_roundtrip() {
        for v in [
            Color::Albus,
            Color::Rubeus,
            Color::Viridis,
            Color::Violaceus,
            Color::Rosaceus,
            Color::Niger,
        ] {
            assert_eq!(Color::try_from_u8(v as u8), Ok(v));
        }
    }

    #[test]
    fn liturgical_period_roundtrip() {
        for v in [
            LiturgicalPeriod::TempusOrdinarium,
            LiturgicalPeriod::TempusAdventus,
            LiturgicalPeriod::TempusNativitatis,
            LiturgicalPeriod::TempusQuadragesimae,
            LiturgicalPeriod::TriduumPaschale,
            LiturgicalPeriod::TempusPaschale,
            LiturgicalPeriod::DiesSancti,
        ] {
            assert_eq!(LiturgicalPeriod::try_from_u8(v as u8), Ok(v));
        }
    }
}
