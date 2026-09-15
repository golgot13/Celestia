use std::f64::consts::PI;
use crate::{
    KeplerianElements, GAUSSIAN_GRAVITATIONAL_CONSTANT_K, J2000_OBLIQUITY_RAD,
};

#[derive(Clone, Debug, PartialEq)]
pub struct AstrometricObservation {
    pub epoch_jd: f64,
    pub ra_deg: f64,
    pub dec_deg: f64,
    pub observer_pos_au: [f64; 3], // Heliocentric equatorial observer position (J2000)
}

#[derive(Clone, Debug, PartialEq)]
pub struct IODResult {
    pub elements: KeplerianElements,
    pub heliocentric_pos_au: [f64; 3],
    pub heliocentric_vel_au_day: [f64; 3],
    pub slant_range_au: (f64, f64, f64),
    pub orbital_period_years: f64,
    pub semi_major_axis_au: f64,
    pub eccentricity: f64,
    pub inclination_deg: f64,
    pub converged: bool,
}

fn cross_product(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot_product(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn magnitude(a: [f64; 3]) -> f64 {
    dot_product(a, a).sqrt()
}

pub fn state_vector_to_keplerian_elements(
    r_eq: [f64; 3],
    v_eq: [f64; 3],
    epoch_jd: f64,
    name: &'static str,
) -> Result<KeplerianElements, String> {
    // Transform from equatorial J2000 to ecliptic J2000
    let cos_eps = J2000_OBLIQUITY_RAD.cos();
    let sin_eps = J2000_OBLIQUITY_RAD.sin();

    let r_ecl = [
        r_eq[0],
        r_eq[1] * cos_eps + r_eq[2] * sin_eps,
        -r_eq[1] * sin_eps + r_eq[2] * cos_eps,
    ];

    let v_ecl = [
        v_eq[0],
        v_eq[1] * cos_eps + v_eq[2] * sin_eps,
        -v_eq[1] * sin_eps + v_eq[2] * cos_eps,
    ];

    let mu = GAUSSIAN_GRAVITATIONAL_CONSTANT_K * GAUSSIAN_GRAVITATIONAL_CONSTANT_K; // AU^3 / day^2
    let r_mag = magnitude(r_ecl);
    let v_mag = magnitude(v_ecl);

    if r_mag <= 1e-6 {
        return Err("heliocentric distance too small".to_string());
    }

    // Specific orbital energy: epsilon = v^2/2 - mu/r = -mu / (2a)
    let energy = 0.5 * v_mag * v_mag - mu / r_mag;
    let semi_major_axis_au = if energy.abs() > 1e-12 {
        -mu / (2.0 * energy)
    } else {
        f64::INFINITY
    };

    // Specific angular momentum vector: h = r x v
    let h_vec = cross_product(r_ecl, v_ecl);
    let h_mag = magnitude(h_vec);

    // Eccentricity vector: e_vec = (v x h)/mu - r/|r|
    let v_cross_h = cross_product(v_ecl, h_vec);
    let e_vec = [
        v_cross_h[0] / mu - r_ecl[0] / r_mag,
        v_cross_h[1] / mu - r_ecl[1] / r_mag,
        v_cross_h[2] / mu - r_ecl[2] / r_mag,
    ];
    let eccentricity = magnitude(e_vec);

    // Inclination: cos(i) = h_z / |h|
    let inc_rad = (h_vec[2] / h_mag.max(1e-12)).clamp(-1.0, 1.0).acos();
    let inclination_deg = inc_rad.to_degrees();

    // Node vector: N = k x h = [-h_y, h_x, 0]
    let n_vec = [-h_vec[1], h_vec[0], 0.0];
    let n_mag = magnitude(n_vec);

    let node_rad = if n_mag > 1e-9 {
        let mut n_angle = (n_vec[0] / n_mag).clamp(-1.0, 1.0).acos();
        if n_vec[1] < 0.0 {
            n_angle = 2.0 * PI - n_angle;
        }
        n_angle
    } else {
        0.0
    };
    let longitude_ascending_node_deg = node_rad.to_degrees();

    // Argument of periapsis: cos(omega) = (N . e) / (|N| * |e|)
    let peri_rad = if n_mag > 1e-9 && eccentricity > 1e-6 {
        let mut w_angle = (dot_product(n_vec, e_vec) / (n_mag * eccentricity)).clamp(-1.0, 1.0).acos();
        if e_vec[2] < 0.0 {
            w_angle = 2.0 * PI - w_angle;
        }
        w_angle
    } else {
        0.0
    };
    let argument_periapsis_deg = peri_rad.to_degrees();

    // True anomaly: cos(nu) = (e . r) / (|e| * |r|)
    let nu_rad = if eccentricity > 1e-6 {
        let mut nu_angle = (dot_product(e_vec, r_ecl) / (eccentricity * r_mag)).clamp(-1.0, 1.0).acos();
        if dot_product(r_ecl, v_ecl) < 0.0 {
            nu_angle = 2.0 * PI - nu_angle;
        }
        nu_angle
    } else {
        0.0
    };

    // Mean anomaly from eccentric anomaly
    let cos_ea = (eccentricity + nu_rad.cos()) / (1.0 + eccentricity * nu_rad.cos());
    let sin_ea = ((1.0 - eccentricity * eccentricity).max(0.0).sqrt() * nu_rad.sin()) / (1.0 + eccentricity * nu_rad.cos());
    let ea_rad = sin_ea.atan2(cos_ea).rem_euclid(2.0 * PI);
    let m_rad = (ea_rad - eccentricity * ea_rad.sin()).rem_euclid(2.0 * PI);
    let mean_anomaly_epoch_deg = m_rad.to_degrees();

    Ok(KeplerianElements {
        name,
        semi_major_axis_au,
        eccentricity,
        inclination_deg,
        longitude_ascending_node_deg,
        argument_periapsis_deg,
        mean_anomaly_epoch_deg,
        epoch_jd,
        absolute_magnitude_h: 12.0,
        slope_parameter_g: 0.15,
    })
}

pub fn solve_gauss_initial_orbit_determination(
    obs1: &AstrometricObservation,
    obs2: &AstrometricObservation,
    obs3: &AstrometricObservation,
    object_name: &'static str,
) -> Result<IODResult, String> {
    let k = GAUSSIAN_GRAVITATIONAL_CONSTANT_K;
    let tau1 = k * (obs1.epoch_jd - obs2.epoch_jd);
    let tau3 = k * (obs3.epoch_jd - obs2.epoch_jd);
    let tau = tau3 - tau1;

    if tau1.abs() < 1e-6 || tau3.abs() < 1e-6 {
        return Err("observation epochs must be distinct".to_string());
    }

    // Compute unit line-of-sight vectors L_i = [cos(dec)*cos(ra), cos(dec)*sin(ra), sin(dec)]
    let line_of_sight = |obs: &AstrometricObservation| -> [f64; 3] {
        let ra = obs.ra_deg.to_radians();
        let dec = obs.dec_deg.to_radians();
        [dec.cos() * ra.cos(), dec.cos() * ra.sin(), dec.sin()]
    };

    let l1 = line_of_sight(obs1);
    let l2 = line_of_sight(obs2);
    let l3 = line_of_sight(obs3);

    // Compute scalar triple products
    let l2_cross_l3 = cross_product(l2, l3);
    let d0 = dot_product(l1, l2_cross_l3);
    if d0.abs() < 1e-12 {
        return Err("coplanar sightlines cannot resolve orbit via Gauss method".to_string());
    }

    let d11 = dot_product(obs1.observer_pos_au, l2_cross_l3);
    let d12 = dot_product(obs2.observer_pos_au, l2_cross_l3);
    let d13 = dot_product(obs3.observer_pos_au, l2_cross_l3);

    let l1_cross_l3 = cross_product(l1, l3);
    let d21 = dot_product(obs1.observer_pos_au, l1_cross_l3);
    let d22 = dot_product(obs2.observer_pos_au, l1_cross_l3);
    let d23 = dot_product(obs3.observer_pos_au, l1_cross_l3);

    let l1_cross_l2 = cross_product(l1, l2);
    let d31 = dot_product(obs1.observer_pos_au, l1_cross_l2);
    let d32 = dot_product(obs2.observer_pos_au, l1_cross_l2);
    let d33 = dot_product(obs3.observer_pos_au, l1_cross_l2);

    let a1 = tau3 / tau;
    let b1 = (tau3 * (tau * tau - tau3 * tau3)) / (6.0 * tau);
    let a3 = -tau1 / tau;
    let b3 = (-tau1 * (tau * tau - tau1 * tau1)) / (6.0 * tau);

    let e_scalar = dot_product(l2, obs2.observer_pos_au);
    let q_scalar = dot_product(obs2.observer_pos_au, obs2.observer_pos_au);

    // Solve for heliocentric radius r2 using Newton-Raphson on Gauss 8th degree polynomial equation
    let mut r2 = 2.5; // Initial guess for main belt asteroid
    for _ in 0..30 {
        let u2 = 1.0 / (r2 * r2 * r2);
        let c1 = a1 + b1 * u2;
        let c3 = a3 + b3 * u2;

        let rho2 = (c1 * d21 - d22 + c3 * d23) / (-d0);
        let r2_new_sq = rho2 * rho2 + 2.0 * rho2 * e_scalar + q_scalar;
        let r2_new = r2_new_sq.max(0.01).sqrt();

        let diff = (r2_new - r2).abs();
        r2 = 0.5 * (r2 + r2_new);
        if diff < 1e-12 {
            break;
        }
    }

    let u2 = 1.0 / (r2 * r2 * r2);
    let c1 = a1 + b1 * u2;
    let c3 = a3 + b3 * u2;

    let rho1 = (-c1 * d11 + d12 - c3 * d13) / (c1 * d0);
    let rho2 = (c1 * d21 - d22 + c3 * d23) / (-d0);
    let rho3 = (-c1 * d31 + d32 - c3 * d33) / (c3 * d0);

    // Compute heliocentric position vectors: r_i = R_i + rho_i * L_i
    let r1 = [
        obs1.observer_pos_au[0] + rho1 * l1[0],
        obs1.observer_pos_au[1] + rho1 * l1[1],
        obs1.observer_pos_au[2] + rho1 * l1[2],
    ];
    let r2_vec = [
        obs2.observer_pos_au[0] + rho2 * l2[0],
        obs2.observer_pos_au[1] + rho2 * l2[1],
        obs2.observer_pos_au[2] + rho2 * l2[2],
    ];
    let r3 = [
        obs3.observer_pos_au[0] + rho3 * l3[0],
        obs3.observer_pos_au[1] + rho3 * l3[1],
        obs3.observer_pos_au[2] + rho3 * l3[2],
    ];

    // Compute velocity v2 using Lagrange f and g coefficients
    let f1 = 1.0 - 0.5 * u2 * tau1 * tau1;
    let f3 = 1.0 - 0.5 * u2 * tau3 * tau3;
    let g1 = tau1 - (1.0 / 6.0) * u2 * tau1.powi(3);
    let g3 = tau3 - (1.0 / 6.0) * u2 * tau3.powi(3);

    let fg_denom = f1 * g3 - f3 * g1;
    if fg_denom.abs() < 1e-12 {
        return Err("Lagrange series determinant near zero".to_string());
    }

    // Velocity in AU/day (divided by Gaussian constant k to get per day)
    let v2_vec = [
        ((-f3 * r1[0] + f1 * r3[0]) / fg_denom) * k,
        ((-f3 * r1[1] + f1 * r3[1]) / fg_denom) * k,
        ((-f3 * r1[2] + f1 * r3[2]) / fg_denom) * k,
    ];

    let elements = state_vector_to_keplerian_elements(r2_vec, v2_vec, obs2.epoch_jd, object_name)?;
    let period_years = elements.semi_major_axis_au.powf(1.5);

    Ok(IODResult {
        elements: elements.clone(),
        heliocentric_pos_au: r2_vec,
        heliocentric_vel_au_day: v2_vec,
        slant_range_au: (rho1, rho2, rho3),
        orbital_period_years: period_years,
        semi_major_axis_au: elements.semi_major_axis_au,
        eccentricity: elements.eccentricity,
        inclination_deg: elements.inclination_deg,
        converged: elements.semi_major_axis_au > 0.1 && elements.eccentricity < 1.0,
    })
}

pub fn iod_result_to_json(result: &IODResult) -> String {
    format!(
        "{{\n  \"object_name\": \"{}\",\n  \"epoch_jd\": {:.4},\n  \"semi_major_axis_au\": {:.6},\n  \"eccentricity\": {:.6},\n  \"inclination_deg\": {:.4},\n  \"node_deg\": {:.4},\n  \"periapsis_deg\": {:.4},\n  \"mean_anomaly_deg\": {:.4},\n  \"orbital_period_years\": {:.4},\n  \"slant_ranges_au\": [{:.4}, {:.4}, {:.4}],\n  \"converged\": {}\n}}\n",
        result.elements.name,
        result.elements.epoch_jd,
        result.semi_major_axis_au,
        result.eccentricity,
        result.inclination_deg,
        result.elements.longitude_ascending_node_deg,
        result.elements.argument_periapsis_deg,
        result.elements.mean_anomaly_epoch_deg,
        result.orbital_period_years,
        result.slant_range_au.0,
        result.slant_range_au.1,
        result.slant_range_au.2,
        result.converged
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{compute_apparent_ephemeris, KeplerianElements};

    #[test]
    fn solves_initial_orbit_from_three_sightlines() {
        // True asteroid orbit: a=2.5 AU, e=0.15, i=10 deg
        let true_asteroid = KeplerianElements {
            name: "2026-NEO-01",
            semi_major_axis_au: 2.50,
            eccentricity: 0.15,
            inclination_deg: 10.0,
            longitude_ascending_node_deg: 45.0,
            argument_periapsis_deg: 30.0,
            mean_anomaly_epoch_deg: 0.0,
            epoch_jd: 2459000.0,
            absolute_magnitude_h: 18.0,
            slope_parameter_g: 0.15,
        };

        // Observer (Earth) positions at t0, t0+5 days, t0+10 days
        let t1 = 2459000.0;
        let t2 = 2459005.0;
        let t3 = 2459010.0;

        let earth_pos1 = [1.0, 0.0, 0.0];
        let earth_pos2 = [0.996, 0.086, 0.0];
        let earth_pos3 = [0.985, 0.171, 0.0];

        let ephem1 = compute_apparent_ephemeris(&true_asteroid, earth_pos1, t1);
        let ephem2 = compute_apparent_ephemeris(&true_asteroid, earth_pos2, t2);
        let ephem3 = compute_apparent_ephemeris(&true_asteroid, earth_pos3, t3);

        let obs1 = AstrometricObservation {
            epoch_jd: t1,
            ra_deg: ephem1.ra_deg,
            dec_deg: ephem1.dec_deg,
            observer_pos_au: earth_pos1,
        };
        let obs2 = AstrometricObservation {
            epoch_jd: t2,
            ra_deg: ephem2.ra_deg,
            dec_deg: ephem2.dec_deg,
            observer_pos_au: earth_pos2,
        };
        let obs3 = AstrometricObservation {
            epoch_jd: t3,
            ra_deg: ephem3.ra_deg,
            dec_deg: ephem3.dec_deg,
            observer_pos_au: earth_pos3,
        };

        let iod = solve_gauss_initial_orbit_determination(&obs1, &obs2, &obs3, "2026-NEO-01").unwrap();
        assert!(iod.converged);
        assert!((iod.semi_major_axis_au - 2.50).abs() < 0.25);
        assert!((iod.eccentricity - 0.15).abs() < 0.1);
        assert!(iod.orbital_period_years > 3.0 && iod.orbital_period_years < 5.0);
    }
}
