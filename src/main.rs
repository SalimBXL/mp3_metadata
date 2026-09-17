use mp3_metadata::{FrameContent, read_mp3_file};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mp3 = read_mp3_file("a_kind_of_magic.mp3")?;
    println!("{mp3}");

    let Some(tag) = &mp3.id3v2 else {
        println!("Pas de tag ID3v2");
        return Ok(());
    };

    println!("{tag}");

    // Les accesseurs donnent directement les métadonnées usuelles.
    println!("Titre   : {}", tag.title().unwrap_or("?"));
    println!("Artiste : {}", tag.artist().unwrap_or("?"));
    println!("Album   : {}", tag.album().unwrap_or("?"));
    println!("Année   : {}", tag.year().unwrap_or("?"));
    for frame in tag.pictures() {
        if let FrameContent::Picture {
            mime_type, data, ..
        } = &frame.content
        {
            println!("Pochette : {mime_type}, {} octets", data.len());
        }
    }

    println!("===============================");

    /*
       // Et les frames restent accessibles une par une.
       for frame in &tag.frames {
           println!("{frame}");
       }
    */
    Ok(())
}
