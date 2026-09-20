use mp3_metadata::{Id3v2Tag, MpegAudio, read_mp3_file, read_mp3_file_with_audio};
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

    let Some(tag) = &mp3.id3v2 else {
        println!("Pas de tag ID3v2");
        return Ok(());
    };

    println!("{tag}");

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
