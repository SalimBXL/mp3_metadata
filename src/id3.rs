pub mod frame;
pub mod header;

/// Décode un entier "synchsafe" sur 4 octets : chaque octet ne porte que
/// 7 bits utiles (le bit de poids fort vaut toujours 0), afin qu'aucun
/// champ de taille ne puisse, par accident, contenir une suite d'octets
/// ressemblant à un repère de synchronisation MPEG.
///
/// Utilisé pour la taille du tag ID3v2 (toutes versions) et pour la
/// taille des frames en ID3v2.4.
pub(crate) fn synchsafe_to_u32(bytes: [u8; 4]) -> u32 {
    ((bytes[0] as u32) << 21)
        | ((bytes[1] as u32) << 14)
        | ((bytes[2] as u32) << 7)
        | (bytes[3] as u32)
}

/// Annule l'unsynchronisation ID3v2 : toute séquence `0xFF 0x00` devient
/// `0xFF`, l'octet `0x00` inséré à l'écriture étant retiré.
///
/// L'unsynchronisation empêche qu'une frame ID3v2 ne contienne, par
/// accident, une suite d'octets ressemblant à un repère de synchronisation
/// MPEG (`0xFF` suivi d'un octet dont les 3 bits de poids fort sont à 1),
/// ce qui perturberait un lecteur cherchant le flux audio en sautant
/// directement au milieu du fichier.
///
/// Ne traite que l'unsynchronisation déclarée au niveau du tag (bit 7 des
/// flags de l'en-tête principal). ID3v2.4 permet aussi une
/// unsynchronisation déclarée frame par frame ; ce cas n'est pas géré ici.
pub(crate) fn deunsynchronize(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        out.push(data[i]);
        if data[i] == 0xFF && data.get(i + 1) == Some(&0x00) {
            i += 2;
        } else {
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_synchsafe_to_u32_synthetic() {
        // (2 << 7) + 1 = 257, valeurs choisies pour être vérifiables à la main.
        assert_eq!(synchsafe_to_u32([0x00, 0x00, 0x02, 0x01]), 257);
    }

    #[test]
    fn test_synchsafe_to_u32_real_file() {
        // Octets exacts de la taille de tag dans a_kind_of_magic.mp3.
        assert_eq!(synchsafe_to_u32([0x00, 0x01, 0x5B, 0x61]), 28129);
    }

    #[test]
    fn test_synchsafe_to_u32_zero() {
        assert_eq!(synchsafe_to_u32([0, 0, 0, 0]), 0);
    }

    #[test]
    fn test_deunsynchronize_removes_stuffed_zero() {
        assert_eq!(deunsynchronize(&[0xFF, 0x00, 0x01]), vec![0xFF, 0x01]);
    }

    #[test]
    fn test_deunsynchronize_keeps_ff_not_followed_by_zero() {
        assert_eq!(deunsynchronize(&[0xFF, 0x01]), vec![0xFF, 0x01]);
    }

    #[test]
    fn test_deunsynchronize_keeps_consecutive_pairs() {
        // Deux paires consécutives : chacune perd son 0x00.
        assert_eq!(
            deunsynchronize(&[0xFF, 0x00, 0xFF, 0x00, 0x01]),
            vec![0xFF, 0xFF, 0x01]
        );
    }

    #[test]
    fn test_deunsynchronize_trailing_ff_without_following_byte() {
        assert_eq!(deunsynchronize(&[0x01, 0xFF]), vec![0x01, 0xFF]);
    }

    #[test]
    fn test_deunsynchronize_empty() {
        assert_eq!(deunsynchronize(&[]), Vec::<u8>::new());
    }
}
