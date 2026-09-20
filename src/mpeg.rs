//! Décodage minimal d'un en-tête de frame audio MPEG (les 4 premiers
//! octets d'une frame MP3), à des fins d'affichage : version, couche,
//! débit binaire, taux d'échantillonnage, mode de canaux.
//!
//! Ne décode pas le flux audio lui-même — seulement l'en-tête de la
//! première frame trouvée, suffisant pour connaître le format du fichier
//! et estimer sa durée sans avoir à lire les données audio en entier.
//!
//! # Portée volontairement limitée
//!
//! - La recherche de la première frame ([`find_frame_header`]) ne
//!   vérifie pas qu'une deuxième frame valide suit à la position
//!   attendue. Un très court passage de données non-audio pourrait, par
//!   pure coïncidence, produire un faux positif — en pratique marginal,
//!   puisque la recherche démarre juste après le tag ID3v2 d'un fichier
//!   MP3 réel, pas au milieu de données arbitraires.
//! - Les frames à débit "free" (bitrate index `0000`, débit variable
//!   défini par comptage entre repères de synchronisation plutôt que
//!   déclaré dans l'en-tête) ne sont pas reconnues comme valides : elles
//!   sont exceptionnelles en pratique, et les gérer demanderait de
//!   localiser une deuxième frame pour en déduire le débit.
//! - La durée estimée ([`AudioFormat::duration_secs`]) suppose un débit
//!   constant (CBR) : `octets_audio * 8 / débit`. Pour un fichier à débit
//!   variable (VBR) sans repère Xing/VBRI interprété par cette
//!   bibliothèque, l'estimation peut être sensiblement fausse.

use std::fmt;

/// Version MPEG déclarée dans l'en-tête d'une frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MpegVersion {
    /// MPEG-1 (44.1 / 48 / 32 kHz).
    V1,
    /// MPEG-2 (« LSF », taux d'échantillonnage moitié de MPEG-1).
    V2,
    /// MPEG-2.5 (extension non officielle, taux d'échantillonnage encore
    /// plus bas, surtout utilisée pour la voix).
    V2_5,
}

impl fmt::Display for MpegVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MpegVersion::V1 => write!(f, "MPEG-1"),
            MpegVersion::V2 => write!(f, "MPEG-2"),
            MpegVersion::V2_5 => write!(f, "MPEG-2.5"),
        }
    }
}

/// Couche (layer) MPEG déclarée dans l'en-tête d'une frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MpegLayer {
    /// Layer I — rare pour du MP3, plutôt utilisé par des formats comme le
    /// DAB.
    LayerI,
    /// Layer II — utilisé par exemple par MP2, la radio DAB.
    LayerII,
    /// Layer III — la couche qui donne son nom au « MP3 » (`.mp3`).
    LayerIII,
}

impl fmt::Display for MpegLayer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MpegLayer::LayerI => write!(f, "Layer I"),
            MpegLayer::LayerII => write!(f, "Layer II"),
            MpegLayer::LayerIII => write!(f, "Layer III"),
        }
    }
}

/// Mode de canaux déclaré dans l'en-tête d'une frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelMode {
    /// Deux canaux encodés indépendamment.
    Stereo,
    /// Deux canaux encodés en exploitant leur redondance (partage de
    /// certaines informations entre canaux pour réduire le débit).
    JointStereo,
    /// Deux canaux mono indépendants regroupés dans une même frame (pas
    /// de mise en commun d'informations, contrairement à `JointStereo`).
    DualChannel,
    /// Un seul canal.
    Mono,
}

impl fmt::Display for ChannelMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChannelMode::Stereo => write!(f, "Stereo"),
            ChannelMode::JointStereo => write!(f, "Joint Stereo"),
            ChannelMode::DualChannel => write!(f, "Dual Channel"),
            ChannelMode::Mono => write!(f, "Mono"),
        }
    }
}

/// En-tête décodé d'une frame audio MPEG (ses 4 premiers octets).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MpegFrameHeader {
    /// Version MPEG (MPEG-1, MPEG-2, MPEG-2.5).
    pub version: MpegVersion,
    /// Couche MPEG (Layer I, II ou III).
    pub layer: MpegLayer,
    /// Débit binaire en kbit/s.
    pub bitrate_kbps: u16,
    /// Taux d'échantillonnage en Hz (ex. 44100 pour 44.1 kHz).
    pub sample_rate_hz: u32,
    /// Mode de canaux (stéréo, mono...).
    pub channel_mode: ChannelMode,
    /// Bit de padding de l'en-tête : `true` si cette frame porte un octet
    /// supplémentaire pour ajuster sa taille au débit binaire moyen visé.
    pub padding: bool,
}

/// Format audio d'un fichier MP3, déduit de sa première frame audio, et
/// durée estimée à partir de la taille du fichier — voir les limites en
/// tête de module.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioFormat {
    /// En-tête de la première frame audio trouvée, d'où sont dérivés le
    /// débit, le taux d'échantillonnage, etc.
    pub header: MpegFrameHeader,
    /// Durée estimée en secondes, à débit constant (CBR).
    pub duration_secs: f64,
}

impl AudioFormat {
    /// Construit un format audio à partir d'un en-tête de frame et du
    /// nombre d'octets audio du fichier (hors tag ID3v2), en supposant un
    /// débit constant.
    pub(crate) fn from_header_and_audio_bytes(header: MpegFrameHeader, audio_bytes: u64) -> Self {
        let duration_secs = (audio_bytes as f64 * 8.0) / (header.bitrate_kbps as f64 * 1000.0);
        AudioFormat {
            header,
            duration_secs,
        }
    }
}

/// Affiche la section "Audio" : format MPEG, débit, taux
/// d'échantillonnage, canaux, et durée estimée (voir les limites en tête
/// de module concernant cette dernière).
impl fmt::Display for AudioFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let minutes = (self.duration_secs / 60.0) as u64;
        let seconds = (self.duration_secs % 60.0) as u64;
        let sample_rate_khz = self.header.sample_rate_hz as f64 / 1000.0;

        writeln!(f, "Audio")?;
        writeln!(f, "{}", crate::SECTION_SEPARATOR)?;
        writeln!(
            f,
            "{:<11}: {} {}",
            "MPEG", self.header.version, self.header.layer
        )?;
        writeln!(f, "{:<11}: {} kbps", "Bitrate", self.header.bitrate_kbps)?;
        writeln!(f, "{:<11}: {sample_rate_khz:.1} kHz", "Sample rate")?;
        writeln!(f, "{:<11}: {}", "Channels", self.header.channel_mode)?;
        write!(f, "{:<11}: {minutes}:{seconds:02}", "Duration")
    }
}

/// Décode l'en-tête de frame MPEG codé sur ces 4 octets.
///
/// Renvoie `None` si les 11 premiers bits ne sont pas le repère de
/// synchronisation attendu, ou si l'un des champs (version, couche, débit,
/// taux d'échantillonnage) porte une valeur réservée ou non prise en
/// charge (voir les limites en tête de module pour le débit "free").
fn parse_frame_header(bytes: [u8; 4]) -> Option<MpegFrameHeader> {
    let word = u32::from_be_bytes(bytes);

    if word >> 21 != 0b111_1111_1111 {
        return None;
    }

    let version = match (word >> 19) & 0b11 {
        0b00 => MpegVersion::V2_5,
        0b10 => MpegVersion::V2,
        0b11 => MpegVersion::V1,
        _ => return None, // 0b01 réservé
    };

    let layer = match (word >> 17) & 0b11 {
        0b01 => MpegLayer::LayerIII,
        0b10 => MpegLayer::LayerII,
        0b11 => MpegLayer::LayerI,
        _ => return None, // 0b00 réservé
    };

    let bitrate_kbps = bitrate_table(version, layer, ((word >> 12) & 0b1111) as usize)?;
    let sample_rate_hz = sample_rate_table(version, ((word >> 10) & 0b11) as usize)?;
    let padding = (word >> 9) & 1 == 1;

    let channel_mode = match (word >> 6) & 0b11 {
        0b00 => ChannelMode::Stereo,
        0b01 => ChannelMode::JointStereo,
        0b10 => ChannelMode::DualChannel,
        _ => ChannelMode::Mono, // 0b11
    };

    Some(MpegFrameHeader {
        version,
        layer,
        bitrate_kbps,
        sample_rate_hz,
        channel_mode,
        padding,
    })
}

/// Cherche la première frame MPEG valide dans `data`, à partir du début.
///
/// Renvoie l'en-tête décodé et le décalage (en octets, dans `data`)
/// auquel il commence. `None` si aucune frame valide n'a été trouvée.
pub(crate) fn find_frame_header(data: &[u8]) -> Option<(usize, MpegFrameHeader)> {
    if data.len() < 4 {
        return None;
    }

    for offset in 0..=data.len() - 4 {
        let bytes: [u8; 4] = data[offset..offset + 4]
            .try_into()
            .expect("slice de 4 octets, conversion infaillible");
        if let Some(header) = parse_frame_header(bytes) {
            return Some((offset, header));
        }
    }

    None
}

/// Débit binaire (kbit/s) pour cette combinaison version/couche/index, ou
/// `None` pour un index hors table (`1111`, réservé) ou "free" (`0000`,
/// voir les limites en tête de module).
///
/// Table ISO/IEC 11172-3 : MPEG2 et MPEG2.5 partagent la même table,
/// distincte de celle de MPEG1.
fn bitrate_table(version: MpegVersion, layer: MpegLayer, index: usize) -> Option<u16> {
    const MPEG1: [[u16; 15]; 3] = [
        [
            0, 32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448,
        ], // Layer I
        [
            0, 32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384,
        ], // Layer II
        [
            0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
        ], // Layer III
    ];
    const MPEG2: [[u16; 15]; 3] = [
        [
            0, 32, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 224, 256,
        ], // Layer I
        [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160], // Layer II
        [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160], // Layer III
    ];

    let table = match version {
        MpegVersion::V1 => &MPEG1,
        MpegVersion::V2 | MpegVersion::V2_5 => &MPEG2,
    };

    let layer_index = match layer {
        MpegLayer::LayerI => 0,
        MpegLayer::LayerII => 1,
        MpegLayer::LayerIII => 2,
    };

    match table[layer_index].get(index) {
        Some(&0) | None => None, // "free" (index 0) ou hors table (index 15)
        Some(&kbps) => Some(kbps),
    }
}

/// Taux d'échantillonnage (Hz) pour cette version/index, ou `None` pour un
/// index hors table (`11`, réservé).
fn sample_rate_table(version: MpegVersion, index: usize) -> Option<u32> {
    const RATES: [[u32; 3]; 3] = [
        [44100, 48000, 32000], // MPEG1
        [22050, 24000, 16000], // MPEG2
        [11025, 12000, 8000],  // MPEG2.5
    ];

    let version_index = match version {
        MpegVersion::V1 => 0,
        MpegVersion::V2 => 1,
        MpegVersion::V2_5 => 2,
    };

    RATES[version_index].get(index).copied()
}

//
// ---------- TESTS ----------
//

#[cfg(test)]
mod tests {
    use super::*;

    // Vecteurs calculés bit à bit (voir la conversation de conception),
    // pas transcrits à la main depuis une table.

    #[test]
    fn test_parse_frame_header_mpeg1_layer3_320kbps_44100_stereo() {
        let header = parse_frame_header([0xFF, 0xFB, 0xE0, 0x00]).unwrap();

        assert_eq!(header.version, MpegVersion::V1);
        assert_eq!(header.layer, MpegLayer::LayerIII);
        assert_eq!(header.bitrate_kbps, 320);
        assert_eq!(header.sample_rate_hz, 44100);
        assert_eq!(header.channel_mode, ChannelMode::Stereo);
        assert!(!header.padding);
    }

    #[test]
    fn test_parse_frame_header_mpeg2_layer3_64kbps_24000_mono() {
        let header = parse_frame_header([0xFF, 0xF3, 0x84, 0xC0]).unwrap();

        assert_eq!(header.version, MpegVersion::V2);
        assert_eq!(header.layer, MpegLayer::LayerIII);
        assert_eq!(header.bitrate_kbps, 64);
        assert_eq!(header.sample_rate_hz, 24000);
        assert_eq!(header.channel_mode, ChannelMode::Mono);
    }

    #[test]
    fn test_parse_frame_header_mpeg2_5_layer3_32kbps_8000_joint_stereo_padded() {
        let header = parse_frame_header([0xFF, 0xE3, 0x4A, 0x40]).unwrap();

        assert_eq!(header.version, MpegVersion::V2_5);
        assert_eq!(header.layer, MpegLayer::LayerIII);
        assert_eq!(header.bitrate_kbps, 32);
        assert_eq!(header.sample_rate_hz, 8000);
        assert_eq!(header.channel_mode, ChannelMode::JointStereo);
        assert!(header.padding);
    }

    #[test]
    fn test_parse_frame_header_no_sync_returns_none() {
        assert!(parse_frame_header([0x00, 0x00, 0x00, 0x00]).is_none());
    }

    #[test]
    fn test_parse_frame_header_reserved_layer_returns_none() {
        // Bits de couche à 00, réservés.
        assert!(parse_frame_header([0xFF, 0xF9, 0x50, 0x00]).is_none());
    }

    #[test]
    fn test_parse_frame_header_free_bitrate_returns_none() {
        // Bitrate index 0000 ("free") : non pris en charge, voir la doc.
        let mut bytes = [0xFF, 0xFB, 0xE0, 0x00];
        bytes[2] &= 0b0000_1111; // remet l'index de débit à 0000
        assert!(parse_frame_header(bytes).is_none());
    }

    // ----- find_frame_header -----

    #[test]
    fn test_find_frame_header_at_start() {
        let data = [0xFF, 0xFB, 0xE0, 0x00, 0xAA, 0xAA];
        let (offset, header) = find_frame_header(&data).unwrap();

        assert_eq!(offset, 0);
        assert_eq!(header.bitrate_kbps, 320);
    }

    #[test]
    fn test_find_frame_header_after_some_padding() {
        let mut data = vec![0x00, 0x00, 0x00, 0x00, 0x00]; // padding avant l'audio
        data.extend_from_slice(&[0xFF, 0xFB, 0xE0, 0x00]);
        let (offset, header) = find_frame_header(&data).unwrap();

        assert_eq!(offset, 5);
        assert_eq!(header.bitrate_kbps, 320);
    }

    #[test]
    fn test_find_frame_header_none_when_absent() {
        let data = [0x00; 32];
        assert!(find_frame_header(&data).is_none());
    }

    #[test]
    fn test_find_frame_header_too_short() {
        assert!(find_frame_header(&[0xFF, 0xFB]).is_none());
    }

    // ----- AudioFormat -----

    #[test]
    fn test_audio_format_duration_at_320kbps() {
        let header = parse_frame_header([0xFF, 0xFB, 0xE0, 0x00]).unwrap();
        // 320 kbps pendant 10 secondes = 320_000 * 10 / 8 octets.
        let audio_bytes = 320_000 * 10 / 8;

        let format = AudioFormat::from_header_and_audio_bytes(header, audio_bytes);

        assert!((format.duration_secs - 10.0).abs() < 0.01);
    }

    #[test]
    fn test_audio_format_display_matches_expected_layout() {
        let header = parse_frame_header([0xFF, 0xFB, 0xE0, 0x00]).unwrap();
        // 320 kbps pendant 264 secondes (4 min 24 s) tapées rond.
        let format = AudioFormat::from_header_and_audio_bytes(header, 320_000 * 264 / 8);
        let text = format.to_string();

        assert!(text.starts_with("Audio\n"));
        assert!(text.contains("MPEG       : MPEG-1 Layer III"));
        assert!(text.contains("Bitrate    : 320 kbps"));
        assert!(text.contains("Sample rate: 44.1 kHz"));
        assert!(text.contains("Channels   : Stereo"));
        assert!(text.contains("Duration   : 4:24"));
    }
}
