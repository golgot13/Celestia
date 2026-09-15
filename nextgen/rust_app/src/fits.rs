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
}
