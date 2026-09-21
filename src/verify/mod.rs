//! Vérification des métadonnées d'un tag ID3v2 auprès de MusicBrainz.
//!
//! [`verify_tag`] combine deux stratégies de recherche complémentaires :
//!
//! - **Par morceau** (toujours tentée) : requête vers
//!   `https://musicbrainz.org/ws/2/recording`, l'API de recherche
//!   d'enregistrements de MusicBrainz, à partir du titre et de l'artiste
//!   lus dans le tag local, et compare le résultat le plus pertinent aux
//!   valeurs locales.
//! - **Par album** (tentée en plus si le tag local a un champ album —
//!   voir [`album::search_by_album`]) : requête vers
//!   `https://musicbrainz.org/ws/2/release`, cette fois à partir du titre
//!   d'album local, puis récupération de sa liste de pistes pour y
//!   retrouver le morceau local. Pensée pour les titres très repris en
//!   concert ou très réédités (voir plus bas), où la recherche par
//!   morceau seule échoue en pratique : des centaines d'enregistrements
//!   distincts (versions studio, live, remasters...) partagent alors le
//!   même score de pertinence maximal, sans qu'aucun signal ne permette
//!   de distinguer celui qui correspond réellement à l'édition locale —
//!   alors qu'une recherche directe sur le nom de l'édition n'a
//!   généralement affaire qu'à une poignée de candidats.
//!
//! Le rapport obtenu par album, s'il y en a un, est toujours placé en
//! tête de [`VerifyOutcome::reports`] : voir la documentation de
//! [`verify_tag`]. Vue tabulaire de plusieurs rapports : voir le
//! sous-module [`table`].
//!
//! # Portée volontairement limitée
//!
//! - La comparaison de texte est insensible à la casse, mais uniquement
//!   pour les caractères ASCII (`eq_ignore_ascii_case`) : `"café"` et
//!   `"CAFÉ"` sont considérés différents. Un repli Unicode complet
//!   demanderait une dépendance supplémentaire (`unicase` ou équivalent),
//!   jugée disproportionnée pour ce besoin.
//! - La requête Lucene envoyée à MusicBrainz n'échappe que les guillemets
//!   du titre, de l'artiste et de l'album, pas l'ensemble des caractères
//!   spéciaux de la syntaxe Lucene (`+ - && || ! ( ) { } [ ] ^ ~ * ? : \`).
//!   Un titre contenant l'un de ces caractères peut donner une requête mal
//!   formée et donc [`VerifyError::NoMatch`] plutôt qu'une vraie erreur
//!   réseau.
//! - Un même titre correspond souvent à plusieurs enregistrements
//!   MusicBrainz distincts (version studio, live, remaster, session
//!   radio...), parfois avec un score de pertinence textuelle identique
//!   entre eux — voir la recherche par album ci-dessus, pensée pour ce
//!   cas. La recherche par morceau seule ne tranche pas entre eux : elle
//!   renvoie un rapport par candidat renvoyé par la recherche (jusqu'à
//!   `limit` résultats, voir [`verify_tag`]), triés — voir
//!   [`sort_recordings`] — à charge pour l'appelant de choisir.
//! - Un enregistrement MusicBrainz peut être associé à plusieurs éditions
//!   (`releases`), chacune avec son propre titre d'album et sa propre
//!   date — un morceau souvent réédité peut en avoir des dizaines.
//!   [`best_matching_release`] retient celle dont le titre correspond à
//!   l'album du tag local s'il y en a une, et ne retombe sur la première
//!   de la liste qu'à défaut (ou si le tag local n'a pas d'album) : rien
//!   ne garantit alors sa pertinence.
//! - La recherche par album ([`album::search_by_album`]) n'examine que
//!   les [`album::RELEASE_LOOKUP_CANDIDATES`] premières éditions
//!   renvoyées par la recherche, et s'arrête à la première dont la liste
//!   de pistes contient un titre reconnaissable (voir
//!   [`album::best_matching_track`]) : si cette édition n'a pas la bonne
//!   liste de pistes dans la base MusicBrainz (données communautaires,
//!   parfois incomplètes ou mal reliées), la recherche échoue
//!   silencieusement (`None`) plutôt que d'essayer les éditions suivantes
//!   indéfiniment.
//!
//! # Étiquette de l'API
//!
//! MusicBrainz demande un en-tête `User-Agent` identifiant l'application
//! (voir <https://musicbrainz.org/doc/MusicBrainz_API/Rate_Limiting>) et
//! limite à environ une requête par seconde par application. Un appel à
//! [`verify_tag`] peut à lui seul déclencher plusieurs requêtes
//! successives (une pour la recherche par morceau, et jusqu'à
//! `1 + RELEASE_LOOKUP_CANDIDATES` pour la recherche par album) ; une
//! pause de [`REQUEST_INTERVAL`] est donc insérée entre elles. En boucle
//! sur plusieurs fichiers, il faudrait par contre ajouter une pause
//! supplémentaire *entre* les appels à [`verify_tag`], qui n'en insère
//! aucune de son côté à l'ouverture ni à la fin.

use crate::id3::header::Id3v2Tag;
use serde::Deserialize;
use std::fmt;
use std::time::Duration;

mod album;
mod table;
pub use table::{FieldMatch, VerificationReport, VerificationTable};

const MUSICBRAINZ_RECORDING_URL: &str = "https://musicbrainz.org/ws/2/recording";

/// Délai entre deux requêtes MusicBrainz consécutives au sein d'un même
/// appel à [`verify_tag`] (voir [`album::search_by_album`], qui peut à
/// lui seul en déclencher plusieurs) — un peu plus d'une seconde, pour
/// respecter la limite d'environ une requête par seconde par application
/// que demande l'étiquette d'usage de l'API (voir plus bas). N'a aucun
/// effet en dehors de ces appels réseau : les fonctions pures de ce
/// module (tri, comparaison, choix de la meilleure piste...) ne
/// l'utilisent jamais, et aucun test ne déclenche donc cette pause.
const REQUEST_INTERVAL: Duration = Duration::from_millis(1100);

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
/// Résultat de [`verify_tag`] : les rapports obtenus, et l'éventuelle
/// erreur de l'étape "recherche par album" si elle a échoué sans empêcher
/// le reste de réussir (voir la doc de [`verify_tag`]).
///
/// `album_search_error` est purement diagnostique : sa présence ne
/// dégrade en rien [`VerifyOutcome::reports`], qui reste aussi complet
/// que l'étape 2 (recherche par titre/artiste) le permet. L'appelant est
/// libre de l'ignorer, de la logger, ou de l'afficher (le CLI le fait en
/// mode `--verbose`) — sans elle, cette erreur réseau ou de réponse
/// disparaîtrait sans aucune trace.
#[derive(Debug)]
pub struct VerifyOutcome {
    /// Rapports de vérification, dans le même ordre que documenté par
    /// [`verify_tag`] (celui de l'étape 1 en tête, s'il y en a un).
    pub reports: Vec<VerificationReport>,
    /// Erreur de l'étape "recherche par album", si elle a échoué.
    pub album_search_error: Option<VerifyError>,
}

/// Vérifie le titre, l'artiste, l'album et l'année d'un tag ID3v2 auprès de
/// MusicBrainz.
///
/// Combine deux stratégies — voir la vue d'ensemble en tête de module :
///
/// 1. Si le tag local a un album, tente d'abord [`album::search_by_album`]. En
///    cas de succès, son rapport est placé en tête des rapports renvoyés ;
///    en cas d'échec, l'erreur est conservée dans
///    [`VerifyOutcome::album_search_error`] à titre diagnostique (voir sa
///    doc), sans empêcher l'étape 2 d'être tentée.
/// 2. Cherche ensuite par titre et/ou artiste (au moins l'un des deux doit
///    être présent dans le tag local) et ajoute un rapport par
///    enregistrement candidat renvoyé par cette recherche (jusqu'à
///    `limit` résultats), trié — voir [`sort_recordings`]. Le candidat
///    déjà couvert par l'étape 1, le cas échéant, n'est pas dupliqué.
///
/// [`DEFAULT_SEARCH_LIMIT`] est une valeur par défaut raisonnable pour
/// `limit` si l'appelant n'a pas de préférence.
///
/// Une erreur réseau à l'étape 1 n'empêche pas l'étape 2 d'être tentée
/// (et inversement) : [`VerifyOutcome::reports`] peut donc être incomplet
/// plutôt que l'appel entier échouer, tant qu'au moins une des deux
/// étapes a abouti.
///
/// # Erreurs
///
/// - [`VerifyError::NothingToSearch`] si le tag local n'a ni titre ni
///   artiste (l'album seul ne suffit pas : voir [`album::search_by_album`], qui
///   a besoin d'un titre local pour identifier la bonne piste dans
///   l'édition trouvée).
/// - [`VerifyError::Request`] ou [`VerifyError::Response`] si la recherche
///   par titre/artiste échoue *et* qu'aucun rapport par album n'a pu être
///   obtenu.
/// - [`VerifyError::NoMatch`] si aucune des deux stratégies n'a produit le
///   moindre rapport.
pub fn verify_tag(tag: &Id3v2Tag, limit: u32) -> Result<VerifyOutcome, VerifyError> {
    let title = tag.title();
    let artist = tag.artist();

    if title.is_none() && artist.is_none() {
        return Err(VerifyError::NothingToSearch);
    }

    // Étape 1 : recherche ciblée par album, si le tag local en a un. Une
    // erreur ici (réseau, réponse illisible, rien trouvé...) n'interrompt
    // pas la vérification : ce n'est qu'un complément, l'étape 2 reste la
    // recherche principale. L'erreur elle-même est conservée plutôt que
    // silencieusement perdue — voir `VerifyOutcome::album_search_error`.
    let (album_report, album_search_error) = match album::search_by_album(tag) {
        Ok(report) => (report, None),
        Err(err) => (None, Some(err)),
    };
    if album_report.is_some() {
        std::thread::sleep(REQUEST_INTERVAL);
    }

    // Étape 2 : recherche par titre/artiste.
    let query = build_query(title, artist);
    let recording_search = search_recordings(&query, limit);

    let mut reports = Vec::new();
    reports.extend(album_report.clone());

    match recording_search {
        Ok(mut recordings) => {
            sort_recordings(tag, &mut recordings);
            reports.extend(
                recordings
                    .iter()
                    // Ne pas montrer deux fois le même enregistrement si
                    // l'étape 1 l'a déjà trouvé.
                    .filter(|recording| {
                        album_report
                            .as_ref()
                            .is_none_or(|report| report.recording_id != recording.id)
                    })
                    .map(|recording| build_report(tag, recording)),
            );
        }
        // L'étape 1 a donné quelque chose : on renvoie ça plutôt que
        // d'échouer entièrement sur une erreur de l'étape 2.
        Err(_) if !reports.is_empty() => {}
        Err(err) => return Err(err),
    }

    if reports.is_empty() {
        return Err(VerifyError::NoMatch);
    }

    Ok(VerifyOutcome {
        reports,
        album_search_error,
    })
}
/// Requête de recherche par titre/artiste auprès de
/// [`MUSICBRAINZ_RECORDING_URL`] — la partie réseau de l'étape 2 de
/// [`verify_tag`], isolée pour que celle-ci puisse intercepter ses erreurs
/// sans avorter l'étape 1.
fn search_recordings(query: &str, limit: u32) -> Result<Vec<RemoteRecording>, VerifyError> {
    let limit_str = limit.to_string();
    let response: SearchResponse = ureq::get(MUSICBRAINZ_RECORDING_URL)
        .set("User-Agent", USER_AGENT)
        .query("query", query)
        .query("fmt", "json")
        .query("limit", &limit_str)
        .call()?
        .into_json()?;

    Ok(response.recordings)
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
        via_album: false,
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

#[cfg(test)]
mod tests;
