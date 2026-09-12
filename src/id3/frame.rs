/// Contenu décodé d'une frame ID3v2, selon son type.
///
/// Une valeur `DecodedFrame` est produite par [`decode_frame`], qui choisit
/// la variante appropriée en fonction de l'identifiant de la frame
/// (`TIT2`, `APIC`, `COMM`, etc.).
#[derive(Debug)]
enum DecodedFrame {
    /// Contenu d'une frame texte (ex. `TIT2` pour le titre, `TPE1` pour
    /// l'artiste, `TALB` pour l'album), décodé selon l'encoding indiqué
    /// dans la frame (Latin-1, UTF-16 ou UTF-8). Voir [`decode_text_frame`].
    Text(String),
    /// Contenu d'une frame `APIC` (image jointe, ex. pochette d'album).
    Image {
        /// Type MIME de l'image (ex. `image/jpeg`).
        ///
        /// Non encore extrait des données de la frame : vaut toujours
        /// `"inconnu"` pour le moment (voir [`decode_frame`]).
        mime_type: String,
        /// Données brutes de l'image.
        data: Vec<u8>,
    },
    /// Contenu d'une frame `COMM` (commentaire), décodé en UTF-8 avec
    /// remplacement des octets invalides, sans tenir compte de l'octet
    /// d'encoding ni des champs langue/description de la frame.
    Comment(String),
    /// Contenu brut d'une frame dont l'identifiant n'est pas reconnu.
    Unknown(Vec<u8>),
}

/// Une frame ID3v2 individuelle (ex. `TIT2` pour le titre, `TPE1` pour
/// l'artiste, `APIC` pour une image de couverture).
///
/// Une valeur `Frame` est typiquement construite par
/// [`id3::frame::read_frame`], qui analyse une frame à partir d'un décalage
/// donné dans les données du tag ID3v2.
pub struct Frame {
    /// Identifiant de la frame sur 4 octets (ex. `b"TIT2"`, `b"TPE1"`).
    pub id: [u8; 4],
    /// Taille de `frame_data` en octets, telle que déclarée dans l'en-tête
    /// de la frame (n'inclut pas les 10 octets de l'en-tête de frame
    /// lui-même : 4 octets d'id + 4 octets de taille + 2 octets de flags).
    pub size: u32,
    /// Flags de la frame (bits d'options telles que la compression, le
    /// chiffrement, le groupement, etc., selon la version ID3v2).
    pub flags: u16,
    /// Contenu brut de la frame, hors en-tête de frame.
    pub frame_data: Vec<u8>,
    /// Décalage (en octets, depuis le début des données du tag) auquel
    /// cette frame a commencé à être lue.
    pub offset: usize,
    /// Décalage (en octets, depuis le début des données du tag) auquel
    /// commence la frame suivante, à utiliser pour poursuivre la lecture
    /// séquentielle des frames.
    pub next_offset: usize,
}

/// Affiche un résumé lisible d'une frame ID3v2 : identifiant, flags,
/// taille, position dans le tag, et un aperçu de son contenu décodé.
///
/// Le contenu est décodé via [`decode_frame`] : le texte est affiché tel
/// quel pour les frames texte et les commentaires, la taille et le MIME
/// type pour une image, la taille pour les frames non reconnues. Si le
/// décodage échoue, aucun contenu n'est affiché.
impl std::fmt::Display for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let decoded_data = match decode_frame(&self.id, &self.frame_data) {
            Some(DecodedFrame::Text(texte)) => texte,
            Some(DecodedFrame::Comment(commentaire)) => commentaire,
            Some(DecodedFrame::Image { mime_type, data }) => {
                format!("Image ({mime_type}, {} octets)", data.len())
            }
            Some(DecodedFrame::Unknown(data)) => format!("Unknown data: {} octets", data.len()),
            None => String::from(""),
        };
        writeln!(f, "- FRAME -----------------------")?;
        writeln!(f, "Id          : {}", String::from_utf8_lossy(&self.id))?;
        writeln!(f, "Flags       : {:02X}", self.flags)?;
        writeln!(f, "Taille      : {} octets", self.size)?;
        writeln!(f, "Offset      : {}", self.offset)?;
        writeln!(f, "Next offset : {}", self.next_offset)?;
        writeln!(f, "Data        : {decoded_data}")?;
        write!(f, "-------------------------------")
    }
}

/// Lit une frame ID3v2 à partir d'un décalage donné dans les données du tag.
///
/// Une frame ID3v2 commence par un en-tête de 10 octets (4 octets d'id +
/// 4 octets de taille, big-endian + 2 octets de flags), suivi de `size`
/// octets de données.
///
/// # Retour
///
/// - `Some(frame)` si une frame valide a pu être lue à `offset`.
/// - `None` si :
///   - il ne reste pas assez d'octets à partir de `offset` pour contenir
///     un en-tête de frame complet ou son corps déclaré (fichier tronqué
///     ou décalage invalide) ;
///   - l'id de la frame est composé uniquement d'octets nuls, ce qui
///     correspond au padding de fin de tag prévu par la spécification
///     ID3v2, et signale donc qu'il n'y a plus de frame à lire.
pub fn read_frame(id3_data: &[u8], offset: usize) -> Option<Frame> {
    let header_end = offset.checked_add(10)?;
    if id3_data.len() < header_end {
        return None;
    }

    let frame_id: [u8; 4] = id3_data[offset..offset + 4]
        .try_into()
        .expect("slice de 4 octets, conversion infaillible");

    if frame_id == [0, 0, 0, 0] {
        // Padding de fin de tag : plus de frame à lire.
        return None;
    }

    let frame_size = &id3_data[offset + 4..offset + 8];
    let frame_flags = &id3_data[offset + 8..header_end];
    let size = u32::from_be_bytes([frame_size[0], frame_size[1], frame_size[2], frame_size[3]]);
    let flags = u16::from_be_bytes([frame_flags[0], frame_flags[1]]);

    let frame_end = header_end.checked_add(size as usize)?;
    if frame_end > id3_data.len() {
        return None;
    }

    let frame_data = id3_data[header_end..frame_end].to_vec();

    Some(Frame {
        id: frame_id,
        size,
        flags,
        frame_data,
        offset,
        next_offset: frame_end,
    })
}

/// Décode le contenu d'une frame ID3v2 selon son identifiant.
///
/// Les frames texte connues (`TIT2`, `TPE1`, `TPE2`, `TALB`, `TRCK`,
/// `TCON`) sont décodées via [`decode_text_frame`], qui respecte l'octet
/// d'encoding en tête de la frame. Les frames `APIC` (image) et `COMM`
/// (commentaire) reçoivent un traitement dédié. Toute autre frame est
/// conservée telle quelle dans [`DecodedFrame::Unknown`].
///
/// # Retour
///
/// - `None` si `frame_data` est vide, ou si le décodage texte d'une frame
///   texte connue échoue (encoding inconnu, séquence invalide — voir
///   [`decode_text_frame`]).
/// - `Some(DecodedFrame)` sinon.
///
/// # À faire
///
/// L'extraction du MIME type réel pour les frames `APIC` n'est pas encore
/// implémentée (`mime_type` vaut toujours `"inconnu"`).
fn decode_frame(frame_id: &[u8; 4], frame_data: &[u8]) -> Option<DecodedFrame> {
    if frame_data.is_empty() {
        return None;
    }

    let decoded = match frame_id {
        b"TIT2" | b"TPE1" | b"TPE2" | b"TALB" | b"TRCK" | b"TCON" => {
            DecodedFrame::Text(decode_text_frame(frame_data)?)
        }
        b"APIC" => DecodedFrame::Image {
            mime_type: "inconnu".to_string(), // TODO: extraire le vrai MIME type
            data: frame_data.to_vec(),
        },
        b"COMM" => DecodedFrame::Comment(String::from_utf8_lossy(frame_data).into_owned()),
        _ => DecodedFrame::Unknown(frame_data.to_vec()),
    };

    Some(decoded)
}

/// Décode le contenu textuel d'une frame ID3v2, en tenant compte de
/// l'octet d'encoding placé en tête du contenu de la frame.
///
/// Le premier octet de `frame_data` indique l'encoding du texte qui suit,
/// selon la spécification ID3v2 :
/// - `0` : ISO-8859-1 (Latin-1)
/// - `1` : UTF-16 avec byte-order-mark (BOM), little ou big-endian
/// - `2` : UTF-16BE, sans BOM
/// - `3` : UTF-8 (ID3v2.4 uniquement)
///
/// # Retour
///
/// `None` si `frame_data` est vide, si l'octet d'encoding n'est pas l'une
/// des quatre valeurs reconnues, si le BOM est absent ou invalide pour
/// l'encoding 1, ou si les octets qui suivent ne forment pas une chaîne
/// valide dans l'encoding indiqué.
///
/// Un dernier octet isolé (nombre d'octets restants impair pour un
/// encoding UTF-16) est silencieusement ignoré plutôt que de provoquer une
/// erreur.
fn decode_text_frame(frame_data: &[u8]) -> Option<String> {
    if frame_data.is_empty() {
        return None;
    }

    let encoding = frame_data[0];
    let text_data = &frame_data[1..];

    match encoding {
        // ISO-8859-1
        0 => Some(text_data.iter().map(|&byte| byte as char).collect()),

        // UTF-16 avec BOM
        1 => {
            if text_data.len() < 2 {
                return None;
            }

            let bom = [text_data[0], text_data[1]];
            let text_data = &text_data[2..];

            let units: Vec<u16> = text_data
                .as_chunks::<2>()
                .0
                .iter()
                .map(|chunk| match bom {
                    [0xFF, 0xFE] => u16::from_le_bytes(*chunk),
                    [0xFE, 0xFF] => u16::from_be_bytes(*chunk),
                    _ => 0,
                })
                .collect();

            match bom {
                [0xFF, 0xFE] | [0xFE, 0xFF] => String::from_utf16(&units).ok(),
                _ => None,
            }
        }

        // UTF-16BE
        2 => {
            let units: Vec<u16> = text_data
                .as_chunks::<2>()
                .0
                .iter()
                .map(|chunk| u16::from_be_bytes(*chunk))
                .collect();

            String::from_utf16(&units).ok()
        }

        // UTF-8
        3 => String::from_utf8(text_data.to_vec()).ok(),

        _ => None,
    }
}

//
// ---------- TESTS ----------
//

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_text_frame_empty_returns_none() {
        assert!(decode_text_frame(&[]).is_none());
    }

    #[test]
    fn test_decode_text_frame_latin1() {
        // Encoding 0 = ISO-8859-1. 0xE9 = 'é' en Latin-1.
        let data = [0, b'H', b'i', 0xE9];
        let text = decode_text_frame(&data).unwrap();
        assert_eq!(text, "Hié");
    }

    #[test]
    fn test_decode_text_frame_latin1_empty_text() {
        // Encoding valide, mais aucun octet de texte après.
        let data = [0];
        let text = decode_text_frame(&data).unwrap();
        assert_eq!(text, "");
    }

    #[test]
    fn test_decode_text_frame_utf16_le_bom() {
        // Encoding 1 = UTF-16 avec BOM. BOM 0xFF 0xFE = little-endian.
        // "Hi" en UTF-16LE : 'H' = 0x0048, 'i' = 0x0069.
        let data = [1, 0xFF, 0xFE, 0x48, 0x00, 0x69, 0x00];
        let text = decode_text_frame(&data).unwrap();
        assert_eq!(text, "Hi");
    }

    #[test]
    fn test_decode_text_frame_utf16_be_bom() {
        // BOM 0xFE 0xFF = big-endian.
        let data = [1, 0xFE, 0xFF, 0x00, 0x48, 0x00, 0x69];
        let text = decode_text_frame(&data).unwrap();
        assert_eq!(text, "Hi");
    }

    #[test]
    fn test_decode_text_frame_utf16_invalid_bom_returns_none() {
        let data = [1, 0x12, 0x34, 0x00, 0x48];
        assert!(decode_text_frame(&data).is_none());
    }

    #[test]
    fn test_decode_text_frame_utf16_too_short_for_bom_returns_none() {
        // Un seul octet après l'encoding : pas assez pour un BOM (2 octets).
        let data = [1, 0xFF];
        assert!(decode_text_frame(&data).is_none());
    }

    #[test]
    fn test_decode_text_frame_utf16_odd_trailing_byte_ignored() {
        // "Hi" en UTF-16LE suivi d'un octet isolé en trop.
        let data = [1, 0xFF, 0xFE, 0x48, 0x00, 0x69, 0x00, 0xAB];
        let text = decode_text_frame(&data).unwrap();
        assert_eq!(text, "Hi"); // l'octet en trop est silencieusement ignoré
    }

    #[test]
    fn test_decode_text_frame_utf16be_no_bom() {
        // Encoding 2 = UTF-16BE sans BOM.
        let data = [2, 0x00, 0x48, 0x00, 0x69];
        let text = decode_text_frame(&data).unwrap();
        assert_eq!(text, "Hi");
    }

    #[test]
    fn test_decode_text_frame_utf16be_invalid_surrogate_returns_none() {
        // 0xD800 est une moitié de paire de substitution isolée : invalide en UTF-16.
        let data = [2, 0xD8, 0x00];
        assert!(decode_text_frame(&data).is_none());
    }

    #[test]
    fn test_decode_text_frame_utf8() {
        // Encoding 3 = UTF-8.
        let mut data = vec![3];
        data.extend_from_slice("Café".as_bytes());
        let text = decode_text_frame(&data).unwrap();
        assert_eq!(text, "Café");
    }

    #[test]
    fn test_decode_text_frame_utf8_invalid_bytes_returns_none() {
        // 0xFF seul n'est jamais un début de séquence UTF-8 valide.
        let data = [3, 0xFF, 0xFF];
        assert!(decode_text_frame(&data).is_none());
    }

    #[test]
    fn test_decode_text_frame_unknown_encoding_returns_none() {
        let data = [9, b'H', b'i'];
        assert!(decode_text_frame(&data).is_none());
    }

    /// Construit les octets d'une frame ID3v2 valide : 4 octets d'id,
    /// 4 octets de taille (big-endian), 2 octets de flags, puis le corps.
    fn build_frame_bytes(id: &[u8; 4], flags: u16, body: &[u8]) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(id);
        data.extend_from_slice(&(body.len() as u32).to_be_bytes());
        data.extend_from_slice(&flags.to_be_bytes());
        data.extend_from_slice(body);
        data
    }

    #[test]
    fn test_read_frame_valid() {
        let data = build_frame_bytes(b"TIT2", 0x0000, b"Hello");
        let frame = read_frame(&data, 0).unwrap();

        assert_eq!(&frame.id, b"TIT2");
        assert_eq!(frame.size, 5);
        assert_eq!(frame.flags, 0x0000);
        assert_eq!(frame.frame_data, b"Hello");
        assert_eq!(frame.offset, 0);
        assert_eq!(frame.next_offset, 10 + 5);
    }

    #[test]
    fn test_read_frame_at_nonzero_offset() {
        let mut data = vec![0xAA; 20]; // du bruit avant la frame
        data.extend(build_frame_bytes(b"TPE1", 0, b"Queen"));

        let frame = read_frame(&data, 20).unwrap();

        assert_eq!(&frame.id, b"TPE1");
        assert_eq!(frame.offset, 20);
        assert_eq!(frame.next_offset, 20 + 10 + 5);
    }

    #[test]
    fn test_read_frame_zero_size_body() {
        let data = build_frame_bytes(b"TCON", 0, b"");
        let frame = read_frame(&data, 0).unwrap();

        assert_eq!(frame.size, 0);
        assert!(frame.frame_data.is_empty());
        assert_eq!(frame.next_offset, 10);
    }

    #[test]
    fn test_read_frame_too_short_for_header() {
        let data = [0u8; 5]; // moins de 10 octets
        assert!(read_frame(&data, 0).is_none());
    }

    #[test]
    fn test_read_frame_exactly_too_short() {
        let data = [0u8; 9]; // 1 octet manquant pour l'en-tête complet
        assert!(read_frame(&data, 0).is_none());
    }

    #[test]
    fn test_read_frame_padding_returns_none() {
        let data = [0u8; 10]; // id à zéro : padding de fin de tag
        assert!(read_frame(&data, 0).is_none());
    }

    #[test]
    fn test_read_frame_declared_size_exceeds_available_data() {
        // On déclare une taille de corps plus grande que ce qui est réellement fourni.
        let mut data = build_frame_bytes(b"APIC", 0, &[0u8; 100]);
        data.truncate(15); // le fichier est tronqué

        assert!(read_frame(&data, 0).is_none());
    }

    #[test]
    fn test_read_frame_offset_beyond_data() {
        let data = build_frame_bytes(b"TIT2", 0, b"Hello");
        assert!(read_frame(&data, data.len()).is_none());
    }

    #[test]
    fn test_read_frame_offset_overflow_does_not_panic() {
        // Un offset proche de usize::MAX ne doit jamais paniquer par
        // dépassement arithmétique — checked_add doit renvoyer None.
        let data = [0u8; 20];
        assert!(read_frame(&data, usize::MAX - 5).is_none());
    }

    #[test]
    fn test_read_frame_size_overflow_does_not_panic() {
        // Une taille de frame proche de u32::MAX, combinée à un offset
        // suffisamment grand, ne doit pas paniquer non plus.
        let mut data = vec![0u8; 10];
        data[0..4].copy_from_slice(b"TIT2");
        data[4..8].copy_from_slice(&u32::MAX.to_be_bytes());

        assert!(read_frame(&data, usize::MAX - 20).is_none());
    }

    #[test]
    fn test_decode_frame_empty_data_returns_none() {
        assert!(decode_frame(b"TIT2", &[]).is_none());
    }

    #[test]
    fn test_decode_frame_text_utf8() {
        // Encoding 3 = UTF-8
        let mut frame_data = vec![3];
        frame_data.extend_from_slice("Bohemian Rhapsody".as_bytes());

        let decoded = decode_frame(b"TIT2", &frame_data).unwrap();

        match decoded {
            DecodedFrame::Text(text) => assert_eq!(text, "Bohemian Rhapsody"),
            other => panic!("attendu DecodedFrame::Text, obtenu {other:?}"),
        }
    }

    #[test]
    fn test_decode_frame_text_all_known_ids() {
        // Vérifie que toutes les frames texte listées passent bien par
        // decode_text_frame plutôt que dans la branche Unknown.
        let mut frame_data = vec![3];
        frame_data.extend_from_slice("Queen".as_bytes());

        for id in [b"TIT2", b"TPE1", b"TPE2", b"TALB", b"TRCK", b"TCON"] {
            let decoded = decode_frame(id, &frame_data).unwrap();
            assert!(
                matches!(decoded, DecodedFrame::Text(_)),
                "frame {:?} devrait être décodée comme Text",
                String::from_utf8_lossy(id)
            );
        }
    }

    #[test]
    fn test_decode_frame_text_invalid_encoding_propagates_none() {
        // Octet d'encoding invalide (ni 0, 1, 2 ni 3) : decode_text_frame
        // renvoie None, et decode_frame doit propager ce None via `?`.
        let frame_data = vec![9, b'H', b'i'];
        assert!(decode_frame(b"TIT2", &frame_data).is_none());
    }

    #[test]
    fn test_decode_frame_apic() {
        let frame_data = vec![0xDE, 0xAD, 0xBE, 0xEF]; // contenu image bidon
        let decoded = decode_frame(b"APIC", &frame_data).unwrap();

        match decoded {
            DecodedFrame::Image { mime_type, data } => {
                assert_eq!(mime_type, "inconnu");
                assert_eq!(data, frame_data);
            }
            other => panic!("attendu DecodedFrame::Image, obtenu {other:?}"),
        }
    }

    #[test]
    fn test_decode_frame_comm() {
        let frame_data = "Super chanson".as_bytes().to_vec();
        let decoded = decode_frame(b"COMM", &frame_data).unwrap();

        match decoded {
            DecodedFrame::Comment(text) => assert_eq!(text, "Super chanson"),
            other => panic!("attendu DecodedFrame::Comment, obtenu {other:?}"),
        }
    }

    #[test]
    fn test_decode_frame_unknown_id() {
        let frame_data = vec![1, 2, 3, 4, 5];
        let decoded = decode_frame(b"XXXX", &frame_data).unwrap();

        match decoded {
            DecodedFrame::Unknown(data) => assert_eq!(data, frame_data),
            other => panic!("attendu DecodedFrame::Unknown, obtenu {other:?}"),
        }
    }
}
