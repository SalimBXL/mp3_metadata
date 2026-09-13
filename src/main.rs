fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mp3_filename = "a_kind_of_magic.mp3";
    let mp3_file = mp3_metadata::read_mp3_file(mp3_filename)?;

    println!("{}", mp3_file);

    if let Some(id3v2) = &mp3_file.id3v2 {
        println!("{}", id3v2);

        let frames = mp3_metadata::read_frames(id3v2)?;
        for frame in &frames {
            println!("{}", frame);
        }
    } else {
        println!("Pas de tag ID3v2");
    }

    Ok(())
}
