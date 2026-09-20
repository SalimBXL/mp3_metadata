use mp3_metadata::{Id3v2Tag, Mp3File, MpegAudio, read_mp3_file, read_mp3_file_with_audio};
use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut path = None;
    let mut verbose = false;
    let mut verify = false;
    let mut load_audio = false;
    #[cfg(feature = "verify")]
    let mut limit = mp3_metadata::verify::DEFAULT_SEARCH_LIMIT;
    #[cfg(not(feature = "verify"))]
    let mut limit: u32 = 20;

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--verbose" | "-v" => verbose = true,
            "--verify" => verify = true,
            "--load-audio" => load_audio = true,
            "--limit" => {
                let Some(value) = args.next() else {
                    eprintln!("--limit attend une valeur (ex. --limit 20)");
                    return ExitCode::FAILURE;
                };
                match value.parse() {
                    Ok(n) => limit = n,
                    Err(_) => {
                        eprintln!("--limit attend un nombre entier positif, reçu : {value}");
                        return ExitCode::FAILURE;
                    }
                }
            }
            _ => path = Some(arg),
        }
    }

    let Some(path) = path else {
        eprintln!(
            "Usage : mp3_metadata [--verbose] [--verify] [--load-audio] [--limit N] <fichier.mp3>"
        );
        return ExitCode::FAILURE;
    };

    if let Err(err) = run(&path, verbose, verify, load_audio, limit) {
        eprintln!("Erreur : {err}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

/// Orchestre la commande : lit le fichier selon les options demandées,
/// puis délègue chaque bloc d'affichage à sa propre fonction. Les
/// sections "MP3", "Audio" (format et durée) et "ID3v2" s'affichent via
/// leur propre `Display` ; seuls les éléments propres à la CLI (frames en
/// détail, octets audio bruts effectivement chargés, vérification en
/// ligne) sont assemblés ici.
fn run(
    path: &str,
    verbose: bool,
    verify: bool,
    load_audio: bool,
    limit: u32,
) -> Result<(), mp3_metadata::Mp3Error> {
    // Par défaut, seuls le tag ID3v2 et une petite sonde MPEG sont lus
    // (quelques dizaines de Ko au plus) : les données audio elles-mêmes,
    // potentiellement énormes, ne sont chargées que si --load-audio est
    // passé.
    let mp3 = if load_audio {
        read_mp3_file_with_audio(path)?
    } else {
        read_mp3_file(path)?
    };

    println!("{mp3}");
    println!();

    if let Some(audio_format) = &mp3.audio_format {
        println!("{audio_format}");
        if let Some(audio) = &mp3.audio {
            print_loaded_audio_size(audio);
        }
        println!();
    }

    print_id3_tags(&mp3);

    let Some(tag) = &mp3.id3v2 else {
        return Ok(());
    };

    if verbose {
        println!();
        print_frames(tag);
    }

    if verify {
        println!();
        print_verification(tag, limit);
    }

    Ok(())
}

/// Affiche les tags ID3v2 et ID3v1 d'un fichier.
///
/// Si les deux sont présents, ils sont affichés côte à côte (voir
/// [`side_by_side`]) plutôt que l'un sous l'autre : le tag ID3v1, plus
/// sommaire, vient ainsi compléter le tag ID3v2 sans allonger la sortie.
/// Si un seul des deux est présent, seul celui-là s'affiche ; s'il n'y en
/// a aucun, un message l'indique.
fn print_id3_tags(mp3: &Mp3File) {
    match (&mp3.id3v2, &mp3.id3v1) {
        (Some(v2), Some(v1)) => println!("{}", side_by_side(&v2.to_string(), &v1.to_string())),
        (Some(v2), None) => println!("{v2}"),
        (None, Some(v1)) => {
            println!("Pas de tag ID3v2");
            println!();
            println!("{v1}");
        }
        (None, None) => println!("Pas de tag ID3v2"),
    }
}

/// Nombre d'espaces séparant les deux colonnes dans [`side_by_side`].
const COLUMN_GAP: usize = 4;

/// Assemble deux blocs de texte multi-lignes côte à côte, séparés par
/// [`COLUMN_GAP`] espaces.
///
/// La largeur de la colonne de gauche s'aligne sur sa ligne la plus
/// longue ; les lignes plus courtes de `left` sont complétées par des
/// espaces pour que la colonne de droite reste alignée verticalement sur
/// toutes les lignes. Si un bloc a plus de lignes que l'autre (par
/// exemple un tag ID3v2 avec plusieurs pochettes), les lignes en trop de
/// `left` restent seules sur leur ligne plutôt que de laisser des espaces
/// de fin inutiles.
fn side_by_side(left: &str, right: &str) -> String {
    let left_lines: Vec<&str> = left.lines().collect();
    let right_lines: Vec<&str> = right.lines().collect();
    let left_width = left_lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);

    let total_lines = left_lines.len().max(right_lines.len());
    let mut out = String::new();

    for i in 0..total_lines {
        let l = left_lines.get(i).copied().unwrap_or("");
        let r = right_lines.get(i).copied().unwrap_or("");

        if i > 0 {
            out.push('\n');
        }

        if r.is_empty() {
            out.push_str(l);
        } else {
            let gap = " ".repeat(COLUMN_GAP);
            out.push_str(&format!("{l:<left_width$}{gap}{r}"));
        }
    }

    out
}

/// Affiche la taille des données audio brutes effectivement chargées
/// (mode --load-audio) — distinct du format et de la durée estimée,
/// affichés par défaut sans charger l'audio complet.
fn print_loaded_audio_size(audio: &MpegAudio) {
    let taille_mio = audio.data.len() as f64 / (1024.0 * 1024.0);
    println!(
        "{:<11}: {} octets chargés ({taille_mio:.2} MiB)",
        "Loaded",
        audio.data.len()
    );
}

/// Affiche le détail de chaque frame du tag (mode --verbose).
fn print_frames(tag: &Id3v2Tag) {
    for frame in &tag.frames {
        println!("{frame}");
    }
}

/// Interroge MusicBrainz et affiche le résultat (mode --verify), jusqu'à
/// `limit` candidats (voir --limit).
///
/// Un échec de vérification (pas de réseau, aucun résultat...) n'invalide
/// pas le reste : le tag local a bien été lu, seul le contrôle en ligne
/// n'a pas abouti.
fn print_verification(tag: &Id3v2Tag, limit: u32) {
    #[cfg(feature = "verify")]
    match mp3_metadata::verify::verify_tag(tag, limit) {
        Ok(reports) => print!("{}", mp3_metadata::verify::VerificationTable(&reports)),
        Err(err) => eprintln!("Vérification MusicBrainz impossible : {err}"),
    }
    #[cfg(not(feature = "verify"))]
    {
        let _ = (tag, limit); // non utilisés sans la feature "verify"
        eprintln!("Compilé sans la fonctionnalité 'verify' (voir Cargo.toml)");
    }
}

//
// ---------- TESTS ----------
//
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_side_by_side_pads_left_column_to_its_own_width() {
        let left = "AB\nC";
        let right = "x\ny";
        let gap = " ".repeat(COLUMN_GAP);

        let result = side_by_side(left, right);

        assert_eq!(result, format!("AB{gap}x\nC {gap}y"));
    }

    #[test]
    fn test_side_by_side_keeps_extra_left_lines_alone() {
        // Bloc de gauche plus long que celui de droite (ex. un tag ID3v2
        // avec plusieurs pochettes) : les lignes en trop restent seules.
        let left = "L1\nL2\nL3";
        let right = "R1";

        let result = side_by_side(left, right);
        let lines: Vec<&str> = result.lines().collect();

        assert_eq!(lines.len(), 3);
        assert!(lines[0].starts_with("L1") && lines[0].ends_with("R1"));
        assert_eq!(lines[1], "L2");
        assert_eq!(lines[2], "L3");
    }

    #[test]
    fn test_side_by_side_empty_right_returns_left_unchanged() {
        assert_eq!(side_by_side("A\nB", ""), "A\nB");
    }

    #[test]
    fn test_side_by_side_empty_left_still_shows_right() {
        let result = side_by_side("", "R1\nR2");
        let lines: Vec<&str> = result.lines().collect();

        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with("R1"));
        assert!(lines[1].ends_with("R2"));
    }
}
