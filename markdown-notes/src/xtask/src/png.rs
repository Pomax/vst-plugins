//! A PNG of one colour, written for a UI test to paste or drop.
//!
//! The picture has to be a real file a decoder accepts, and nothing about it
//! matters beyond its size and that every pixel is the same, so the pixels go
//! into the file uncompressed: a stored deflate block is a deflate stream that
//! needs no compressor.

/// A `width` by `height` PNG, every pixel `colour` at full opacity.
pub fn solid(width: u32, height: u32, colour: [u8; 3]) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((height * (1 + width * 4)) as usize);
    for _ in 0..height {
        // The filter each row was encoded with: none.
        pixels.push(0);
        for _ in 0..width {
            pixels.extend_from_slice(&[colour[0], colour[1], colour[2], 255]);
        }
    }

    // Eight bits a channel, colour type 6 (red, green, blue, alpha), the only
    // compression and filter methods there are, not interlaced.
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]);

    let mut png = Vec::from(*b"\x89PNG\r\n\x1a\n");
    chunk(&mut png, b"IHDR", &header);
    chunk(&mut png, b"IDAT", &zlib(&pixels));
    chunk(&mut png, b"IEND", &[]);
    png
}

/// Length, kind, body, and the checksum over the kind and the body.
fn chunk(png: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    let mut covered = Vec::with_capacity(4 + body.len());
    covered.extend_from_slice(kind);
    covered.extend_from_slice(body);

    png.extend_from_slice(&(body.len() as u32).to_be_bytes());
    png.extend_from_slice(&covered);
    png.extend_from_slice(&crc32(&covered).to_be_bytes());
}

/// `data` as a zlib stream of stored blocks, which carry their bytes as they
/// are. Each holds at most 65535 of them, and the last is marked final.
fn zlib(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let mut rest = data;
    loop {
        let take = rest.len().min(0xFFFF);
        let (block, left) = rest.split_at(take);
        out.push(u8::from(left.is_empty()));
        out.extend_from_slice(&(take as u16).to_le_bytes());
        out.extend_from_slice(&(!(take as u16)).to_le_bytes());
        out.extend_from_slice(block);
        rest = left;
        if rest.is_empty() {
            break;
        }
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let carry = crc & 1;
            crc >>= 1;
            if carry != 0 {
                crc ^= 0xEDB8_8320;
            }
        }
    }
    !crc
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut low, mut high) = (1u32, 0u32);
    for byte in bytes {
        low = (low + u32::from(*byte)) % 65521;
        high = (high + low) % 65521;
    }
    (high << 16) | low
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_checksums_are_the_ones_the_formats_define() {
        // The CRC every PNG's empty IEND chunk carries, and the Adler-32 the
        // zlib specification gives for "abc".
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
        assert_eq!(adler32(b"abc"), 0x024D_0127);
    }

    #[test]
    fn a_picture_is_a_png_the_plugin_would_take() {
        let made = solid(40, 30, [200, 40, 160]);
        assert_eq!(markdown_notes_core::images::kind_of(&made), Some("image/png"));
    }

    #[test]
    fn a_picture_decodes_to_the_size_and_colour_it_was_asked_for() {
        let made = solid(40, 30, [200, 40, 160]);
        let decoded = image::load_from_memory(&made)
            .expect("the picture does not decode")
            .into_rgba8();

        assert_eq!(decoded.dimensions(), (40, 30));
        assert!(decoded.pixels().all(|pixel| pixel.0 == [200, 40, 160, 255]));
    }

    #[test]
    fn a_picture_larger_than_one_stored_block_decodes_too() {
        // Over 65535 bytes of pixels, so the zlib stream has to run to a
        // second block.
        let made = solid(200, 200, [10, 20, 30]);
        let decoded = image::load_from_memory(&made)
            .expect("the picture does not decode")
            .into_rgba8();

        assert_eq!(decoded.dimensions(), (200, 200));
        assert!(decoded.pixels().all(|pixel| pixel.0 == [10, 20, 30, 255]));
    }
}
