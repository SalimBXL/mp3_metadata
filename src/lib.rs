mod error;
use error::Mp3Error;
use std::fs;
use std::io;
use std::path::Path;
mod id3;
use id3::header::Header;
use id3::header::read_header;

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

/// Lit le contenu brut d'un fichier `.mp3` sur le disque.
///
/// La fonction vérifie d'abord que le chemin a bien l'extension `.mp3` et
/// que le fichier existe (voir [`mp3_file_exists`]), puis lit l'intégralité
/// du fichier en mémoire.
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
///
/// # Exemples
///
/// ```ignore
/// let data = read_mp3_file("musique/chanson.mp3")?;
/// println!("{} octets lus", data.len());
/// ```
pub fn read_mp3_file(mp3_file: impl AsRef<Path>) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let path = mp3_file.as_ref();
    if !mp3_file_exists(path)? {
        return Err(Mp3Error::NotFound(path.to_path_buf()).into());
    }
    let data = fs::read(path).map_err(|e| Mp3Error::ReadFailed {
        path: path.to_path_buf(),
        source: e,
    })?;
    Ok(data)
}

pub fn read_id3_header(data: &[u8]) -> Result<Header, Box<dyn std::error::Error>> {
    match read_header(data) {
        Ok(header) => {
            println!("-------------------------------");
            println!("ID3v2 détecté");
            println!(
                "Version : {}.{}",
                header.version.major, header.version.minor
            );
            println!("Flags   : {:02X}", header.flags);
            println!("Taille  : {} octets", header.size);
            println!("-------------------------------");
            Ok(header)
        }
        Err(e) => Err(e),
    }
}

/* fn read_frames(data: &[u8], start: usize, end: usize) {
    println!("Lecture des frames ID3... {} à {}", start, end);
    let mut offset = start;
    while offset + 10 <= end {
        let Some(frame) = id3::frame::read_frame(data, offset) else {
            break;
        };
        let Some(decoded_content) = mp3_metadata::id3::frame::decode_frame(&frame) else {
            println!(
                ". . Décodage de la frame ID : {} (Erreur de décodage)",
                String::from_utf8_lossy(&frame.id)
            );
            offset = frame.next_offset;
            continue;
        };
        println!(
            ". . Décodage de la frame ID : {} (Contenu : {})",
            String::from_utf8_lossy(&frame.id),
            decoded_content
        );
        mp3_metadata::id3::frame::print_frame(&frame);
        offset = frame.next_offset;
    }
} */

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
    fn test_read_mp3_file() {
        let named_tempfile = Builder::new()
            .prefix("my-temporary-note")
            .suffix(".mp3")
            .rand_bytes(5)
            .tempfile()
            .unwrap();
        let result = read_mp3_file(named_tempfile.path());
        assert!(result.is_ok());
    }
}
