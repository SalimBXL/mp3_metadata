use std::fmt;
use std::io;
use std::path::PathBuf;

#[derive(Debug)]
pub enum Mp3Error {
    NotFound(PathBuf),
    ReadFailed { path: PathBuf, source: io::Error },
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
