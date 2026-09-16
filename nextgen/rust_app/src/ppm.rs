//! Reader for binary PPM (Netpbm P6) images, used both for the surface maps of the 3D
//! engine and as a real image source for the FITS conversion tool.

#[derive(Clone, Debug, PartialEq)]
pub struct PpmImage {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

impl PpmImage {
    pub fn pixel_count(&self) -> usize {
        self.width as usize * self.height as usize
    }

    /// Rec. 709 luminance of every pixel, in the original 0-255 range.
    pub fn luminance(&self) -> Vec<f64> {
        self.rgb
            .chunks_exact(3)
            .map(|pixel| {
                0.2126 * pixel[0] as f64 + 0.7152 * pixel[1] as f64 + 0.0722 * pixel[2] as f64
            })
            .collect()
    }
}

fn next_token(bytes: &[u8], cursor: &mut usize) -> Option<String> {
    loop {
        while *cursor < bytes.len() && bytes[*cursor].is_ascii_whitespace() {
            *cursor += 1;
        }
        if *cursor >= bytes.len() || bytes[*cursor] != b'#' {
            break;
        }
        while *cursor < bytes.len() && bytes[*cursor] != b'\n' {
            *cursor += 1;
        }
    }

    let start = *cursor;
    while *cursor < bytes.len() && !bytes[*cursor].is_ascii_whitespace() {
        *cursor += 1;
    }
    if start == *cursor {
        return None;
    }

    std::str::from_utf8(&bytes[start..*cursor])
        .ok()
        .map(str::to_string)
}

pub fn parse_ppm_rgb(bytes: &[u8]) -> Result<PpmImage, String> {
    let mut cursor = 0usize;
    let magic = next_token(bytes, &mut cursor).ok_or_else(|| "missing PPM magic".to_string())?;
    if magic != "P6" {
        return Err(format!("unsupported PPM magic '{magic}'"));
    }

    let width = next_token(bytes, &mut cursor)
        .ok_or_else(|| "missing PPM width".to_string())?
        .parse::<u32>()
        .map_err(|error| format!("invalid PPM width: {error}"))?;
    let height = next_token(bytes, &mut cursor)
        .ok_or_else(|| "missing PPM height".to_string())?
        .parse::<u32>()
        .map_err(|error| format!("invalid PPM height: {error}"))?;
    let max_value = next_token(bytes, &mut cursor)
        .ok_or_else(|| "missing PPM max value".to_string())?
        .parse::<u32>()
        .map_err(|error| format!("invalid PPM max value: {error}"))?;
    if width == 0 || height == 0 {
        return Err("PPM dimensions must be non-zero".to_string());
    }
    if max_value != 255 {
        return Err(format!("unsupported PPM max value {max_value}"));
    }

    if cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }

    let expected = width as usize * height as usize * 3;
    let remaining = bytes.len().saturating_sub(cursor);
    if remaining != expected {
        return Err(format!("expected {expected} RGB bytes, found {remaining}"));
    }

    Ok(PpmImage {
        width,
        height,
        rgb: bytes[cursor..].to_vec(),
    })
}

pub fn read_ppm_file(path: &str) -> Result<PpmImage, String> {
    let bytes = std::fs::read(path)
        .map_err(|error| format!("failed to read PPM image '{path}': {error}"))?;
    parse_ppm_rgb(&bytes).map_err(|error| format!("invalid PPM image '{path}': {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_binary_ppm_with_comments() {
        let ppm = b"P6\n# generated test texture\n2 1\n255\n\x0a\x20\x30\x40\x50\x60";
        let image = parse_ppm_rgb(ppm).unwrap();
        assert_eq!(image.width, 2);
        assert_eq!(image.height, 1);
        assert_eq!(image.pixel_count(), 2);
        assert_eq!(image.rgb, vec![0x0a, 0x20, 0x30, 0x40, 0x50, 0x60]);
    }

    #[test]
    fn computes_rec709_luminance_per_pixel() {
        let ppm = b"P6\n1 1\n255\n\xff\x00\x00";
        let image = parse_ppm_rgb(ppm).unwrap();
        let luminance = image.luminance();
        assert_eq!(luminance.len(), 1);
        assert!((luminance[0] - 0.2126 * 255.0).abs() < 1e-9);
    }

    #[test]
    fn rejects_unsupported_magic_and_depth() {
        assert!(parse_ppm_rgb(b"P3\n1 1\n255\n0 0 0").is_err());
        assert!(parse_ppm_rgb(b"P6\n1 1\n65535\n\x00\x00\x00\x00\x00\x00").is_err());
    }

    #[test]
    fn rejects_truncated_pixel_payload() {
        let error = parse_ppm_rgb(b"P6\n2 2\n255\n\x00\x01\x02").unwrap_err();
        assert!(error.contains("expected 12 RGB bytes"), "{error}");
    }

    #[test]
    fn reports_a_missing_file() {
        assert!(read_ppm_file("absent_image.ppm").is_err());
    }
}
