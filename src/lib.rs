mod error;
mod id3;

pub use error::Mp3Error;
pub use id3::frame::{Frame, FrameContent};
pub use id3::header::{Id3Version, Id3v2Tag};

use id3::header::read_tag;
use std::fs;
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
    /// Séquence audio. Pas encore extraite.
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

/// Lit un fichier `.mp3` depuis le disque et en extrait le tag ID3v2
/// complet.
///
/// La fonction vérifie d'abord que le chemin a bien l'extension `.mp3` et
/// que le fichier existe (voir [`mp3_file_exists`]), lit l'intégralité du
/// fichier en mémoire, puis analyse le tag ID3v2 en tête de fichier — en-tête
/// et frames, décodées au passage (voir [`id3::header::read_tag`]).
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
/// - Toute erreur renvoyée par [`id3::header::read_tag`] si le tag ID3v2
///   est tronqué ou invalide.
///
/// # Exemples
///
/// ```ignore
/// let mp3 = read_mp3_file("musique/chanson.mp3")?;
/// println!("{mp3}");
/// ```
pub fn read_mp3_file(mp3_file: impl AsRef<Path>) -> Result<Mp3File, Mp3Error> {
    let path = mp3_file.as_ref();

    if !mp3_file_exists(path)? {
        return Err(Mp3Error::NotFound(path.to_path_buf()));
    }

    let data = fs::read(path).map_err(|source| Mp3Error::ReadFailed {
        path: path.to_path_buf(),
        source,
    })?;

    let id3v2 = read_tag(&data)?;

    Ok(Mp3File {
        path: path.to_path_buf(),
        size: data.len(),
        id3v2,
        id3v1: None,
        audio: None,
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
}
