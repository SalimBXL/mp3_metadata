//! Décodage du contenu d'une frame ID3v2, une fois ses octets bruts
//! isolés par [`super::read_frame`] : choix de la variante
//! [`FrameContent`] appropriée selon l'identifiant de la frame, et
//! interprétation de l'encoding de texte ID3v2 (ISO-8859-1, UTF-16 avec
//! BOM, UTF-16BE, UTF-8).

use crate::error::Mp3Error;

/// Contenu décodé d'une frame ID3v2.
///
/// Une valeur `FrameContent` est produite par [`decode_frame`], qui choisit
/// la variante appropriée en fonction de l'identifiant de la frame
/// (`TIT2`, `APIC`, `COMM`, etc.), et est stockée dans [`Frame::content`]
/// au moment de la lecture.
#[derive(Debug, Clone, PartialEq)]
pub enum FrameContent {
    /// Frame vide : l'en-tête déclare une taille de 0 octet, il n'y a rien
    /// à décoder. Ce n'est pas une erreur.
    Empty,

    /// Contenu d'une frame texte, c'est-à-dire toute frame dont
    /// l'identifiant commence par `T`, à l'exception de `TXXX`
    /// (voir [`FrameContent::UserText`]).
    ///
    /// Le `Vec` contient une entrée par valeur : la spécification ID3v2.4
    /// autorise plusieurs valeurs dans une même frame texte, séparées par
    /// un caractère nul. En ID3v2.3 il n'y a en pratique qu'une seule
    /// valeur, et le `Vec` est de longueur 1.
    Text(Vec<String>),

    /// Contenu d'une frame `TXXX` : une paire description / valeur définie
    /// par l'application qui a écrit le tag.
    UserText {
        /// Nom du champ défini par l'application (ex. `"MusicBrainz Track Id"`).
        description: String,
        /// Valeur associée à cette description.
        value: String,
    },

    /// Contenu d'une frame `COMM` (commentaire) ou `USLT` (paroles non
    /// synchronisées). Ces deux frames partagent exactement la même
    /// disposition : octet d'encoding, code langue sur 3 octets,
    /// description terminée par un nul, puis le texte.
    FullText {
        /// Code langue ISO-639-2 sur 3 caractères (ex. `eng`, `fra`).
        language: String,
        /// Description courte, souvent vide.
        description: String,
        /// Texte du commentaire ou des paroles.
        text: String,
    },

    /// Contenu d'une frame `APIC` (image jointe, ex. pochette d'album).
    ///
    /// Décodage valable pour ID3v2.3 et ID3v2.4. En ID3v2.2, la frame
    /// équivalente (`PIC`) code le format d'image sur 3 lettres (`JPG`,
    /// `PNG`, ...) plutôt qu'une chaîne MIME terminée par un nul : sur un
    /// tag v2.2, `mime_type` et `description` seront mal découpés. Ce cas
    /// n'est pas géré.
    Picture {
        /// Type MIME de l'image, tel que déclaré dans la frame
        /// (ex. `image/jpeg`).
        mime_type: String,
        /// Rôle de l'image selon la spécification ID3v2 (`3` = pochette
        /// avant, `4` = pochette arrière, etc.).
        picture_type: u8,
        /// Description courte de l'image, souvent vide.
        description: String,
        /// Données brutes de l'image.
        data: Vec<u8>,
    },

    /// Contenu brut d'une frame dont l'identifiant n'est pas reconnu.
    Unknown(Vec<u8>),
}
/// Décode le corps d'une frame ID3v2 selon son identifiant.
///
/// - Toute frame dont l'identifiant commence par `T`, sauf `TXXX`, est une
///   frame texte et passe par [`decode_text_values`].
/// - `TXXX` porte une paire description / valeur définie par l'utilisateur.
/// - `COMM` et `USLT` partagent la disposition langue / description / texte.
/// - `APIC` porte une image, avec son type MIME et sa description.
/// - Toute autre frame est conservée telle quelle dans
///   [`FrameContent::Unknown`].
///
/// # Erreurs
///
/// - [`Mp3Error::UnknownTextEncoding`] si l'octet d'encoding en tête du
///   corps de la frame n'est pas l'une des quatre valeurs reconnues.
/// - [`Mp3Error::InvalidTextData`] si le texte est mal formé, ou si un
///   champ terminé par un nul ne l'est pas (frame tronquée ou corrompue).
pub(super) fn decode_frame(
    frame_id: &[u8; 4],
    frame_data: &[u8],
) -> Result<FrameContent, Mp3Error> {
    if frame_data.is_empty() {
        return Ok(FrameContent::Empty);
    }

    let content = match frame_id {
        b"TXXX" => {
            let (description, value) = decode_described_text(frame_data, 0)?;
            FrameContent::UserText { description, value }
        }

        // Toute autre frame `T***` est une frame texte.
        [b'T', ..] => FrameContent::Text(decode_text_values(frame_data)?),

        b"COMM" | b"USLT" => {
            // Octet d'encoding, puis 3 octets de code langue, puis la
            // disposition description / texte.
            if frame_data.len() < 4 {
                return Err(Mp3Error::InvalidTextData {
                    encoding: frame_data[0],
                });
            }
            let language = frame_data[1..4].iter().map(|&b| b as char).collect();
            let (description, text) = decode_described_text(frame_data, 3)?;
            FrameContent::FullText {
                language,
                description,
                text,
            }
        }

        b"APIC" => decode_picture_frame(frame_data)?,

        _ => FrameContent::Unknown(frame_data.to_vec()),
    };

    Ok(content)
}
/// Décode le corps d'une frame `APIC`.
///
/// Disposition : octet d'encoding, type MIME en ISO-8859-1 terminé par un
/// nul, octet de type d'image, description terminée par un nul (dans
/// l'encoding déclaré), puis les données brutes de l'image.
fn decode_picture_frame(frame_data: &[u8]) -> Result<FrameContent, Mp3Error> {
    let (&encoding, rest) = frame_data
        .split_first()
        .ok_or(Mp3Error::InvalidTextData { encoding: 0 })?;

    // Le type MIME est toujours en ISO-8859-1, quel que soit l'encoding
    // déclaré pour la description.
    let (mime_bytes, rest) =
        split_at_terminator(0, rest).ok_or(Mp3Error::InvalidTextData { encoding })?;
    let mime_type = decode_string(0, mime_bytes)?;

    let (&picture_type, rest) = rest
        .split_first()
        .ok_or(Mp3Error::InvalidTextData { encoding })?;

    let (description_bytes, data) =
        split_at_terminator(encoding, rest).ok_or(Mp3Error::InvalidTextData { encoding })?;
    let description = decode_string(encoding, description_bytes)?;

    Ok(FrameContent::Picture {
        mime_type,
        picture_type,
        description,
        data: data.to_vec(),
    })
}
/// Décode une frame bâtie sur le motif « description terminée par un nul,
/// puis texte » : `TXXX`, `COMM`, `USLT`.
///
/// `skip` est le nombre d'octets à ignorer entre l'octet d'encoding et la
/// description (0 pour `TXXX`, 3 pour le code langue de `COMM` et `USLT`).
fn decode_described_text(frame_data: &[u8], skip: usize) -> Result<(String, String), Mp3Error> {
    let (&encoding, rest) = frame_data
        .split_first()
        .ok_or(Mp3Error::InvalidTextData { encoding: 0 })?;

    let rest = rest
        .get(skip..)
        .ok_or(Mp3Error::InvalidTextData { encoding })?;

    let (description_bytes, text_bytes) =
        split_at_terminator(encoding, rest).ok_or(Mp3Error::InvalidTextData { encoding })?;

    Ok((
        decode_string(encoding, description_bytes)?,
        decode_string(encoding, strip_trailing_terminator(encoding, text_bytes))?,
    ))
}
/// Décode le corps d'une frame texte en une liste de valeurs.
///
/// Le premier octet indique l'encoding, le reste contient une ou plusieurs
/// valeurs séparées par un caractère nul (plusieurs valeurs ne sont
/// légales qu'en ID3v2.4, mais les accepter partout est sans risque). Le
/// terminateur final, que beaucoup d'encodeurs ajoutent, ne produit pas de
/// valeur vide supplémentaire.
///
/// En UTF-16, chaque valeur porte son propre BOM : le découpage est fait
/// au niveau des octets, avant décodage, pour que chaque valeur soit
/// décodée avec le sien.
fn decode_text_values(frame_data: &[u8]) -> Result<Vec<String>, Mp3Error> {
    let Some((&encoding, mut rest)) = frame_data.split_first() else {
        // Pas d'octet d'encoding : rien à décoder, ce n'est pas une erreur.
        return Ok(Vec::new());
    };

    let mut values = Vec::new();
    while !rest.is_empty() {
        match split_at_terminator(encoding, rest) {
            Some((value_bytes, remainder)) => {
                values.push(decode_string(encoding, value_bytes)?);
                rest = remainder;
            }
            // Pas de terminateur : le reste forme la dernière valeur.
            None => {
                values.push(decode_string(encoding, rest)?);
                break;
            }
        }
    }

    Ok(values)
}
/// Découpe `data` au premier terminateur nul, et renvoie `(avant, après)`.
///
/// La taille du terminateur dépend de l'encoding : un octet nul pour
/// ISO-8859-1 et UTF-8, deux octets nuls alignés sur une frontière de
/// caractère pour les deux variantes d'UTF-16.
fn split_at_terminator(encoding: u8, data: &[u8]) -> Option<(&[u8], &[u8])> {
    match encoding {
        1 | 2 => {
            let (pairs, _) = data.as_chunks::<2>();
            let index = pairs.iter().position(|&pair| pair == [0, 0])? * 2;
            Some((&data[..index], &data[index + 2..]))
        }
        _ => {
            let index = data.iter().position(|&byte| byte == 0)?;
            Some((&data[..index], &data[index + 1..]))
        }
    }
}
/// Retire le terminateur nul final s'il est présent.
fn strip_trailing_terminator(encoding: u8, data: &[u8]) -> &[u8] {
    match encoding {
        1 | 2 => match data {
            [head @ .., 0, 0] if head.len() % 2 == 0 => head,
            _ => data,
        },
        _ => match data {
            [head @ .., 0] => head,
            _ => data,
        },
    }
}
/// Décode une suite d'octets en chaîne, selon un encoding ID3v2 :
///
/// - `0` : ISO-8859-1 (Latin-1)
/// - `1` : UTF-16 avec byte-order-mark (BOM), little ou big-endian
/// - `2` : UTF-16BE, sans BOM
/// - `3` : UTF-8 (ID3v2.4 uniquement)
fn decode_string(encoding: u8, data: &[u8]) -> Result<String, Mp3Error> {
    match encoding {
        0 => Ok(data.iter().map(|&byte| byte as char).collect()),

        1 => {
            if data.is_empty() {
                return Ok(String::new());
            }
            if data.len() < 2 {
                return Err(Mp3Error::InvalidTextData { encoding });
            }

            let bom = [data[0], data[1]];
            if !matches!(bom, [0xFF, 0xFE] | [0xFE, 0xFF]) {
                return Err(Mp3Error::InvalidTextData { encoding });
            }

            let little_endian = bom == [0xFF, 0xFE];
            decode_utf16(&data[2..], little_endian, encoding)
        }

        2 => decode_utf16(data, false, encoding),

        3 => std::str::from_utf8(data)
            .map(str::to_owned)
            .map_err(|_| Mp3Error::InvalidTextData { encoding }),

        _ => Err(Mp3Error::UnknownTextEncoding { encoding }),
    }
}
/// Assemble des paires d'octets en unités UTF-16 puis en chaîne.
fn decode_utf16(data: &[u8], little_endian: bool, encoding: u8) -> Result<String, Mp3Error> {
    let (pairs, _) = data.as_chunks::<2>();
    let units: Vec<u16> = pairs
        .iter()
        .map(|&pair| {
            if little_endian {
                u16::from_le_bytes(pair)
            } else {
                u16::from_be_bytes(pair)
            }
        })
        .collect();

    String::from_utf16(&units).map_err(|_| Mp3Error::InvalidTextData { encoding })
}

//
// ---------- TESTS ----------
//

#[cfg(test)]
mod tests;
