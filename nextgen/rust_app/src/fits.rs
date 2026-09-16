#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FitsBitPix {
    Byte8 = 8,
    Short16 = 16,
    Long32 = 32,
    Float32 = -32,
    Double64 = -64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FitsHeaderCard {
    pub keyword: String,
    pub value: String,
    pub comment: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FitsHeader {
    pub cards: Vec<FitsHeaderCard>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FitsImage {
    pub header: FitsHeader,
    pub width: usize,
    pub height: usize,
    pub bitpix: FitsBitPix,
    pub data: Vec<f64>,
}

impl FitsHeader {
    pub fn new() -> Self {
        Self { cards: Vec::new() }
    }

    pub fn set_str(&mut self, keyword: &str, value: &str, comment: Option<&str>) {
        let kw = format!("{:8}", keyword).to_uppercase();
        let kw = kw[..8].to_string();
        if let Some(card) = self.cards.iter_mut().find(|c| c.keyword == kw) {
            card.value = format!("'{}'", value);
            card.comment = comment.map(str::to_owned);
        } else {
            self.cards.push(FitsHeaderCard {
                keyword: kw,
                value: format!("'{}'", value),
                comment: comment.map(str::to_owned),
            });
        }
    }

    pub fn set_int(&mut self, keyword: &str, value: i64, comment: Option<&str>) {
        let kw = format!("{:8}", keyword).to_uppercase();
        let kw = kw[..8].to_string();
        if let Some(card) = self.cards.iter_mut().find(|c| c.keyword == kw) {
            card.value = format!("{:20}", value);
            card.comment = comment.map(str::to_owned);
        } else {
            self.cards.push(FitsHeaderCard {
                keyword: kw,
                value: format!("{:20}", value),
                comment: comment.map(str::to_owned),
            });
        }
    }

    pub fn set_float(&mut self, keyword: &str, value: f64, comment: Option<&str>) {
        let kw = format!("{:8}", keyword).to_uppercase();
        let kw = kw[..8].to_string();
        if let Some(card) = self.cards.iter_mut().find(|c| c.keyword == kw) {
            card.value = format!("{:20.8}", value);
            card.comment = comment.map(str::to_owned);
        } else {
            self.cards.push(FitsHeaderCard {
                keyword: kw,
                value: format!("{:20.8}", value),
                comment: comment.map(str::to_owned),
            });
        }
    }

    pub fn set_bool(&mut self, keyword: &str, value: bool, comment: Option<&str>) {
        let kw = format!("{:8}", keyword).to_uppercase();
        let kw = kw[..8].to_string();
        let val_str = if value { "T" } else { "F" };
        if let Some(card) = self.cards.iter_mut().find(|c| c.keyword == kw) {
            card.value = format!("{:20}", val_str);
            card.comment = comment.map(str::to_owned);
        } else {
            self.cards.push(FitsHeaderCard {
                keyword: kw,
                value: format!("{:20}", val_str),
                comment: comment.map(str::to_owned),
            });
        }
    }

    pub fn raw_value(&self, keyword: &str) -> Option<&str> {
        let kw = keyword.trim().to_uppercase();
        self.cards
            .iter()
            .find(|card| card.keyword.trim() == kw)
            .map(|card| card.value.trim())
    }

    pub fn get_str(&self, keyword: &str) -> Option<String> {
        let raw = self.raw_value(keyword)?;
        let unquoted = raw.strip_prefix('\'')?.strip_suffix('\'')?;
        Some(unquoted.replace("''", "'").trim_end().to_string())
    }

    pub fn get_int(&self, keyword: &str) -> Option<i64> {
        self.raw_value(keyword)?.parse::<i64>().ok()
    }

    pub fn get_float(&self, keyword: &str) -> Option<f64> {
        let raw = self.raw_value(keyword)?;
        raw.replace(['D', 'd'], "E").parse::<f64>().ok()
    }

    pub fn get_bool(&self, keyword: &str) -> Option<bool> {
        match self.raw_value(keyword)? {
            "T" => Some(true),
            "F" => Some(false),
            _ => None,
        }
    }

    pub fn format_card(&self, card: &FitsHeaderCard) -> [u8; 80] {
        let mut block = [b' '; 80];
        let kw_bytes = card.keyword.as_bytes();
        let len_kw = kw_bytes.len().min(8);
        block[..len_kw].copy_from_slice(&kw_bytes[..len_kw]);

        block[8] = b'=';
        block[9] = b' ';

        let val_bytes = card.value.as_bytes();
        let val_start = 10;
        let val_end = (val_start + val_bytes.len()).min(79);
        block[val_start..val_end].copy_from_slice(&val_bytes[..(val_end - val_start)]);

        if let Some(cmt) = &card.comment {
            let cmt_start = (val_end + 2).min(78);
            if cmt_start < 78 {
                block[cmt_start] = b'/';
                block[cmt_start + 1] = b' ';
                let cmt_bytes = cmt.as_bytes();
                let cmt_end = (cmt_start + 2 + cmt_bytes.len()).min(80);
                block[(cmt_start + 2)..cmt_end].copy_from_slice(&cmt_bytes[..(cmt_end - cmt_start - 2)]);
            }
        }

        block
    }
}

pub fn create_astronomical_fits_image(
    data: &[f64],
    width: usize,
    height: usize,
    object_name: &str,
    filter_name: &str,
    exposure_s: f64,
    jd: f64,
) -> FitsImage {
    let mut header = FitsHeader::new();
    header.set_bool("SIMPLE", true, Some("Standard FITS format (NOST 100-2.0)"));
    header.set_int("BITPIX", -64, Some("IEEE double precision floating point"));
    header.set_int("NAXIS", 2, Some("Number of coordinate axes"));
    header.set_int("NAXIS1", width as i64, Some("Width in pixels"));
    header.set_int("NAXIS2", height as i64, Some("Height in pixels"));
    header.set_str("OBJECT", object_name, Some("Target observed"));
    header.set_str("FILTER", filter_name, Some("Optical filter"));
    header.set_float("EXPTIME", exposure_s, Some("Total exposure duration in seconds"));
    header.set_float("JD", jd, Some("Julian Date at start of exposure"));
    header.set_str("ORIGIN", "Observatory-Core-Rust", Some("Celestia NextGen Core"));

    FitsImage {
        header,
        width,
        height,
        bitpix: FitsBitPix::Double64,
        data: data.to_vec(),
    }
}

pub fn write_fits_binary(image: &FitsImage, output_path: &str) -> Result<usize, String> {
    if image.data.len() != image.width * image.height {
        return Err("image data length does not match width * height".to_string());
    }

    let mut buffer = Vec::new();

    // 1. Write Header Cards
    for card in &image.header.cards {
        let card_bytes = image.header.format_card(card);
        buffer.extend_from_slice(&card_bytes);
    }

    // Write END card
    let mut end_card = [b' '; 80];
    end_card[0..3].copy_from_slice(b"END");
    buffer.extend_from_slice(&end_card);

    // Header padding to 2880 bytes (FITS block standard)
    let header_rem = buffer.len() % 2880;
    if header_rem != 0 {
        let pad_len = 2880 - header_rem;
        buffer.extend(std::iter::repeat(b' ').take(pad_len));
    }

    // 2. Write Data Block in Big-Endian Double Floating Point (FITS standard)
    for &val in &image.data {
        buffer.extend_from_slice(&val.to_be_bytes());
    }

    // Data padding to 2880 bytes
    let data_rem = (image.data.len() * 8) % 2880;
    if data_rem != 0 {
        let pad_len = 2880 - data_rem;
        buffer.extend(std::iter::repeat(0u8).take(pad_len));
    }

    let total_written = buffer.len();
    std::fs::write(output_path, buffer)
        .map_err(|error| format!("failed to write FITS file '{output_path}': {error}"))?;

    Ok(total_written)
}

const FITS_BLOCK_BYTES: usize = 2880;
const FITS_CARD_BYTES: usize = 80;

fn split_card_value_and_comment(body: &str) -> (String, Option<String>) {
    let trimmed = body.trim_start();
    if let Some(after_quote) = trimmed.strip_prefix('\'') {
        let bytes = after_quote.as_bytes();
        let mut index = 0usize;
        while index < bytes.len() {
            if bytes[index] == b'\'' {
                if index + 1 < bytes.len() && bytes[index + 1] == b'\'' {
                    index += 2;
                    continue;
                }
                break;
            }
            index += 1;
        }
        let value = format!("'{}'", &after_quote[..index.min(after_quote.len())]);
        let remainder = after_quote.get(index + 1..).unwrap_or("").trim();
        let comment = remainder
            .strip_prefix('/')
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty());
        return (value, comment);
    }

    match trimmed.split_once('/') {
        Some((value, comment)) => {
            let comment = comment.trim();
            (
                value.trim().to_string(),
                if comment.is_empty() {
                    None
                } else {
                    Some(comment.to_string())
                },
            )
        }
        None => (trimmed.trim().to_string(), None),
    }
}

fn bitpix_from_code(code: i64) -> Result<FitsBitPix, String> {
    match code {
        8 => Ok(FitsBitPix::Byte8),
        16 => Ok(FitsBitPix::Short16),
        32 => Ok(FitsBitPix::Long32),
        -32 => Ok(FitsBitPix::Float32),
        -64 => Ok(FitsBitPix::Double64),
        other => Err(format!("unsupported BITPIX value {other}")),
    }
}

fn bitpix_sample_bytes(bitpix: FitsBitPix) -> usize {
    match bitpix {
        FitsBitPix::Byte8 => 1,
        FitsBitPix::Short16 => 2,
        FitsBitPix::Long32 | FitsBitPix::Float32 => 4,
        FitsBitPix::Double64 => 8,
    }
}

fn decode_sample(bitpix: FitsBitPix, raw: &[u8]) -> f64 {
    match bitpix {
        FitsBitPix::Byte8 => raw[0] as f64,
        FitsBitPix::Short16 => i16::from_be_bytes([raw[0], raw[1]]) as f64,
        FitsBitPix::Long32 => i32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]) as f64,
        FitsBitPix::Float32 => f32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]) as f64,
        FitsBitPix::Double64 => f64::from_be_bytes([
            raw[0], raw[1], raw[2], raw[3], raw[4], raw[5], raw[6], raw[7],
        ]),
    }
}

/// Parses a primary-HDU FITS image (2 axes) from raw file bytes.
pub fn parse_fits_image(bytes: &[u8]) -> Result<FitsImage, String> {
    if bytes.len() < FITS_BLOCK_BYTES {
        return Err("FITS payload is shorter than one 2880-byte block".to_string());
    }
    if !bytes.starts_with(b"SIMPLE") {
        return Err("missing SIMPLE keyword: not a primary FITS HDU".to_string());
    }

    let mut header = FitsHeader::new();
    let mut cursor = 0usize;
    let mut end_reached = false;

    while cursor + FITS_CARD_BYTES <= bytes.len() {
        let card = &bytes[cursor..cursor + FITS_CARD_BYTES];
        cursor += FITS_CARD_BYTES;

        let keyword = std::str::from_utf8(&card[..8])
            .map_err(|error| format!("non ASCII keyword in FITS header: {error}"))?;
        let keyword_trimmed = keyword.trim();

        if keyword_trimmed == "END" {
            end_reached = true;
            cursor = cursor.div_ceil(FITS_BLOCK_BYTES) * FITS_BLOCK_BYTES;
            break;
        }
        if keyword_trimmed.is_empty() || card[8] != b'=' {
            continue;
        }

        let body = std::str::from_utf8(&card[10..])
            .map_err(|error| format!("non ASCII card value in FITS header: {error}"))?;
        let (value, comment) = split_card_value_and_comment(body);
        header.cards.push(FitsHeaderCard {
            keyword: keyword.to_string(),
            value,
            comment,
        });
    }

    if !end_reached {
        return Err("FITS header has no END card".to_string());
    }

    let naxis = header
        .get_int("NAXIS")
        .ok_or_else(|| "FITS header has no NAXIS card".to_string())?;
    if naxis != 2 {
        return Err(format!("only 2-axis FITS images are supported, found NAXIS={naxis}"));
    }

    let bitpix = bitpix_from_code(
        header
            .get_int("BITPIX")
            .ok_or_else(|| "FITS header has no BITPIX card".to_string())?,
    )?;
    let width = header
        .get_int("NAXIS1")
        .ok_or_else(|| "FITS header has no NAXIS1 card".to_string())?;
    let height = header
        .get_int("NAXIS2")
        .ok_or_else(|| "FITS header has no NAXIS2 card".to_string())?;
    if width <= 0 || height <= 0 {
        return Err(format!("invalid FITS image dimensions {width}x{height}"));
    }

    let width = width as usize;
    let height = height as usize;
    let sample_bytes = bitpix_sample_bytes(bitpix);
    let pixel_count = width
        .checked_mul(height)
        .ok_or_else(|| "FITS image dimensions overflow".to_string())?;
    let data_bytes = pixel_count
        .checked_mul(sample_bytes)
        .ok_or_else(|| "FITS data segment size overflow".to_string())?;

    if bytes.len() < cursor + data_bytes {
        return Err(format!(
            "FITS data segment truncated: expected {data_bytes} bytes after header, found {}",
            bytes.len().saturating_sub(cursor)
        ));
    }

    let bzero = header.get_float("BZERO").unwrap_or(0.0);
    let bscale = header.get_float("BSCALE").unwrap_or(1.0);
    let blank = header.get_int("BLANK");

    let mut data = Vec::with_capacity(pixel_count);
    for index in 0..pixel_count {
        let offset = cursor + index * sample_bytes;
        let raw = &bytes[offset..offset + sample_bytes];
        let sample = decode_sample(bitpix, raw);
        let is_blank = matches!(blank, Some(value) if sample == value as f64)
            && !matches!(bitpix, FitsBitPix::Float32 | FitsBitPix::Double64);
        data.push(if is_blank {
            f64::NAN
        } else {
            bzero + bscale * sample
        });
    }

    Ok(FitsImage {
        header,
        width,
        height,
        bitpix,
        data,
    })
}

/// Reads a FITS image from disk and converts its samples to physical values.
pub fn read_fits_file(path: &str) -> Result<FitsImage, String> {
    let bytes = std::fs::read(path)
        .map_err(|error| format!("failed to read FITS file '{path}': {error}"))?;
    parse_fits_image(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_and_writes_fits_file_with_exact_2880_block_alignment() {
        let width = 8;
        let height = 8;
        let data: Vec<f64> = (0..64).map(|i| 100.0 + (i as f64) * 2.5).collect();

        let image = create_astronomical_fits_image(
            &data,
            width,
            height,
            "M31_Core",
            "R",
            120.0,
            2459000.5,
        );

        let temp_path = std::env::temp_dir().join("test_output_frame.fits");
        let bytes_written = write_fits_binary(&image, temp_path.to_str().unwrap()).unwrap();

        assert_eq!(bytes_written % 2880, 0); // FITS block standard compliance
        assert!(bytes_written >= 2880 * 2); // At least 1 header block + 1 data block

        let file_bytes = std::fs::read(&temp_path).unwrap();
        assert_eq!(file_bytes.len(), bytes_written);

        let header_str = String::from_utf8_lossy(&file_bytes[..80]);
        assert!(header_str.starts_with("SIMPLE"));

        let _ = std::fs::remove_file(temp_path);
    }

    #[test]
    fn writes_then_reads_back_identical_pixel_values() {
        let width = 5;
        let height = 4;
        let data: Vec<f64> = (0..20).map(|i| 1_000.0 - (i as f64) * 7.25).collect();
        let image = create_astronomical_fits_image(&data, width, height, "NGC_7000", "Ha", 300.0, 2460123.25);

        let temp_path = std::env::temp_dir().join("test_roundtrip_frame.fits");
        write_fits_binary(&image, temp_path.to_str().unwrap()).unwrap();

        let decoded = read_fits_file(temp_path.to_str().unwrap()).unwrap();
        assert_eq!(decoded.width, width);
        assert_eq!(decoded.height, height);
        assert_eq!(decoded.bitpix, FitsBitPix::Double64);
        assert_eq!(decoded.data, data);
        assert_eq!(decoded.header.get_str("OBJECT").as_deref(), Some("NGC_7000"));
        assert_eq!(decoded.header.get_str("FILTER").as_deref(), Some("Ha"));
        assert_eq!(decoded.header.get_int("NAXIS1"), Some(width as i64));
        assert!((decoded.header.get_float("EXPTIME").unwrap() - 300.0).abs() < 1e-9);

        let _ = std::fs::remove_file(temp_path);
    }

    #[test]
    fn decodes_scaled_16_bit_camera_frame() {
        let mut header_text = String::new();
        let mut push = |card: &str| {
            header_text.push_str(&format!("{card:<80}"));
        };
        push("SIMPLE  =                    T / conforms to FITS standard");
        push("BITPIX  =                   16 / 16-bit signed integers");
        push("NAXIS   =                    2");
        push("NAXIS1  =                    3");
        push("NAXIS2  =                    2");
        push("BZERO   =              32768.0 / unsigned offset");
        push("BSCALE  =                  1.0");
        push("OBJECT  = 'M13     '           / globular cluster");
        push("END");

        let mut bytes = header_text.into_bytes();
        bytes.resize(2880, b' ');

        let raw_samples: [i16; 6] = [-32768, -32767, 0, 1, 32766, 32767];
        for sample in raw_samples {
            bytes.extend_from_slice(&sample.to_be_bytes());
        }
        bytes.resize(2880 * 2, 0);

        let image = parse_fits_image(&bytes).unwrap();
        assert_eq!(image.width, 3);
        assert_eq!(image.height, 2);
        assert_eq!(image.bitpix, FitsBitPix::Short16);
        assert_eq!(image.data, vec![0.0, 1.0, 32768.0, 32769.0, 65534.0, 65535.0]);
        assert_eq!(image.header.get_str("OBJECT").as_deref(), Some("M13"));
    }

    #[test]
    fn rejects_payload_without_simple_keyword() {
        let bytes = vec![b'X'; 2880];
        assert!(parse_fits_image(&bytes).is_err());
    }
}
