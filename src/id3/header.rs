struct Id3Header {
    version: Id3Version,
    flags: u8,
    size: u32,
}

struct Header {
    major: u8,
    minor: u8,
    flags: u8,
    size: u32,
    data: Vec<u8>,
}

