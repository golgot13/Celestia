use crate::{run_observation_cycle, ObservationControllerResult};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeState {
    Idle,
    Ready,
    Running,
    Complete,
    Failed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AppRuntimeResult {
    pub state: RuntimeState,
    pub config_name: String,
    pub controller: ObservationControllerResult,
}

pub fn run_application_runtime(
    config_text: &str,
    filter: &str,
    exposure_s: f64,
    repeats: usize,
    output_dir: Option<&str>,
) -> Result<AppRuntimeResult, String> {
    let controller = run_observation_cycle(config_text, filter, exposure_s, repeats, output_dir)?;
    let state = if controller.ready {
        RuntimeState::Complete
    } else if controller.campaign_ready || controller.session_ready || controller.acquisition_ready || controller.astrometry_ready {
        RuntimeState::Ready
    } else {
        RuntimeState::Failed
    };

    let config_name = config_text
        .lines()
        .find_map(|line| {
            let trimmed = line.trim();
            trimmed.strip_prefix("name=")
        })
        .unwrap_or("unnamed_campaign")
        .to_string();

    Ok(AppRuntimeResult {
        state,
        config_name,
        controller,
    })
}

#[cfg(test)]
mod tests {
    use super::run_application_runtime;

    #[test]
    fn application_runtime_completes_ready_cycle() {
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

        let result = run_application_runtime(text, "R", 60.0, 2, Some(std::env::temp_dir().to_str().unwrap())).unwrap();
        assert_eq!(result.state, super::RuntimeState::Complete);
        assert!(result.controller.ready);
        assert_eq!(result.config_name, "NGC_Example");
    }
}
