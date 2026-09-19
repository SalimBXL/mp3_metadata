mod error;
mod id3;
#[cfg(feature = "verify")]
pub mod verify;

pub use error::Mp3Error;
pub use id3::frame::{Frame, FrameContent};
pub use id3::header::{Id3Version, Id3v2Tag};

use id3::header::read_tag;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

pub struct MpegAudio {
    pub data: Vec<u8>,
}

pub struct Id3v1Tag {
    pub version: String,
    pub flags: u8,
    pub size: u32,
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
    /// Données audio brutes. `None` sauf lecture avec
    /// [`read_mp3_file_with_audio`].
    pub audio: Option<MpegAudio>,
    /// Tag ID3v1 extrait de la fin du fichier. Pas encore extrait.
    pub id3v1: Option<Id3v1Tag>,
}

impl Mp3File {
    /// Renvoie la taille du fichier en mébioctets (Mo, base 1024).
    ///
    /// Calculée à partir de [`Mp3File::size`] (taille en octets), divisée
    /// par 1024² (1 048 576).
    pub fn size_mo(&self) -> f64 {
        self.size as f64 / (1024.0 * 1024.0)
    }
}

/// Affiche un résumé lisible du fichier MP3 : nom de fichier et taille.
///
/// Le tag ID3v2 n'est pas affiché : il implémente lui-même `Display`.
impl std::fmt::Display for Mp3File {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let size_mo = self.size_mo();
        writeln!(f, "- MP3 -------------------------")?;
        writeln!(f, "Filename : {}", self.path.display())?;
        writeln!(f, "Size     : {} octets ({size_mo:.2} Mo)", self.size)?;
        write!(f, "-------------------------------")
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

/// Lit un fichier `.mp3` depuis le disque et en extrait le tag ID3v2
/// complet, sans charger les données audio qui suivent.
///
/// La fonction vérifie d'abord que le chemin a bien l'extension `.mp3` et
/// que le fichier existe (voir [`mp3_file_exists`]), lit sa taille sans en
/// lire le contenu (`File::metadata`), puis lit uniquement les octets du
/// tag ID3v2 — en-tête principal (10 octets) et corps, dont la taille est
/// connue dès les 10 premiers octets (voir
/// [`id3::header::declared_tag_body_size`]) — avant de le décoder (voir
/// [`id3::header::read_tag`]). Le reste du fichier (les données audio
/// proprement dites) n'est jamais lu : pour un fichier de plusieurs
/// centaines de mégaoctets, seuls quelques dizaines de kilooctets sont
/// donc chargés en mémoire. Pour charger aussi les données audio, voir
/// [`read_mp3_file_with_audio`].
///
/// Un fichier sans tag ID3v2 n'est pas une erreur : le champ
/// [`Mp3File::id3v2`] vaut alors `None`.
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

    if !mp3_file_exists(path)? {
        return Err(Mp3Error::NotFound(path.to_path_buf()));
    }

    let mut file = File::open(path).map_err(|source| Mp3Error::ReadFailed {
        path: path.to_path_buf(),
        source,
    })?;
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

    let id3v2 = match id3::header::declared_tag_body_size(&header) {
        Some(body_size) => {
            let tag_len = ID3V2_HEADER_LEN + body_size as usize;
            if tag_len > size {
                return Err(Mp3Error::InvalidTagSize {
                    declared: body_size,
                    available: size,
                });
            }

            let mut tag_data = vec![0u8; tag_len];
            tag_data[..ID3V2_HEADER_LEN].copy_from_slice(&header);
            file.read_exact(&mut tag_data[ID3V2_HEADER_LEN..])
                .map_err(read_failed)?;
            read_tag(&tag_data)?
        }
        None => None,
    };

    let audio = if include_audio {
        let mut data = Vec::new();
        file.read_to_end(&mut data).map_err(read_failed)?;
        Some(MpegAudio { data })
    } else {
        None
    };

    Ok(Mp3File {
        path: path.to_path_buf(),
        size,
        id3v2,
        id3v1: None,
        audio,
    })
}

//
// ---------- TESTS ----------
//
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::Builder;

    #[test]
    fn test_mp3_file_extension() {
        assert!(!mp3_file_exists("a_kind_of_magic.txt").unwrap());
    }

    #[test]
    fn test_mp3_file_exists() {
        let named_tempfile = Builder::new()
            .prefix("my-temporary-note")
            .suffix(".mp3")
            .rand_bytes(5)
            .tempfile()
            .unwrap();
        assert!(mp3_file_exists(named_tempfile.path()).unwrap());
    }

    #[test]
    fn test_mp3_file_does_not_exist() {
        assert!(!mp3_file_exists("non_existent_file.mp3").unwrap());
    }

    #[test]
    fn test_read_mp3_file_not_found() {
        assert!(matches!(
            read_mp3_file("non_existent_file.mp3"),
            Err(Mp3Error::NotFound(_))
        ));
    }

    #[test]
    fn test_read_mp3_file_without_id3_tag() {
        // Un fichier .mp3 sans tag ID3v2 se lit sans erreur.
        use std::io::Write;

        let mut file = Builder::new().suffix(".mp3").tempfile().unwrap();
        file.write_all(&[0xFF; 64]).unwrap();
        file.flush().unwrap();

        let mp3 = read_mp3_file(file.path()).unwrap();

        assert!(mp3.id3v2.is_none());
        assert_eq!(mp3.size, 64);
    }

    /// Construit un tag ID3v2.3 minimal (une seule frame TIT2) suivi
    /// d'octets qui simulent des données audio.
    fn tag_with_trailer(trailer_len: usize) -> (Vec<u8>, Vec<u8>) {
        let mut tag = Vec::new();
        tag.extend_from_slice(b"ID3");
        tag.extend_from_slice(&[3, 0, 0]); // version 2.3.0, flags 0
        tag.extend_from_slice(&[0, 0, 0, 13]); // taille synchsafe du corps : 13
        tag.extend_from_slice(b"TIT2");
        tag.extend_from_slice(&3u32.to_be_bytes()); // taille de frame
        tag.extend_from_slice(&[0, 0]); // flags de frame
        tag.extend_from_slice(&[3, b'H', b'i']); // encoding UTF-8, "Hi"

        let trailer = vec![0xAA; trailer_len];
        (tag, trailer)
    }

    #[test]
    fn test_read_mp3_file_reads_tag_without_loading_the_trailer() {
        use std::io::Write;

        let (tag, trailer) = tag_with_trailer(500);
        let mut file = Builder::new().suffix(".mp3").tempfile().unwrap();
        file.write_all(&tag).unwrap();
        file.write_all(&trailer).unwrap();
        file.flush().unwrap();

        let mp3 = read_mp3_file(file.path()).unwrap();

        assert_eq!(mp3.id3v2.as_ref().and_then(|t| t.title()), Some("Hi"));
        assert_eq!(mp3.size, tag.len() + trailer.len());
        assert!(mp3.audio.is_none());
    }

    #[test]
    fn test_read_mp3_file_with_audio_loads_the_trailer() {
        use std::io::Write;

        let (tag, trailer) = tag_with_trailer(500);
        let mut file = Builder::new().suffix(".mp3").tempfile().unwrap();
        file.write_all(&tag).unwrap();
        file.write_all(&trailer).unwrap();
        file.flush().unwrap();

        let mp3 = read_mp3_file_with_audio(file.path()).unwrap();

        assert_eq!(mp3.audio.map(|a| a.data), Some(trailer));
    }

    #[test]
    fn test_read_mp3_file_invalid_tag_size_does_not_require_the_declared_bytes_to_exist() {
        // L'en-tête annonce un corps de 100 octets, mais le fichier ne
        // contient que 5 octets après l'en-tête : l'erreur doit venir de
        // la taille annoncée, et non d'un échec de lecture générique --
        // ce qui suppose de l'avoir détectée sans tenter de lire les 100
        // octets promis.
        use std::io::Write;

        let mut data = Vec::new();
        data.extend_from_slice(b"ID3");
        data.extend_from_slice(&[3, 0, 0]);
        data.extend_from_slice(&[0, 0, 0, 100]); // annonce 100 octets...
        data.extend_from_slice(&[0u8; 5]); // ...mais il n'y en a que 5

        let mut file = Builder::new().suffix(".mp3").tempfile().unwrap();
        file.write_all(&data).unwrap();
        file.flush().unwrap();

        assert!(matches!(
            read_mp3_file(file.path()),
            Err(Mp3Error::InvalidTagSize {
                declared: 100,
                available
            }) if available == data.len()
        ));
    }
}
