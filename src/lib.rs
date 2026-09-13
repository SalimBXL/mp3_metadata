mod error;
use error::Mp3Error;
use std::fs;
use std::io;
use std::path::Path;
mod id3;
use id3::frame::Frame;
use id3::header::Id3Tag;
use id3::header::read_header;
use std::path::PathBuf;

pub struct MpegAudio {
    pub data: Vec<u8>,
}

pub struct Id3v1Tag {
    pub version: String,
    pub flags: u8,
    pub size: u32,
    pub data: Vec<u8>,
}

/// Représente un fichier MP3 chargé en mémoire, avec ses métadonnées ID3v2.
///
/// Une valeur `Mp3File` est typiquement construite par [`read_mp3_file`],
/// qui lit le fichier sur le disque et en extrait l'en-tête ID3v2.
pub struct Mp3File {
    /// Chemin du fichier sur le disque.
    pub path: PathBuf,
    /// Taille du fichier en octets (correspond à `data.len()`).
    pub size: usize,
    /// En-tête ID3v2 extrait du début du fichier.
    pub id3v2: Option<Id3Tag>,
    /// Séquence audio
    pub audio: Option<MpegAudio>,
    /// Tags ID3v1 extrait de la fin du fichier.
    pub id3v1: Option<Id3v1Tag>,
}

impl Mp3File {
    /// Renvoie la taille du fichier en mébioctets (Mo, base 1024).
    ///
    /// Calculée à partir de [`Mp3File::size`] (taille en octets), divisée
    /// par 1024² (1 048 576).
    ///
    /// # Exemples
    ///
    /// ```ignore
    /// let mp3 = read_mp3_file("musique/chanson.mp3")?;
    /// println!("{:.2} Mo", mp3.size_mo());
    /// ```
    pub fn size_mo(&self) -> f64 {
        self.size as f64 / (1024.0 * 1024.0)
    }
}

/// Affiche un résumé lisible du fichier MP3 : nom de fichier et taille.
///
/// Le contenu de `data` n'est pas affiché (il serait illisible en brut).
/// Pour afficher l'en-tête ID3v2 associé, utiliser `mp3_file.header`, qui
/// implémente également `Display`.
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
/// La fonction contrôle d'abord l'extension du fichier (insensible à la casse,
/// donc `.mp3` et `.MP3` sont acceptés) avant d'interroger le système de
/// fichiers, afin d'éviter un accès disque inutile si l'extension ne
/// correspond pas.
///
/// # Retour
///
/// - `Ok(true)` si le chemin a l'extension `.mp3` et que le fichier existe.
/// - `Ok(false)` si l'extension n'est pas `.mp3`, ou si l'extension est
///   correcte mais que le fichier n'existe pas.
/// - `Err(io::Error)` si la vérification d'existence échoue pour une raison
///   autre que l'absence du fichier (par exemple, permissions refusées).
///
/// # Exemples
///
/// ```ignore
/// let existe = mp3_file_exists("musique/chanson.mp3")?);
/// ```
fn mp3_file_exists(mp3_file: impl AsRef<Path>) -> io::Result<bool> {
    let path = mp3_file.as_ref();
    let has_mp3_extension = path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("mp3"));
    if !has_mp3_extension {
        return Ok(false);
    }
    fs::exists(path)
}

/// Lit un fichier `.mp3` depuis le disque et en extrait l'en-tête ID3v2.
///
/// La fonction vérifie d'abord que le chemin a bien l'extension `.mp3` et
/// que le fichier existe (voir [`mp3_file_exists`]), lit l'intégralité du
/// fichier en mémoire, puis analyse l'en-tête ID3v2 en tête de fichier
/// (voir [`read_header`]).
///
/// # Erreurs
///
/// - [`Mp3Error::NotFound`] si le chemin n'a pas l'extension `.mp3`, ou si
///   le fichier n'existe pas.
/// - [`Mp3Error::ReadFailed`] si la lecture du fichier échoue (permissions
///   refusées, erreur d'E/S, etc.).
/// - Toute erreur renvoyée par [`mp3_file_exists`] lors de la vérification
///   d'existence (par exemple, permissions refusées sur un répertoire
///   parent).
/// - Toute erreur renvoyée par [`read_header`] si l'en-tête ID3v2 est
///   absent, tronqué ou invalide.
///
/// # Exemples
///
/// ```ignore
/// let mp3 = read_mp3_file("musique/chanson.mp3")?;
/// println!("{} octets lus", mp3.size());
/// println!("{}", mp3.header);
/// ```
pub fn read_mp3_file(mp3_file: impl AsRef<Path>) -> Result<Mp3File, Box<dyn std::error::Error>> {
    let path = mp3_file.as_ref();

    if !mp3_file_exists(path)? {
        return Err(Mp3Error::NotFound(path.to_path_buf()).into());
    }

    let data = fs::read(path).map_err(|e| Mp3Error::ReadFailed {
        path: path.to_path_buf(),
        source: e,
    })?;

    let header = match read_header(&data) {
        Ok(header) => Some(header),
        Err(e) => {
            if e.downcast_ref::<Mp3Error>()
                .is_some_and(|err| matches!(err, Mp3Error::MissingId3Tag))
            {
                None
            } else {
                return Err(e);
            }
        }
    };

    Ok(Mp3File {
        path: path.to_path_buf(),
        size: data.len(),
        id3v2: header,
        id3v1: None,
        audio: None,
    })
}

/// Lit séquentiellement toutes les frames contenues dans un tag ID3v2.
///
/// Parcourt `header.data` à partir du début du corps du tag, en avançant
/// d'une frame à l'autre via [`Frame::next_offset`], jusqu'à atteindre la
/// fin du tag (`header.size`) ou jusqu'à ce qu'une frame ne puisse plus
/// être lue (voir [`id3::frame::read_frame`]).
///
/// # Retour
///
/// La liste des frames lues avec succès, dans l'ordre où elles apparaissent
/// dans le tag. La lecture s'arrête normalement (sans erreur) dès que le
/// padding de fin de tag est atteint. En revanche, une frame tronquée ou
/// dont la taille déclarée dépasse les données disponibles interrompt la
/// lecture avec une erreur (voir [`Mp3Error::FrameTooShort`] et
/// [`Mp3Error::FrameSizeOverflow`]).
///
/// # Erreurs
///
/// Toute erreur renvoyée par [`id3::frame::read_frame`] lors de la lecture
/// d'une frame (fichier tronqué, en-tête de frame corrompu).
///
/// # Exemples
///
/// ```ignore
/// let header = read_header(&data)?;
/// let frames = read_frames(&header)?;
/// for frame in &frames {
///     println!("{frame}");
/// }
/// ```
pub fn read_frames(header: &Id3Tag) -> Result<Vec<Frame>, Box<dyn std::error::Error>> {
    let id3_data = &header.data;
    let end = header.size as usize;
    let mut offset: usize = 10;
    let mut frames = Vec::new();

    while offset.checked_add(10).is_some_and(|next| next <= end) {
        match id3::frame::read_frame(id3_data, offset)? {
            Some(frame) => {
                offset = frame.next_offset;
                frames.push(frame);
            }
            None => break,
        }
    }

    Ok(frames)
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
}
