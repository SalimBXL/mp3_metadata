use std::fmt;
use std::io;
use std::path::PathBuf;

#[derive(Debug)]
pub enum Mp3Error {
    NotFound(PathBuf),
    ReadFailed { path: PathBuf, source: io::Error },
    TooSmall { len: usize },
    MissingId3Tag,
    InvalidTagSize { declared: u32, available: usize },
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
            Mp3Error::MissingId3Tag => {
                write!(f, "Pas de tag ID3v2 au début du fichier")
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
