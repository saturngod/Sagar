//! Image probing for KFX. Source bytes are preserved; only JPEG, PNG and GIF
//! are supported. Dimensions are read from the format headers.

pub struct ImageResource {
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// KFX format symbol: 285 JPEG, 284 PNG, 286 GIF.
    pub format: u64,
    pub mime: &'static str,
}

impl ImageResource {
    pub fn probe(data: Vec<u8>) -> Option<Self> {
        let (width, height, format, mime) = if data.starts_with(&[0xff, 0xd8]) {
            let (w, h) = jpeg_size(&data)?;
            (w, h, 285, "image/jpeg")
        } else if data.starts_with(&[0x89, b'P', b'N', b'G']) {
            if data.len() < 24 || &data[12..16] != b"IHDR" {
                return None;
            }
            let w = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
            let h = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
            (w, h, 284, "image/png")
        } else if data.starts_with(b"GIF") {
            if data.len() < 10 {
                return None;
            }
            let w = u16::from_le_bytes([data[6], data[7]]) as u32;
            let h = u16::from_le_bytes([data[8], data[9]]) as u32;
            (w, h, 286, "image/gif")
        } else {
            return None;
        };
        (width > 0 && height > 0).then_some(Self {
            data,
            width,
            height,
            format,
            mime,
        })
    }
}

fn jpeg_size(data: &[u8]) -> Option<(u32, u32)> {
    let mut index = 2;
    while index + 4 <= data.len() {
        if data[index] != 0xff {
            return None;
        }
        let marker = data[index + 1];
        if marker == 0xff {
            index += 1;
            continue;
        }
        if marker == 0xd8 || marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            index += 2;
            continue;
        }
        let length = u16::from_be_bytes([data[index + 2], data[index + 3]]) as usize;
        let is_sof = (0xc0..=0xcf).contains(&marker) && !matches!(marker, 0xc4 | 0xc8 | 0xcc);
        if is_sof {
            if index + 9 > data.len() {
                return None;
            }
            let h = u16::from_be_bytes([data[index + 5], data[index + 6]]) as u32;
            let w = u16::from_be_bytes([data[index + 7], data[index + 8]]) as u32;
            return Some((w, h));
        }
        if length < 2 {
            return None;
        }
        index += 2 + length;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probes_png_gif_and_jpeg_headers() {
        let mut png = vec![0x89, b'P', b'N', b'G', 13, 10, 26, 10, 0, 0, 0, 13];
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&[0, 0, 0, 3, 0, 0, 0, 2]);
        let png = ImageResource::probe(png).expect("png");
        assert_eq!((png.width, png.height, png.format), (3, 2, 284));

        let gif = ImageResource::probe(b"GIF89a\x05\x00\x07\x00".to_vec()).expect("gif");
        assert_eq!((gif.width, gif.height), (5, 7));

        let jpeg = vec![
            0xff, 0xd8, 0xff, 0xe0, 0, 4, 0, 0, 0xff, 0xc0, 0, 11, 8, 0, 20, 0, 10, 3,
        ];
        let jpeg = ImageResource::probe(jpeg).expect("jpeg");
        assert_eq!((jpeg.width, jpeg.height, jpeg.format), (10, 20, 285));

        assert!(ImageResource::probe(b"<svg/>".to_vec()).is_none());
    }
}
