/// Erreur retournée par les conversions de types de domaine.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DomainError {
    /// Valeur numérique hors plage des variants définis.
    InvalidDiscriminant(u8),
}

// ── Precedence ───────────────────────────────────────────────────────────────

/// Ordre de préséance liturgique (Ordo Romanus).
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Precedence {
    /// Triduum Sacrum (priorité absolue).
    TriduumSacrum = 0,
    /// Solennités fixes majeures (Noël, Épiphanie, Ascension…).
    SollemnitatesFixaeMaior = 1,
    /// Dimanches privilégiés majeurs (Avent, Carême, Pâques).
    DominicaePrivilegiataeMaior = 2,
    /// Féries privilégiées majeures (Mercredi des Cendres, Semaine Sainte).
    FeriaePrivilegiataeMaior = 3,
    /// Solennités générales du calendrier universel.
    SollemnitatesGenerales = 4,
    /// Solennités propres (diocésaines, religieuses).
    SollemnitatesPropria = 5,
    /// Fêtes du Seigneur inscrites au calendrier général.
    FestaDomini = 6,
    /// Dimanches du temps ordinaire.
    DominicaePerAnnum = 7,
    /// Fêtes de la BVM et des saints du calendrier général.
    FestaBMVEtSanctorumGenerales = 8,
    /// Fêtes propres.
    FestaPropria = 9,
    /// Féries d'Avent et octave de Noël (17–24 décembre).
    FeriaeAdventusEtOctavaNativitatis = 10,
    /// Mémoires obligatoires.
    MemoriaeObligatoriae = 11,
    /// Féries du temps ordinaire et mémoires facultatives.
    FeriaePerAnnumEtMemoriaeAdLibitum = 12,
    // 13–15 : réservés système — V2 interdit ces valeurs dans les entrées YAML.
}

impl Precedence {
    /// Convertit un `u8` en `Precedence`. Variants 13–15 → `Err`.
    pub fn try_from_u8(val: u8) -> Result<Self, DomainError> {
        match val {
            0 => Ok(Self::TriduumSacrum),
            1 => Ok(Self::SollemnitatesFixaeMaior),
            2 => Ok(Self::DominicaePrivilegiataeMaior),
            3 => Ok(Self::FeriaePrivilegiataeMaior),
            4 => Ok(Self::SollemnitatesGenerales),
            5 => Ok(Self::SollemnitatesPropria),
            6 => Ok(Self::FestaDomini),
            7 => Ok(Self::DominicaePerAnnum),
            8 => Ok(Self::FestaBMVEtSanctorumGenerales),
            9 => Ok(Self::FestaPropria),
            10 => Ok(Self::FeriaeAdventusEtOctavaNativitatis),
            11 => Ok(Self::MemoriaeObligatoriae),
            12 => Ok(Self::FeriaePerAnnumEtMemoriaeAdLibitum),
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
    /// Mémoire (obligatoire ou facultative).
    Memoria = 2,
    /// Férie (y compris les dimanches — cf. spec §4.2).
    Feria = 3,
    /// Commémoration.
    Commemoratio = 4,
    // 5–7 : réservés.
}

impl Nature {
    /// Variants 5–7 → `Err`.
    pub fn try_from_u8(val: u8) -> Result<Self, DomainError> {
        match val {
            0 => Ok(Self::Sollemnitas),
            1 => Ok(Self::Festum),
            2 => Ok(Self::Memoria),
            3 => Ok(Self::Feria),
            4 => Ok(Self::Commemoratio),
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
    Roseus = 4,
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
            4 => Ok(Self::Roseus),
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
            Precedence::SollemnitatesFixaeMaior,
            Precedence::DominicaePrivilegiataeMaior,
            Precedence::FeriaePrivilegiataeMaior,
            Precedence::SollemnitatesGenerales,
            Precedence::SollemnitatesPropria,
            Precedence::FestaDomini,
            Precedence::DominicaePerAnnum,
            Precedence::FestaBMVEtSanctorumGenerales,
            Precedence::FestaPropria,
            Precedence::FeriaeAdventusEtOctavaNativitatis,
            Precedence::MemoriaeObligatoriae,
            Precedence::FeriaePerAnnumEtMemoriaeAdLibitum,
        ];
        for v in variants {
            assert_eq!(Precedence::try_from_u8(v as u8), Ok(v));
        }
    }

    #[test]
    fn precedence_reserved() {
        assert_eq!(
            Precedence::try_from_u8(13),
            Err(DomainError::InvalidDiscriminant(13))
        );
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
            Color::Roseus,
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
