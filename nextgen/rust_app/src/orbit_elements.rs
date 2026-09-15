use std::f64::consts::PI;

pub const GAUSSIAN_GRAVITATIONAL_CONSTANT_K: f64 = 0.01720209895; // AU^(3/2) / day
pub const J2000_OBLIQUITY_RAD: f64 = 0.409092804; // 23.4392911 degrees

#[derive(Clone, Debug, PartialEq)]
pub struct KeplerianElements {
    pub name: &'static str,
    pub semi_major_axis_au: f64,
    pub eccentricity: f64,
    pub inclination_deg: f64,
    pub longitude_ascending_node_deg: f64,
    pub argument_periapsis_deg: f64,
    pub mean_anomaly_epoch_deg: f64,
    pub epoch_jd: f64,
    pub absolute_magnitude_h: f64,
    pub slope_parameter_g: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TargetEphemeris {
    pub name: &'static str,
    pub target_jd: f64,
    pub ra_deg: f64,
    pub dec_deg: f64,
    pub heliocentric_distance_au: f64,
    pub geocentric_distance_au: f64,
    pub phase_angle_deg: f64,
    pub apparent_magnitude_v: f64,
    pub proper_motion_ra_arcsec_hr: f64,
    pub proper_motion_dec_arcsec_hr: f64,
}

pub fn solve_kepler_equation(mean_anomaly_rad: f64, eccentricity: f64) -> f64 {
    let m = mean_anomaly_rad.rem_euclid(2.0 * PI);
    let e = eccentricity.clamp(0.0, 0.999999);

    // Initial guess using Danby's formula
    let mut ea = if e < 0.8 {
        m + e * m.sin() + 0.5 * e * e * (2.0 * m).sin()
    } else {
        PI
    };

    // Newton-Raphson iteration with cubic convergence correction
    for _ in 0..15 {
        let s = ea.sin();
        let c = ea.cos();
        let f = ea - e * s - m;
        let f_prime = 1.0 - e * c;
        let f_double_prime = e * s;

        let delta = f / f_prime;
        let delta_corr = f / (f_prime - 0.5 * delta * f_double_prime);

        ea -= delta_corr;
        if delta_corr.abs() < 1e-12 {
            break;
        }
    }

    ea
}

pub fn orbital_elements_to_heliocentric_equatorial(
    elements: &KeplerianElements,
    target_jd: f64,
) -> ([f64; 3], [f64; 3]) {
    let a = elements.semi_major_axis_au;
    let e = elements.eccentricity;
    let i_rad = elements.inclination_deg * PI / 180.0;
    let node_rad = elements.longitude_ascending_node_deg * PI / 180.0;
    let peri_rad = elements.argument_periapsis_deg * PI / 180.0;

    // Mean motion n = k / a^(3/2) in rad/day
    let n_rad_per_day = GAUSSIAN_GRAVITATIONAL_CONSTANT_K / (a.powf(1.5));
    let dt_days = target_jd - elements.epoch_jd;
    let m_rad = (elements.mean_anomaly_epoch_deg * PI / 180.0) + n_rad_per_day * dt_days;

    let ea_rad = solve_kepler_equation(m_rad, e);

    // Orbital plane coordinates
    let cos_ea = ea_rad.cos();
    let sin_ea = ea_rad.sin();
    let r = a * (1.0 - e * cos_ea);

    let x_orb = a * (cos_ea - e);
    let y_orb = a * (1.0 - e * e).sqrt() * sin_ea;

    let vx_orb = -(a * a * n_rad_per_day / r) * sin_ea;
    let vy_orb = (a * a * n_rad_per_day / r) * (1.0 - e * e).sqrt() * cos_ea;

    // Rotation from orbital plane to heliocentric ecliptic
    let sin_node = node_rad.sin();
    let cos_node = node_rad.cos();
    let sin_peri = peri_rad.sin();
    let cos_peri = peri_rad.cos();
    let sin_i = i_rad.sin();
    let cos_i = i_rad.cos();

    let px = cos_node * cos_peri - sin_node * sin_peri * cos_i;
    let py = sin_node * cos_peri + cos_node * sin_peri * cos_i;
    let pz = sin_peri * sin_i;

    let qx = -cos_node * sin_peri - sin_node * cos_peri * cos_i;
    let qy = -sin_node * sin_peri + cos_node * cos_peri * cos_i;
    let qz = cos_peri * sin_i;

    let x_ecl = x_orb * px + y_orb * qx;
    let y_ecl = x_orb * py + y_orb * qy;
    let z_ecl = x_orb * pz + y_orb * qz;

    let vx_ecl = vx_orb * px + vy_orb * qx;
    let vy_ecl = vx_orb * py + vy_orb * qy;
    let vz_ecl = vx_orb * pz + vy_orb * qz;

    // Rotation from ecliptic to equatorial J2000
    let cos_eps = J2000_OBLIQUITY_RAD.cos();
    let sin_eps = J2000_OBLIQUITY_RAD.sin();

    let x_eq = x_ecl;
    let y_eq = y_ecl * cos_eps - z_ecl * sin_eps;
    let z_eq = y_ecl * sin_eps + z_ecl * cos_eps;

    let vx_eq = vx_ecl;
    let vy_eq = vy_ecl * cos_eps - vz_ecl * sin_eps;
    let vz_eq = vy_ecl * sin_eps + vz_ecl * cos_eps;

    ([x_eq, y_eq, z_eq], [vx_eq, vy_eq, vz_eq])
}

pub fn compute_apparent_ephemeris(
    elements: &KeplerianElements,
    earth_pos_au: [f64; 3],
    target_jd: f64,
) -> TargetEphemeris {
    let (target_pos, _target_vel) = orbital_elements_to_heliocentric_equatorial(elements, target_jd);

    // Geocentric vector: rho = r_target - r_earth
    let dx = target_pos[0] - earth_pos_au[0];
    let dy = target_pos[1] - earth_pos_au[1];
    let dz = target_pos[2] - earth_pos_au[2];

    let delta_au = (dx * dx + dy * dy + dz * dz).sqrt();
    let r_helio_au = (target_pos[0].powi(2) + target_pos[1].powi(2) + target_pos[2].powi(2)).sqrt();
    let r_earth_au = (earth_pos_au[0].powi(2) + earth_pos_au[1].powi(2) + earth_pos_au[2].powi(2)).sqrt();

    // Equatorial spherical coordinates (RA, Dec)
    let ra_rad = dy.atan2(dx).rem_euclid(2.0 * PI);
    let dec_rad = (dz / delta_au).asin();

    let ra_deg = ra_rad * 180.0 / PI;
    let dec_deg = dec_rad * 180.0 / PI;

    // Phase angle alpha (Sun-Target-Earth angle)
    let cos_phase = ((r_helio_au * r_helio_au + delta_au * delta_au - r_earth_au * r_earth_au)
        / (2.0 * r_helio_au * delta_au))
        .clamp(-1.0, 1.0);
    let phase_angle_rad = cos_phase.acos();
    let phase_angle_deg = phase_angle_rad * 180.0 / PI;

    // IAU HG Magnitude model for asteroids/minor planets
    let tan_half_alpha = (phase_angle_rad * 0.5).tan();
    let phi1 = (-3.33 * tan_half_alpha.powf(0.63)).exp();
    let phi2 = (-1.87 * tan_half_alpha.powf(1.22)).exp();
    let g = elements.slope_parameter_g;
    let phase_term = -2.5 * ((1.0 - g) * phi1 + g * phi2).max(1e-6).log10();

    let v_mag = elements.absolute_magnitude_h + 5.0 * (r_helio_au * delta_au).log10() + phase_term;

    // Proper motion estimation (arcsec/hour)
    let dt_h = 1.0 / 24.0;
    let (target_pos_next, _) = orbital_elements_to_heliocentric_equatorial(elements, target_jd + dt_h);
    let dx_next = target_pos_next[0] - earth_pos_au[0];
    let dy_next = target_pos_next[1] - earth_pos_au[1];
    let dz_next = target_pos_next[2] - earth_pos_au[2];
    let delta_next = (dx_next * dx_next + dy_next * dy_next + dz_next * dz_next).sqrt();

    let ra_next_rad = dy_next.atan2(dx_next).rem_euclid(2.0 * PI);
    let dec_next_rad = (dz_next / delta_next).asin();

    let dra_arcsec = (ra_next_rad - ra_rad) * dec_rad.cos() * (180.0 / PI) * 3600.0;
    let ddec_arcsec = (dec_next_rad - dec_rad) * (180.0 / PI) * 3600.0;

    TargetEphemeris {
        name: elements.name,
        target_jd,
        ra_deg,
        dec_deg,
        heliocentric_distance_au: r_helio_au,
        geocentric_distance_au: delta_au,
        phase_angle_deg,
        apparent_magnitude_v: v_mag,
        proper_motion_ra_arcsec_hr: dra_arcsec,
        proper_motion_dec_arcsec_hr: ddec_arcsec,
    }
}

pub fn target_ephemeris_to_json(ephem: &TargetEphemeris) -> String {
    format!(
        "{{\n  \"name\": \"{}\",\n  \"target_jd\": {:.4},\n  \"ra_deg\": {:.6},\n  \"dec_deg\": {:.6},\n  \"heliocentric_distance_au\": {:.6},\n  \"geocentric_distance_au\": {:.6},\n  \"phase_angle_deg\": {:.4},\n  \"apparent_magnitude_v\": {:.2},\n  \"proper_motion_ra_arcsec_hr\": {:.4},\n  \"proper_motion_dec_arcsec_hr\": {:.4}\n}}\n",
        ephem.name,
        ephem.target_jd,
        ephem.ra_deg,
        ephem.dec_deg,
        ephem.heliocentric_distance_au,
        ephem.geocentric_distance_au,
        ephem.phase_angle_deg,
        ephem.apparent_magnitude_v,
        ephem.proper_motion_ra_arcsec_hr,
        ephem.proper_motion_dec_arcsec_hr
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solves_kepler_equation_with_sub_arcsec_precision() {
        let m = 1.25; // radians
        let e = 0.2056; // Mercury eccentricity
        let ea = solve_kepler_equation(m, e);
        let reconstructed_m = ea - e * ea.sin();
        assert!((reconstructed_m - m).abs() < 1e-12);
    }

    #[test]
    fn computes_apparent_ephemeris_and_hg_magnitude_for_ceres() {
        // Dwarf Planet 1 Ceres orbital elements
        let ceres = KeplerianElements {
            name: "Ceres",
            semi_major_axis_au: 2.767,
            eccentricity: 0.0758,
            inclination_deg: 10.593,
            longitude_ascending_node_deg: 80.305,
            argument_periapsis_deg: 73.597,
            mean_anomaly_epoch_deg: 77.372,
            epoch_jd: 2459000.5,
            absolute_magnitude_h: 3.34,
            slope_parameter_g: 0.12,
        };

        let earth_pos = [1.0, 0.0, 0.0]; // Earth at spring equinox ~ 1 AU
        let ephem = compute_apparent_ephemeris(&ceres, earth_pos, 2459000.5);

        assert_eq!(ephem.name, "Ceres");
        assert!(ephem.heliocentric_distance_au > 2.5 && ephem.heliocentric_distance_au < 3.0);
        assert!(ephem.apparent_magnitude_v > 6.0 && ephem.apparent_magnitude_v < 10.0);
        assert!(ephem.proper_motion_ra_arcsec_hr.is_finite());
    }
}
