pub mod abi;
pub mod acquisition;
pub mod astro;
pub mod astrometry;
pub mod bench;
pub mod calibration;
pub mod cpu;
pub mod ephemeris;
pub mod geo;
pub mod instrument;
pub mod observation;
pub mod sequence;

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
pub use acquisition::{
    initialize_mount, start_capture, CaptureResult, CaptureSession, MountState,
};
pub use geo::{
    compute_airmass, deg_to_rad, equatorial_to_horizontal, normalize_angle, rad_to_deg,
    GeographicCoord, HorizontalCoordinates,
};
pub use instrument::{initialize_instrument, InstrumentConfig, InstrumentStatus};
pub use observation::{plan_observation, ObservationMeta, ObservationPlan};
pub use sequence::{build_sequence, SequencePlan, SequenceStep};
