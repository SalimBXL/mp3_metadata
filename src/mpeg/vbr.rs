//! Durée et débit moyen d'un fichier MP3, à partir d'un éventuel en-tête
//! Xing/Info ou VBRI trouvé dans la première frame audio.
//!
//! Les encodeurs à débit variable (VBR) — et beaucoup à débit constant
//! aussi, LAME en tête — y déclarent le nombre total de frames du
//! fichier, ce qui donne une durée exacte plutôt qu'estimée à partir de
//! la seule taille du fichier. [`AudioFormat`] combine ce calcul avec
//! l'en-tête de frame décodé par le module parent ([`super`]).
//!
//! # Portée volontairement limitée
//!
//! - Xing/Info et VBRI ne sont cherchés que dans la première frame audio
//!   trouvée, et seulement si cette recherche tombe dans la fenêtre déjà
//!   sondée pour y trouver l'en-tête de frame (voir
//!   [`crate::MPEG_PROBE_LEN`]) — largement suffisant en pratique, ces
//!   en-têtes se trouvant toujours au tout début de cette première frame.
//!   Si aucun des deux n'est trouvé (fichier CBR sans métadonnées LAME,
//!   ou VBR encodé par un outil qui n'en écrit pas), la durée retombe sur
//!   l'estimation à débit constant : `octets_audio * 8 / débit`, qui peut
//!   alors être sensiblement fausse pour un fichier réellement VBR.
//! - Ni Xing/Info ni VBRI ne rendent compte du délai et du padding que
//!   certains encodeurs (LAME notamment) ajoutent en début et fin de
//!   flux pour un décodage "gapless" ; ces quelques dizaines de
//!   millisecondes ne sont pas retranchées de la durée calculée ici.

use super::{ChannelMode, MpegFrameHeader, MpegLayer, MpegVersion};
use std::fmt;

#[cfg(test)]
use super::parse_frame_header;

/// Format audio d'un fichier MP3, déduit de sa première frame audio, et
/// durée estimée à partir de la taille du fichier — voir les limites en
/// tête de module.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioFormat {
    /// En-tête de la première frame audio trouvée, d'où sont dérivés le
    /// débit, le taux d'échantillonnage, etc. Pour un fichier VBR, son
    /// `bitrate_kbps` n'est que celui déclaré par cette première frame —
    /// voir [`AudioFormat::average_bitrate_kbps`] pour une valeur
    /// représentative de l'ensemble du fichier.
    pub header: MpegFrameHeader,
    /// Durée en secondes — voir [`AudioFormat::duration_is_exact`] pour
    /// savoir si elle est exacte ou estimée.
    pub duration_secs: f64,
    /// `true` si [`AudioFormat::duration_secs`] a été calculée à partir
    /// du nombre exact de frames déclaré par un en-tête Xing/Info ou
    /// VBRI trouvé dans la première frame audio ; `false` si elle n'est
    /// qu'une estimation à débit constant (CBR) — voir les limites en
    /// tête de module. Ne préjuge pas que le fichier soit VBR ou CBR : un
    /// fichier CBR encodé par LAME porte généralement un en-tête `"Info"`
    /// tout aussi exact que le `"Xing"` d'un fichier réellement VBR.
    pub duration_is_exact: bool,
    /// Débit binaire moyen sur l'ensemble du fichier, calculé à partir de
    /// [`AudioFormat::duration_secs`] quand celle-ci est exacte — `None`
    /// sinon (voir [`AudioFormat::duration_is_exact`]). Pour un fichier
    /// VBR, c'est cette valeur qui est représentative, pas
    /// `header.bitrate_kbps` qui n'est que celui de la première frame.
    pub average_bitrate_kbps: Option<u32>,
}
impl AudioFormat {
    /// Construit un format audio à partir d'un en-tête de frame et du
    /// nombre d'octets audio du fichier (hors tag ID3v2), en supposant un
    /// débit constant. Utilisé par [`AudioFormat::from_probe`] quand
    /// aucun en-tête Xing/Info/VBRI n'est trouvé.
    fn from_header_and_audio_bytes(header: MpegFrameHeader, audio_bytes: u64) -> Self {
        let duration_secs = (audio_bytes as f64 * 8.0) / (header.bitrate_kbps as f64 * 1000.0);
        AudioFormat {
            header,
            duration_secs,
            duration_is_exact: false,
            average_bitrate_kbps: None,
        }
    }

    /// Construit un format audio à partir d'un en-tête de frame déjà
    /// décodé, du nombre d'octets audio du fichier (hors tag ID3v2), et
    /// de la fenêtre sondée dans laquelle cet en-tête a été trouvé (voir
    /// [`crate::MPEG_PROBE_LEN`]) — nécessaire pour y chercher un en-tête
    /// Xing/Info ou VBRI (voir [`vbr_stream_info`]).
    ///
    /// `frame_offset` est la position, dans `probe`, où commence la
    /// première frame trouvée (voir [`super::find_frame_header`]) : c'est à
    /// partir de là, et seulement là, que Xing/Info/VBRI sont cherchés —
    /// ni avant (aucun sens), ni sur une éventuelle frame suivante (leur
    /// emplacement standard est toujours la toute première frame audio du
    /// fichier).
    ///
    /// Retombe sur [`AudioFormat::from_header_and_audio_bytes`] (estimation
    /// CBR) si aucun des deux en-têtes n'est trouvé.
    pub(crate) fn from_probe(
        probe: &[u8],
        frame_offset: usize,
        header: MpegFrameHeader,
        audio_bytes: u64,
    ) -> Self {
        let frame = probe.get(frame_offset..).unwrap_or(&[]);

        let Some(info) = vbr_stream_info(frame, header.version, header.channel_mode) else {
            return Self::from_header_and_audio_bytes(header, audio_bytes);
        };

        let duration_secs = info.frame_count as f64
            * samples_per_frame(header.version, header.layer) as f64
            / header.sample_rate_hz as f64;

        // À défaut d'une taille de flux déclarée par l'en-tête lui-même
        // (Xing/Info sans le bit "bytes", voir [`parse_xing_stream_info`]),
        // la taille audio du fichier reste une valeur raisonnable : c'est
        // la même que celle utilisée pour l'estimation CBR par ailleurs.
        let total_bytes = info.total_bytes.map(|b| b as f64).unwrap_or(audio_bytes as f64);
        let average_bitrate_kbps = (duration_secs > 0.0)
            .then(|| ((total_bytes * 8.0) / duration_secs / 1000.0).round() as u32);

        AudioFormat {
            header,
            duration_secs,
            duration_is_exact: true,
            average_bitrate_kbps,
        }
    }
}
/// Affiche la section "Audio" : format MPEG, débit, taux
/// d'échantillonnage, canaux, et durée (voir les limites en tête de
/// module concernant sa précision). Un `~` précède la durée quand elle
/// n'est qu'une estimation (voir [`AudioFormat::duration_is_exact`]), et
/// le débit affiché est le débit moyen réel plutôt que celui de la seule
/// première frame quand il est connu (voir
/// [`AudioFormat::average_bitrate_kbps`]).
impl fmt::Display for AudioFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let minutes = (self.duration_secs / 60.0) as u64;
        let seconds = (self.duration_secs % 60.0) as u64;
        let sample_rate_khz = self.header.sample_rate_hz as f64 / 1000.0;
        let estimated = if self.duration_is_exact { "" } else { "~" };

        writeln!(f, "Audio")?;
        writeln!(f, "{}", crate::SECTION_SEPARATOR)?;
        writeln!(
            f,
            "{:<11}: {} {}",
            "MPEG", self.header.version, self.header.layer
        )?;
        match self.average_bitrate_kbps {
            Some(avg) => writeln!(f, "{:<11}: {avg} kbps (moyen)", "Bitrate")?,
            None => writeln!(f, "{:<11}: {} kbps", "Bitrate", self.header.bitrate_kbps)?,
        }
        writeln!(f, "{:<11}: {sample_rate_khz:.1} kHz", "Sample rate")?;
        writeln!(f, "{:<11}: {}", "Channels", self.header.channel_mode)?;
        write!(f, "{:<11}: {estimated}{minutes}:{seconds:02}", "Duration")
    }
}
/// Nombre d'échantillons audio encodés par frame, selon la version et la
/// couche MPEG — nécessaire pour convertir un nombre de frames (Xing/Info
/// ou VBRI, voir [`vbr_stream_info`]) en durée exacte. MPEG2/2.5 Layer III
/// encode deux fois moins d'échantillons par frame que MPEG1 pour le même
/// taux d'échantillonnage nominal ; les autres couches ne changent pas
/// selon la version.
fn samples_per_frame(version: MpegVersion, layer: MpegLayer) -> u32 {
    match layer {
        MpegLayer::LayerI => 384,
        MpegLayer::LayerII => 1152,
        MpegLayer::LayerIII => match version {
            MpegVersion::V1 => 1152,
            MpegVersion::V2 | MpegVersion::V2_5 => 576,
        },
    }
}
/// Taille (en octets) de l'information annexe ("side info") qui suit
/// immédiatement l'en-tête de 4 octets d'une frame Layer III, avant son
/// contenu audio — c'est juste après elle qu'un en-tête Xing/Info est
/// placé s'il y en a un (voir [`parse_xing_stream_info`]). Plus courte en
/// MPEG2/2.5 (taux d'échantillonnage divisé par deux, donc moins de
/// données à décrire) et en mono (un seul canal).
fn side_info_len(version: MpegVersion, channel_mode: ChannelMode) -> usize {
    match (version, channel_mode) {
        (MpegVersion::V1, ChannelMode::Mono) => 17,
        (MpegVersion::V1, _) => 32,
        (_, ChannelMode::Mono) => 9,
        (_, _) => 17,
    }
}
/// Ce qu'un en-tête Xing/Info ou VBRI déclare sur l'ensemble du flux audio
/// — pas seulement la frame qui le porte — et qui intéresse ce module :
/// voir [`vbr_stream_info`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VbrStreamInfo {
    /// Nombre total de frames audio dans le fichier.
    frame_count: u32,
    /// Taille totale du flux audio en octets, si l'en-tête la déclare —
    /// toujours le cas pour VBRI ; conditionnel pour Xing/Info (voir le
    /// bit 1 de ses flags dans [`parse_xing_stream_info`]).
    total_bytes: Option<u32>,
}
/// Cherche un en-tête Xing ou Info dans cette frame, et renvoie ce qu'il
/// déclare sur l'ensemble du flux, s'il déclare au moins le nombre de
/// frames (voir le bit 0 de ses flags — sans lui, l'en-tête ne nous sert
/// à rien ici).
///
/// `frame` doit commencer à l'en-tête de 4 octets de la première frame
/// audio (voir [`super::find_frame_header`]). Les encodeurs qui écrivent ce genre
/// d'en-tête (LAME, entre autres) le placent juste après l'information
/// annexe de cette première frame (voir [`side_info_len`]), qu'ils
/// laissent sinon silencieuse pour y faire de la place. L'étiquette est
/// `"Xing"` pour un fichier réellement à débit variable, `"Info"` pour un
/// débit constant accompagné quand même de ces métadonnées (LAME le fait
/// systématiquement) — les deux sont traitées de la même façon ici : dans
/// les deux cas, le nombre de frames déclaré donne une durée exacte, pas
/// la peine de distinguer VBR/CBR pour ce calcul.
fn parse_xing_stream_info(
    frame: &[u8],
    version: MpegVersion,
    channel_mode: ChannelMode,
) -> Option<VbrStreamInfo> {
    let offset = 4 + side_info_len(version, channel_mode);
    let tag = frame.get(offset..offset + 4)?;
    if tag != b"Xing" && tag != b"Info" {
        return None;
    }

    let flags = u32::from_be_bytes(frame.get(offset + 4..offset + 8)?.try_into().ok()?);
    if flags & 0x1 == 0 {
        return None; // bit "nombre de frames présent" non posé : rien d'exploitable
    }
    let frame_count = u32::from_be_bytes(frame.get(offset + 8..offset + 12)?.try_into().ok()?);

    let total_bytes = (flags & 0x2 != 0)
        .then(|| frame.get(offset + 12..offset + 16))
        .flatten()
        .and_then(|bytes| bytes.try_into().ok())
        .map(u32::from_be_bytes);

    Some(VbrStreamInfo {
        frame_count,
        total_bytes,
    })
}
/// Décalage, depuis le début de la frame, auquel un en-tête VBRI
/// commence — voir [`parse_vbri_stream_info`].
const VBRI_OFFSET: usize = 36;
/// Cherche un en-tête VBRI dans cette frame, et renvoie ce qu'il déclare
/// sur l'ensemble du flux.
///
/// `frame` doit commencer à l'en-tête de 4 octets de la première frame
/// audio, comme pour [`parse_xing_stream_info`]. Contrairement à
/// Xing/Info, un en-tête VBRI (Fraunhofer) est toujours à un décalage
/// fixe ([`VBRI_OFFSET`]) depuis le début de la frame, quels que soient
/// la version MPEG ou le mode de canaux — ce décalage correspond à la
/// plus grande taille possible d'information annexe, laissée vide qu'elle
/// soit nécessaire ou non pour cette frame précise.
fn parse_vbri_stream_info(frame: &[u8]) -> Option<VbrStreamInfo> {
    let tag = frame.get(VBRI_OFFSET..VBRI_OFFSET + 4)?;
    if tag != b"VBRI" {
        return None;
    }

    // 4 (étiquette) + 2 (version) + 2 (délai) + 2 (qualité) = 10 octets
    // avant le nombre d'octets, puis 4 octets avant le nombre de frames.
    let bytes_offset = VBRI_OFFSET + 10;
    let total_bytes = u32::from_be_bytes(frame.get(bytes_offset..bytes_offset + 4)?.try_into().ok()?);
    let frame_count_offset = bytes_offset + 4;
    let frame_count = u32::from_be_bytes(
        frame
            .get(frame_count_offset..frame_count_offset + 4)?
            .try_into()
            .ok()?,
    );

    Some(VbrStreamInfo {
        frame_count,
        total_bytes: Some(total_bytes),
    })
}
/// Cherche un en-tête Xing/Info ou VBRI dans la première frame audio —
/// essayés dans cet ordre, Xing/Info étant le plus courant en pratique
/// (LAME, l'encodeur MP3 le plus répandu, l'écrit systématiquement).
/// Utilisé par [`AudioFormat::from_probe`] pour calculer une durée exacte
/// plutôt qu'estimée — voir les limites en tête de module.
fn vbr_stream_info(
    frame: &[u8],
    version: MpegVersion,
    channel_mode: ChannelMode,
) -> Option<VbrStreamInfo> {
    parse_xing_stream_info(frame, version, channel_mode).or_else(|| parse_vbri_stream_info(frame))
}

//
// ---------- TESTS ----------
//

#[cfg(test)]
mod tests;
