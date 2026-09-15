pub mod abi;
pub mod astro;
pub mod bench;
pub mod calibration;
pub mod cpu;
pub mod ephemeris;
pub mod geo;

pub use abi::{
    AstroBatchHeader, AstroStatus, ASTRO_ABI_VERSION_MAJOR, ASTRO_ABI_VERSION_MINOR,
    ASTRO_REQUIRED_CPU_FEATURES_AVX2,
};
pub use astro::{summarize_samples, MeasurementSummary};
pub use bench::{run_ephemeris_benchmark, BenchmarkReport};
pub use calibration::{
    aperture_photometry, calibrate_frame, calibrate_frame_2d, apply_flat_field, subtract_bias,
    AperturePhotometry, CalibrationFrame, ReducedFrame,
};
pub use cpu::{detect_cpu_features, CpuFeatureFlags};
pub use ephemeris::{
    interpolate_ephemeris, interpolate_ephemeris_batch_avx2, mean_anomaly_from_jd,
    EphemerisSample, EphemerisState,
};
pub use geo::{
    compute_airmass, deg_to_rad, equatorial_to_horizontal, normalize_angle, rad_to_deg,
    GeographicCoord, HorizontalCoordinates,
};
