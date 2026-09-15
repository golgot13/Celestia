use crate::{run_application_runtime, AppRuntimeResult};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServicePhase {
    Idle,
    Boot,
    ValidateConfig,
    RunCycle,
    PublishResults,
    Complete,
    Failed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ServiceExecution {
    pub phase: ServicePhase,
    pub runtime: AppRuntimeResult,
}

pub fn execute_service(
    config_text: &str,
    filter: &str,
    exposure_s: f64,
    repeats: usize,
    output_dir: Option<&str>,
) -> Result<ServiceExecution, String> {
    let runtime = run_application_runtime(config_text, filter, exposure_s, repeats, output_dir)?;
    let phase = if runtime.controller.ready {
        ServicePhase::Complete
    } else if runtime.controller.campaign_ready {
        ServicePhase::PublishResults
    } else if runtime.controller.session_ready {
        ServicePhase::RunCycle
    } else if runtime.controller.acquisition_ready {
        ServicePhase::ValidateConfig
    } else {
        ServicePhase::Failed
    };

    Ok(ServiceExecution { phase, runtime })
}

#[cfg(test)]
mod tests {
    use super::execute_service;

    #[test]
    fn executes_service_for_ready_cycle() {
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

        let result = execute_service(text, "R", 60.0, 2, Some(std::env::temp_dir().to_str().unwrap())).unwrap();
        assert_eq!(result.phase, super::ServicePhase::Complete);
        assert!(result.runtime.controller.ready);
    }
}
