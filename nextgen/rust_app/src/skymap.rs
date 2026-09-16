//! Real-time local sky chart model: equatorial catalogue positions projected onto
//! the observer horizontal dome, using the same astrometric primitives as the
//! scheduling and pointing subsystems.

use crate::{
    compute_airmass, compute_local_sidereal_time_rad, deg_to_rad, equatorial_to_horizontal,
    rad_to_deg, GeographicCoord, SiteLimits,
};
use std::f64::consts::{PI, TAU};

/// Mean obliquity of the ecliptic at J2000.0 (IAU 2006), in degrees.
pub const J2000_MEAN_OBLIQUITY_DEG: f64 = 23.439_279_444_444_445;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkyObjectClass {
    CampaignTarget,
    SolarSystemBody,
    MountPointing,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SkyObjectRequest {
    pub name: String,
    pub ra_deg: f64,
    pub dec_deg: f64,
    pub class: SkyObjectClass,
    pub priority: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SkyObjectPlacement {
    pub name: String,
    pub class: SkyObjectClass,
    pub priority: u8,
    pub ra_deg: f64,
    pub dec_deg: f64,
    pub hour_angle_deg: f64,
    pub altitude_deg: f64,
    pub azimuth_deg: f64,
    pub airmass: f64,
    pub is_above_horizon: bool,
    pub is_observable: bool,
    /// Azimuthal-equidistant coordinates on the unit disk: north up, east left.
    pub chart_x: f64,
    pub chart_y: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SkyChart {
    pub julian_day: f64,
    pub local_sidereal_time_rad: f64,
    pub site_latitude_deg: f64,
    pub site_longitude_deg: f64,
    pub objects: Vec<SkyObjectPlacement>,
    pub above_horizon_count: usize,
    pub observable_count: usize,
}

impl SkyChart {
    pub fn local_sidereal_time_hours(&self) -> f64 {
        self.local_sidereal_time_rad * 12.0 / PI
    }

    pub fn find(&self, name: &str) -> Option<&SkyObjectPlacement> {
        self.objects.iter().find(|object| object.name == name)
    }
}

/// Projects horizontal coordinates onto the unit disk with an azimuthal-equidistant
/// mapping (zenith at the centre, horizon on the unit circle, north up, east left).
pub fn project_horizontal_to_unit_disk(azimuth_rad: f64, altitude_rad: f64) -> (f64, f64) {
    let radius = (1.0 - altitude_rad / (PI / 2.0)).clamp(0.0, 2.0);
    let x = -radius * azimuth_rad.sin();
    let y = radius * azimuth_rad.cos();
    (x, y)
}

/// Converts a heliocentric/geocentric ecliptic rectangular vector into equatorial
/// right ascension, declination and range.
pub fn ecliptic_vector_to_equatorial(x_ecl: f64, y_ecl: f64, z_ecl: f64) -> (f64, f64, f64) {
    let obliquity = deg_to_rad(J2000_MEAN_OBLIQUITY_DEG);
    let cos_eps = obliquity.cos();
    let sin_eps = obliquity.sin();

    let x_eq = x_ecl;
    let y_eq = y_ecl * cos_eps - z_ecl * sin_eps;
    let z_eq = y_ecl * sin_eps + z_ecl * cos_eps;

    let range = (x_eq * x_eq + y_eq * y_eq + z_eq * z_eq).sqrt();
    if range <= f64::EPSILON {
        return (0.0, 0.0, 0.0);
    }

    let ra_deg = rad_to_deg(y_eq.atan2(x_eq).rem_euclid(TAU));
    let dec_deg = rad_to_deg((z_eq / range).clamp(-1.0, 1.0).asin());
    (ra_deg, dec_deg, range)
}

/// Places a single equatorial position on the local dome for the given sidereal time.
pub fn place_object(
    request: &SkyObjectRequest,
    site: &GeographicCoord,
    limits: &SiteLimits,
    lst_rad: f64,
) -> SkyObjectPlacement {
    let ra_rad = deg_to_rad(request.ra_deg);
    let dec_rad = deg_to_rad(request.dec_deg);
    let latitude_rad = deg_to_rad(site.latitude_deg);

    let hour_angle_rad = (lst_rad - ra_rad).rem_euclid(TAU);
    let hour_angle_signed = if hour_angle_rad > PI {
        hour_angle_rad - TAU
    } else {
        hour_angle_rad
    };

    let horizontal = equatorial_to_horizontal(hour_angle_signed, dec_rad, latitude_rad);
    let altitude_deg = rad_to_deg(horizontal.altitude_rad);
    let azimuth_deg = rad_to_deg(horizontal.azimuth_rad);
    let airmass = compute_airmass(horizontal.altitude_rad);
    let is_above_horizon = altitude_deg > 0.0;
    let is_observable =
        altitude_deg >= limits.min_altitude_deg && airmass.is_finite() && airmass <= limits.max_airmass;

    let (chart_x, chart_y) =
        project_horizontal_to_unit_disk(horizontal.azimuth_rad, horizontal.altitude_rad);

    SkyObjectPlacement {
        name: request.name.clone(),
        class: request.class,
        priority: request.priority,
        ra_deg: request.ra_deg.rem_euclid(360.0),
        dec_deg: request.dec_deg,
        hour_angle_deg: rad_to_deg(hour_angle_signed),
        altitude_deg,
        azimuth_deg,
        airmass,
        is_above_horizon,
        is_observable,
        chart_x,
        chart_y,
    }
}

/// Builds the full local sky chart for the requested instant.
pub fn build_sky_chart(
    requests: &[SkyObjectRequest],
    site: &GeographicCoord,
    limits: &SiteLimits,
    julian_day: f64,
) -> SkyChart {
    let lst_rad = compute_local_sidereal_time_rad(julian_day, site.longitude_deg);
    let objects: Vec<SkyObjectPlacement> = requests
        .iter()
        .map(|request| place_object(request, site, limits, lst_rad))
        .collect();

    let above_horizon_count = objects.iter().filter(|object| object.is_above_horizon).count();
    let observable_count = objects.iter().filter(|object| object.is_observable).count();

    SkyChart {
        julian_day,
        local_sidereal_time_rad: lst_rad,
        site_latitude_deg: site.latitude_deg,
        site_longitude_deg: site.longitude_deg,
        objects,
        above_horizon_count,
        observable_count,
    }
}

/// Samples the horizon-projected path of a fixed equatorial position over a time window,
/// used to draw the nightly trail of a target on the chart.
pub fn sample_diurnal_track(
    ra_deg: f64,
    dec_deg: f64,
    site: &GeographicCoord,
    julian_day: f64,
    window_hours: f64,
    samples: usize,
) -> Vec<(f64, f64, f64)> {
    if samples < 2 || window_hours <= 0.0 {
        return Vec::new();
    }

    let latitude_rad = deg_to_rad(site.latitude_deg);
    let dec_rad = deg_to_rad(dec_deg);
    let ra_rad = deg_to_rad(ra_deg);
    let mut track = Vec::with_capacity(samples);

    for index in 0..samples {
        let fraction = index as f64 / (samples - 1) as f64;
        let offset_hours = -window_hours / 2.0 + fraction * window_hours;
        let jd = julian_day + offset_hours / 24.0;
        let lst_rad = compute_local_sidereal_time_rad(jd, site.longitude_deg);
        let hour_angle_rad = (lst_rad - ra_rad).rem_euclid(TAU);
        let hour_angle_signed = if hour_angle_rad > PI {
            hour_angle_rad - TAU
        } else {
            hour_angle_rad
        };
        let horizontal = equatorial_to_horizontal(hour_angle_signed, dec_rad, latitude_rad);
        let (x, y) =
            project_horizontal_to_unit_disk(horizontal.azimuth_rad, horizontal.altitude_rad);
        track.push((x, y, rad_to_deg(horizontal.altitude_rad)));
    }

    track
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_site() -> GeographicCoord {
        GeographicCoord {
            latitude_deg: 43.9346,
            longitude_deg: 5.7133,
            elevation_m: 650.0,
        }
    }

    #[test]
    fn zenith_projects_to_disk_centre() {
        let (x, y) = project_horizontal_to_unit_disk(1.23, PI / 2.0);
        assert!(x.abs() < 1e-12);
        assert!(y.abs() < 1e-12);
    }

    #[test]
    fn horizon_projects_to_unit_circle_with_north_up() {
        let (x, y) = project_horizontal_to_unit_disk(0.0, 0.0);
        assert!(x.abs() < 1e-12);
        assert!((y - 1.0).abs() < 1e-12);

        let (east_x, east_y) = project_horizontal_to_unit_disk(PI / 2.0, 0.0);
        assert!((east_x + 1.0).abs() < 1e-12);
        assert!(east_y.abs() < 1e-12);
    }

    #[test]
    fn ecliptic_pole_maps_to_equatorial_declination_of_obliquity_complement() {
        let (_, dec_deg, range) = ecliptic_vector_to_equatorial(0.0, 0.0, 1.0);
        assert!((range - 1.0).abs() < 1e-12);
        assert!((dec_deg - (90.0 - J2000_MEAN_OBLIQUITY_DEG)).abs() < 1e-9);
    }

    #[test]
    fn vernal_equinox_direction_is_preserved() {
        let (ra_deg, dec_deg, _) = ecliptic_vector_to_equatorial(1.0, 0.0, 0.0);
        assert!(ra_deg.abs() < 1e-9 || (ra_deg - 360.0).abs() < 1e-9);
        assert!(dec_deg.abs() < 1e-9);
    }

    #[test]
    fn object_at_local_meridian_and_site_latitude_is_near_zenith() {
        let site = test_site();
        let limits = SiteLimits::default();
        let julian_day = 2_460_000.5;
        let lst_rad = compute_local_sidereal_time_rad(julian_day, site.longitude_deg);
        let request = SkyObjectRequest {
            name: "meridian_probe".to_string(),
            ra_deg: rad_to_deg(lst_rad),
            dec_deg: site.latitude_deg,
            class: SkyObjectClass::CampaignTarget,
            priority: 5,
        };

        let placement = place_object(&request, &site, &limits, lst_rad);
        assert!(placement.altitude_deg > 89.9);
        assert!(placement.is_observable);
        assert!((placement.airmass - 1.0).abs() < 1e-3);
        assert!(placement.chart_x.hypot(placement.chart_y) < 2.0e-3);
    }

    #[test]
    fn chart_counts_match_placement_flags() {
        let site = test_site();
        let limits = SiteLimits::default();
        let julian_day = 2_460_000.5;
        let requests = vec![
            SkyObjectRequest {
                name: "north_pole".to_string(),
                ra_deg: 0.0,
                dec_deg: 90.0,
                class: SkyObjectClass::CampaignTarget,
                priority: 4,
            },
            SkyObjectRequest {
                name: "south_pole".to_string(),
                ra_deg: 0.0,
                dec_deg: -90.0,
                class: SkyObjectClass::CampaignTarget,
                priority: 4,
            },
        ];

        let chart = build_sky_chart(&requests, &site, &limits, julian_day);
        assert_eq!(chart.objects.len(), 2);
        assert_eq!(chart.above_horizon_count, 1);
        assert_eq!(chart.observable_count, 1);
        assert!(chart.find("north_pole").unwrap().is_above_horizon);
        assert!(!chart.find("south_pole").unwrap().is_above_horizon);
    }

    #[test]
    fn diurnal_track_is_continuous_and_bounded() {
        let site = test_site();
        let track = sample_diurnal_track(83.633, 22.014, &site, 2_460_000.5, 8.0, 33);
        assert_eq!(track.len(), 33);
        for (x, y, altitude_deg) in &track {
            assert!(x.is_finite() && y.is_finite());
            assert!(*altitude_deg >= -90.0 && *altitude_deg <= 90.0);
        }
        for pair in track.windows(2) {
            let step = (pair[1].0 - pair[0].0).hypot(pair[1].1 - pair[0].1);
            assert!(step < 0.5);
        }
    }
}
