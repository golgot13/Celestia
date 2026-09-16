//! Physical, rotational and orbital catalogue of the solar system.
//!
//! Rotational elements follow the IAU/IAG Working Group on Cartographic Coordinates
//! and Rotational Elements (WGCCRE) formulation: the body-fixed frame is defined by
//! the north pole direction (alpha0, delta0) in the ICRF/J2000 equatorial frame and by
//! the prime meridian angle W measured from the ascending node of the body equator on
//! the ICRF equator. Periodic (trigonometric) correction terms of the WGCCRE report are
//! not carried; only the secular terms are, which is stated per body through
//! [`RotationModel::Iau`].

use std::f64::consts::TAU;

pub const J2000_EPOCH_JD: f64 = 2_451_545.0;
pub const ASTRONOMICAL_UNIT_KM: f64 = 149_597_870.7;
pub const JULIAN_CENTURY_DAYS: f64 = 36_525.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyClass {
    Star,
    TerrestrialPlanet,
    GasGiant,
    IceGiant,
    Satellite,
    MinorPlanet,
}

impl BodyClass {
    pub fn label(self) -> &'static str {
        match self {
            BodyClass::Star => "etoile",
            BodyClass::TerrestrialPlanet => "planete tellurique",
            BodyClass::GasGiant => "geante gazeuse",
            BodyClass::IceGiant => "geante de glaces",
            BodyClass::Satellite => "satellite naturel",
            BodyClass::MinorPlanet => "planete mineure",
        }
    }
}

/// Secular WGCCRE rotational elements. Angles in degrees, rates in degrees per Julian
/// century for the pole and degrees per day for the prime meridian.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IauRotationElements {
    pub pole_ra_deg: f64,
    pub pole_ra_rate_deg_per_century: f64,
    pub pole_dec_deg: f64,
    pub pole_dec_rate_deg_per_century: f64,
    pub prime_meridian_deg: f64,
    pub rotation_rate_deg_per_day: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RotationModel {
    /// Full WGCCRE secular solution: pole direction and prime meridian are constrained.
    Iau(IauRotationElements),
    /// Only the sidereal rotation period is measured; the spin axis direction is not
    /// constrained by the IAU report, so the body spins about the ecliptic normal.
    MeasuredPeriodOnly { sidereal_period_days: f64 },
}

impl RotationModel {
    /// Sidereal rotation period in days. Negative values denote retrograde rotation.
    pub fn sidereal_period_days(&self) -> f64 {
        match self {
            RotationModel::Iau(elements) => {
                if elements.rotation_rate_deg_per_day.abs() < f64::EPSILON {
                    f64::INFINITY
                } else {
                    360.0 / elements.rotation_rate_deg_per_day
                }
            }
            RotationModel::MeasuredPeriodOnly {
                sidereal_period_days,
            } => *sidereal_period_days,
        }
    }

    pub fn is_pole_constrained(&self) -> bool {
        matches!(self, RotationModel::Iau(_))
    }
}

/// Instantaneous body-fixed frame orientation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyOrientation {
    pub pole_ra_deg: f64,
    pub pole_dec_deg: f64,
    pub prime_meridian_deg: f64,
    pub pole_constrained: bool,
}

/// Evaluates the WGCCRE secular solution at the requested Julian date.
pub fn evaluate_body_orientation(model: &RotationModel, julian_day: f64) -> BodyOrientation {
    let days_since_epoch = julian_day - J2000_EPOCH_JD;
    let centuries_since_epoch = days_since_epoch / JULIAN_CENTURY_DAYS;

    match model {
        RotationModel::Iau(elements) => BodyOrientation {
            pole_ra_deg: elements.pole_ra_deg
                + elements.pole_ra_rate_deg_per_century * centuries_since_epoch,
            pole_dec_deg: elements.pole_dec_deg
                + elements.pole_dec_rate_deg_per_century * centuries_since_epoch,
            prime_meridian_deg: (elements.prime_meridian_deg
                + elements.rotation_rate_deg_per_day * days_since_epoch)
                .rem_euclid(360.0),
            pole_constrained: true,
        },
        RotationModel::MeasuredPeriodOnly {
            sidereal_period_days,
        } => {
            let rate = if sidereal_period_days.abs() < f64::EPSILON {
                0.0
            } else {
                360.0 / sidereal_period_days
            };
            // Pole unconstrained: the spin axis is taken along the ecliptic north pole,
            // whose J2000 equatorial coordinates are alpha0 = 270 deg, delta0 = 66.561 deg.
            BodyOrientation {
                pole_ra_deg: 270.0,
                pole_dec_deg: 90.0 - crate::skymap::J2000_MEAN_OBLIQUITY_DEG,
                prime_meridian_deg: (rate * days_since_epoch).rem_euclid(360.0),
                pole_constrained: false,
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RingGeometry {
    pub inner_radius_km: f64,
    pub outer_radius_km: f64,
    /// Normal optical depth used as the base opacity of the ring plane.
    pub normal_optical_depth: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeliocentricOrbit {
    pub semi_major_axis_au: f64,
    pub eccentricity: f64,
    pub inclination_deg: f64,
    pub longitude_ascending_node_deg: f64,
    pub longitude_perihelion_deg: f64,
    pub mean_longitude_deg: f64,
    pub orbital_period_days: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReferencePlane {
    Ecliptic,
    ParentEquator,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SatelliteOrbit {
    pub parent: &'static str,
    pub semi_major_axis_km: f64,
    pub eccentricity: f64,
    pub inclination_deg: f64,
    pub reference_plane: ReferencePlane,
    pub argument_of_periapsis_deg: f64,
    pub longitude_ascending_node_deg: f64,
    pub mean_anomaly_at_j2000_deg: f64,
    /// Negative period denotes retrograde revolution.
    pub sidereal_period_days: f64,
    /// False when the epoch phase is not sourced; the orbit geometry stays exact but the
    /// position along the orbit is not tied to a published ephemeris.
    pub epoch_phase_constrained: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OrbitModel {
    /// Fixed at the origin of the heliocentric frame.
    Fixed,
    Heliocentric(HeliocentricOrbit),
    Satellite(SatelliteOrbit),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SolarSystemBody {
    pub name: &'static str,
    pub class: BodyClass,
    pub equatorial_radius_km: f64,
    /// Geometric flattening (a - b) / a.
    pub flattening: f64,
    pub geometric_albedo: f64,
    pub rotation: RotationModel,
    pub orbit: OrbitModel,
    pub ring: Option<RingGeometry>,
    /// Surface map directory under `assets/textures`, when a real map is available.
    pub texture_asset: Option<&'static str>,
    /// Linear sRGB base colour used where no surface map exists.
    pub base_color: [f32; 3],
    pub atmosphere_color: [f32; 3],
    /// Atmospheric scale height, zero for airless bodies.
    pub atmosphere_scale_height_km: f64,
    /// Equatorial zonal wind speed relative to the body-fixed frame, in metres/second.
    pub equatorial_zonal_wind_m_per_s: f64,
}

impl SolarSystemBody {
    pub fn polar_radius_km(&self) -> f64 {
        self.equatorial_radius_km * (1.0 - self.flattening)
    }

    pub fn equatorial_radius_au(&self) -> f64 {
        self.equatorial_radius_km / ASTRONOMICAL_UNIT_KM
    }

    pub fn has_atmosphere(&self) -> bool {
        self.atmosphere_scale_height_km > 0.0
    }

    /// Angular drift of the cloud deck relative to the body-fixed frame, in degrees per day.
    pub fn zonal_wind_drift_deg_per_day(&self) -> f64 {
        if self.equatorial_zonal_wind_m_per_s == 0.0 || self.equatorial_radius_km <= 0.0 {
            return 0.0;
        }
        let circumference_m = TAU * self.equatorial_radius_km * 1_000.0;
        self.equatorial_zonal_wind_m_per_s * 86_400.0 / circumference_m * 360.0
    }
}

fn iau(
    pole_ra_deg: f64,
    pole_ra_rate_deg_per_century: f64,
    pole_dec_deg: f64,
    pole_dec_rate_deg_per_century: f64,
    prime_meridian_deg: f64,
    rotation_rate_deg_per_day: f64,
) -> RotationModel {
    RotationModel::Iau(IauRotationElements {
        pole_ra_deg,
        pole_ra_rate_deg_per_century,
        pole_dec_deg,
        pole_dec_rate_deg_per_century,
        prime_meridian_deg,
        rotation_rate_deg_per_day,
    })
}

/// Full catalogue: the Sun, the eight planets, nine major satellites and eight minor
/// planets, with their physical, rotational and orbital parameters.
// Hygiea's semi-major axis of 3.1415 au is a measured value, not the mathematical pi.
#[allow(clippy::approx_constant)]
pub fn solar_system_catalogue() -> Vec<SolarSystemBody> {
    vec![
        SolarSystemBody {
            name: "Sun",
            class: BodyClass::Star,
            equatorial_radius_km: 696_000.0,
            flattening: 9.0e-6,
            geometric_albedo: 0.0,
            rotation: iau(286.13, 0.0, 63.87, 0.0, 84.176, 14.1844000),
            orbit: OrbitModel::Fixed,
            ring: None,
            texture_asset: None,
            base_color: [1.0, 0.94, 0.84],
            atmosphere_color: [1.0, 0.62, 0.26],
            atmosphere_scale_height_km: 140.0,
            equatorial_zonal_wind_m_per_s: 0.0,
        },
        SolarSystemBody {
            name: "Mercury",
            class: BodyClass::TerrestrialPlanet,
            equatorial_radius_km: 2_439.7,
            flattening: 0.0009,
            geometric_albedo: 0.142,
            rotation: iau(281.0103, -0.0328, 61.4155, -0.0049, 329.5988, 6.1385108),
            orbit: OrbitModel::Heliocentric(HeliocentricOrbit {
                semi_major_axis_au: 0.387098,
                eccentricity: 0.205630,
                inclination_deg: 7.00487,
                longitude_ascending_node_deg: 48.33167,
                longitude_perihelion_deg: 77.45645,
                mean_longitude_deg: 252.25084,
                orbital_period_days: 87.9691,
            }),
            ring: None,
            texture_asset: Some("mercury"),
            base_color: [0.55, 0.50, 0.44],
            atmosphere_color: [0.0, 0.0, 0.0],
            atmosphere_scale_height_km: 0.0,
            equatorial_zonal_wind_m_per_s: 0.0,
        },
        SolarSystemBody {
            name: "Venus",
            class: BodyClass::TerrestrialPlanet,
            equatorial_radius_km: 6_051.8,
            flattening: 0.0,
            geometric_albedo: 0.689,
            rotation: iau(272.76, 0.0, 67.16, 0.0, 160.20, -1.4813688),
            orbit: OrbitModel::Heliocentric(HeliocentricOrbit {
                semi_major_axis_au: 0.723332,
                eccentricity: 0.006772,
                inclination_deg: 3.39471,
                longitude_ascending_node_deg: 76.68069,
                longitude_perihelion_deg: 131.53298,
                mean_longitude_deg: 181.97973,
                orbital_period_days: 224.701,
            }),
            ring: None,
            texture_asset: Some("venus"),
            base_color: [0.86, 0.72, 0.48],
            atmosphere_color: [0.96, 0.82, 0.55],
            atmosphere_scale_height_km: 15.9,
            equatorial_zonal_wind_m_per_s: 100.0,
        },
        SolarSystemBody {
            name: "Earth",
            class: BodyClass::TerrestrialPlanet,
            equatorial_radius_km: 6_378.137,
            flattening: 0.0033528107,
            geometric_albedo: 0.434,
            rotation: iau(0.0, -0.641, 90.0, -0.557, 190.147, 360.9856235),
            orbit: OrbitModel::Heliocentric(HeliocentricOrbit {
                semi_major_axis_au: 1.000000,
                eccentricity: 0.016710,
                inclination_deg: 0.00005,
                longitude_ascending_node_deg: -11.26064,
                longitude_perihelion_deg: 102.94719,
                mean_longitude_deg: 100.46435,
                orbital_period_days: 365.256363,
            }),
            ring: None,
            texture_asset: Some("earth"),
            base_color: [0.24, 0.36, 0.52],
            atmosphere_color: [0.42, 0.64, 1.0],
            atmosphere_scale_height_km: 8.5,
            equatorial_zonal_wind_m_per_s: 10.0,
        },
        SolarSystemBody {
            name: "Mars",
            class: BodyClass::TerrestrialPlanet,
            equatorial_radius_km: 3_396.19,
            flattening: 0.005886,
            geometric_albedo: 0.170,
            rotation: iau(317.269, -0.10, 54.432, -0.0061, 176.049, 350.891982443297),
            orbit: OrbitModel::Heliocentric(HeliocentricOrbit {
                semi_major_axis_au: 1.523662,
                eccentricity: 0.093412,
                inclination_deg: 1.85061,
                longitude_ascending_node_deg: 49.57854,
                longitude_perihelion_deg: 336.04084,
                mean_longitude_deg: 355.45332,
                orbital_period_days: 686.980,
            }),
            ring: None,
            texture_asset: Some("mars"),
            base_color: [0.62, 0.35, 0.22],
            atmosphere_color: [0.72, 0.52, 0.36],
            atmosphere_scale_height_km: 11.1,
            equatorial_zonal_wind_m_per_s: 10.0,
        },
        SolarSystemBody {
            name: "Jupiter",
            class: BodyClass::GasGiant,
            equatorial_radius_km: 71_492.0,
            flattening: 0.06487,
            geometric_albedo: 0.538,
            rotation: iau(268.056595, -0.006499, 64.495303, 0.002413, 284.95, 870.5360000),
            orbit: OrbitModel::Heliocentric(HeliocentricOrbit {
                semi_major_axis_au: 5.203363,
                eccentricity: 0.048393,
                inclination_deg: 1.30530,
                longitude_ascending_node_deg: 100.55615,
                longitude_perihelion_deg: 14.75385,
                mean_longitude_deg: 34.40438,
                orbital_period_days: 4332.589,
            }),
            ring: Some(RingGeometry {
                inner_radius_km: 122_500.0,
                outer_radius_km: 129_000.0,
                normal_optical_depth: 3.0e-6,
            }),
            texture_asset: Some("jupiter"),
            base_color: [0.78, 0.68, 0.56],
            atmosphere_color: [0.82, 0.72, 0.58],
            atmosphere_scale_height_km: 27.0,
            equatorial_zonal_wind_m_per_s: 100.0,
        },
        SolarSystemBody {
            name: "Saturn",
            class: BodyClass::GasGiant,
            equatorial_radius_km: 60_268.0,
            flattening: 0.09796,
            geometric_albedo: 0.499,
            rotation: iau(40.589, -0.036, 83.537, -0.004, 38.90, 810.7939024),
            orbit: OrbitModel::Heliocentric(HeliocentricOrbit {
                semi_major_axis_au: 9.537070,
                eccentricity: 0.054151,
                inclination_deg: 2.48446,
                longitude_ascending_node_deg: 113.71504,
                longitude_perihelion_deg: 92.43194,
                mean_longitude_deg: 49.94432,
                orbital_period_days: 10759.22,
            }),
            ring: Some(RingGeometry {
                inner_radius_km: 74_658.0,
                outer_radius_km: 136_775.0,
                normal_optical_depth: 0.7,
            }),
            texture_asset: Some("saturn"),
            base_color: [0.82, 0.74, 0.56],
            atmosphere_color: [0.86, 0.78, 0.60],
            atmosphere_scale_height_km: 59.5,
            equatorial_zonal_wind_m_per_s: 400.0,
        },
        SolarSystemBody {
            name: "Uranus",
            class: BodyClass::IceGiant,
            equatorial_radius_km: 25_559.0,
            flattening: 0.02293,
            geometric_albedo: 0.488,
            rotation: iau(257.311, 0.0, -15.175, 0.0, 203.81, -501.1600928),
            orbit: OrbitModel::Heliocentric(HeliocentricOrbit {
                semi_major_axis_au: 19.191264,
                eccentricity: 0.047168,
                inclination_deg: 0.76986,
                longitude_ascending_node_deg: 74.22988,
                longitude_perihelion_deg: 170.96424,
                mean_longitude_deg: 313.23218,
                orbital_period_days: 30685.4,
            }),
            ring: Some(RingGeometry {
                inner_radius_km: 41_837.0,
                outer_radius_km: 51_149.0,
                normal_optical_depth: 0.3,
            }),
            texture_asset: Some("uranus"),
            base_color: [0.52, 0.76, 0.80],
            atmosphere_color: [0.46, 0.74, 0.86],
            atmosphere_scale_height_km: 27.7,
            equatorial_zonal_wind_m_per_s: 200.0,
        },
        SolarSystemBody {
            name: "Neptune",
            class: BodyClass::IceGiant,
            equatorial_radius_km: 24_764.0,
            flattening: 0.01708,
            geometric_albedo: 0.442,
            rotation: iau(299.36, 0.0, 43.46, 0.0, 253.18, 536.3128492),
            orbit: OrbitModel::Heliocentric(HeliocentricOrbit {
                semi_major_axis_au: 30.068963,
                eccentricity: 0.008586,
                inclination_deg: 1.76917,
                longitude_ascending_node_deg: 131.72169,
                longitude_perihelion_deg: 44.97135,
                mean_longitude_deg: 304.88003,
                orbital_period_days: 60190.0,
            }),
            ring: Some(RingGeometry {
                inner_radius_km: 41_900.0,
                outer_radius_km: 62_933.0,
                normal_optical_depth: 0.02,
            }),
            texture_asset: Some("neptune"),
            base_color: [0.30, 0.44, 0.78],
            atmosphere_color: [0.28, 0.46, 0.88],
            atmosphere_scale_height_km: 19.7,
            equatorial_zonal_wind_m_per_s: 400.0,
        },
        SolarSystemBody {
            name: "Moon",
            class: BodyClass::Satellite,
            equatorial_radius_km: 1_737.4,
            flattening: 0.0012,
            geometric_albedo: 0.136,
            rotation: iau(269.9949, 0.0031, 66.5392, 0.0130, 38.3213, 13.17635815),
            orbit: OrbitModel::Satellite(SatelliteOrbit {
                parent: "Earth",
                semi_major_axis_km: 384_400.0,
                eccentricity: 0.0549,
                inclination_deg: 5.145,
                reference_plane: ReferencePlane::Ecliptic,
                argument_of_periapsis_deg: 318.15,
                longitude_ascending_node_deg: 125.08,
                mean_anomaly_at_j2000_deg: 135.27,
                sidereal_period_days: 27.321582,
                epoch_phase_constrained: true,
            }),
            ring: None,
            texture_asset: None,
            base_color: [0.42, 0.40, 0.38],
            atmosphere_color: [0.0, 0.0, 0.0],
            atmosphere_scale_height_km: 0.0,
            equatorial_zonal_wind_m_per_s: 0.0,
        },
        SolarSystemBody {
            name: "Phobos",
            class: BodyClass::Satellite,
            equatorial_radius_km: 11.267,
            flattening: 0.30,
            geometric_albedo: 0.071,
            rotation: iau(317.68, -0.108, 52.90, -0.061, 35.06, 1128.8445850),
            orbit: OrbitModel::Satellite(SatelliteOrbit {
                parent: "Mars",
                semi_major_axis_km: 9_376.0,
                eccentricity: 0.0151,
                inclination_deg: 1.093,
                reference_plane: ReferencePlane::ParentEquator,
                argument_of_periapsis_deg: 0.0,
                longitude_ascending_node_deg: 0.0,
                mean_anomaly_at_j2000_deg: 0.0,
                sidereal_period_days: 0.31891023,
                epoch_phase_constrained: false,
            }),
            ring: None,
            texture_asset: None,
            base_color: [0.26, 0.24, 0.22],
            atmosphere_color: [0.0, 0.0, 0.0],
            atmosphere_scale_height_km: 0.0,
            equatorial_zonal_wind_m_per_s: 0.0,
        },
        SolarSystemBody {
            name: "Deimos",
            class: BodyClass::Satellite,
            equatorial_radius_km: 6.2,
            flattening: 0.25,
            geometric_albedo: 0.068,
            rotation: iau(316.65, -0.108, 53.52, -0.061, 79.41, 285.1618970),
            orbit: OrbitModel::Satellite(SatelliteOrbit {
                parent: "Mars",
                semi_major_axis_km: 23_463.2,
                eccentricity: 0.00033,
                inclination_deg: 0.93,
                reference_plane: ReferencePlane::ParentEquator,
                argument_of_periapsis_deg: 0.0,
                longitude_ascending_node_deg: 0.0,
                mean_anomaly_at_j2000_deg: 0.0,
                sidereal_period_days: 1.263,
                epoch_phase_constrained: false,
            }),
            ring: None,
            texture_asset: None,
            base_color: [0.27, 0.25, 0.23],
            atmosphere_color: [0.0, 0.0, 0.0],
            atmosphere_scale_height_km: 0.0,
            equatorial_zonal_wind_m_per_s: 0.0,
        },
        SolarSystemBody {
            name: "Io",
            class: BodyClass::Satellite,
            equatorial_radius_km: 1_821.6,
            flattening: 0.0,
            geometric_albedo: 0.63,
            rotation: iau(268.05, -0.009, 64.50, 0.003, 200.39, 203.4889538),
            orbit: OrbitModel::Satellite(SatelliteOrbit {
                parent: "Jupiter",
                semi_major_axis_km: 421_800.0,
                eccentricity: 0.0041,
                inclination_deg: 0.036,
                reference_plane: ReferencePlane::ParentEquator,
                argument_of_periapsis_deg: 0.0,
                longitude_ascending_node_deg: 0.0,
                mean_anomaly_at_j2000_deg: 0.0,
                sidereal_period_days: 1.769137786,
                epoch_phase_constrained: false,
            }),
            ring: None,
            texture_asset: None,
            base_color: [0.82, 0.72, 0.38],
            atmosphere_color: [0.0, 0.0, 0.0],
            atmosphere_scale_height_km: 0.0,
            equatorial_zonal_wind_m_per_s: 0.0,
        },
        SolarSystemBody {
            name: "Europa",
            class: BodyClass::Satellite,
            equatorial_radius_km: 1_560.8,
            flattening: 0.0,
            geometric_albedo: 0.67,
            rotation: iau(268.08, -0.009, 64.51, 0.003, 36.022, 101.3747235),
            orbit: OrbitModel::Satellite(SatelliteOrbit {
                parent: "Jupiter",
                semi_major_axis_km: 671_100.0,
                eccentricity: 0.0094,
                inclination_deg: 0.466,
                reference_plane: ReferencePlane::ParentEquator,
                argument_of_periapsis_deg: 0.0,
                longitude_ascending_node_deg: 0.0,
                mean_anomaly_at_j2000_deg: 0.0,
                sidereal_period_days: 3.551181041,
                epoch_phase_constrained: false,
            }),
            ring: None,
            texture_asset: None,
            base_color: [0.76, 0.72, 0.66],
            atmosphere_color: [0.0, 0.0, 0.0],
            atmosphere_scale_height_km: 0.0,
            equatorial_zonal_wind_m_per_s: 0.0,
        },
        SolarSystemBody {
            name: "Ganymede",
            class: BodyClass::Satellite,
            equatorial_radius_km: 2_634.1,
            flattening: 0.0,
            geometric_albedo: 0.43,
            rotation: iau(268.20, -0.009, 64.57, 0.003, 44.064, 50.3176081),
            orbit: OrbitModel::Satellite(SatelliteOrbit {
                parent: "Jupiter",
                semi_major_axis_km: 1_070_400.0,
                eccentricity: 0.0013,
                inclination_deg: 0.177,
                reference_plane: ReferencePlane::ParentEquator,
                argument_of_periapsis_deg: 0.0,
                longitude_ascending_node_deg: 0.0,
                mean_anomaly_at_j2000_deg: 0.0,
                sidereal_period_days: 7.15455296,
                epoch_phase_constrained: false,
            }),
            ring: None,
            texture_asset: None,
            base_color: [0.52, 0.48, 0.44],
            atmosphere_color: [0.0, 0.0, 0.0],
            atmosphere_scale_height_km: 0.0,
            equatorial_zonal_wind_m_per_s: 0.0,
        },
        SolarSystemBody {
            name: "Callisto",
            class: BodyClass::Satellite,
            equatorial_radius_km: 2_410.3,
            flattening: 0.0,
            geometric_albedo: 0.22,
            rotation: iau(268.72, -0.009, 64.83, 0.003, 259.51, 21.5710715),
            orbit: OrbitModel::Satellite(SatelliteOrbit {
                parent: "Jupiter",
                semi_major_axis_km: 1_882_700.0,
                eccentricity: 0.0074,
                inclination_deg: 0.192,
                reference_plane: ReferencePlane::ParentEquator,
                argument_of_periapsis_deg: 0.0,
                longitude_ascending_node_deg: 0.0,
                mean_anomaly_at_j2000_deg: 0.0,
                sidereal_period_days: 16.6890184,
                epoch_phase_constrained: false,
            }),
            ring: None,
            texture_asset: None,
            base_color: [0.34, 0.31, 0.28],
            atmosphere_color: [0.0, 0.0, 0.0],
            atmosphere_scale_height_km: 0.0,
            equatorial_zonal_wind_m_per_s: 0.0,
        },
        SolarSystemBody {
            name: "Titan",
            class: BodyClass::Satellite,
            equatorial_radius_km: 2_574.7,
            flattening: 0.0,
            geometric_albedo: 0.22,
            rotation: iau(39.4827, 0.0, 83.4279, 0.0, 186.5855, 22.5769768),
            orbit: OrbitModel::Satellite(SatelliteOrbit {
                parent: "Saturn",
                semi_major_axis_km: 1_221_870.0,
                eccentricity: 0.0288,
                inclination_deg: 0.348,
                reference_plane: ReferencePlane::ParentEquator,
                argument_of_periapsis_deg: 0.0,
                longitude_ascending_node_deg: 0.0,
                mean_anomaly_at_j2000_deg: 0.0,
                sidereal_period_days: 15.945,
                epoch_phase_constrained: false,
            }),
            ring: None,
            texture_asset: None,
            base_color: [0.68, 0.50, 0.26],
            atmosphere_color: [0.86, 0.66, 0.34],
            atmosphere_scale_height_km: 21.0,
            equatorial_zonal_wind_m_per_s: 60.0,
        },
        SolarSystemBody {
            name: "Triton",
            class: BodyClass::Satellite,
            equatorial_radius_km: 1_353.4,
            flattening: 0.0,
            geometric_albedo: 0.76,
            rotation: iau(299.36, 0.0, 41.17, 0.0, 296.53, -61.2572637),
            orbit: OrbitModel::Satellite(SatelliteOrbit {
                parent: "Neptune",
                semi_major_axis_km: 354_759.0,
                eccentricity: 0.000016,
                inclination_deg: 156.885,
                reference_plane: ReferencePlane::ParentEquator,
                argument_of_periapsis_deg: 0.0,
                longitude_ascending_node_deg: 0.0,
                mean_anomaly_at_j2000_deg: 0.0,
                sidereal_period_days: -5.876854,
                epoch_phase_constrained: false,
            }),
            ring: None,
            texture_asset: None,
            base_color: [0.74, 0.70, 0.66],
            atmosphere_color: [0.70, 0.76, 0.82],
            atmosphere_scale_height_km: 8.0,
            equatorial_zonal_wind_m_per_s: 0.0,
        },
        SolarSystemBody {
            name: "Ceres",
            class: BodyClass::MinorPlanet,
            equatorial_radius_km: 482.1,
            flattening: 0.0742,
            geometric_albedo: 0.090,
            rotation: iau(291.418, 0.0, 66.764, 0.0, 170.650, 952.1532),
            orbit: OrbitModel::Heliocentric(HeliocentricOrbit {
                semi_major_axis_au: 2.7675,
                eccentricity: 0.0758,
                inclination_deg: 10.59,
                longitude_ascending_node_deg: 80.30,
                longitude_perihelion_deg: 73.60,
                mean_longitude_deg: 95.99,
                orbital_period_days: 1_680.0,
            }),
            ring: None,
            texture_asset: None,
            base_color: [0.24, 0.23, 0.22],
            atmosphere_color: [0.0, 0.0, 0.0],
            atmosphere_scale_height_km: 0.0,
            equatorial_zonal_wind_m_per_s: 0.0,
        },
        SolarSystemBody {
            name: "Vesta",
            class: BodyClass::MinorPlanet,
            equatorial_radius_km: 285.0,
            flattening: 0.20,
            geometric_albedo: 0.423,
            rotation: iau(309.031, 0.0, 42.235, 0.0, 285.39, 1617.3329428),
            orbit: OrbitModel::Heliocentric(HeliocentricOrbit {
                semi_major_axis_au: 2.3618,
                eccentricity: 0.0887,
                inclination_deg: 7.14,
                longitude_ascending_node_deg: 103.85,
                longitude_perihelion_deg: 150.73,
                mean_longitude_deg: 151.20,
                orbital_period_days: 1_325.8,
            }),
            ring: None,
            texture_asset: None,
            base_color: [0.48, 0.45, 0.40],
            atmosphere_color: [0.0, 0.0, 0.0],
            atmosphere_scale_height_km: 0.0,
            equatorial_zonal_wind_m_per_s: 0.0,
        },
        SolarSystemBody {
            name: "Pallas",
            class: BodyClass::MinorPlanet,
            equatorial_radius_km: 275.0,
            flattening: 0.18,
            geometric_albedo: 0.155,
            rotation: iau(33.0, 0.0, -3.0, 0.0, 38.0, 1105.8036),
            orbit: OrbitModel::Heliocentric(HeliocentricOrbit {
                semi_major_axis_au: 2.7730,
                eccentricity: 0.2310,
                inclination_deg: 34.84,
                longitude_ascending_node_deg: 173.10,
                longitude_perihelion_deg: 310.17,
                mean_longitude_deg: 33.22,
                orbital_period_days: 1_686.0,
            }),
            ring: None,
            texture_asset: None,
            base_color: [0.30, 0.29, 0.27],
            atmosphere_color: [0.0, 0.0, 0.0],
            atmosphere_scale_height_km: 0.0,
            equatorial_zonal_wind_m_per_s: 0.0,
        },
        SolarSystemBody {
            name: "Hygiea",
            class: BodyClass::MinorPlanet,
            equatorial_radius_km: 216.5,
            flattening: 0.05,
            geometric_albedo: 0.0717,
            rotation: RotationModel::MeasuredPeriodOnly {
                sidereal_period_days: 13.826 / 24.0,
            },
            orbit: OrbitModel::Heliocentric(HeliocentricOrbit {
                semi_major_axis_au: 3.1415,
                eccentricity: 0.1125,
                inclination_deg: 3.83,
                longitude_ascending_node_deg: 283.20,
                longitude_perihelion_deg: 312.32,
                mean_longitude_deg: 60.90,
                orbital_period_days: 2_034.0,
            }),
            ring: None,
            texture_asset: None,
            base_color: [0.22, 0.21, 0.20],
            atmosphere_color: [0.0, 0.0, 0.0],
            atmosphere_scale_height_km: 0.0,
            equatorial_zonal_wind_m_per_s: 0.0,
        },
        SolarSystemBody {
            name: "Interamnia",
            class: BodyClass::MinorPlanet,
            equatorial_radius_km: 166.0,
            flattening: 0.08,
            geometric_albedo: 0.0742,
            rotation: RotationModel::MeasuredPeriodOnly {
                sidereal_period_days: 8.727 / 24.0,
            },
            orbit: OrbitModel::Heliocentric(HeliocentricOrbit {
                semi_major_axis_au: 3.0620,
                eccentricity: 0.1550,
                inclination_deg: 17.31,
                longitude_ascending_node_deg: 280.36,
                longitude_perihelion_deg: 95.76,
                mean_longitude_deg: 280.0,
                orbital_period_days: 1_956.0,
            }),
            ring: None,
            texture_asset: None,
            base_color: [0.22, 0.21, 0.21],
            atmosphere_color: [0.0, 0.0, 0.0],
            atmosphere_scale_height_km: 0.0,
            equatorial_zonal_wind_m_per_s: 0.0,
        },
        SolarSystemBody {
            name: "Davida",
            class: BodyClass::MinorPlanet,
            equatorial_radius_km: 148.5,
            flattening: 0.22,
            geometric_albedo: 0.054,
            rotation: RotationModel::MeasuredPeriodOnly {
                sidereal_period_days: 5.1294 / 24.0,
            },
            orbit: OrbitModel::Heliocentric(HeliocentricOrbit {
                semi_major_axis_au: 3.1640,
                eccentricity: 0.1860,
                inclination_deg: 15.94,
                longitude_ascending_node_deg: 107.60,
                longitude_perihelion_deg: 337.60,
                mean_longitude_deg: 310.0,
                orbital_period_days: 2_055.0,
            }),
            ring: None,
            texture_asset: None,
            base_color: [0.20, 0.19, 0.18],
            atmosphere_color: [0.0, 0.0, 0.0],
            atmosphere_scale_height_km: 0.0,
            equatorial_zonal_wind_m_per_s: 0.0,
        },
        SolarSystemBody {
            name: "Psyche",
            class: BodyClass::MinorPlanet,
            equatorial_radius_km: 111.0,
            flattening: 0.23,
            geometric_albedo: 0.120,
            rotation: RotationModel::MeasuredPeriodOnly {
                sidereal_period_days: 4.195948 / 24.0,
            },
            orbit: OrbitModel::Heliocentric(HeliocentricOrbit {
                semi_major_axis_au: 2.9230,
                eccentricity: 0.1340,
                inclination_deg: 3.10,
                longitude_ascending_node_deg: 150.04,
                longitude_perihelion_deg: 229.33,
                mean_longitude_deg: 35.0,
                orbital_period_days: 1_827.0,
            }),
            ring: None,
            texture_asset: None,
            base_color: [0.38, 0.35, 0.31],
            atmosphere_color: [0.0, 0.0, 0.0],
            atmosphere_scale_height_km: 0.0,
            equatorial_zonal_wind_m_per_s: 0.0,
        },
        SolarSystemBody {
            name: "Eros",
            class: BodyClass::MinorPlanet,
            equatorial_radius_km: 8.42,
            flattening: 0.50,
            geometric_albedo: 0.25,
            rotation: iau(11.35, 0.0, 17.22, 0.0, 326.07, 1639.38864745),
            orbit: OrbitModel::Heliocentric(HeliocentricOrbit {
                semi_major_axis_au: 1.4580,
                eccentricity: 0.2230,
                inclination_deg: 10.83,
                longitude_ascending_node_deg: 304.30,
                longitude_perihelion_deg: 178.80,
                mean_longitude_deg: 178.0,
                orbital_period_days: 643.0,
            }),
            ring: None,
            texture_asset: None,
            base_color: [0.36, 0.32, 0.27],
            atmosphere_color: [0.0, 0.0, 0.0],
            atmosphere_scale_height_km: 0.0,
            equatorial_zonal_wind_m_per_s: 0.0,
        },
    ]
}

/// Solves Kepler's equation `M = E - e sin E` by Newton iteration.
pub fn solve_eccentric_anomaly(mean_anomaly_rad: f64, eccentricity: f64) -> f64 {
    let mean_anomaly = mean_anomaly_rad.rem_euclid(TAU);
    let mut eccentric_anomaly = if eccentricity < 0.8 {
        mean_anomaly
    } else {
        std::f64::consts::PI
    };

    for _ in 0..32 {
        let numerator =
            eccentric_anomaly - eccentricity * eccentric_anomaly.sin() - mean_anomaly;
        let denominator = 1.0 - eccentricity * eccentric_anomaly.cos();
        if denominator.abs() < 1.0e-15 {
            break;
        }
        let delta = numerator / denominator;
        eccentric_anomaly -= delta;
        if delta.abs() < 1.0e-14 {
            break;
        }
    }

    eccentric_anomaly
}

/// Rectangular coordinates of a Keplerian orbit expressed in the basis of its
/// reference plane, in the same length unit as `semi_major_axis`.
pub fn orbital_plane_position(
    semi_major_axis: f64,
    eccentricity: f64,
    inclination_rad: f64,
    ascending_node_rad: f64,
    argument_of_periapsis_rad: f64,
    mean_anomaly_rad: f64,
) -> [f64; 3] {
    let eccentric_anomaly = solve_eccentric_anomaly(mean_anomaly_rad, eccentricity);
    let cos_e = eccentric_anomaly.cos();
    let sin_e = eccentric_anomaly.sin();

    let radius = semi_major_axis * (1.0 - eccentricity * cos_e);
    let true_anomaly = ((1.0 - eccentricity * eccentricity).sqrt() * sin_e)
        .atan2(cos_e - eccentricity);

    let argument = argument_of_periapsis_rad + true_anomaly;
    let cos_arg = argument.cos();
    let sin_arg = argument.sin();
    let cos_node = ascending_node_rad.cos();
    let sin_node = ascending_node_rad.sin();
    let cos_inc = inclination_rad.cos();
    let sin_inc = inclination_rad.sin();

    [
        radius * (cos_node * cos_arg - sin_node * sin_arg * cos_inc),
        radius * (sin_node * cos_arg + cos_node * sin_arg * cos_inc),
        radius * (sin_arg * sin_inc),
    ]
}

/// Heliocentric ecliptic rectangular position in astronomical units.
pub fn heliocentric_position_au(orbit: &HeliocentricOrbit, julian_day: f64) -> [f64; 3] {
    let days_since_epoch = julian_day - J2000_EPOCH_JD;
    let mean_anomaly_deg = orbit.mean_longitude_deg - orbit.longitude_perihelion_deg
        + 360.0 * days_since_epoch / orbit.orbital_period_days;
    let argument_of_periapsis_deg =
        orbit.longitude_perihelion_deg - orbit.longitude_ascending_node_deg;

    orbital_plane_position(
        orbit.semi_major_axis_au,
        orbit.eccentricity,
        orbit.inclination_deg.to_radians(),
        orbit.longitude_ascending_node_deg.to_radians(),
        argument_of_periapsis_deg.to_radians(),
        mean_anomaly_deg.to_radians(),
    )
}

fn unit_vector_from_equatorial(ra_deg: f64, dec_deg: f64) -> [f64; 3] {
    let ra = ra_deg.to_radians();
    let dec = dec_deg.to_radians();
    [dec.cos() * ra.cos(), dec.cos() * ra.sin(), dec.sin()]
}

/// Converts an ICRF equatorial unit vector into ecliptic rectangular coordinates.
pub fn equatorial_to_ecliptic(vector: [f64; 3]) -> [f64; 3] {
    let obliquity = crate::skymap::J2000_MEAN_OBLIQUITY_DEG.to_radians();
    let cos_eps = obliquity.cos();
    let sin_eps = obliquity.sin();
    [
        vector[0],
        vector[1] * cos_eps + vector[2] * sin_eps,
        -vector[1] * sin_eps + vector[2] * cos_eps,
    ]
}

/// North pole direction of a body expressed in the ecliptic frame.
pub fn pole_direction_ecliptic(orientation: &BodyOrientation) -> [f64; 3] {
    equatorial_to_ecliptic(unit_vector_from_equatorial(
        orientation.pole_ra_deg,
        orientation.pole_dec_deg,
    ))
}

fn normalize(vector: [f64; 3]) -> [f64; 3] {
    let length = (vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2]).sqrt();
    if length <= f64::EPSILON {
        return [0.0, 0.0, 1.0];
    }
    [vector[0] / length, vector[1] / length, vector[2] / length]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Orthonormal basis of the plane normal to `normal`, with the first axis along the
/// ascending node of that plane on the ecliptic.
pub fn plane_basis_from_normal(normal: [f64; 3]) -> ([f64; 3], [f64; 3], [f64; 3]) {
    let w = normalize(normal);
    let ecliptic_north = [0.0, 0.0, 1.0];
    let node = cross(ecliptic_north, w);
    let node_length = (node[0] * node[0] + node[1] * node[1] + node[2] * node[2]).sqrt();
    let u = if node_length < 1.0e-12 {
        [1.0, 0.0, 0.0]
    } else {
        normalize(node)
    };
    let v = cross(w, u);
    (u, v, w)
}

/// Offset of a satellite from its parent, in astronomical units, in the ecliptic frame.
pub fn satellite_offset_au(
    orbit: &SatelliteOrbit,
    parent_pole_ecliptic: [f64; 3],
    julian_day: f64,
) -> [f64; 3] {
    let days_since_epoch = julian_day - J2000_EPOCH_JD;
    let mean_motion_deg_per_day = if orbit.sidereal_period_days.abs() < f64::EPSILON {
        0.0
    } else {
        360.0 / orbit.sidereal_period_days
    };
    let mean_anomaly_deg =
        orbit.mean_anomaly_at_j2000_deg + mean_motion_deg_per_day * days_since_epoch;

    let plane_position = orbital_plane_position(
        orbit.semi_major_axis_km / ASTRONOMICAL_UNIT_KM,
        orbit.eccentricity,
        orbit.inclination_deg.to_radians(),
        orbit.longitude_ascending_node_deg.to_radians(),
        orbit.argument_of_periapsis_deg.to_radians(),
        mean_anomaly_deg.to_radians(),
    );

    let (u, v, w) = match orbit.reference_plane {
        ReferencePlane::Ecliptic => ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        ReferencePlane::ParentEquator => plane_basis_from_normal(parent_pole_ecliptic),
    };

    [
        plane_position[0] * u[0] + plane_position[1] * v[0] + plane_position[2] * w[0],
        plane_position[0] * u[1] + plane_position[1] * v[1] + plane_position[2] * w[1],
        plane_position[0] * u[2] + plane_position[1] * v[2] + plane_position[2] * w[2],
    ]
}

/// Heliocentric ecliptic positions, in astronomical units, of every catalogued body.
/// Satellites are resolved after their parent, whatever the catalogue ordering.
pub fn compute_catalogue_positions_au(
    catalogue: &[SolarSystemBody],
    julian_day: f64,
) -> Vec<[f64; 3]> {
    let mut positions = vec![[0.0_f64; 3]; catalogue.len()];
    let mut resolved = vec![false; catalogue.len()];

    for (index, body) in catalogue.iter().enumerate() {
        match &body.orbit {
            OrbitModel::Fixed => {
                positions[index] = [0.0, 0.0, 0.0];
                resolved[index] = true;
            }
            OrbitModel::Heliocentric(orbit) => {
                positions[index] = heliocentric_position_au(orbit, julian_day);
                resolved[index] = true;
            }
            OrbitModel::Satellite(_) => {}
        }
    }

    // Satellites of satellites are supported by iterating until the graph is closed.
    let mut pending = catalogue.len();
    while pending > 0 {
        let mut progressed = false;
        pending = 0;

        for (index, body) in catalogue.iter().enumerate() {
            if resolved[index] {
                continue;
            }
            let OrbitModel::Satellite(orbit) = &body.orbit else {
                resolved[index] = true;
                continue;
            };
            let Some(parent_index) = catalogue
                .iter()
                .position(|candidate| candidate.name == orbit.parent)
            else {
                resolved[index] = true;
                continue;
            };
            if !resolved[parent_index] {
                pending += 1;
                continue;
            }

            let parent_orientation =
                evaluate_body_orientation(&catalogue[parent_index].rotation, julian_day);
            let parent_pole = pole_direction_ecliptic(&parent_orientation);
            let offset = satellite_offset_au(orbit, parent_pole, julian_day);
            let parent_position = positions[parent_index];
            positions[index] = [
                parent_position[0] + offset[0],
                parent_position[1] + offset[1],
                parent_position[2] + offset[2],
            ];
            resolved[index] = true;
            progressed = true;
        }

        if !progressed {
            break;
        }
    }

    positions
}

#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn catalogue_names_are_unique() {
        let catalogue = solar_system_catalogue();
        let mut names: Vec<&str> = catalogue.iter().map(|body| body.name).collect();
        names.sort_unstable();
        let unique_count = {
            names.dedup();
            names.len()
        };
        assert_eq!(unique_count, catalogue.len());
    }

    #[test]
    fn every_satellite_references_an_existing_parent() {
        let catalogue = solar_system_catalogue();
        for body in &catalogue {
            if let OrbitModel::Satellite(orbit) = body.orbit {
                assert!(
                    catalogue.iter().any(|candidate| candidate.name == orbit.parent),
                    "{} references unknown parent {}",
                    body.name,
                    orbit.parent
                );
            }
        }
    }

    #[test]
    fn physical_parameters_stay_in_admissible_ranges() {
        for body in solar_system_catalogue() {
            assert!(body.equatorial_radius_km > 0.0, "{}", body.name);
            assert!(
                (0.0..1.0).contains(&body.flattening),
                "{} flattening {}",
                body.name,
                body.flattening
            );
            assert!(
                (0.0..=1.0).contains(&body.geometric_albedo),
                "{} albedo {}",
                body.name,
                body.geometric_albedo
            );
            assert!(body.polar_radius_km() <= body.equatorial_radius_km);
        }
    }

    #[test]
    fn earth_sidereal_day_matches_measured_duration() {
        let earth = solar_system_catalogue()
            .into_iter()
            .find(|body| body.name == "Earth")
            .expect("Earth is catalogued");
        let period_seconds = earth.rotation.sidereal_period_days() * 86_400.0;
        // 23 h 56 min 4.0905 s
        assert!((period_seconds - 86_164.0905).abs() < 0.01);
    }

    #[test]
    fn venus_and_uranus_and_triton_rotate_retrograde() {
        let catalogue = solar_system_catalogue();
        for name in ["Venus", "Uranus", "Triton"] {
            let body = catalogue
                .iter()
                .find(|body| body.name == name)
                .unwrap_or_else(|| panic!("{name} is catalogued"));
            assert!(
                body.rotation.sidereal_period_days() < 0.0,
                "{name} should be retrograde"
            );
        }
    }

    #[test]
    fn jupiter_system_three_period_matches_published_value() {
        let jupiter = solar_system_catalogue()
            .into_iter()
            .find(|body| body.name == "Jupiter")
            .expect("Jupiter is catalogued");
        let hours = jupiter.rotation.sidereal_period_days() * 24.0;
        // System III rotation period: 9 h 55 min 29.7 s
        assert!((hours - (9.0 + 55.0 / 60.0 + 29.7 / 3600.0)).abs() < 1.0e-4);
    }

    #[test]
    fn prime_meridian_advances_by_one_turn_over_one_rotation() {
        let earth = solar_system_catalogue()
            .into_iter()
            .find(|body| body.name == "Earth")
            .expect("Earth is catalogued");
        let period = earth.rotation.sidereal_period_days();
        let start = evaluate_body_orientation(&earth.rotation, J2000_EPOCH_JD);
        let after = evaluate_body_orientation(&earth.rotation, J2000_EPOCH_JD + period);
        assert!((start.prime_meridian_deg - after.prime_meridian_deg).abs() < 1.0e-6);
        assert!(start.pole_constrained);
    }

    #[test]
    fn unconstrained_pole_is_flagged_and_uses_ecliptic_normal() {
        let hygiea = solar_system_catalogue()
            .into_iter()
            .find(|body| body.name == "Hygiea")
            .expect("Hygiea is catalogued");
        let orientation = evaluate_body_orientation(&hygiea.rotation, J2000_EPOCH_JD + 10.0);
        assert!(!orientation.pole_constrained);
        assert!((orientation.pole_ra_deg - 270.0).abs() < 1.0e-9);
        assert!(
            (orientation.pole_dec_deg - (90.0 - crate::skymap::J2000_MEAN_OBLIQUITY_DEG)).abs()
                < 1.0e-9
        );
    }

    #[test]
    fn earth_pole_precesses_away_from_the_celestial_pole() {
        let earth = solar_system_catalogue()
            .into_iter()
            .find(|body| body.name == "Earth")
            .expect("Earth is catalogued");
        let at_epoch = evaluate_body_orientation(&earth.rotation, J2000_EPOCH_JD);
        let one_century_later =
            evaluate_body_orientation(&earth.rotation, J2000_EPOCH_JD + JULIAN_CENTURY_DAYS);
        assert!((at_epoch.pole_dec_deg - 90.0).abs() < 1.0e-12);
        assert!((one_century_later.pole_dec_deg - (90.0 - 0.557)).abs() < 1.0e-9);
    }

    #[test]
    fn ring_geometry_is_ordered_and_outside_the_body() {
        for body in solar_system_catalogue() {
            let Some(ring) = body.ring else {
                continue;
            };
            assert!(ring.inner_radius_km > body.equatorial_radius_km, "{}", body.name);
            assert!(ring.outer_radius_km > ring.inner_radius_km, "{}", body.name);
            assert!(ring.normal_optical_depth > 0.0, "{}", body.name);
        }
    }

    #[test]
    fn declared_texture_assets_exist_on_disk() {
        for body in solar_system_catalogue() {
            let Some(asset) = body.texture_asset else {
                continue;
            };
            let path = std::path::Path::new("assets").join("textures").join(asset);
            assert!(
                path.join("lod0.ppm").exists(),
                "missing surface map for {} at {}",
                body.name,
                path.display()
            );
        }
    }

    #[test]
    fn zonal_wind_drift_is_zero_without_wind_and_positive_otherwise() {
        let catalogue = solar_system_catalogue();
        let mercury = catalogue.iter().find(|b| b.name == "Mercury").unwrap();
        assert_eq!(mercury.zonal_wind_drift_deg_per_day(), 0.0);

        let saturn = catalogue.iter().find(|b| b.name == "Saturn").unwrap();
        let drift = saturn.zonal_wind_drift_deg_per_day();
        assert!(drift > 0.0);
        // 400 m/s along a 60268 km equatorial radius is about 0.0913 turn/day, i.e. 32.9 deg/day.
        assert!((drift - 32.86).abs() < 0.1, "drift = {drift}");
    }

    fn norm(vector: [f64; 3]) -> f64 {
        (vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2]).sqrt()
    }

    fn position_of(name: &str, julian_day: f64) -> [f64; 3] {
        let catalogue = solar_system_catalogue();
        let positions = compute_catalogue_positions_au(&catalogue, julian_day);
        let index = catalogue
            .iter()
            .position(|body| body.name == name)
            .unwrap_or_else(|| panic!("{name} is catalogued"));
        positions[index]
    }

    #[test]
    fn kepler_solver_inverts_the_equation() {
        for eccentricity in [0.0, 0.05, 0.2, 0.5, 0.9] {
            for step in 0..24 {
                let mean_anomaly = step as f64 * TAU / 24.0;
                let eccentric = solve_eccentric_anomaly(mean_anomaly, eccentricity);
                let residual = eccentric - eccentricity * eccentric.sin() - mean_anomaly;
                assert!(residual.abs() < 1.0e-10, "e={eccentricity} residual={residual}");
            }
        }
    }

    #[test]
    fn planet_distances_stay_within_their_perihelion_aphelion_range() {
        let catalogue = solar_system_catalogue();
        for julian_day in [
            J2000_EPOCH_JD,
            J2000_EPOCH_JD + 1_234.5,
            J2000_EPOCH_JD + 9_876.0,
        ] {
            let positions = compute_catalogue_positions_au(&catalogue, julian_day);
            for (body, position) in catalogue.iter().zip(positions.iter()) {
                let OrbitModel::Heliocentric(orbit) = body.orbit else {
                    continue;
                };
                let distance = norm(*position);
                let perihelion = orbit.semi_major_axis_au * (1.0 - orbit.eccentricity);
                let aphelion = orbit.semi_major_axis_au * (1.0 + orbit.eccentricity);
                assert!(
                    distance >= perihelion - 1.0e-9 && distance <= aphelion + 1.0e-9,
                    "{} at {} au is outside [{}, {}]",
                    body.name,
                    distance,
                    perihelion,
                    aphelion
                );
            }
        }
    }

    #[test]
    fn earth_reaches_perihelion_in_early_january() {
        let catalogue = solar_system_catalogue();
        let mut closest_day = 0.0_f64;
        let mut closest_distance = f64::INFINITY;
        for day in 0..366 {
            let julian_day = 2_459_945.5 + day as f64; // 2023-01-01T00:00 UTC
            let positions = compute_catalogue_positions_au(&catalogue, julian_day);
            let index = catalogue.iter().position(|b| b.name == "Earth").unwrap();
            let distance = norm(positions[index]);
            if distance < closest_distance {
                closest_distance = distance;
                closest_day = day as f64;
            }
        }
        assert!((closest_distance - 0.9833).abs() < 0.002, "{closest_distance}");
        assert!(closest_day < 10.0, "perihelion found on day {closest_day}");
    }

    #[test]
    fn lunar_distance_stays_inside_the_measured_range() {
        let earth_index = solar_system_catalogue()
            .iter()
            .position(|body| body.name == "Earth")
            .unwrap();
        let catalogue = solar_system_catalogue();
        let moon_index = catalogue.iter().position(|b| b.name == "Moon").unwrap();

        for day in 0..60 {
            let julian_day = J2000_EPOCH_JD + day as f64;
            let positions = compute_catalogue_positions_au(&catalogue, julian_day);
            let offset = [
                positions[moon_index][0] - positions[earth_index][0],
                positions[moon_index][1] - positions[earth_index][1],
                positions[moon_index][2] - positions[earth_index][2],
            ];
            let distance_km = norm(offset) * ASTRONOMICAL_UNIT_KM;
            assert!(
                (356_000.0..=407_000.0).contains(&distance_km),
                "lunar distance {distance_km} km on day {day}"
            );
        }
    }

    #[test]
    fn galilean_moons_orbit_inside_their_semi_major_axis() {
        let catalogue = solar_system_catalogue();
        let jupiter_index = catalogue.iter().position(|b| b.name == "Jupiter").unwrap();
        let positions = compute_catalogue_positions_au(&catalogue, J2000_EPOCH_JD + 500.0);

        for name in ["Io", "Europa", "Ganymede", "Callisto"] {
            let index = catalogue.iter().position(|b| b.name == name).unwrap();
            let OrbitModel::Satellite(orbit) = catalogue[index].orbit else {
                panic!("{name} should be a satellite");
            };
            let offset = [
                positions[index][0] - positions[jupiter_index][0],
                positions[index][1] - positions[jupiter_index][1],
                positions[index][2] - positions[jupiter_index][2],
            ];
            let distance_km = norm(offset) * ASTRONOMICAL_UNIT_KM;
            let periapsis = orbit.semi_major_axis_km * (1.0 - orbit.eccentricity);
            let apoapsis = orbit.semi_major_axis_km * (1.0 + orbit.eccentricity);
            assert!(
                distance_km >= periapsis - 1.0 && distance_km <= apoapsis + 1.0,
                "{name} at {distance_km} km"
            );
        }
    }

    #[test]
    fn satellite_orbit_planes_follow_their_parent_equator() {
        let catalogue = solar_system_catalogue();
        let uranus = catalogue.iter().find(|b| b.name == "Uranus").unwrap();
        let orientation = evaluate_body_orientation(&uranus.rotation, J2000_EPOCH_JD);
        let pole = pole_direction_ecliptic(&orientation);
        // Uranus spins nearly in its orbital plane: its pole is close to the ecliptic plane.
        assert!(pole[2].abs() < 0.2, "pole z = {}", pole[2]);

        let (u, v, w) = plane_basis_from_normal(pole);
        for (a, b) in [(u, v), (v, w), (w, u)] {
            let dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
            assert!(dot.abs() < 1.0e-12);
        }
    }

    #[test]
    fn the_sun_stays_at_the_origin() {
        let position = position_of("Sun", J2000_EPOCH_JD + 4_321.0);
        assert_eq!(position, [0.0, 0.0, 0.0]);
    }
}
