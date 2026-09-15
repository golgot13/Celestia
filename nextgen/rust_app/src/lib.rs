pub mod abi;
pub mod acquisition;
pub mod app;
pub mod astro;
pub mod astrometry;
pub mod bench;
pub mod calibration;
pub mod campaign;
pub mod config;
pub mod controller;
pub mod cpu;
pub mod diagnostics;
pub mod ephemeris;
pub mod execution;
pub mod fits;
pub mod geo;
pub mod instrument;
pub mod ledger;
pub mod lightcurve;
pub mod observation;
pub mod orchestrator;
pub mod photometry_calib;
pub mod pipeline;
pub mod psf;
pub mod qc;
pub mod reporting;
pub mod runtime;
pub mod scheduler;
pub mod sequence;
pub mod service;
pub mod session;
pub mod stacking;

pub use abi::{
    AstroBatchHeader, AstroStatus, ASTRO_ABI_VERSION_MAJOR, ASTRO_ABI_VERSION_MINOR,
    ASTRO_REQUIRED_CPU_FEATURES_AVX2,
};
pub use app::{build_application, ObservatoryApplication};
pub use astro::{summarize_samples, MeasurementSummary};
pub use astrometry::{
    pixel_to_world, solve_wcs_from_reference_points, world_to_pixel, PixelCoord, WcsTransform,
    WorldCoord,
};
pub use bench::{run_ephemeris_benchmark, BenchmarkReport};
pub use calibration::{
    aperture_photometry, calibrate_frame, calibrate_frame_2d, apply_flat_field, subtract_bias,
    AperturePhotometry, CalibrationFrame, ReducedFrame,
};
pub use cpu::{detect_cpu_features, CpuFeatureFlags};
pub use diagnostics::{
    diagnostic_report_to_json, evaluate_astrometry_residuals, evaluate_calibration_snr,
    run_system_diagnostics, DiagnosticMetrics, SystemDiagnosticReport,
};
pub use ephemeris::{
    interpolate_ephemeris, interpolate_ephemeris_batch_avx2, mean_anomaly_from_jd,
    EphemerisSample, EphemerisState,
};
pub use acquisition::{
    initialize_mount, start_capture, CaptureResult, CaptureSession, MountState,
};
pub use campaign::{build_campaign, CampaignSummary, CampaignTarget};
pub use config::{config_to_targets, parse_campaign_config, CampaignConfig, CampaignTargetConfig};
pub use controller::{run_observation_cycle, ObservationControllerResult};
pub use execution::{execute_campaign, CampaignExecutionReport, ExecutionSummary};
pub use fits::{
    create_astronomical_fits_image, write_fits_binary, FitsBitPix, FitsHeader, FitsHeaderCard,
    FitsImage,
};
pub use geo::{
    compute_airmass, deg_to_rad, equatorial_to_horizontal, normalize_angle, rad_to_deg,
    GeographicCoord, HorizontalCoordinates,
};
pub use instrument::{initialize_instrument, InstrumentConfig, InstrumentStatus};
pub use ledger::{LedgerEntry, LedgerEventType, SessionLedger};
pub use lightcurve::{
    analyze_light_curve, compute_differential_photometry, compute_lomb_scargle_periodogram,
    light_curve_analysis_to_json, DifferentialMeasurement, LightCurveAnalysis, PeriodogramPeak,
    PhotometricPoint,
};
pub use observation::{plan_observation, ObservationMeta, ObservationPlan};
pub use orchestrator::{run_campaign, CampaignOutcome};
pub use photometry_calib::{
    calibrate_target_magnitude, calibration_result_to_json, compute_instrumental_magnitude,
    solve_zero_point_and_extinction, CalibratedStarMagnitude, ObservedStarPhotometry,
    StandardStar, ZeroPointCalibration,
};
pub use pipeline::{reduce_sequence, ReductionRequest, SequenceReductionResult};
pub use psf::{
    assess_seeing_quality, fit_gaussian_profile_1d, fit_moffat_profile_1d, seeing_assessment_to_json,
    SeeingAssessment, StarProfileFit, StarProfileModel,
};
pub use qc::{
    evaluate_frame_quality, frame_quality_to_json, FrameQualitySummary, QcThresholds, QualityFlag,
    StarMeasurement,
};
pub use reporting::{
    build_campaign_report, campaign_report_to_json, write_campaign_report_json, CampaignReport,
};
pub use runtime::{run_application_runtime, AppRuntimeResult, RuntimeState};
pub use scheduler::{
    compute_local_sidereal_time_rad, compute_target_visibility, schedule_observation_queue,
    schedule_plan_to_json, SchedulePlan, ScheduledObservation, SiteLimits, TargetVisibility,
};
pub use sequence::{build_sequence, SequencePlan, SequenceStep};
pub use service::{execute_service, ServiceExecution, ServicePhase};
pub use session::{
    build_session_summary, session_summary_to_json, write_session_summary_json, SessionSummary,
};
pub use stacking::{
    stack_frames_2d, stacked_result_to_json, StackedResult, StackingMethod, StackingParams,
};
