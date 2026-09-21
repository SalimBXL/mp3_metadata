//! Représentation et affichage d'un rapport de vérification MusicBrainz :
//! comparaison champ par champ ([`FieldMatch`], [`VerificationReport`]) et
//! vue tabulaire de plusieurs rapports ([`VerificationTable`]).
//!
//! Module purement présentationnel, sans accès réseau : les rapports
//! qu'il affiche sont construits ailleurs (voir [`super::verify_tag`]).

use std::fmt;

/// Résultat d'une comparaison entre un champ du tag local et la valeur
/// correspondante renvoyée par MusicBrainz.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldMatch {
    /// Les deux valeurs coïncident (comparaison ASCII insensible à la
    /// casse — voir la portée limitée en tête de module).
    Match,
    /// Les deux valeurs diffèrent.
    Mismatch { remote: String },
    /// Le tag local n'a pas ce champ ; rien à comparer, mais MusicBrainz
    /// propose une valeur.
    LocalMissing { remote: String },
}
impl fmt::Display for FieldMatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FieldMatch::Match => write!(f, "concorde"),
            FieldMatch::Mismatch { remote } => {
                write!(f, "diffère (MusicBrainz : \"{remote}\")")
            }
            FieldMatch::LocalMissing { remote } => {
                write!(f, "absent localement (MusicBrainz : \"{remote}\")")
            }
        }
    }
}
impl FieldMatch {
    /// Représentation compacte pour une cellule de [`VerificationTable`] :
    /// une simple coche en cas de concordance (la valeur locale est déjà
    /// visible ailleurs), sinon la valeur MusicBrainz précédée d'un
    /// symbole.
    fn cell(&self) -> String {
        match self {
            FieldMatch::Match => "✓".to_string(),
            FieldMatch::Mismatch { remote } => format!("✗ {remote}"),
            FieldMatch::LocalMissing { remote } => format!("· {remote}"),
        }
    }
}
/// Résultat de la vérification d'un tag auprès de MusicBrainz : l'
/// enregistrement retenu, et la comparaison champ par champ.
#[derive(Debug, Clone)]
pub struct VerificationReport {
    /// Identifiant MusicBrainz (MBID) de l'enregistrement retenu.
    pub recording_id: String,
    /// Score de pertinence renvoyé par la recherche (0 à 100). Pour un
    /// rapport trouvé par album (voir [`VerificationReport::via_album`]),
    /// c'est le score de la recherche d'édition, pas d'enregistrement —
    /// les deux ne sont pas directement comparables entre eux.
    pub score: Option<u32>,
    /// Comparaison du titre local à celui de l'enregistrement distant.
    pub title: FieldMatch,
    /// Comparaison de l'artiste local à celui de l'enregistrement distant.
    pub artist: FieldMatch,
    /// `None` si l'enregistrement distant n'est associé à aucune édition.
    pub album: Option<FieldMatch>,
    /// `None` si l'enregistrement distant n'est associé à aucune édition
    /// datée.
    pub year: Option<FieldMatch>,
    /// `true` si ce rapport vient de la recherche par album
    /// ([`album::search_by_album`]) plutôt que de la recherche par morceau
    /// habituelle — voir la stratégie combinée en tête de module. Un
    /// rapport par album a cherché directement l'édition locale, donc son
    /// champ `album` concorde presque toujours (voir
    /// [`album::build_album_report`]) ; l'information utile y est surtout dans
    /// `title` et `year`.
    pub via_album: bool,
}
impl fmt::Display for VerificationReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let score = self
            .score
            .map(|s| s.to_string())
            .unwrap_or_else(|| "?".to_string());

        writeln!(f, "- VÉRIFICATION MUSICBRAINZ -----")?;
        writeln!(f, "Enregistrement : {} (score {score})", self.recording_id)?;
        writeln!(
            f,
            "Lien           : https://musicbrainz.org/recording/{}",
            self.recording_id
        )?;
        if self.via_album {
            writeln!(f, "Trouvé via     : recherche par album")?;
        }
        writeln!(f, "Titre   : {}", self.title)?;
        writeln!(f, "Artiste : {}", self.artist)?;
        if let Some(album) = &self.album {
            writeln!(f, "Album   : {album}")?;
        }
        if let Some(year) = &self.year {
            writeln!(f, "Année   : {year}")?;
        }
        write!(f, "-------------------------------")
    }
}
/// Vue tabulaire de plusieurs rapports de vérification : une ligne par
/// enregistrement candidat, une colonne par champ comparé.
///
/// Plus lisible que plusieurs [`VerificationReport`] affichés à la suite
/// quand MusicBrainz renvoie plusieurs candidats à égalité de score (voir
/// [`verify_tag`]) — les colonnes s'alignent, les différences sautent aux
/// yeux. Les liens MusicBrainz, trop longs pour tenir dans une colonne,
/// sont listés séparément après le tableau.
pub struct VerificationTable<'a>(pub &'a [VerificationReport]);
/// En-têtes de colonnes de [`VerificationTable`] — voir [`build_rows`] et
/// [`column_widths`], qui s'y accordent (même ordre, même nombre de
/// colonnes).
const TABLE_HEADERS: [&str; 7] = ["#", "Score", "Titre", "Artiste", "Album", "Année", "Voie"];
impl fmt::Display for VerificationTable<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let reports = self.0;
        if reports.is_empty() {
            return Ok(());
        }

        let rows = build_rows(reports);
        let widths = column_widths(&TABLE_HEADERS, &rows);

        write_row(f, &TABLE_HEADERS, &widths)?;
        write_separator(f, &widths)?;
        for row in &rows {
            write_row(f, row, &widths)?;
        }

        write_links(f, reports)
    }
}
/// Construit une ligne de tableau par rapport, dans l'ordre de
/// [`TABLE_HEADERS`].
fn build_rows(reports: &[VerificationReport]) -> Vec<[String; 7]> {
    reports
        .iter()
        .enumerate()
        .map(|(index, report)| {
            [
                (index + 1).to_string(),
                report
                    .score
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "?".to_string()),
                report.title.cell(),
                report.artist.cell(),
                report
                    .album
                    .as_ref()
                    .map(FieldMatch::cell)
                    .unwrap_or_else(|| "—".to_string()),
                report
                    .year
                    .as_ref()
                    .map(FieldMatch::cell)
                    .unwrap_or_else(|| "—".to_string()),
                if report.via_album { "Album" } else { "Titre" }.to_string(),
            ]
        })
        .collect()
}
/// Largeur de chaque colonne : la plus longue valeur qu'elle contient,
/// en-tête compris. Compté en caractères (`chars`), pas en octets : les
/// accents et les symboles ✓/✗/· tiennent sur plusieurs octets en UTF-8
/// mais un seul caractère affiché.
fn column_widths(headers: &[&str; 7], rows: &[[String; 7]]) -> [usize; 7] {
    let mut widths = [0usize; 7];
    for (col, width) in widths.iter_mut().enumerate() {
        *width = rows
            .iter()
            .map(|row| row[col].chars().count())
            .chain(std::iter::once(headers[col].chars().count()))
            .max()
            .unwrap_or(0);
    }
    widths
}
/// Écrit une ligne du tableau (en-tête ou ligne de données), chaque
/// cellule alignée à gauche sur la largeur de sa colonne (voir
/// [`column_widths`]).
fn write_row<T: AsRef<str>>(
    f: &mut fmt::Formatter<'_>,
    cells: &[T; 7],
    widths: &[usize; 7],
) -> fmt::Result {
    for (col, cell) in cells.iter().enumerate() {
        write!(f, "{:<width$}  ", cell.as_ref(), width = widths[col])?;
    }
    writeln!(f)
}
/// Écrit la ligne de tirets séparant l'en-tête des lignes de données.
fn write_separator(f: &mut fmt::Formatter<'_>, widths: &[usize; 7]) -> fmt::Result {
    for &width in widths {
        write!(f, "{}  ", "-".repeat(width))?;
    }
    writeln!(f)
}
/// Écrit la liste des liens MusicBrainz vers chaque enregistrement
/// candidat, un par ligne, après le tableau — trop longs pour tenir dans
/// une colonne (voir la doc de [`VerificationTable`]).
fn write_links(f: &mut fmt::Formatter<'_>, reports: &[VerificationReport]) -> fmt::Result {
    writeln!(f)?;
    writeln!(f, "Liens :")?;
    for (index, report) in reports.iter().enumerate() {
        writeln!(
            f,
            "{:>2}. https://musicbrainz.org/recording/{}",
            index + 1,
            report.recording_id
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ----- FieldMatch::cell -----

    #[test]
    fn test_field_match_cell_match_is_a_checkmark() {
        assert_eq!(FieldMatch::Match.cell(), "✓");
    }

    #[test]
    fn test_field_match_cell_mismatch_shows_remote_value() {
        assert_eq!(
            FieldMatch::Mismatch {
                remote: "Nihon".to_string()
            }
            .cell(),
            "✗ Nihon"
        );
    }

    #[test]
    fn test_field_match_cell_local_missing_shows_remote_value() {
        assert_eq!(
            FieldMatch::LocalMissing {
                remote: "Nihon".to_string()
            }
            .cell(),
            "· Nihon"
        );
    }

    // ----- VerificationTable -----

    fn sample_report(recording_id: &str, album_remote: &str) -> VerificationReport {
        VerificationReport {
            recording_id: recording_id.to_string(),
            score: Some(100),
            title: FieldMatch::Match,
            artist: FieldMatch::Match,
            album: Some(FieldMatch::Mismatch {
                remote: album_remote.to_string(),
            }),
            year: Some(FieldMatch::Match),
            via_album: false,
        }
    }

    #[test]
    fn test_verification_table_empty_is_empty_string() {
        assert_eq!(VerificationTable(&[]).to_string(), "");
    }

    /// Position, en nombre de *caractères* (pas d'octets — ✓/✗/· sont
    /// multi-octets en UTF-8, ce qui fausserait la comparaison entre deux
    /// chaînes n'en contenant pas le même nombre avant le point cherché).
    fn char_index_of(haystack: &str, needle: &str) -> Option<usize> {
        let byte_index = haystack.find(needle)?;
        Some(haystack[..byte_index].chars().count())
    }

    #[test]
    fn test_verification_table_header_and_row_columns_align() {
        let reports = [sample_report("abc-123", "Nihon")];
        let table = VerificationTable(&reports).to_string();
        let mut lines = table.lines();

        let header = lines.next().unwrap();
        let separator = lines.next().unwrap();
        let row = lines.next().unwrap();

        // Chaque colonne d'en-tête doit démarrer exactement à la même
        // position que la colonne correspondante de la ligne de données,
        // et le séparateur doit avoir la même longueur que l'en-tête.
        assert_eq!(
            char_index_of(header, "Album"),
            char_index_of(row, "✗ Nihon")
        );
        assert_eq!(header.chars().count(), separator.chars().count());
    }

    #[test]
    fn test_verification_table_contains_one_link_per_report() {
        let reports = [
            sample_report("abc-123", "Nihon"),
            sample_report("def-456", "On Air"),
        ];
        let table = VerificationTable(&reports).to_string();

        assert!(table.contains("https://musicbrainz.org/recording/abc-123"));
        assert!(table.contains("https://musicbrainz.org/recording/def-456"));
    }

    #[test]
    fn test_verification_table_missing_album_shows_dash() {
        let mut report = sample_report("abc-123", "Nihon");
        report.album = None;
        let reports = [report];

        let table = VerificationTable(&reports).to_string();

        assert!(table.lines().nth(2).unwrap().contains('—'));
    }

    #[test]
    fn test_verification_table_marks_via_album_reports() {
        let mut by_album = sample_report("abc-123", "Nihon");
        by_album.via_album = true;
        let by_title = sample_report("def-456", "Nihon");
        let reports = [by_album, by_title];

        let table = VerificationTable(&reports).to_string();
        let mut lines = table.lines().skip(2); // en-tête + séparateur

        assert!(lines.next().unwrap().contains("Album"));
        assert!(lines.next().unwrap().contains("Titre"));
    }
}
