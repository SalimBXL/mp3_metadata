//! Vérification des métadonnées d'un tag ID3v2 auprès de MusicBrainz.
//!
//! Cette fonctionnalité fait une requête réseau vers
//! `https://musicbrainz.org/ws/2/recording`, l'API de recherche
//! d'enregistrements de MusicBrainz, à partir du titre et de l'artiste lus
//! dans le tag local, et compare le résultat le plus pertinent aux valeurs
//! locales.
//!
//! # Portée volontairement limitée
//!
//! - La comparaison de texte est insensible à la casse, mais uniquement
//!   pour les caractères ASCII (`eq_ignore_ascii_case`) : `"café"` et
//!   `"CAFÉ"` sont considérés différents. Un repli Unicode complet
//!   demanderait une dépendance supplémentaire (`unicase` ou équivalent),
//!   jugée disproportionnée pour ce besoin.
//! - La requête Lucene envoyée à MusicBrainz n'échappe que les guillemets
//!   du titre et de l'artiste, pas l'ensemble des caractères spéciaux de
//!   la syntaxe Lucene (`+ - && || ! ( ) { } [ ] ^ ~ * ? : \`). Un titre
//!   contenant l'un de ces caractères peut donner une requête mal formée
//!   et donc [`VerifyError::NoMatch`] plutôt qu'une vraie erreur réseau.
//! - Un même titre correspond souvent à plusieurs enregistrements
//!   MusicBrainz distincts (version studio, live, remaster, session
//!   radio...), parfois avec un score de pertinence textuelle identique
//!   entre eux. [`verify_tag`] ne tranche pas seul entre eux : il renvoie
//!   un rapport par candidat renvoyé par la recherche (jusqu'à `limit`
//!   résultats, voir [`verify_tag`]), triés
//!   — voir [`sort_recordings`] — à charge pour l'appelant de choisir.
//! - Un enregistrement MusicBrainz peut être associé à plusieurs éditions
//!   (`releases`), chacune avec son propre titre d'album et sa propre
//!   date — un morceau souvent réédité peut en avoir des dizaines.
//!   [`best_matching_release`] retient celle dont le titre correspond à
//!   l'album du tag local s'il y en a une, et ne retombe sur la première
//!   de la liste qu'à défaut (ou si le tag local n'a pas d'album) : rien
//!   ne garantit alors sa pertinence.
//!
//! # Étiquette de l'API
//!
//! MusicBrainz demande un en-tête `User-Agent` identifiant l'application
//! (voir <https://musicbrainz.org/doc/MusicBrainz_API/Rate_Limiting>) et
//! limite à environ une requête par seconde par application. Cette
//! fonction n'en fait qu'une par appel ; en boucle sur plusieurs fichiers,
//! il faudrait ajouter une pause entre les appels.

use crate::id3::header::Id3v2Tag;
use serde::Deserialize;
use std::fmt;

const MUSICBRAINZ_SEARCH_URL: &str = "https://musicbrainz.org/ws/2/recording";

/// Nombre de résultats demandés à MusicBrainz par défaut lorsque
/// l'appelant n'en précise pas d'autre — voir [`verify_tag`]. MusicBrainz
/// peut appliquer son propre plafond au-delà d'une certaine valeur, non
/// vérifié ici.
pub const DEFAULT_SEARCH_LIMIT: u32 = 20;

/// Identifie l'application auprès de MusicBrainz, comme leur étiquette
/// d'utilisation le demande.
const USER_AGENT: &str = concat!(
    "mp3_metadata/",
    env!("CARGO_PKG_VERSION"),
    " ( https://github.com/SalimBXL/mp3_metadata )"
);

/// Erreur survenue lors de la vérification en ligne d'un tag.
///
/// Distincte de [`crate::Mp3Error`] : celle-ci ne concerne que le réseau et
/// la réponse de MusicBrainz, jamais la lecture ou le décodage du fichier
/// MP3 lui-même.
#[derive(Debug)]
pub enum VerifyError {
    /// Le tag local ne porte ni titre ni artiste : rien d'assez précis à
    /// chercher sur MusicBrainz.
    NothingToSearch,
    /// Erreur réseau, ou statut HTTP d'erreur renvoyé par MusicBrainz.
    Request(Box<ureq::Error>),
    /// La réponse a été reçue mais son corps JSON n'a pas pu être lu dans
    /// la forme attendue.
    Response(std::io::Error),
    /// La recherche n'a renvoyé aucun enregistrement.
    NoMatch,
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VerifyError::NothingToSearch => {
                write!(
                    f,
                    "le tag local ne contient ni titre ni artiste : rien à vérifier"
                )
            }
            VerifyError::Request(err) => {
                write!(f, "requête à MusicBrainz échouée : {err}")
            }
            VerifyError::Response(err) => {
                write!(f, "réponse MusicBrainz illisible : {err}")
            }
            VerifyError::NoMatch => {
                write!(
                    f,
                    "aucun enregistrement correspondant trouvé sur MusicBrainz"
                )
            }
        }
    }
}

impl std::error::Error for VerifyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            VerifyError::Request(err) => Some(err),
            VerifyError::Response(err) => Some(err),
            VerifyError::NothingToSearch | VerifyError::NoMatch => None,
        }
    }
}

impl From<ureq::Error> for VerifyError {
    fn from(err: ureq::Error) -> Self {
        VerifyError::Request(Box::new(err))
    }
}

impl From<std::io::Error> for VerifyError {
    fn from(err: std::io::Error) -> Self {
        VerifyError::Response(err)
    }
}

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
    /// Score de pertinence renvoyé par la recherche (0 à 100).
    pub score: Option<u32>,
    pub title: FieldMatch,
    pub artist: FieldMatch,
    /// `None` si l'enregistrement distant n'est associé à aucune édition.
    pub album: Option<FieldMatch>,
    /// `None` si l'enregistrement distant n'est associé à aucune édition
    /// datée.
    pub year: Option<FieldMatch>,
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

impl fmt::Display for VerificationTable<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let reports = self.0;
        if reports.is_empty() {
            return Ok(());
        }

        let headers = ["#", "Score", "Titre", "Artiste", "Album", "Année"];
        let rows: Vec<[String; 6]> = reports
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
                ]
            })
            .collect();

        // Largeur de chaque colonne = la plus longue valeur qu'elle
        // contient, en-tête compris. Compté en caractères (`chars`), pas
        // en octets : les accents et les symboles ✓/✗/· tiennent sur
        // plusieurs octets en UTF-8 mais un seul caractère affiché.
        let widths: Vec<usize> = (0..headers.len())
            .map(|col| {
                rows.iter()
                    .map(|row| row[col].chars().count())
                    .chain(std::iter::once(headers[col].chars().count()))
                    .max()
                    .unwrap_or(0)
            })
            .collect();

        for (col, header) in headers.iter().enumerate() {
            write!(f, "{header:<width$}  ", width = widths[col])?;
        }
        writeln!(f)?;

        for &width in &widths {
            write!(f, "{}  ", "-".repeat(width))?;
        }
        writeln!(f)?;

        for row in &rows {
            for (col, cell) in row.iter().enumerate() {
                write!(f, "{cell:<width$}  ", width = widths[col])?;
            }
            writeln!(f)?;
        }

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
}

/// Vérifie le titre, l'artiste, l'album et l'année d'un tag ID3v2 auprès de
/// MusicBrainz.
///
/// Cherche par titre et/ou artiste (au moins l'un des deux doit être
/// présent dans le tag local) et renvoie un rapport par enregistrement
/// candidat renvoyé par la recherche (jusqu'à `limit` résultats), trié —
/// voir [`sort_recordings`]. S'il n'y en a qu'un, le `Vec` renvoyé n'a
/// qu'un élément. [`DEFAULT_SEARCH_LIMIT`] est une valeur par défaut
/// raisonnable si l'appelant n'a pas de préférence.
///
/// # Erreurs
///
/// - [`VerifyError::NothingToSearch`] si le tag local n'a ni titre ni
///   artiste.
/// - [`VerifyError::Request`] pour toute erreur réseau ou tout statut HTTP
///   d'erreur renvoyé par MusicBrainz.
/// - [`VerifyError::Response`] si le corps de la réponse ne peut pas être
///   lu dans la forme JSON attendue.
/// - [`VerifyError::NoMatch`] si la recherche ne renvoie aucun résultat.
pub fn verify_tag(tag: &Id3v2Tag, limit: u32) -> Result<Vec<VerificationReport>, VerifyError> {
    let title = tag.title();
    let artist = tag.artist();

    if title.is_none() && artist.is_none() {
        return Err(VerifyError::NothingToSearch);
    }

    let query = build_query(title, artist);
    let limit_str = limit.to_string();

    let response: SearchResponse = ureq::get(MUSICBRAINZ_SEARCH_URL)
        .set("User-Agent", USER_AGENT)
        .query("query", &query)
        .query("fmt", "json")
        .query("limit", &limit_str)
        .call()?
        .into_json()?;

    let mut recordings = response.recordings;
    if recordings.is_empty() {
        return Err(VerifyError::NoMatch);
    }

    sort_recordings(tag, &mut recordings);

    Ok(recordings
        .iter()
        .map(|recording| build_report(tag, recording))
        .collect())
}

/// Trie les enregistrements renvoyés par la recherche : score de
/// pertinence MusicBrainz décroissant en clé principale, puis — pour
/// départager les égalités de score de façon stable et prévisible plutôt
/// qu'arbitrairement (un même titre correspond souvent à plusieurs
/// enregistrements distincts : version studio, live, remaster, session
/// radio...) — artiste, titre, année et album en ordre alphabétique
/// croissant. L'année et l'album considérés sont ceux de l'édition
/// choisie par [`best_matching_release`] pour cet enregistrement — la même
/// que celle utilisée pour construire son [`VerificationReport`], pour que
/// le tri et l'affichage restent cohérents entre eux.
fn sort_recordings(tag: &Id3v2Tag, recordings: &mut [RemoteRecording]) {
    recordings.sort_by_key(|recording| recording_sort_key(tag, recording));
}

/// Clé de tri d'un enregistrement — voir [`sort_recordings`]. Le score est
/// enveloppé dans [`std::cmp::Reverse`] pour trier décroissant tout en
/// gardant les autres champs croissants dans le même tuple.
fn recording_sort_key(
    tag: &Id3v2Tag,
    recording: &RemoteRecording,
) -> (std::cmp::Reverse<u32>, String, String, String, String) {
    let release = best_matching_release(tag, &recording.releases);
    let year = release
        .and_then(|release| release.date.as_deref())
        .map(release_year)
        .unwrap_or("")
        .to_string();
    let album = release
        .map(|release| release.title.clone())
        .unwrap_or_default();

    (
        std::cmp::Reverse(recording.score.unwrap_or(0)),
        recording.artist(),
        recording.title.clone(),
        year,
        album,
    )
}

/// Construit la requête Lucene envoyée à MusicBrainz à partir du titre
/// et/ou de l'artiste locaux — voir les limites de l'échappement en tête
/// de module.
fn build_query(title: Option<&str>, artist: Option<&str>) -> String {
    let mut parts = Vec::new();
    if let Some(title) = title {
        parts.push(format!("recording:{}", quote_lucene(title)));
    }
    if let Some(artist) = artist {
        parts.push(format!("artist:{}", quote_lucene(artist)));
    }
    parts.join(" AND ")
}

/// Met `value` entre guillemets pour Lucene, en échappant les guillemets
/// qu'elle contiendrait déjà. N'échappe rien d'autre — voir les limites en
/// tête de module.
fn quote_lucene(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\\\""))
}

fn build_report(tag: &Id3v2Tag, remote: &RemoteRecording) -> VerificationReport {
    let release = best_matching_release(tag, &remote.releases);

    VerificationReport {
        recording_id: remote.id.clone(),
        score: remote.score,
        title: compare_field(tag.title(), &remote.title),
        artist: compare_field(tag.artist(), &remote.artist()),
        album: release.map(|release| compare_field(tag.album(), &release.title)),
        year: release
            .and_then(|release| release.date.as_deref())
            .map(|date| compare_field(tag.year(), release_year(date))),
    }
}

/// Choisit l'édition la plus pertinente pour comparer l'album et l'année.
///
/// Une égalité stricte échoue trop souvent en pratique : MusicBrainz et un
/// tag local ne nomment pas toujours une édition de la même façon (ex.
/// `"Greatest Hits Vol.2"` localement contre `"Greatest Hits II"` chez
/// MusicBrainz — aucun des deux n'est faux, ce sont deux conventions de
/// nommage différentes pour la même édition). On compare donc les deux
/// titres par recouvrement de mots (voir [`word_overlap`]) et on retient
/// l'édition avec le plus grand recouvrement, à condition qu'il soit non
/// nul. Sans album local, sans mot en commun avec aucune édition, ou sans
/// édition du tout, on retombe sur la première renvoyée par l'API — sans
/// garantie de pertinence dans ce cas.
fn best_matching_release<'a>(
    tag: &Id3v2Tag,
    releases: &'a [RemoteRelease],
) -> Option<&'a RemoteRelease> {
    let Some(local_album) = tag.album() else {
        return releases.first();
    };

    let local_words = normalize_words(local_album);
    if local_words.is_empty() {
        return releases.first();
    }

    releases
        .iter()
        .map(|release| {
            (
                release,
                word_overlap(&local_words, &normalize_words(&release.title)),
            )
        })
        .max_by_key(|(_, score)| *score)
        .filter(|(_, score)| *score > 0)
        .map(|(release, _)| release)
        .or_else(|| releases.first())
}

/// Découpe une chaîne en mots alphanumériques, en minuscules ASCII (même
/// limite que [`compare_field`] : les accents ne sont pas repliés).
fn normalize_words(s: &str) -> std::collections::HashSet<String> {
    s.to_ascii_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect()
}

/// Nombre de mots communs entre deux ensembles déjà normalisés par
/// [`normalize_words`].
fn word_overlap(
    a: &std::collections::HashSet<String>,
    b: &std::collections::HashSet<String>,
) -> usize {
    a.intersection(b).count()
}

/// Extrait l'année d'une date MusicBrainz (`"1991-05-27"`, `"1991-05"` ou
/// `"1991"`), qui peut être partielle.
fn release_year(date: &str) -> &str {
    date.split('-').next().unwrap_or(date)
}

fn compare_field(local: Option<&str>, remote: &str) -> FieldMatch {
    match local {
        Some(local) if local.eq_ignore_ascii_case(remote) => FieldMatch::Match,
        Some(_) => FieldMatch::Mismatch {
            remote: remote.to_string(),
        },
        None => FieldMatch::LocalMissing {
            remote: remote.to_string(),
        },
    }
}

//
// ---------- Réponse JSON de l'API de recherche MusicBrainz ----------
//

#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    recordings: Vec<RemoteRecording>,
}

#[derive(Debug, Deserialize)]
struct RemoteRecording {
    id: String,
    #[serde(default)]
    score: Option<u32>,
    title: String,
    #[serde(rename = "artist-credit", default)]
    artist_credit: Vec<ArtistCredit>,
    #[serde(default)]
    releases: Vec<RemoteRelease>,
}

impl RemoteRecording {
    /// Joint les crédits d'artiste par un espace. MusicBrainz fournit en
    /// réalité un `joinphrase` par crédit pour un rendu exact (ex.
    /// `"A feat. B"`) ; cette version plus simple ne l'utilise pas.
    fn artist(&self) -> String {
        self.artist_credit
            .iter()
            .map(|credit| credit.name.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[derive(Debug, Deserialize)]
struct ArtistCredit {
    name: String,
}

#[derive(Debug, Deserialize)]
struct RemoteRelease {
    title: String,
    #[serde(default)]
    date: Option<String>,
}

//
// ---------- TESTS ----------
//
// Uniquement la logique pure (construction de requête, comparaison) : un
// test qui interrogerait le vrai MusicBrainz serait lent, dépendant du
// réseau, et fragile en CI.
//

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id3::frame::{Frame, FrameContent};
    use crate::id3::header::Id3Version;

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

    // ----- quote_lucene / build_query -----

    #[test]
    fn test_quote_lucene_escapes_embedded_quotes() {
        assert_eq!(quote_lucene(r#"He said "hi""#), r#""He said \"hi\"""#);
    }

    #[test]
    fn test_quote_lucene_plain_value() {
        assert_eq!(quote_lucene("Queen"), r#""Queen""#);
    }

    #[test]
    fn test_build_query_both_fields() {
        assert_eq!(
            build_query(Some("A Kind of Magic"), Some("Queen")),
            r#"recording:"A Kind of Magic" AND artist:"Queen""#
        );
    }

    #[test]
    fn test_build_query_title_only() {
        assert_eq!(build_query(Some("Hello"), None), r#"recording:"Hello""#);
    }

    #[test]
    fn test_build_query_artist_only() {
        assert_eq!(build_query(None, Some("Queen")), r#"artist:"Queen""#);
    }

    // ----- release_year -----

    #[test]
    fn test_release_year_full_date() {
        assert_eq!(release_year("1991-05-27"), "1991");
    }

    #[test]
    fn test_release_year_partial_date() {
        assert_eq!(release_year("1991"), "1991");
    }

    // ----- compare_field -----

    #[test]
    fn test_compare_field_match_is_ascii_case_insensitive() {
        assert_eq!(compare_field(Some("queen"), "Queen"), FieldMatch::Match);
    }

    #[test]
    fn test_compare_field_mismatch() {
        assert_eq!(
            compare_field(Some("The Beatles"), "Queen"),
            FieldMatch::Mismatch {
                remote: "Queen".to_string()
            }
        );
    }

    #[test]
    fn test_compare_field_local_missing() {
        assert_eq!(
            compare_field(None, "Queen"),
            FieldMatch::LocalMissing {
                remote: "Queen".to_string()
            }
        );
    }

    // ----- RemoteRecording::artist -----

    #[test]
    fn test_remote_recording_artist_joins_multiple_credits() {
        let recording = RemoteRecording {
            id: "x".to_string(),
            score: None,
            title: "T".to_string(),
            artist_credit: vec![
                ArtistCredit {
                    name: "Artist A".to_string(),
                },
                ArtistCredit {
                    name: "Artist B".to_string(),
                },
            ],
            releases: vec![],
        };
        assert_eq!(recording.artist(), "Artist A Artist B");
    }

    // ----- build_report -----

    /// Construit un tag local minimal directement (sans passer par les
    /// octets bruts) : seuls les champs texte utilisés par `build_report`
    /// nous intéressent ici.
    fn sample_local_tag(fields: &[(&[u8; 4], &str)]) -> Id3v2Tag {
        Id3v2Tag {
            version: Id3Version { major: 3, minor: 0 },
            flags: 0,
            size: 0,
            frames: fields
                .iter()
                .map(|(id, value)| Frame {
                    id: **id,
                    size: 0,
                    flags: 0,
                    content: FrameContent::Text(vec![value.to_string()]),
                    offset: 0,
                    next_offset: 0,
                })
                .collect(),
        }
    }

    fn sample_remote_recording() -> RemoteRecording {
        RemoteRecording {
            id: "abc-123".to_string(),
            score: Some(100),
            title: "A Kind of Magic".to_string(),
            artist_credit: vec![ArtistCredit {
                name: "Queen".to_string(),
            }],
            releases: vec![RemoteRelease {
                title: "Greatest Hits".to_string(),
                date: Some("1991-01-01".to_string()),
            }],
        }
    }

    #[test]
    fn test_build_report_all_fields_match() {
        let tag = sample_local_tag(&[
            (b"TIT2", "A Kind of Magic"),
            (b"TPE1", "Queen"),
            (b"TALB", "Greatest Hits"),
            (b"TYER", "1991"),
        ]);
        let report = build_report(&tag, &sample_remote_recording());

        assert_eq!(report.title, FieldMatch::Match);
        assert_eq!(report.artist, FieldMatch::Match);
        assert_eq!(report.album, Some(FieldMatch::Match));
        assert_eq!(report.year, Some(FieldMatch::Match));
    }

    #[test]
    fn test_build_report_year_compares_only_the_year_part_of_the_date() {
        let tag = sample_local_tag(&[(b"TYER", "1991")]);
        let report = build_report(&tag, &sample_remote_recording());

        // La date distante est "1991-01-01" ; seule l'année doit compter.
        assert_eq!(report.year, Some(FieldMatch::Match));
    }

    #[test]
    fn test_build_report_detects_mismatch() {
        let tag = sample_local_tag(&[(b"TIT2", "Une Autre Chanson")]);
        let report = build_report(&tag, &sample_remote_recording());

        assert_eq!(
            report.title,
            FieldMatch::Mismatch {
                remote: "A Kind of Magic".to_string()
            }
        );
    }

    #[test]
    fn test_build_report_no_release_means_no_album_or_year() {
        let tag = sample_local_tag(&[(b"TIT2", "A Kind of Magic")]);
        let mut remote = sample_remote_recording();
        remote.releases.clear();

        let report = build_report(&tag, &remote);

        assert_eq!(report.album, None);
        assert_eq!(report.year, None);
    }

    // ----- normalize_words / word_overlap -----

    #[test]
    fn test_normalize_words_splits_on_punctuation_and_lowercases() {
        assert_eq!(
            normalize_words("Greatest Hits Vol.2"),
            ["greatest", "hits", "vol", "2"]
                .into_iter()
                .map(String::from)
                .collect()
        );
    }

    #[test]
    fn test_normalize_words_empty_for_punctuation_only() {
        assert!(normalize_words("...").is_empty());
    }

    #[test]
    fn test_word_overlap_counts_shared_words() {
        let a = normalize_words("Greatest Hits Vol.2");
        let b = normalize_words("Greatest Hits II");
        assert_eq!(word_overlap(&a, &b), 2); // "greatest", "hits"
    }

    #[test]
    fn test_word_overlap_zero_when_unrelated() {
        let a = normalize_words("Greatest Hits Vol.2");
        let b = normalize_words("Big in Japan");
        assert_eq!(word_overlap(&a, &b), 0);
    }

    // ----- sort_recordings / recording_sort_key -----

    fn recording(id: &str, score: u32, release_title: &str) -> RemoteRecording {
        RemoteRecording {
            id: id.to_string(),
            score: Some(score),
            title: "A Kind of Magic".to_string(),
            artist_credit: vec![ArtistCredit {
                name: "Queen".to_string(),
            }],
            releases: vec![RemoteRelease {
                title: release_title.to_string(),
                date: None,
            }],
        }
    }

    #[test]
    fn test_sort_recordings_orders_by_score_descending_first() {
        let tag = sample_local_tag(&[]);
        let mut recordings = [
            recording("low", 60, "Tribute Album"),
            recording("high", 100, "On Air"),
        ];

        sort_recordings(&tag, &mut recordings);

        assert_eq!(recordings[0].id, "high");
        assert_eq!(recordings[1].id, "low");
    }

    #[test]
    fn test_sort_recordings_breaks_score_ties_alphabetically_by_artist() {
        let tag = sample_local_tag(&[]);
        let mut b_artist = recording("b", 100, "On Air");
        b_artist.artist_credit = vec![ArtistCredit {
            name: "Bee Artist".to_string(),
        }];
        let mut a_artist = recording("a", 100, "On Air");
        a_artist.artist_credit = vec![ArtistCredit {
            name: "Aardvark Artist".to_string(),
        }];
        let mut recordings = [b_artist, a_artist];

        sort_recordings(&tag, &mut recordings);

        assert_eq!(recordings[0].id, "a"); // "Aardvark..." < "Bee..."
        assert_eq!(recordings[1].id, "b");
    }

    #[test]
    fn test_sort_recordings_breaks_remaining_ties_by_title_then_year_then_album() {
        let tag = sample_local_tag(&[]);

        let mut older = recording("older", 100, "Album");
        older.releases[0].date = Some("1990-01-01".to_string());
        let mut newer = recording("newer", 100, "Album");
        newer.releases[0].date = Some("2000-01-01".to_string());
        // Même score, même artiste, même titre : seule l'année diffère.
        let mut recordings = [newer, older];

        sort_recordings(&tag, &mut recordings);

        assert_eq!(recordings[0].id, "older"); // "1990" < "2000"
        assert_eq!(recordings[1].id, "newer");
    }

    #[test]
    fn test_sort_recordings_stable_order_when_everything_ties() {
        let tag = sample_local_tag(&[]);
        let mut recordings = [
            recording("first", 100, "Same Album"),
            recording("second", 100, "Same Album"),
        ];

        sort_recordings(&tag, &mut recordings);

        // Rien ne les distingue : l'ordre d'origine est préservé (tri
        // stable), pas une erreur.
        assert_eq!(recordings[0].id, "first");
        assert_eq!(recordings[1].id, "second");
    }

    // ----- best_matching_release -----

    fn releases_greatest_hits_ii_and_big_in_japan() -> Vec<RemoteRelease> {
        vec![
            RemoteRelease {
                title: "Big in Japan".to_string(),
                date: Some("1994-05-01".to_string()),
            },
            RemoteRelease {
                title: "Greatest Hits II".to_string(),
                date: Some("1991-10-28".to_string()),
            },
        ]
    }

    #[test]
    fn test_best_matching_release_exact_match_wins() {
        let tag = sample_local_tag(&[(b"TALB", "Greatest Hits II")]);
        let releases = releases_greatest_hits_ii_and_big_in_japan();

        let release = best_matching_release(&tag, &releases).unwrap();

        assert_eq!(release.title, "Greatest Hits II");
        assert_eq!(release.date.as_deref(), Some("1991-10-28"));
    }

    #[test]
    fn test_best_matching_release_finds_overlap_despite_different_naming() {
        // Régression concrète : le tag local dit "Greatest Hits Vol.2",
        // MusicBrainz dit "Greatest Hits II" — aucune correspondance
        // exacte, mais un net recouvrement ("greatest", "hits") face à
        // zéro recouvrement avec "Big in Japan".
        let tag = sample_local_tag(&[(b"TALB", "Greatest Hits Vol.2")]);
        let releases = releases_greatest_hits_ii_and_big_in_japan();

        let release = best_matching_release(&tag, &releases).unwrap();

        assert_eq!(release.title, "Greatest Hits II");
    }

    #[test]
    fn test_best_matching_release_match_is_ascii_case_insensitive() {
        let tag = sample_local_tag(&[(b"TALB", "greatest hits vol.2")]);
        let releases = releases_greatest_hits_ii_and_big_in_japan();

        let release = best_matching_release(&tag, &releases).unwrap();

        assert_eq!(release.title, "Greatest Hits II");
    }

    #[test]
    fn test_best_matching_release_falls_back_to_first_when_no_word_overlap() {
        let tag = sample_local_tag(&[(b"TALB", "Something Else Entirely")]);
        let releases = releases_greatest_hits_ii_and_big_in_japan();

        let release = best_matching_release(&tag, &releases).unwrap();

        assert_eq!(release.title, "Big in Japan"); // la première, faute de mieux
    }

    #[test]
    fn test_best_matching_release_falls_back_to_first_when_local_has_no_album() {
        let tag = sample_local_tag(&[(b"TIT2", "A Kind of Magic")]); // pas de TALB
        let releases = releases_greatest_hits_ii_and_big_in_japan();

        let release = best_matching_release(&tag, &releases).unwrap();

        assert_eq!(release.title, "Big in Japan");
    }

    #[test]
    fn test_best_matching_release_falls_back_to_first_when_local_album_is_only_punctuation() {
        let tag = sample_local_tag(&[(b"TALB", "...")]);
        let releases = releases_greatest_hits_ii_and_big_in_japan();

        let release = best_matching_release(&tag, &releases).unwrap();

        assert_eq!(release.title, "Big in Japan");
    }

    #[test]
    fn test_best_matching_release_none_when_no_releases() {
        let tag = sample_local_tag(&[(b"TALB", "Greatest Hits II")]);
        assert!(best_matching_release(&tag, &[]).is_none());
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
}
