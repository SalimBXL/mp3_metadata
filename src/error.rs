use std::fmt;
use std::io;
use std::path::PathBuf;

#[derive(Debug)]
pub enum Mp3Error {
    NotFound(PathBuf),
    ReadFailed {
        path: PathBuf,
        source: io::Error,
    },
    TooSmall {
        len: usize,
    },
    InvalidTagSize {
        declared: u32,
        available: usize,
    },
    /// Version majeure d'ID3v2 non prise en charge (uniquement 2, 3 et 4
    /// sont gérées).
    UnsupportedVersion {
        major: u8,
    },
    /// L'extended header déclaré dans les flags de l'en-tête principal ne
    /// tient pas dans les octets disponibles.
    ExtendedHeaderTooShort {
        available: usize,
    },
    FrameTooShort {
        offset: usize,
    },
    FrameSizeOverflow {
        offset: usize,
        declared: u32,
        available: usize,
    },
    UnknownTextEncoding {
        encoding: u8,
    },
    InvalidTextData {
        encoding: u8,
    },
}

impl fmt::Display for Mp3Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Mp3Error::NotFound(p) => {
                write!(
                    f,
                    "Le fichier '{}' n'existe pas ou n'est pas un .mp3",
                    p.display()
                )
            }
            Mp3Error::ReadFailed { path, source } => {
                write!(
                    f,
                    "Erreur lors de la lecture du fichier '{}' : {}",
                    path.display(),
                    source
                )
            }
            Mp3Error::TooSmall { len } => {
                write!(
                    f,
                    "Fichier trop petit pour contenir un en-tête ID3v2 ({len} octets, 10 requis)"
                )
            }
            Mp3Error::InvalidTagSize {
                declared,
                available,
            } => {
                write!(
                    f,
                    "Taille du tag ID3v2 invalide : {declared} octets, mais seulement {available} octets dans le fichier"
                )
            }
            Mp3Error::UnsupportedVersion { major } => {
                write!(f, "Version ID3v2.{major} non prise en charge")
            }
            Mp3Error::ExtendedHeaderTooShort { available } => {
                write!(
                    f,
                    "Extended header ID3v2 tronqué : seulement {available} octets disponibles"
                )
            }
            Mp3Error::FrameTooShort { offset } => {
                write!(
                    f,
                    "Frame ID3v2 tronquée à l'offset {offset} : pas assez d'octets pour un en-tête de frame complet"
                )
            }
            Mp3Error::FrameSizeOverflow {
                offset,
                declared,
                available,
            } => {
                write!(
                    f,
                    "Frame ID3v2 à l'offset {offset} : taille déclarée ({declared} octets) dépasse les données disponibles ({available} octets)"
                )
            }
            Mp3Error::UnknownTextEncoding { encoding } => {
                write!(f, "Encoding de texte ID3v2 inconnu : {encoding}")
            }
            Mp3Error::InvalidTextData { encoding } => {
                write!(
                    f,
                    "Données de texte invalides pour l'encoding {encoding} (BOM manquant/invalide ou séquence mal formée)"
                )
            }
        }
    }
}

impl std::error::Error for Mp3Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Mp3Error::ReadFailed { source, .. } => Some(source),
            _ => None,
        }
    }
}
