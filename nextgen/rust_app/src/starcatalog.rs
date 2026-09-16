//! Star catalogue subsystem.
//!
//! Every star comes from a catalogue file supplied by the operator. Three published
//! formats are ingested:
//!
//! * the Celestia `stars.dat` binary catalogue (`CELSTARS` magic), whose records carry a
//!   HIP number, a rectangular position in light years, an absolute magnitude and a
//!   packed spectral type;
//! * the Hipparcos main catalogue `hip_main.dat`, a `|` separated ASCII table;
//! * a generic delimited export, such as a Gaia archive CSV.
//!
//! Nothing is generated: an absent or unreadable catalogue yields an error, never a
//! synthetic star field.

use crate::tabular::Table;
use std::f64::consts::PI;

/// Parsecs in one light year.
pub const PARSEC_PER_LIGHT_YEAR: f64 = 0.306_601_393_79;
/// Light years in one parsec.
pub const LIGHT_YEAR_PER_PARSEC: f64 = 3.261_563_777;
/// Mean obliquity of the ecliptic at J2000.0, in degrees.
const J2000_OBLIQUITY_DEG: f64 = 23.439_279_444_444_445;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatalogFormat {
    CelestiaBinary,
    HipparcosMain,
    DelimitedTable,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Star {
    /// Catalogue identifier, HIP number when the source provides one.
    pub catalog_number: u32,
    pub right_ascension_deg: f64,
    pub declination_deg: f64,
    /// Distance in light years. Zero when the source provides no parallax.
    pub distance_ly: f64,
    pub absolute_magnitude: f64,
    pub apparent_magnitude: f64,
    /// Johnson B-V colour index.
    pub color_index_b_v: f64,
    pub spectral_type: String,
}

impl Star {
    /// Equatorial unit direction in the ICRF frame.
    pub fn equatorial_direction(&self) -> [f64; 3] {
        let ra = self.right_ascension_deg.to_radians();
        let dec = self.declination_deg.to_radians();
        [dec.cos() * ra.cos(), dec.cos() * ra.sin(), dec.sin()]
    }

    /// Rectangular position in light years, in the ICRF equatorial frame.
    pub fn equatorial_position_ly(&self) -> [f64; 3] {
        let direction = self.equatorial_direction();
        [
            direction[0] * self.distance_ly,
            direction[1] * self.distance_ly,
            direction[2] * self.distance_ly,
        ]
    }

    pub fn parallax_mas(&self) -> f64 {
        if self.distance_ly <= 0.0 {
            return 0.0;
        }
        1_000.0 / (self.distance_ly * PARSEC_PER_LIGHT_YEAR)
    }
}

/// Distance in light years derived from a trigonometric parallax in milliarcseconds.
pub fn distance_ly_from_parallax_mas(parallax_mas: f64) -> f64 {
    if parallax_mas <= 0.0 {
        return 0.0;
    }
    LIGHT_YEAR_PER_PARSEC * 1_000.0 / parallax_mas
}

/// Absolute magnitude from the apparent magnitude and the distance.
pub fn absolute_magnitude(apparent_magnitude: f64, distance_ly: f64) -> f64 {
    if distance_ly <= 0.0 {
        return apparent_magnitude;
    }
    let distance_pc = distance_ly * PARSEC_PER_LIGHT_YEAR;
    apparent_magnitude - 5.0 * (distance_pc / 10.0).log10()
}

/// Apparent magnitude from the absolute magnitude and the distance.
pub fn apparent_magnitude(absolute_magnitude: f64, distance_ly: f64) -> f64 {
    if distance_ly <= 0.0 {
        return absolute_magnitude;
    }
    let distance_pc = distance_ly * PARSEC_PER_LIGHT_YEAR;
    absolute_magnitude + 5.0 * (distance_pc / 10.0).log10()
}

/// Ballesteros' relation between the Johnson B-V colour index and the effective
/// temperature of a star, in kelvin.
pub fn effective_temperature_from_b_v(color_index_b_v: f64) -> f64 {
    let b_v = color_index_b_v.clamp(-0.4, 2.5);
    4600.0 * (1.0 / (0.92 * b_v + 1.7) + 1.0 / (0.92 * b_v + 0.62))
}

/// Linear sRGB colour of a blackbody at the requested temperature, normalised so that
/// the brightest channel reaches one.
pub fn blackbody_srgb(temperature_k: f64) -> [f32; 3] {
    // Planckian locus approximation over 1000 K to 40000 K, expressed in the
    // temperature-scaled form used for star rendering.
    let t = temperature_k.clamp(1_000.0, 40_000.0) / 100.0;

    let red = if t <= 66.0 {
        255.0
    } else {
        329.698_727_446 * (t - 60.0).powf(-0.133_204_759_2)
    };

    let green = if t <= 66.0 {
        99.470_802_586 * t.ln() - 161.119_568_166
    } else {
        288.122_169_528 * (t - 60.0).powf(-0.075_514_849_2)
    };

    let blue = if t >= 66.0 {
        255.0
    } else if t <= 19.0 {
        0.0
    } else {
        138.517_731_223 * (t - 10.0).ln() - 305.044_792_730
    };

    let channels = [
        red.clamp(0.0, 255.0),
        green.clamp(0.0, 255.0),
        blue.clamp(0.0, 255.0),
    ];
    let peak = channels
        .iter()
        .copied()
        .fold(f64::MIN_POSITIVE, f64::max);

    [
        (channels[0] / peak) as f32,
        (channels[1] / peak) as f32,
        (channels[2] / peak) as f32,
    ]
}

/// Johnson B-V colour index of the main sequence class carried by a spectral type
/// string such as `G2V` or `K0III`.
pub fn b_v_from_spectral_type(spectral_type: &str) -> f64 {
    let trimmed = spectral_type.trim();
    let mut characters = trimmed.chars();
    let Some(class) = characters.next().map(|c| c.to_ascii_uppercase()) else {
        return 0.65;
    };
    let subclass = characters
        .next()
        .and_then(|c| c.to_digit(10))
        .map(|digit| digit as f64 / 10.0)
        .unwrap_or(0.5);

    // Anchor values of the main sequence, interpolated over the subclass decade.
    let (start, end) = match class {
        'O' => (-0.33, -0.30),
        'B' => (-0.30, 0.00),
        'A' => (0.00, 0.30),
        'F' => (0.30, 0.58),
        'G' => (0.58, 0.81),
        'K' => (0.81, 1.40),
        'M' => (1.40, 2.00),
        'L' | 'T' | 'Y' => (2.00, 2.50),
        _ => return 0.65,
    };

    start + (end - start) * subclass
}

/// Converts an ICRF equatorial direction into ecliptic rectangular coordinates.
pub fn equatorial_to_ecliptic_direction(vector: [f64; 3]) -> [f64; 3] {
    let obliquity = J2000_OBLIQUITY_DEG.to_radians();
    let cos_eps = obliquity.cos();
    let sin_eps = obliquity.sin();
    [
        vector[0],
        vector[1] * cos_eps + vector[2] * sin_eps,
        -vector[1] * sin_eps + vector[2] * cos_eps,
    ]
}

/// Equal-area declination bands, each split into a number of right ascension cells
/// proportional to the band width, so that cone queries touch few cells at any
/// declination including the poles.
#[derive(Clone, Debug, PartialEq)]
struct SkyIndex {
    band_count: usize,
    cells_per_band: Vec<usize>,
    band_offsets: Vec<usize>,
    cells: Vec<Vec<u32>>,
}

impl SkyIndex {
    fn new(band_count: usize) -> Self {
        let band_count = band_count.max(1);
        let mut cells_per_band = Vec::with_capacity(band_count);
        let mut band_offsets = Vec::with_capacity(band_count);
        let mut total = 0usize;

        for band in 0..band_count {
            let lower = -1.0 + 2.0 * band as f64 / band_count as f64;
            let upper = -1.0 + 2.0 * (band + 1) as f64 / band_count as f64;
            let mean_declination = (0.5 * (lower + upper)).asin();
            let cells = ((2 * band_count) as f64 * mean_declination.cos()).round() as usize;
            let cells = cells.max(1);
            band_offsets.push(total);
            cells_per_band.push(cells);
            total += cells;
        }

        Self {
            band_count,
            cells_per_band,
            band_offsets,
            cells: vec![Vec::new(); total],
        }
    }

    fn band_of(&self, declination_deg: f64) -> usize {
        let sin_dec = declination_deg.to_radians().sin().clamp(-1.0, 1.0);
        let band = ((sin_dec + 1.0) * 0.5 * self.band_count as f64).floor() as usize;
        band.min(self.band_count - 1)
    }

    fn cell_of(&self, right_ascension_deg: f64, declination_deg: f64) -> usize {
        let band = self.band_of(declination_deg);
        let cells = self.cells_per_band[band];
        let fraction = right_ascension_deg.rem_euclid(360.0) / 360.0;
        let column = ((fraction * cells as f64).floor() as usize).min(cells - 1);
        self.band_offsets[band] + column
    }

    fn insert(&mut self, star_index: u32, right_ascension_deg: f64, declination_deg: f64) {
        let cell = self.cell_of(right_ascension_deg, declination_deg);
        self.cells[cell].push(star_index);
    }

    /// Indices of every cell that can overlap the requested cone.
    fn cells_in_cone(&self, right_ascension_deg: f64, declination_deg: f64, radius_deg: f64) -> Vec<usize> {
        let radius = radius_deg.clamp(0.0, 180.0);
        let min_declination = (declination_deg - radius).max(-90.0);
        let max_declination = (declination_deg + radius).min(90.0);
        let first_band = self.band_of(min_declination);
        let last_band = self.band_of(max_declination);

        let mut selected = Vec::new();
        for band in first_band..=last_band {
            let cells = self.cells_per_band[band];
            let offset = self.band_offsets[band];

            // Right ascension half-width of the cone at this declination.
            let band_declination = declination_deg.clamp(
                -90.0 + 180.0 * band as f64 / self.band_count as f64,
                -90.0 + 180.0 * (band + 1) as f64 / self.band_count as f64,
            );
            let cos_dec = band_declination.to_radians().cos().abs();
            let half_width_deg = if cos_dec < 1.0e-6 || radius >= 90.0 {
                180.0
            } else {
                (radius / cos_dec).min(180.0)
            };

            if half_width_deg >= 180.0 {
                selected.extend(offset..offset + cells);
                continue;
            }

            let span = 2.0 * half_width_deg;
            let touched = ((span / 360.0) * cells as f64).ceil() as usize + 2;
            if touched >= cells {
                selected.extend(offset..offset + cells);
                continue;
            }

            let start_fraction = (right_ascension_deg - half_width_deg).rem_euclid(360.0) / 360.0;
            let start_column = (start_fraction * cells as f64).floor() as usize % cells;
            for step in 0..touched {
                selected.push(offset + (start_column + step) % cells);
            }
        }

        selected.sort_unstable();
        selected.dedup();
        selected
    }
}

/// Angular separation between two equatorial positions, in degrees.
pub fn angular_separation_deg(
    ra_a_deg: f64,
    dec_a_deg: f64,
    ra_b_deg: f64,
    dec_b_deg: f64,
) -> f64 {
    let dec_a = dec_a_deg.to_radians();
    let dec_b = dec_b_deg.to_radians();
    let delta_ra = (ra_b_deg - ra_a_deg).to_radians();

    // Vincenty formula: stable for both small and large separations.
    let numerator = ((dec_b.cos() * delta_ra.sin()).powi(2)
        + (dec_a.cos() * dec_b.sin() - dec_a.sin() * dec_b.cos() * delta_ra.cos()).powi(2))
    .sqrt();
    let denominator =
        dec_a.sin() * dec_b.sin() + dec_a.cos() * dec_b.cos() * delta_ra.cos();

    numerator.atan2(denominator) * 180.0 / PI
}

#[derive(Clone, Debug, PartialEq)]
pub struct StarCatalog {
    stars: Vec<Star>,
    index: SkyIndex,
    source: String,
    format: CatalogFormat,
    brightest_magnitude: f64,
    faintest_magnitude: f64,
}

impl StarCatalog {
    pub fn from_stars(stars: Vec<Star>, source: &str, format: CatalogFormat) -> Self {
        // One band per 2 degrees of declination keeps the cells close to square.
        let mut index = SkyIndex::new(90);
        let mut brightest = f64::INFINITY;
        let mut faintest = f64::NEG_INFINITY;

        for (position, star) in stars.iter().enumerate() {
            index.insert(
                position as u32,
                star.right_ascension_deg,
                star.declination_deg,
            );
            brightest = brightest.min(star.apparent_magnitude);
            faintest = faintest.max(star.apparent_magnitude);
        }

        if stars.is_empty() {
            brightest = 0.0;
            faintest = 0.0;
        }

        Self {
            stars,
            index,
            source: source.to_string(),
            format,
            brightest_magnitude: brightest,
            faintest_magnitude: faintest,
        }
    }

    pub fn stars(&self) -> &[Star] {
        &self.stars
    }

    pub fn len(&self) -> usize {
        self.stars.len()
    }

    pub fn is_empty(&self) -> bool {
        self.stars.is_empty()
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn format(&self) -> CatalogFormat {
        self.format
    }

    pub fn brightest_magnitude(&self) -> f64 {
        self.brightest_magnitude
    }

    pub fn faintest_magnitude(&self) -> f64 {
        self.faintest_magnitude
    }

    pub fn find_by_catalog_number(&self, catalog_number: u32) -> Option<&Star> {
        self.stars
            .iter()
            .find(|star| star.catalog_number == catalog_number)
    }

    /// Stars inside a cone, brighter than the requested limiting magnitude, sorted from
    /// the brightest to the faintest.
    pub fn query_cone(
        &self,
        right_ascension_deg: f64,
        declination_deg: f64,
        radius_deg: f64,
        limiting_magnitude: f64,
    ) -> Vec<&Star> {
        let mut selected = Vec::new();

        for cell in self
            .index
            .cells_in_cone(right_ascension_deg, declination_deg, radius_deg)
        {
            for star_index in &self.index.cells[cell] {
                let star = &self.stars[*star_index as usize];
                if star.apparent_magnitude > limiting_magnitude {
                    continue;
                }
                let separation = angular_separation_deg(
                    right_ascension_deg,
                    declination_deg,
                    star.right_ascension_deg,
                    star.declination_deg,
                );
                if separation <= radius_deg {
                    selected.push(star);
                }
            }
        }

        selected.sort_by(|a, b| {
            a.apparent_magnitude
                .partial_cmp(&b.apparent_magnitude)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        selected
    }

    /// Whole-sky selection limited to the requested magnitude, brightest first.
    pub fn brighter_than(&self, limiting_magnitude: f64) -> Vec<&Star> {
        let mut selected: Vec<&Star> = self
            .stars
            .iter()
            .filter(|star| star.apparent_magnitude <= limiting_magnitude)
            .collect();
        selected.sort_by(|a, b| {
            a.apparent_magnitude
                .partial_cmp(&b.apparent_magnitude)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        selected
    }
}

fn decode_celestia_spectral_type(packed: u16) -> String {
    // Celestia packs the spectral class in bits 12-15, the subclass in bits 8-11 and
    // the luminosity class in bits 0-3.
    const CLASSES: [&str; 16] = [
        "O", "B", "A", "F", "G", "K", "M", "R", "S", "N", "WC", "WN", "?", "L", "T", "C",
    ];
    const LUMINOSITY: [&str; 9] = ["Ia-0", "Ia", "Ib", "II", "III", "IV", "V", "VI", ""];

    let class_index = ((packed >> 12) & 0x0f) as usize;
    let subclass = ((packed >> 8) & 0x0f) as usize;
    let luminosity_index = (packed & 0x0f) as usize;

    let class = CLASSES.get(class_index).copied().unwrap_or("?");
    let luminosity = LUMINOSITY.get(luminosity_index).copied().unwrap_or("");

    if subclass <= 9 {
        format!("{class}{subclass}{luminosity}")
    } else {
        format!("{class}{luminosity}")
    }
}

/// Reads the Celestia `stars.dat` binary catalogue.
pub fn parse_celestia_stars_dat(bytes: &[u8], source: &str) -> Result<StarCatalog, String> {
    const HEADER_LENGTH: usize = 8 + 2 + 4;
    const RECORD_LENGTH: usize = 4 + 4 * 3 + 2 + 2;

    if bytes.len() < HEADER_LENGTH {
        return Err(format!("'{source}' is too short to be a stars.dat catalogue"));
    }
    if &bytes[0..8] != b"CELSTARS" {
        return Err(format!("'{source}' does not start with the CELSTARS magic"));
    }

    let version = u16::from_le_bytes([bytes[8], bytes[9]]);
    if version != 0x0100 {
        return Err(format!("unsupported stars.dat version {version:#06x} in '{source}'"));
    }

    let declared_count =
        u32::from_le_bytes([bytes[10], bytes[11], bytes[12], bytes[13]]) as usize;
    let available = (bytes.len() - HEADER_LENGTH) / RECORD_LENGTH;
    if available < declared_count {
        return Err(format!(
            "'{source}' declares {declared_count} stars but only {available} records are present"
        ));
    }

    let mut stars = Vec::with_capacity(declared_count);
    for record in 0..declared_count {
        let offset = HEADER_LENGTH + record * RECORD_LENGTH;
        let chunk = &bytes[offset..offset + RECORD_LENGTH];

        let catalog_number = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        let x = f32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]) as f64;
        let y = f32::from_le_bytes([chunk[8], chunk[9], chunk[10], chunk[11]]) as f64;
        let z = f32::from_le_bytes([chunk[12], chunk[13], chunk[14], chunk[15]]) as f64;
        let absolute = i16::from_le_bytes([chunk[16], chunk[17]]) as f64 / 256.0;
        let packed_type = u16::from_le_bytes([chunk[18], chunk[19]]);

        // Celestia stores positions in a left-handed frame where Y is the celestial
        // north pole and Z points towards right ascension 6h.
        let equatorial = [x, z, y];
        let distance_ly = (equatorial[0] * equatorial[0]
            + equatorial[1] * equatorial[1]
            + equatorial[2] * equatorial[2])
            .sqrt();

        let (right_ascension_deg, declination_deg) = if distance_ly > 0.0 {
            let ra = equatorial[1]
                .atan2(equatorial[0])
                .to_degrees()
                .rem_euclid(360.0);
            let dec = (equatorial[2] / distance_ly).clamp(-1.0, 1.0).asin().to_degrees();
            (ra, dec)
        } else {
            (0.0, 0.0)
        };

        let spectral_type = decode_celestia_spectral_type(packed_type);
        let color_index_b_v = b_v_from_spectral_type(&spectral_type);

        stars.push(Star {
            catalog_number,
            right_ascension_deg,
            declination_deg,
            distance_ly,
            absolute_magnitude: absolute,
            apparent_magnitude: apparent_magnitude(absolute, distance_ly),
            color_index_b_v,
            spectral_type,
        });
    }

    Ok(StarCatalog::from_stars(
        stars,
        source,
        CatalogFormat::CelestiaBinary,
    ))
}

pub fn read_celestia_stars_dat(path: &str) -> Result<StarCatalog, String> {
    let bytes = std::fs::read(path)
        .map_err(|error| format!("failed to read star catalogue '{path}': {error}"))?;
    parse_celestia_stars_dat(&bytes, path)
}

fn parse_field<T: std::str::FromStr>(field: &str) -> Option<T> {
    let trimmed = field.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse::<T>().ok()
}

/// Reads the Hipparcos main catalogue, whose records are `|` separated ASCII fields.
pub fn parse_hipparcos_main(text: &str, source: &str) -> Result<StarCatalog, String> {
    let mut stars = Vec::new();

    for (line_number, line) in text.lines().enumerate() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }

        let fields: Vec<&str> = line.split('|').collect();
        // H0 record identifier, H1 HIP number, H5 Vmag, H8 RA deg, H9 Dec deg,
        // H11 parallax in mas, H37 B-V, H76 spectral type.
        if fields.len() < 78 {
            return Err(format!(
                "'{source}' line {}: expected at least 78 fields, found {}",
                line_number + 1,
                fields.len()
            ));
        }

        let Some(catalog_number) = parse_field::<u32>(fields[1]) else {
            continue;
        };
        let (Some(right_ascension_deg), Some(declination_deg)) = (
            parse_field::<f64>(fields[8]),
            parse_field::<f64>(fields[9]),
        ) else {
            continue;
        };
        let Some(apparent) = parse_field::<f64>(fields[5]) else {
            continue;
        };

        let parallax_mas = parse_field::<f64>(fields[11]).unwrap_or(0.0);
        let distance_ly = distance_ly_from_parallax_mas(parallax_mas);
        let spectral_type = fields[76].trim().to_string();
        let color_index_b_v = parse_field::<f64>(fields[37])
            .unwrap_or_else(|| b_v_from_spectral_type(&spectral_type));

        stars.push(Star {
            catalog_number,
            right_ascension_deg: right_ascension_deg.rem_euclid(360.0),
            declination_deg,
            distance_ly,
            absolute_magnitude: absolute_magnitude(apparent, distance_ly),
            apparent_magnitude: apparent,
            color_index_b_v,
            spectral_type,
        });
    }

    if stars.is_empty() {
        return Err(format!("'{source}' contains no usable Hipparcos record"));
    }

    Ok(StarCatalog::from_stars(
        stars,
        source,
        CatalogFormat::HipparcosMain,
    ))
}

pub fn read_hipparcos_main(path: &str) -> Result<StarCatalog, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("failed to read Hipparcos catalogue '{path}': {error}"))?;
    parse_hipparcos_main(&text, path)
}

/// Reads a delimited catalogue export, such as a Gaia archive CSV.
///
/// Required columns: a right ascension, a declination and an apparent magnitude.
/// Optional columns: identifier, parallax, colour index and spectral type.
pub fn read_delimited_catalog(path: &str) -> Result<StarCatalog, String> {
    let table = Table::read(path)?;

    let ra_column = table.column_index_any(&["ra_deg", "ra", "raj2000", "_ra"])?;
    let dec_column = table.column_index_any(&["dec_deg", "dec", "de", "dej2000", "_de"])?;
    let magnitude_column = table.column_index_any(&[
        "apparent_magnitude",
        "magnitude",
        "mag",
        "vmag",
        "phot_g_mean_mag",
        "gmag",
    ])?;

    let id_column = table
        .column_index_any(&["id", "hip", "source_id", "catalog_number"])
        .ok();
    let parallax_column = table.column_index_any(&["parallax", "plx", "parallax_mas"]).ok();
    let distance_column = table.column_index_any(&["distance_ly", "dist_ly"]).ok();
    let color_column = table
        .column_index_any(&["b_v", "bv", "color_index", "bp_rp"])
        .ok();
    let spectral_column = table
        .column_index_any(&["spectral_type", "sptype", "spec_type"])
        .ok();

    let mut stars = Vec::with_capacity(table.row_count());
    for row in 0..table.row_count() {
        let right_ascension_deg = table.number(row, ra_column)?.rem_euclid(360.0);
        let declination_deg = table.number(row, dec_column)?;
        let apparent = table.number(row, magnitude_column)?;

        let distance_ly = match (distance_column, parallax_column) {
            (Some(column), _) => table.number(row, column)?,
            (None, Some(column)) => distance_ly_from_parallax_mas(table.number(row, column)?),
            (None, None) => 0.0,
        };

        let spectral_type = match spectral_column {
            Some(column) => table.text(row, column)?.to_string(),
            None => String::new(),
        };

        let color_index_b_v = match color_column {
            Some(column) => table.number(row, column)?,
            None => b_v_from_spectral_type(&spectral_type),
        };

        let catalog_number = match id_column {
            Some(column) => table.integer(row, column)?.max(0) as u32,
            None => row as u32 + 1,
        };

        stars.push(Star {
            catalog_number,
            right_ascension_deg,
            declination_deg,
            distance_ly,
            absolute_magnitude: absolute_magnitude(apparent, distance_ly),
            apparent_magnitude: apparent,
            color_index_b_v,
            spectral_type,
        });
    }

    Ok(StarCatalog::from_stars(
        stars,
        path,
        CatalogFormat::DelimitedTable,
    ))
}

/// Loads a catalogue, selecting the reader from the file signature then the extension.
pub fn load_star_catalog(path: &str) -> Result<StarCatalog, String> {
    let mut magic = [0u8; 8];
    if let Ok(bytes) = std::fs::read(path) {
        if bytes.len() >= 8 {
            magic.copy_from_slice(&bytes[0..8]);
            if &magic == b"CELSTARS" {
                return parse_celestia_stars_dat(&bytes, path);
            }
        }
    }

    let lowercase = path.to_ascii_lowercase();
    if lowercase.ends_with("hip_main.dat") || lowercase.ends_with(".hip") {
        return read_hipparcos_main(path);
    }

    read_delimited_catalog(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_star(catalog_number: u32, ra: f64, dec: f64, magnitude: f64) -> Star {
        Star {
            catalog_number,
            right_ascension_deg: ra,
            declination_deg: dec,
            distance_ly: 100.0,
            absolute_magnitude: absolute_magnitude(magnitude, 100.0),
            apparent_magnitude: magnitude,
            color_index_b_v: 0.65,
            spectral_type: "G2V".to_string(),
        }
    }

    #[test]
    fn parallax_and_distance_are_mutually_consistent() {
        // Proxima Centauri: 768.5 mas gives about 4.244 light years.
        let distance = distance_ly_from_parallax_mas(768.5);
        assert!((distance - 4.2441).abs() < 0.001, "{distance}");

        let star = Star {
            distance_ly: distance,
            ..sample_star(70890, 217.4, -62.68, 11.13)
        };
        assert!((star.parallax_mas() - 768.5).abs() < 0.01);
        assert_eq!(distance_ly_from_parallax_mas(0.0), 0.0);
        assert_eq!(distance_ly_from_parallax_mas(-5.0), 0.0);
    }

    #[test]
    fn magnitude_conversions_round_trip() {
        // Vega: apparent 0.03 at 25.04 light years.
        let absolute = absolute_magnitude(0.03, 25.04);
        assert!((absolute - 0.58).abs() < 0.05, "{absolute}");
        let back = apparent_magnitude(absolute, 25.04);
        assert!((back - 0.03).abs() < 1.0e-9);

        // Without a distance the two magnitudes coincide.
        assert_eq!(absolute_magnitude(5.0, 0.0), 5.0);
        assert_eq!(apparent_magnitude(5.0, 0.0), 5.0);
    }

    #[test]
    fn effective_temperature_matches_reference_stars() {
        // The Sun: B-V = 0.65 gives roughly 5750 K.
        let solar = effective_temperature_from_b_v(0.65);
        assert!((solar - 5750.0).abs() < 150.0, "{solar}");

        // Hotter stars are bluer.
        assert!(effective_temperature_from_b_v(-0.2) > effective_temperature_from_b_v(1.5));
    }

    #[test]
    fn blackbody_colours_follow_the_planckian_locus() {
        let hot = blackbody_srgb(20_000.0);
        let solar = blackbody_srgb(5_800.0);
        let cool = blackbody_srgb(3_000.0);

        assert!(hot[2] >= hot[0], "a hot star must be blue dominant");
        assert!(cool[0] > cool[2], "a cool star must be red dominant");
        assert!(solar[0] > 0.7 && solar[1] > 0.7, "a solar star is nearly white");
        for colour in [hot, solar, cool] {
            for channel in colour {
                assert!((0.0..=1.0).contains(&channel));
            }
        }
    }

    #[test]
    fn spectral_types_map_to_monotonic_colour_indices() {
        let o = b_v_from_spectral_type("O5V");
        let b = b_v_from_spectral_type("B5V");
        let g = b_v_from_spectral_type("G2V");
        let m = b_v_from_spectral_type("M5V");
        assert!(o < b && b < g && g < m, "{o} {b} {g} {m}");
        assert!((b_v_from_spectral_type("") - 0.65).abs() < 1.0e-9);
    }

    #[test]
    fn angular_separation_handles_poles_and_antipodes() {
        assert!((angular_separation_deg(0.0, 90.0, 180.0, 90.0)).abs() < 1.0e-9);
        assert!((angular_separation_deg(0.0, 0.0, 180.0, 0.0) - 180.0).abs() < 1.0e-9);
        assert!((angular_separation_deg(0.0, 0.0, 0.0, 10.0) - 10.0).abs() < 1.0e-9);
        // Right ascension separations shrink with the cosine of the declination.
        let near_pole = angular_separation_deg(0.0, 85.0, 10.0, 85.0);
        assert!(near_pole < 1.0, "{near_pole}");
    }

    #[test]
    fn cone_queries_return_exactly_the_enclosed_stars() {
        let stars = vec![
            sample_star(1, 10.0, 41.0, 5.0),
            sample_star(2, 10.5, 41.2, 6.0),
            sample_star(3, 200.0, -30.0, 4.0),
            sample_star(4, 10.2, 41.1, 12.0),
        ];
        let catalog = StarCatalog::from_stars(stars, "memory", CatalogFormat::DelimitedTable);

        let found = catalog.query_cone(10.0, 41.0, 1.0, 20.0);
        assert_eq!(found.len(), 3);
        assert_eq!(found[0].catalog_number, 1);

        let bright = catalog.query_cone(10.0, 41.0, 1.0, 6.5);
        assert_eq!(bright.len(), 2);

        let elsewhere = catalog.query_cone(200.0, -30.0, 0.5, 20.0);
        assert_eq!(elsewhere.len(), 1);
        assert_eq!(elsewhere[0].catalog_number, 3);
    }

    #[test]
    fn cone_queries_wrap_around_the_origin_of_right_ascension() {
        let stars = vec![
            sample_star(1, 359.5, 0.0, 5.0),
            sample_star(2, 0.5, 0.0, 5.0),
            sample_star(3, 180.0, 0.0, 5.0),
        ];
        let catalog = StarCatalog::from_stars(stars, "memory", CatalogFormat::DelimitedTable);

        let found = catalog.query_cone(0.0, 0.0, 1.0, 20.0);
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn cone_queries_near_the_pole_collect_every_longitude() {
        let stars = vec![
            sample_star(1, 0.0, 89.5, 5.0),
            sample_star(2, 120.0, 89.6, 5.0),
            sample_star(3, 240.0, 89.7, 5.0),
            sample_star(4, 0.0, 0.0, 5.0),
        ];
        let catalog = StarCatalog::from_stars(stars, "memory", CatalogFormat::DelimitedTable);

        let found = catalog.query_cone(0.0, 90.0, 1.0, 20.0);
        assert_eq!(found.len(), 3, "the three polar stars must be reported");
    }

    #[test]
    fn a_cone_query_matches_the_exhaustive_scan() {
        let mut stars = Vec::new();
        for step in 0..2_000u32 {
            // Deterministic spread over the whole sky, without random generation.
            let ra = (step as f64 * 137.508).rem_euclid(360.0);
            let dec = ((step as f64 * 0.618_034).fract() * 180.0) - 90.0;
            stars.push(sample_star(step, ra, dec, 3.0 + (step % 10) as f64));
        }
        let catalog = StarCatalog::from_stars(stars, "memory", CatalogFormat::DelimitedTable);

        for (ra, dec, radius) in [
            (0.0, 0.0, 5.0),
            (120.0, 45.0, 10.0),
            (300.0, -70.0, 15.0),
            (10.0, 89.0, 3.0),
        ] {
            let indexed = catalog.query_cone(ra, dec, radius, 30.0).len();
            let exhaustive = catalog
                .stars()
                .iter()
                .filter(|star| {
                    angular_separation_deg(
                        ra,
                        dec,
                        star.right_ascension_deg,
                        star.declination_deg,
                    ) <= radius
                })
                .count();
            assert_eq!(indexed, exhaustive, "cone {ra} {dec} {radius}");
        }
    }

    #[test]
    fn magnitude_selection_is_sorted_from_the_brightest() {
        let stars = vec![
            sample_star(1, 10.0, 0.0, 6.0),
            sample_star(2, 20.0, 0.0, 2.0),
            sample_star(3, 30.0, 0.0, 4.0),
            sample_star(4, 40.0, 0.0, 9.0),
        ];
        let catalog = StarCatalog::from_stars(stars, "memory", CatalogFormat::DelimitedTable);

        let selected = catalog.brighter_than(6.5);
        assert_eq!(selected.len(), 3);
        assert_eq!(selected[0].catalog_number, 2);
        assert_eq!(selected[2].catalog_number, 1);
        assert!((catalog.brightest_magnitude() - 2.0).abs() < 1.0e-9);
        assert!((catalog.faintest_magnitude() - 9.0).abs() < 1.0e-9);
    }

    fn celestia_record(hip: u32, x: f32, y: f32, z: f32, absolute: f64, packed: u16) -> Vec<u8> {
        let mut record = Vec::new();
        record.extend_from_slice(&hip.to_le_bytes());
        record.extend_from_slice(&x.to_le_bytes());
        record.extend_from_slice(&y.to_le_bytes());
        record.extend_from_slice(&z.to_le_bytes());
        record.extend_from_slice(&(((absolute * 256.0).round()) as i16).to_le_bytes());
        record.extend_from_slice(&packed.to_le_bytes());
        record
    }

    #[test]
    fn reads_the_celestia_binary_catalogue() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"CELSTARS");
        bytes.extend_from_slice(&0x0100u16.to_le_bytes());
        bytes.extend_from_slice(&2u32.to_le_bytes());
        // Celestia frame: Y is the celestial north pole, so a star on the +Y axis sits
        // at declination +90 degrees.
        bytes.extend(celestia_record(1, 0.0, 10.0, 0.0, 4.5, 0x4206));
        bytes.extend(celestia_record(2, 10.0, 0.0, 0.0, 1.0, 0x0006));

        let catalog = parse_celestia_stars_dat(&bytes, "memory").unwrap();
        assert_eq!(catalog.len(), 2);
        assert_eq!(catalog.format(), CatalogFormat::CelestiaBinary);

        let polar = catalog.find_by_catalog_number(1).unwrap();
        assert!((polar.declination_deg - 90.0).abs() < 1.0e-4);
        assert!((polar.distance_ly - 10.0).abs() < 1.0e-4);
        assert!((polar.absolute_magnitude - 4.5).abs() < 0.01);
        assert_eq!(polar.spectral_type, "G2V");

        let equatorial = catalog.find_by_catalog_number(2).unwrap();
        assert!(equatorial.declination_deg.abs() < 1.0e-4);
        assert!(equatorial.right_ascension_deg.abs() < 1.0e-4);
    }

    #[test]
    fn rejects_a_corrupt_celestia_catalogue() {
        assert!(parse_celestia_stars_dat(b"NOTSTARS", "memory").is_err());

        let mut truncated = Vec::new();
        truncated.extend_from_slice(b"CELSTARS");
        truncated.extend_from_slice(&0x0100u16.to_le_bytes());
        truncated.extend_from_slice(&5u32.to_le_bytes());
        let error = parse_celestia_stars_dat(&truncated, "memory").unwrap_err();
        assert!(error.contains("declares 5 stars"), "{error}");
    }

    #[test]
    fn reads_a_hipparcos_record() {
        let mut fields = vec![""; 78];
        fields[0] = "H";
        fields[1] = "71683";
        fields[5] = "-0.01";
        fields[8] = "219.92041034";
        fields[9] = "-60.83514707";
        fields[11] = "742.12";
        fields[37] = "0.710";
        fields[76] = "G2V";
        let line = fields.join("|");

        let catalog = parse_hipparcos_main(&line, "memory").unwrap();
        assert_eq!(catalog.len(), 1);
        let star = catalog.find_by_catalog_number(71683).unwrap();
        assert!((star.apparent_magnitude + 0.01).abs() < 1.0e-9);
        assert!((star.color_index_b_v - 0.710).abs() < 1.0e-9);
        // 742.12 mas corresponds to about 4.4 light years.
        assert!((star.distance_ly - 4.395).abs() < 0.01, "{}", star.distance_ly);
        assert!(star.absolute_magnitude > 4.0 && star.absolute_magnitude < 5.0);
    }

    #[test]
    fn rejects_a_hipparcos_file_without_usable_records() {
        let error = parse_hipparcos_main("# header only\n", "memory").unwrap_err();
        assert!(error.contains("no usable"), "{error}");
    }

    #[test]
    fn reads_a_gaia_style_csv_export() {
        let path = std::env::temp_dir().join("starcatalog_gaia_export.csv");
        std::fs::write(
            &path,
            "source_id,ra,dec,parallax,phot_g_mean_mag,bp_rp\n4295806720,297.6958,8.8683,546.976,8.984,2.8\n1872046574,332.0,45.0,7.5,6.2,0.4\n",
        )
        .unwrap();

        let catalog = read_delimited_catalog(path.to_str().unwrap()).unwrap();
        assert_eq!(catalog.len(), 2);
        assert_eq!(catalog.format(), CatalogFormat::DelimitedTable);

        let nearby = &catalog.stars()[0];
        assert!((nearby.distance_ly - 5.963).abs() < 0.01, "{}", nearby.distance_ly);
        assert!((nearby.color_index_b_v - 2.8).abs() < 1.0e-9);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_csv_without_the_required_columns_is_rejected() {
        let path = std::env::temp_dir().join("starcatalog_bad_export.csv");
        std::fs::write(&path, "ra,dec\n10.0,20.0\n").unwrap();
        let error = read_delimited_catalog(path.to_str().unwrap()).unwrap_err();
        assert!(error.contains("magnitude"), "{error}");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_missing_catalogue_file_is_reported() {
        let error = load_star_catalog("absent_catalogue.csv").unwrap_err();
        assert!(error.contains("absent_catalogue.csv"), "{error}");
    }

    #[test]
    fn ecliptic_conversion_preserves_the_vernal_equinox() {
        let direction = equatorial_to_ecliptic_direction([1.0, 0.0, 0.0]);
        assert!((direction[0] - 1.0).abs() < 1.0e-12);
        assert!(direction[1].abs() < 1.0e-12);
        assert!(direction[2].abs() < 1.0e-12);

        // The celestial pole tilts by the obliquity in the ecliptic frame.
        let pole = equatorial_to_ecliptic_direction([0.0, 0.0, 1.0]);
        let tilt = pole[1].atan2(pole[2]).to_degrees();
        assert!((tilt - J2000_OBLIQUITY_DEG).abs() < 1.0e-9);
        // Its ecliptic latitude is therefore 90 degrees minus the obliquity.
        let latitude = pole[2].asin().to_degrees();
        assert!((latitude - (90.0 - J2000_OBLIQUITY_DEG)).abs() < 1.0e-9);
    }

    #[test]
    fn an_empty_catalogue_reports_no_star_rather_than_inventing_one() {
        let catalog = StarCatalog::from_stars(Vec::new(), "memory", CatalogFormat::DelimitedTable);
        assert!(catalog.is_empty());
        assert_eq!(catalog.len(), 0);
        assert!(catalog.query_cone(0.0, 0.0, 180.0, 30.0).is_empty());
        assert!(catalog.brighter_than(30.0).is_empty());
    }
}
