fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mp3_filename = "a_kind_of_magic.mp3";
    let mp3_file = mp3_metadata::read_mp3_file(mp3_filename)?;

    println!("{}", mp3_file);
    println!("{}", mp3_file.header);

    let frames = mp3_metadata::read_frames(&mp3_file.header)?;
    for frame in &frames {
        println!("{}", frame);
    }

    Ok(())
}
