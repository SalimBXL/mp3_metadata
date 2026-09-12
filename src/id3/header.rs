use crate::error::Mp3Error;
/// Version d'un tag ID3v2, telle qu'elle apparaît dans l'en-tête du fichier.
///
/// `major` et `minor` correspondent aux deux octets de version situés juste
/// après la signature `ID3` (par exemple, `major = 3, minor = 0` pour
/// ID3v2.3.0).
#[derive(Debug)]
pub struct Id3Version {
    pub major: u8,
    pub minor: u8,
}

/// En-tête ID3v2 extrait du début d'un fichier MP3.
///
/// Une valeur `Header` est typiquement construite par [`read_header`], qui
/// analyse les 10 premiers octets d'un fichier MP3 pour en extraire la
/// version, les flags, la taille du tag, ainsi que le tag ID3v2 complet.
#[derive(Debug)]
pub struct Header {
    pub version: Id3Version,
    /// Octet de flags du tag ID3v2 (bits d'options telles que
    /// l'unsynchronisation, la présence d'un extended header, etc.).
    pub flags: u8,
    /// Taille du tag ID3v2 en octets, telle que déclarée dans l'en-tête
    /// (n'inclut pas les 10 octets de l'en-tête lui-même).
    pub size: u32,
    /// Contenu brut du tag ID3v2, en-tête compris (10 premiers octets +
    /// `size` octets de données de tag).
    pub data: Vec<u8>,
}

impl Header {
    /// Renvoie la taille du tag ID3v2 en kibioctets (Ko, base 1024).
    ///
    /// Calculée à partir de [`Header::size`] (taille du tag en octets,
    /// telle que déclarée dans l'en-tête ID3v2 — n'inclut pas les 10
    /// octets de l'en-tête lui-même), divisée par 1024.
    ///
    /// # Exemples
    ///
    /// ```ignore
    /// let header = read_header(&data)?;
    /// println!("{:.2} Ko", header.size_ko());
    /// ```
    pub fn size_ko(&self) -> f64 {
        self.size as f64 / 1024.0
    }
}

/// Affiche un résumé lisible de l'en-tête ID3v2 : version, flags et taille.
///
/// Le contenu de `data` n'est pas affiché (il serait illisible en brut).
impl std::fmt::Display for Header {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let size_ko = self.size_ko();
        writeln!(f, "- HEADER ----------------------")?;
        writeln!(f, "ID3v2 détecté")?;
        writeln!(f, "Version : {}.{}", self.version.major, self.version.minor)?;
        writeln!(f, "Flags   : {:02X}", self.flags)?;
        writeln!(f, "Taille  : {} octets ({size_ko:.2} Ko)", self.size)?;
        write!(f, "-------------------------------")
    }
}

/// Analyse l'en-tête ID3v2 situé au début d'un fichier MP3.
///
/// Lit les 10 premiers octets de `data` pour vérifier la signature `ID3`,
/// extraire la version, les flags et la taille du tag (encodée en
/// synchsafe integer sur 4 octets, voir la spécification ID3v2), puis
/// découpe le tag ID3v2 complet (en-tête + corps) dans [`Header::data`].
///
/// # Erreurs
///
/// - [`Mp3Error::TooSmall`] si `data` contient moins de 10 octets (trop
///   court pour contenir un en-tête ID3v2).
/// - [`Mp3Error::MissingId3Tag`] si les 3 premiers octets de `data` ne
///   correspondent pas à la signature `ID3`.
/// - [`Mp3Error::InvalidTagSize`] si la taille de tag déclarée dans
///   l'en-tête dépasse la taille réelle de `data` (fichier tronqué ou
///   en-tête corrompu).
///
/// # Exemples
///
/// ```ignore
/// let data = std::fs::read("musique/chanson.mp3")?;
/// let header = read_header(&data)?;
/// println!("{header}");
/// ```
pub fn read_header(data: &[u8]) -> Result<Header, Box<dyn std::error::Error>> {
    if data.len() < 10 {
        return Err(Mp3Error::TooSmall { len: data.len() }.into());
    }

    if &data[0..3] != b"ID3" {
        return Err(Mp3Error::MissingId3Tag.into());
    }

    let major = data[3];
    let minor = data[4];
    let flags = data[5];

    let size: u32 = ((data[6] as u32) << 21)
        | ((data[7] as u32) << 14)
        | ((data[8] as u32) << 7)
        | data[9] as u32;

    let tag_end = 10usize
        .checked_add(size as usize)
        .filter(|&end| end <= data.len())
        .ok_or(Mp3Error::InvalidTagSize {
            declared: size,
            available: data.len(),
        })?;

    let header = Header {
        version: Id3Version { major, minor },
        flags,
        size,
        data: data[0..tag_end].to_vec(),
    };
    Ok(header)
}

//
// ---------- TESTS ----------
//

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Mp3Error;

    /// Construit un en-tête ID3v2 valide avec la taille de tag donnée
    /// (encodée en synchsafe integer), suivi de `body_len` octets de
    /// remplissage pour simuler le corps du tag.
    fn build_id3_bytes(major: u8, minor: u8, flags: u8, body_len: u32) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(b"ID3");
        data.push(major);
        data.push(minor);
        data.push(flags);
        data.push(((body_len >> 21) & 0x7F) as u8);
        data.push(((body_len >> 14) & 0x7F) as u8);
        data.push(((body_len >> 7) & 0x7F) as u8);
        data.push((body_len & 0x7F) as u8);
        data.extend(std::iter::repeat_n(0u8, body_len as usize));
        data
    }

    #[test]
    fn test_read_header_too_small() {
        let data = [0u8; 5]; // moins de 10 octets
        let err = read_header(&data).unwrap_err();
        let mp3_err = err.downcast_ref::<Mp3Error>().unwrap();
        assert!(matches!(mp3_err, Mp3Error::TooSmall { len: 5 }));
    }

    #[test]
    fn test_read_header_missing_id3_tag() {
        let mut data = [0u8; 10];
        data[0..3].copy_from_slice(b"XYZ");
        let err = read_header(&data).unwrap_err();
        let mp3_err = err.downcast_ref::<Mp3Error>().unwrap();
        assert!(matches!(mp3_err, Mp3Error::MissingId3Tag));
    }

    #[test]
    fn test_read_header_invalid_tag_size() {
        // On déclare une taille de tag plus grande que les données réellement fournies.
        let mut data = build_id3_bytes(3, 0, 0, 100);
        data.truncate(20); // le fichier est plus court que ce que l'en-tête annonce

        let err = read_header(&data).unwrap_err();
        let mp3_err = err.downcast_ref::<Mp3Error>().unwrap();
        assert!(matches!(mp3_err, Mp3Error::InvalidTagSize { .. }));
    }

    #[test]
    fn test_read_header_valid() {
        let data = build_id3_bytes(3, 0, 0x80, 50);
        let header = read_header(&data).unwrap();

        assert_eq!(header.version.major, 3);
        assert_eq!(header.version.minor, 0);
        assert_eq!(header.flags, 0x80);
        assert_eq!(header.size, 50);
        assert_eq!(header.data.len(), 10 + 50); // en-tête (10) + corps (50)
    }

    #[test]
    fn test_read_header_valid_zero_size_tag() {
        // Un tag ID3v2 techniquement valide mais sans corps.
        let data = build_id3_bytes(4, 0, 0, 0);
        let header = read_header(&data).unwrap();

        assert_eq!(header.size, 0);
        assert_eq!(header.data.len(), 10);
    }
}
