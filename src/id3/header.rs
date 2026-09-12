pub struct Id3Version {
    pub major: u8,
    pub minor: u8,
}

pub struct Header {
    pub version: Id3Version,
    pub flags: u8,
    pub size: u32,
    pub data: Vec<u8>,
}

pub fn read_header(data: &[u8]) -> Result<Header, Box<dyn std::error::Error>> {
    if data.len() < 10 {
        return Err("Fichier trop petit".into());
    }

    if &data[0..3] != b"ID3" {
        return Err("Pas de tag ID3v2 au début du fichier".into());
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
        .ok_or("Taille du tag ID3v2 invalide (dépasse la taille du fichier)")?;

    let header = Header {
        version: Id3Version { major, minor },
        flags,
        size,
        data: data[0..tag_end].to_vec(),
    };

    println!("-------------------------------");
    println!("ID3v2 détecté");
    println!(
        "Version : {}.{}",
        header.version.major, header.version.minor
    );
    println!("Flags   : {:02X}", header.flags);
    println!("Taille  : {} octets", header.size);
    println!("-------------------------------");

    Ok(header)
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_read_header() {}
}
