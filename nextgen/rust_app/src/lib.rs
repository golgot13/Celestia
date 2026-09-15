pub mod astro;
pub mod cpu;
pub mod ephemeris;
pub mod geo;

pub use astro::{summarize_samples, MeasurementSummary};
pub use cpu::{detect_cpu_features, CpuFeatureFlags};
pub use ephemeris::{
    interpolate_ephemeris, mean_anomaly_from_jd, EphemerisSample, EphemerisState,
};
pub use geo::{
    compute_airmass, deg_to_rad, equatorial_to_horizontal, normalize_angle, rad_to_deg,
    GeographicCoord, HorizontalCoordinates,
};
