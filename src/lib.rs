//! Bibliothèque de lecture des métadonnées d'un fichier MP3 : tag ID3v2
//! (en-tête, frames et leur contenu décodé), tag ID3v1 / ID3v1.1, et
//! format audio (version/couche MPEG, débit, durée estimée) déduit de la
//! première frame audio. Voir [`read_mp3_file`] pour le point d'entrée
//! principal.

mod error;
mod id3;
mod id3v1;
mod mpeg;
#[cfg(feature = "verify")]
pub mod verify;

pub use error::Mp3Error;
pub use id3::frame::{Frame, FrameContent};
pub use id3::header::{Id3Version, Id3v2Tag};
pub use id3v1::Id3v1Tag;
pub use mpeg::{AudioFormat, ChannelMode, MpegFrameHeader, MpegLayer, MpegVersion};

use id3::header::read_tag;
use id3v1::read_id3v1_tag;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Séparateur utilisé sous chaque en-tête de section à l'affichage (voir
/// [`Mp3File`], [`AudioFormat`], [`Id3v2Tag`]).
pub(crate) const SECTION_SEPARATOR: &str = "────────────────────────────────────";

/// Données audio brutes d'un fichier MP3 (tout ce qui suit le tag ID3v2),
/// chargées par [`read_mp3_file_with_audio`].
pub struct MpegAudio {
    /// Octets audio bruts, tels que lus depuis le fichier.
    pub data: Vec<u8>,
}

/// Représente un fichier MP3 lu depuis le disque, avec toutes ses
/// métadonnées.
///
/// Une valeur `Mp3File` est construite par [`read_mp3_file`], qui lit le
/// fichier et en extrait le tag ID3v2 **complet** : en-tête et frames
/// décodées. C'est la seule structure à manipuler en aval — il n'y a plus
/// d'appel séparé à faire pour obtenir les frames.
///
/// # Exemples
///
/// ```ignore
/// let mp3 = read_mp3_file("musique/chanson.mp3")?;
/// if let Some(tag) = &mp3.id3v2 {
///     println!("{} — {}", tag.artist().unwrap_or("?"), tag.title().unwrap_or("?"));
/// }
/// ```
pub struct Mp3File {
    /// Chemin du fichier sur le disque.
    pub path: PathBuf,
    /// Taille du fichier en octets.
    pub size: usize,
    /// Tag ID3v2 extrait du début du fichier, frames comprises.
    /// `None` si le fichier n'en porte pas.
    pub id3v2: Option<Id3v2Tag>,
    /// Format audio (version/couche MPEG, débit, taux d'échantillonnage,
    /// canaux) et durée estimée, déduits de la première frame audio
    /// trouvée. Lu par défaut par [`read_mp3_file`] — une petite sonde
    /// après le tag suffit, inutile de charger les données audio pour
    /// cela. `None` si aucune frame valide n'a été trouvée dans la
    /// fenêtre sondée.
    pub audio_format: Option<AudioFormat>,
    /// Données audio brutes. `None` sauf lecture avec
    /// [`read_mp3_file_with_audio`].
    pub audio: Option<MpegAudio>,
    /// Tag ID3v1 (ou ID3v1.1) extrait des 128 derniers octets du fichier.
    /// `None` si le fichier fait moins de 128 octets, ou si ces 128 octets
    /// ne commencent pas par la signature `TAG`. Un fichier peut porter à
    /// la fois un tag ID3v2 (au début) et un tag ID3v1 (à la fin) : les
    /// deux sont indépendants l'un de l'autre.
    pub id3v1: Option<Id3v1Tag>,
}

impl Mp3File {
    /// Renvoie la taille du fichier en mébioctets (MiB, base 1024).
    ///
    /// Calculée à partir de [`Mp3File::size`] (taille en octets), divisée
    /// par 1024² (1 048 576).
    pub fn size_mib(&self) -> f64 {
        self.size as f64 / (1024.0 * 1024.0)
    }
}

/// Affiche la section "MP3" : nom de fichier et taille.
///
/// Le tag ID3v2 et le format audio ne sont pas affichés ici : ils
/// implémentent chacun leur propre `Display` ([`Id3v2Tag`],
/// [`AudioFormat`]).
impl std::fmt::Display for Mp3File {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let size_mib = self.size_mib();
        writeln!(f, "MP3")?;
        writeln!(f, "{SECTION_SEPARATOR}")?;
        writeln!(f, "{:<11}: {}", "File", self.path.display())?;
        write!(f, "{:<11}: {size_mib:.2} MiB", "Size")
    }
}

/// Vérifie qu'un chemin pointe vers un fichier `.mp3` existant.
///
/// La fonction contrôle d'abord l'extension du fichier (insensible à la
/// casse, donc `.mp3` et `.MP3` sont acceptés) avant d'interroger le
/// système de fichiers, afin d'éviter un accès disque inutile si
/// l'extension ne correspond pas.
///
/// # Retour
///
/// - `Ok(true)` si le chemin a l'extension `.mp3` et que le fichier existe.
/// - `Ok(false)` si l'extension n'est pas `.mp3`, ou si l'extension est
///   correcte mais que le fichier n'existe pas.
///
/// # Erreurs
///
/// [`Mp3Error::ReadFailed`] si la vérification d'existence échoue pour une
/// raison autre que l'absence du fichier (par exemple, permissions
/// refusées sur un répertoire parent).
fn mp3_file_exists(mp3_file: impl AsRef<Path>) -> Result<bool, Mp3Error> {
    let path = mp3_file.as_ref();
    let has_mp3_extension = path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("mp3"));
    if !has_mp3_extension {
        return Ok(false);
    }

    fs::exists(path).map_err(|source| Mp3Error::ReadFailed {
        path: path.to_path_buf(),
        source,
    })
}

/// Nombre d'octets de l'en-tête principal ID3v2, avant le corps du tag.
const ID3V2_HEADER_LEN: usize = 10;

/// Taille de la fenêtre sondée juste après le tag à la recherche de la
/// première frame MPEG valide — largement suffisant en pratique : un
/// encodeur place l'audio juste après le tag, avec au plus quelques
/// dizaines d'octets de remplissage avant la première frame.
const MPEG_PROBE_LEN: usize = 4096;

/// Lit un fichier `.mp3` depuis le disque et en extrait le tag ID3v2
/// complet ainsi que le format audio de la première frame trouvée, sans
/// charger les données audio elles-mêmes.
///
/// La fonction vérifie d'abord que le chemin a bien l'extension `.mp3` et
/// que le fichier existe (voir [`mp3_file_exists`]), lit sa taille sans en
/// lire le contenu (`File::metadata`), puis lit uniquement les octets du
/// tag ID3v2 — en-tête principal (10 octets) et corps, dont la taille est
/// connue dès les 10 premiers octets (voir
/// [`id3::header::declared_tag_body_size`]) — avant de le décoder (voir
/// [`id3::header::read_tag`]). Une petite fenêtre d'octets juste après le
/// tag est ensuite sondée pour y trouver la première frame audio MPEG
/// (voir [`MPEG_PROBE_LEN`], [`mpeg::find_frame_header`]) et en déduire
/// [`Mp3File::audio_format`]. Les 128 derniers octets du fichier sont
/// aussi lus, indépendamment du reste, pour y chercher un tag ID3v1 (voir
/// [`id3v1::read_id3v1_tag`]). Le reste du fichier (les données audio
/// elles-mêmes) n'est jamais lu : pour un fichier de plusieurs centaines
/// de mégaoctets, seuls quelques dizaines de kilooctets sont donc chargés
/// en mémoire. Pour charger aussi les données audio, voir
/// [`read_mp3_file_with_audio`].
///
/// Un fichier sans tag ID3v2 n'est pas une erreur : le champ
/// [`Mp3File::id3v2`] vaut alors `None`, de même pour
/// [`Mp3File::id3v1`] si le fichier n'a pas de tag ID3v1.
///
/// # Erreurs
///
/// - [`Mp3Error::NotFound`] si le chemin n'a pas l'extension `.mp3`, ou si
///   le fichier n'existe pas.
/// - [`Mp3Error::ReadFailed`] si la lecture du fichier échoue (permissions
///   refusées, erreur d'E/S, etc.).
/// - [`Mp3Error::TooSmall`] si le fichier fait moins de 10 octets.
/// - [`Mp3Error::InvalidTagSize`] si la taille de tag déclarée dans
///   l'en-tête dépasse la taille réelle du fichier.
/// - Toute autre erreur renvoyée par [`id3::header::read_tag`] si le tag
///   ID3v2 est invalide.
///
/// # Exemples
///
/// ```ignore
/// let mp3 = read_mp3_file("musique/chanson.mp3")?;
/// println!("{mp3}");
/// ```
pub fn read_mp3_file(mp3_file: impl AsRef<Path>) -> Result<Mp3File, Mp3Error> {
    read_mp3_file_impl(mp3_file, false)
}

/// Comme [`read_mp3_file`], mais charge aussi les données audio (tout ce
/// qui suit le tag ID3v2) dans [`Mp3File::audio`].
///
/// Pour un fichier volumineux (podcast, livre audio...), ceci charge
/// potentiellement des centaines de mégaoctets en mémoire — à réserver aux
/// cas où ces données sont réellement nécessaires ; pour ne lire que les
/// métadonnées, préférer [`read_mp3_file`].
///
/// Mêmes erreurs que [`read_mp3_file`], plus [`Mp3Error::ReadFailed`] si
/// la lecture des données audio elles-mêmes échoue.
pub fn read_mp3_file_with_audio(mp3_file: impl AsRef<Path>) -> Result<Mp3File, Mp3Error> {
    read_mp3_file_impl(mp3_file, true)
}

fn read_mp3_file_impl(
    mp3_file: impl AsRef<Path>,
    include_audio: bool,
) -> Result<Mp3File, Mp3Error> {
    let path = mp3_file.as_ref();
    let mut file = open_mp3_file(path)?;
    let read_failed = |source: io::Error| Mp3Error::ReadFailed {
        path: path.to_path_buf(),
        source,
    };

    let size = file.metadata().map_err(read_failed)?.len() as usize;
    if size < ID3V2_HEADER_LEN {
        return Err(Mp3Error::TooSmall { len: size });
    }

    let mut header = [0u8; ID3V2_HEADER_LEN];
    file.read_exact(&mut header).map_err(read_failed)?;

    let (id3v2, audio_start, had_tag) = read_id3v2_tag(&mut file, path, &header, size)?;
    let probe = probe_audio_window(&mut file, path, &header, had_tag)?;
    let audio_format = detect_audio_format(&probe, size, audio_start);
    let id3v1 = read_id3v1_tag_if_present(&mut file, path, size)?;
    let audio = read_audio_if_requested(&mut file, path, include_audio, audio_start)?;

    Ok(Mp3File {
        path: path.to_path_buf(),
        size,
        id3v2,
        audio_format,
        id3v1,
        audio,
    })
}

/// Vérifie que `path` existe et a l'extension `.mp3` (voir
/// [`mp3_file_exists`]), puis l'ouvre.
fn open_mp3_file(path: &Path) -> Result<File, Mp3Error> {
    if !mp3_file_exists(path)? {
        return Err(Mp3Error::NotFound(path.to_path_buf()));
    }

    File::open(path).map_err(|source| Mp3Error::ReadFailed {
        path: path.to_path_buf(),
        source,
    })
}

/// Lit le tag ID3v2 en tête du fichier, s'il y en a un — `header` est
/// l'en-tête de 10 octets déjà lu par l'appelant. Renvoie aussi l'offset
/// où commencent les données audio et si un tag a effectivement été
/// trouvé, nécessaires respectivement à [`detect_audio_format`],
/// [`read_audio_if_requested`] et [`probe_audio_window`].
fn read_id3v2_tag(
    file: &mut File,
    path: &Path,
    header: &[u8; ID3V2_HEADER_LEN],
    size: usize,
) -> Result<(Option<Id3v2Tag>, usize, bool), Mp3Error> {
    let read_failed = |source: io::Error| Mp3Error::ReadFailed {
        path: path.to_path_buf(),
        source,
    };

    match id3::header::declared_tag_body_size(header) {
        Some(body_size) => {
            let tag_len = ID3V2_HEADER_LEN + body_size as usize;
            if tag_len > size {
                return Err(Mp3Error::InvalidTagSize {
                    declared: body_size,
                    available: size,
                });
            }

            let mut tag_data = vec![0u8; tag_len];
            tag_data[..ID3V2_HEADER_LEN].copy_from_slice(header);
            file.read_exact(&mut tag_data[ID3V2_HEADER_LEN..])
                .map_err(read_failed)?;
            Ok((read_tag(&tag_data)?, tag_len, true))
        }
        None => Ok((None, 0, false)),
    }
}

/// Sonde une fenêtre d'octets juste après le tag ID3v2 (ou depuis le tout
/// début du fichier s'il n'y en a pas, `had_tag` valant alors `false`) à
/// la recherche de la première frame MPEG valide (voir
/// [`detect_audio_format`]) — quelques Ko (voir [`MPEG_PROBE_LEN`])
/// suffisent, pas besoin de lire les données audio en entier pour en
/// connaître le format. Si le fichier n'a pas de tag, les 10 octets déjà
/// lus dans `header` font partie de l'audio et doivent être inclus dans
/// la fenêtre sondée.
fn probe_audio_window(
    file: &mut File,
    path: &Path,
    header: &[u8; ID3V2_HEADER_LEN],
    had_tag: bool,
) -> Result<Vec<u8>, Mp3Error> {
    let read_failed = |source: io::Error| Mp3Error::ReadFailed {
        path: path.to_path_buf(),
        source,
    };

    let mut probe = if had_tag { Vec::new() } else { header.to_vec() };
    let remaining_probe_len = MPEG_PROBE_LEN.saturating_sub(probe.len());
    file.by_ref()
        .take(remaining_probe_len as u64)
        .read_to_end(&mut probe)
        .map_err(read_failed)?;

    Ok(probe)
}

/// Déduit le format audio de la fenêtre sondée (voir
/// [`probe_audio_window`]), si elle contient une frame MPEG valide.
fn detect_audio_format(probe: &[u8], size: usize, audio_start: usize) -> Option<AudioFormat> {
    mpeg::find_frame_header(probe).map(|(offset, header)| {
        let audio_bytes = size.saturating_sub(audio_start) as u64;
        AudioFormat::from_probe(probe, offset, header, audio_bytes)
    })
}

/// Lit le tag ID3v1 (s'il existe) dans les 128 derniers octets du
/// fichier, indépendamment du tag ID3v2 — voir [`id3v1::read_id3v1_tag`].
fn read_id3v1_tag_if_present(
    file: &mut File,
    path: &Path,
    size: usize,
) -> Result<Option<Id3v1Tag>, Mp3Error> {
    let read_failed = |source: io::Error| Mp3Error::ReadFailed {
        path: path.to_path_buf(),
        source,
    };

    if size < id3v1::ID3V1_LEN {
        return Ok(None);
    }

    file.seek(SeekFrom::End(-(id3v1::ID3V1_LEN as i64)))
        .map_err(read_failed)?;
    let mut tail = [0u8; id3v1::ID3V1_LEN];
    file.read_exact(&mut tail).map_err(read_failed)?;

    Ok(read_id3v1_tag(&tail))
}

/// Charge les données audio (tout ce qui suit le tag ID3v2) si
/// `include_audio` est vrai (voir [`read_mp3_file_with_audio`]) ; `None`
/// sinon, sans rien lire. `audio_start` permet de revenir au tout début
/// du flux audio : les sondes précédentes ([`probe_audio_window`],
/// [`read_id3v1_tag_if_present`]) ont déplacé le curseur ailleurs dans le
/// fichier.
fn read_audio_if_requested(
    file: &mut File,
    path: &Path,
    include_audio: bool,
    audio_start: usize,
) -> Result<Option<MpegAudio>, Mp3Error> {
    let read_failed = |source: io::Error| Mp3Error::ReadFailed {
        path: path.to_path_buf(),
        source,
    };

    if !include_audio {
        return Ok(None);
    }

    file.seek(SeekFrom::Start(audio_start as u64))
        .map_err(read_failed)?;
    let mut data = Vec::new();
    file.read_to_end(&mut data).map_err(read_failed)?;

    Ok(Some(MpegAudio { data }))
}

//
// ---------- TESTS ----------
//
#[cfg(test)]
mod tests;
