use std::fmt;
use std::io;
use std::path::PathBuf;

/// Erreur renvoyée par les fonctions de lecture et de décodage de ce
/// crate — lecture du fichier sur disque comme décodage du tag ID3v2 ou
/// de ses frames. Chaque variante porte les données nécessaires pour
/// reconstituer le message d'erreur (voir son [`fmt::Display`]) sans
/// avoir à ré-analyser le fichier.
#[derive(Debug)]
pub enum Mp3Error {
    /// Le chemin donné n'a pas l'extension `.mp3`, ou le fichier n'existe
    /// pas (voir [`crate::read_mp3_file`]).
    NotFound(PathBuf),
    /// Une opération d'entrée/sortie sur le fichier a échoué (ouverture,
    /// lecture, positionnement du curseur...).
    ReadFailed {
        /// Chemin du fichier concerné.
        path: PathBuf,
        /// Erreur d'E/S d'origine, accessible aussi via
        /// [`std::error::Error::source`].
        source: io::Error,
    },
    /// Le fichier fait moins de 10 octets : pas assez pour contenir un
    /// en-tête ID3v2 complet.
    TooSmall {
        /// Taille réelle du fichier, en octets.
        len: usize,
    },
    /// La taille de tag ID3v2 déclarée dans l'en-tête dépasse la taille
    /// réelle des données disponibles.
    InvalidTagSize {
        /// Taille déclarée dans l'en-tête, en octets.
        declared: u32,
        /// Nombre d'octets réellement disponibles.
        available: usize,
    },
    /// Version majeure d'ID3v2 non prise en charge (uniquement 2, 3 et 4
    /// sont gérées).
    UnsupportedVersion {
        /// Version majeure lue dans l'en-tête (ex. `1` pour un tag
        /// prétendument ID3v2.1, qui n'existe pas).
        major: u8,
    },
    /// L'extended header déclaré dans les flags de l'en-tête principal ne
    /// tient pas dans les octets disponibles.
    ExtendedHeaderTooShort {
        /// Nombre d'octets réellement disponibles pour l'extended header.
        available: usize,
    },
    /// Il ne reste pas assez d'octets, à partir de cette position, pour
    /// contenir un en-tête de frame ID3v2 complet.
    FrameTooShort {
        /// Décalage, dans le corps du tag, où la lecture de la frame a
        /// commencé.
        offset: usize,
    },
    /// La taille de frame déclarée dans son en-tête dépasse les octets
    /// disponibles dans le corps du tag.
    FrameSizeOverflow {
        /// Décalage, dans le corps du tag, où la frame commence.
        offset: usize,
        /// Taille de frame déclarée dans son en-tête, en octets.
        declared: u32,
        /// Nombre d'octets réellement disponibles à partir de `offset`.
        available: usize,
    },
    /// L'octet d'encoding en tête d'une frame texte n'est aucune des
    /// quatre valeurs reconnues par ID3v2 (0 à 3).
    UnknownTextEncoding {
        /// Octet d'encoding lu.
        encoding: u8,
    },
    /// Le texte d'une frame est mal formé pour l'encoding déclaré (BOM
    /// manquant ou invalide en UTF-16, séquence d'octets invalide...).
    InvalidTextData {
        /// Octet d'encoding sous lequel le décodage a échoué.
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

//
// ---------- TESTS ----------
//
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // ----- Display -----
    //
    // Un message par variante : pas une correspondance exacte (le texte
    // peut évoluer), mais la présence des valeurs portées par la variante,
    // pour détecter un champ oublié dans le message si la variante change.

    #[test]
    fn test_display_not_found_includes_path() {
        let err = Mp3Error::NotFound(PathBuf::from("musique/absent.mp3"));
        assert!(err.to_string().contains("musique/absent.mp3"));
    }

    #[test]
    fn test_display_read_failed_includes_path_and_source() {
        let err = Mp3Error::ReadFailed {
            path: PathBuf::from("musique/chanson.mp3"),
            source: io::Error::new(io::ErrorKind::PermissionDenied, "permission denied"),
        };
        let text = err.to_string();
        assert!(text.contains("musique/chanson.mp3"));
        assert!(text.contains("permission denied"));
    }

    #[test]
    fn test_display_too_small_includes_len() {
        let err = Mp3Error::TooSmall { len: 5 };
        assert!(err.to_string().contains('5'));
    }

    #[test]
    fn test_display_invalid_tag_size_includes_declared_and_available() {
        let err = Mp3Error::InvalidTagSize {
            declared: 1000,
            available: 42,
        };
        let text = err.to_string();
        assert!(text.contains("1000"));
        assert!(text.contains("42"));
    }

    #[test]
    fn test_display_unsupported_version_includes_major() {
        let err = Mp3Error::UnsupportedVersion { major: 1 };
        assert!(err.to_string().contains("ID3v2.1"));
    }

    #[test]
    fn test_display_extended_header_too_short_includes_available() {
        let err = Mp3Error::ExtendedHeaderTooShort { available: 2 };
        assert!(err.to_string().contains('2'));
    }

    #[test]
    fn test_display_frame_too_short_includes_offset() {
        let err = Mp3Error::FrameTooShort { offset: 37 };
        assert!(err.to_string().contains("37"));
    }

    #[test]
    fn test_display_frame_size_overflow_includes_all_fields() {
        let err = Mp3Error::FrameSizeOverflow {
            offset: 10,
            declared: 500,
            available: 80,
        };
        let text = err.to_string();
        assert!(text.contains("10"));
        assert!(text.contains("500"));
        assert!(text.contains("80"));
    }

    #[test]
    fn test_display_unknown_text_encoding_includes_encoding() {
        let err = Mp3Error::UnknownTextEncoding { encoding: 9 };
        assert!(err.to_string().contains('9'));
    }

    #[test]
    fn test_display_invalid_text_data_includes_encoding() {
        let err = Mp3Error::InvalidTextData { encoding: 1 };
        assert!(err.to_string().contains('1'));
    }

    // ----- source() -----

    #[test]
    fn test_source_read_failed_returns_the_io_error() {
        let err = Mp3Error::ReadFailed {
            path: PathBuf::from("x.mp3"),
            source: io::Error::new(io::ErrorKind::NotFound, "introuvable"),
        };
        let source = std::error::Error::source(&err).expect("ReadFailed porte une source");
        assert_eq!(source.to_string(), "introuvable");
    }

    #[test]
    fn test_source_is_none_for_every_other_variant() {
        let errors: Vec<Mp3Error> = vec![
            Mp3Error::NotFound(PathBuf::from("x.mp3")),
            Mp3Error::TooSmall { len: 1 },
            Mp3Error::InvalidTagSize {
                declared: 1,
                available: 0,
            },
            Mp3Error::UnsupportedVersion { major: 1 },
            Mp3Error::ExtendedHeaderTooShort { available: 0 },
            Mp3Error::FrameTooShort { offset: 0 },
            Mp3Error::FrameSizeOverflow {
                offset: 0,
                declared: 1,
                available: 0,
            },
            Mp3Error::UnknownTextEncoding { encoding: 9 },
            Mp3Error::InvalidTextData { encoding: 9 },
        ];

        for err in &errors {
            assert!(
                std::error::Error::source(err).is_none(),
                "{err:?} ne devrait pas porter de source"
            );
        }
    }

    // ----- Debug -----
    //
    // Le derive(Debug) est vérifié indirectement : s'il manquait un champ
    // non-Debug, la compilation échouerait déjà. On vérifie seulement que
    // le format ne panique pas et reste non vide.

    #[test]
    fn test_debug_does_not_panic() {
        let err = Mp3Error::TooSmall { len: 3 };
        assert!(!format!("{err:?}").is_empty());
    }
}
