use crate::{
    build_campaign, build_campaign_report, build_sequence, build_session_summary,
    config_to_targets, execute_campaign, initialize_instrument, initialize_mount,
    parse_campaign_config, plan_observation, reduce_sequence, solve_wcs_from_reference_points,
    start_capture, write_campaign_report_json, write_session_summary_json, CaptureSession,
    InstrumentConfig, ObservationMeta, PixelCoord, SequenceStep, WorldCoord,
};

#[derive(Clone, Debug, PartialEq)]
pub struct ObservationControllerResult {
    pub campaign_ready: bool,
    pub session_ready: bool,
    pub acquisition_ready: bool,
    pub astrometry_ready: bool,
    pub report_ready: bool,
    pub report_path: Option<String>,
    pub session_path: Option<String>,
    pub ready: bool,
}

pub fn run_observation_cycle(
    config_text: &str,
    filter: &str,
    exposure_s: f64,
    repeats: usize,
    output_dir: Option<&str>,
) -> Result<ObservationControllerResult, String> {
    let config = parse_campaign_config(config_text)?;
    let targets = config_to_targets(&config);
    if targets.is_empty() {
        return Err("no targets found in campaign config".to_string());
    }

    let campaign = build_campaign(&targets);
    let steps = targets
        .iter()
        .map(|target| SequenceStep {
            target: target.name,
            filter: Box::leak(filter.to_string().into_boxed_str()),
            exposure_s,
            repeat_count: repeats,
        })
        .collect::<Vec<_>>();
    let sequence = build_sequence(targets[0].name, Box::leak(filter.to_string().into_boxed_str()), &steps);
    let session = build_session_summary(&campaign, &sequence);

    let mount = initialize_mount();
    let instrument = initialize_instrument(InstrumentConfig {
        name: "MainCam",
        pixel_width: 4096,
        pixel_height: 4096,
        gain_e_per_adu: 1.2,
        read_noise_e: 6.5,
        temperature_c: -10.0,
        enabled: true,
    });

    let observations = targets
        .iter()
        .map(|target| {
            ObservationMeta {
                target_name: target.name,
                ra_deg: target.ra_deg,
                dec_deg: target.dec_deg,
                exposure_s,
                filter: Box::leak(filter.to_string().into_boxed_str()),
                gain_e_per_adu: 1.2,
                read_noise_e: 6.5,
                temperature_c: -10.0,
            }
        })
        .map(plan_observation)
        .collect::<Vec<_>>();

    let astrometry_points = targets
        .iter()
        .enumerate()
        .map(|(index, target)| {
            let pixel = PixelCoord {
                x: 100.0 + index as f64 * 50.0,
                y: 200.0 + index as f64 * 30.0,
            };
            let world = WorldCoord {
                ra_deg: target.ra_deg,
                dec_deg: target.dec_deg,
            };
            (pixel, world)
        })
        .collect::<Vec<_>>();
    let astrometry_ready = solve_wcs_from_reference_points(&astrometry_points).is_some()
        && astrometry_points.iter().all(|(_, world)| world.ra_deg.is_finite() && world.dec_deg.is_finite());

    let acquisition_ready = mount.tracking && instrument.ready && instrument.exposure_ready && observations.iter().all(|obs| obs.usable) && astrometry_ready;

    let capture = start_capture(CaptureSession {
        target: targets[0].name,
        exposure_s,
        filter: Box::leak(filter.to_string().into_boxed_str()),
        count: repeats * targets.len(),
        enabled: acquisition_ready,
    });

    let reductions = targets
        .iter()
        .map(|_target| reduce_sequence(crate::ReductionRequest {
            width: 16,
            height: 16,
            image: (0..256)
                .map(|index| {
                    let x = index % 16;
                    let y = index / 16;
                    let dx = x as f64 - 8.0;
                    let dy = y as f64 - 8.0;
                    let base = 10.0 + config.bias + config.dark_current;
                    let star = 120.0 * (-((dx * dx + dy * dy) / (2.0 * 1.5 * 1.5))).exp();
                    base + star / config.flat_field.max(1e-12)
                })
                .collect(),
            bias: config.bias,
            dark_current: config.dark_current,
            flat_field: config.flat_field,
            threshold: config.threshold,
        }))
        .collect::<Vec<_>>();

    let execution = execute_campaign(&targets, &reductions);
    let report = build_campaign_report(&execution);

    let mut report_path = None;
    let mut session_path = None;
    if let Some(dir) = output_dir {
        let path = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&path)
            .map_err(|error| format!("failed to create output dir '{dir}': {error}"))?;

        let report_file = path.join("campaign_report.json");
        let session_file = path.join("session_summary.json");

        write_campaign_report_json(report_file.to_str().unwrap(), &report)
            .map_err(|error| format!("{error}"))?;
        write_session_summary_json(session_file.to_str().unwrap(), &session)
            .map_err(|error| format!("{error}"))?;

        report_path = Some(report_file.to_string_lossy().into_owned());
        session_path = Some(session_file.to_string_lossy().into_owned());
    }

    let result = ObservationControllerResult {
        campaign_ready: campaign.ready,
        session_ready: session.ready,
        acquisition_ready,
        astrometry_ready,
        report_ready: report.ready,
        report_path,
        session_path,
        ready: campaign.ready && session.ready && acquisition_ready && astrometry_ready && report.ready && capture.sync_ok,
    };

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::run_observation_cycle;

    #[test]
    fn runs_observation_cycle_for_valid_campaign() {
        let text = r#"
[campaign]
name=NGC_Example
bias=5.0
dark_current=1.0
flat_field=2.0
threshold=25.0

[target]
M31,10.6847,41.2687,5
M45,56.75,24.1167,3
"#;

        let result = run_observation_cycle(text, "R", 60.0, 2, Some(std::env::temp_dir().to_str().unwrap())).unwrap();
        assert!(result.ready);
        assert!(result.campaign_ready);
        assert!(result.session_ready);
        assert!(result.acquisition_ready);
        assert!(result.astrometry_ready);
        assert!(result.report_ready);
    }
}
