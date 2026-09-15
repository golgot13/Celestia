#[derive(Clone, Debug, PartialEq)]
pub struct ObservatoryApplication {
    pub name: String,
    pub config_name: String,
    pub phase: crate::ServicePhase,
    pub ready: bool,
    pub output_dir: Option<String>,
    pub service: crate::ServiceExecution,
}

pub fn build_application(
    config_text: &str,
    filter: &str,
    exposure_s: f64,
    repeats: usize,
    output_dir: Option<&str>,
) -> Result<ObservatoryApplication, String> {
    let service = crate::execute_service(config_text, filter, exposure_s, repeats, output_dir)?;
    let config_name = config_text
        .lines()
        .find_map(|line| {
            let trimmed = line.trim();
            trimmed.strip_prefix("name=")
        })
        .unwrap_or("unnamed_campaign")
        .to_string();

    Ok(ObservatoryApplication {
        name: format!("Observatory::{config_name}"),
        config_name: config_name.clone(),
        phase: service.phase,
        ready: service.runtime.controller.ready,
        output_dir: output_dir.map(str::to_owned),
        service,
    })
}

#[cfg(test)]
mod tests {
    use super::build_application;

    #[test]
    fn builds_application_from_service_result() {
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

        let app = build_application(text, "R", 60.0, 2, Some(std::env::temp_dir().to_str().unwrap())).unwrap();
        assert_eq!(app.config_name, "NGC_Example");
        assert_eq!(app.name, "Observatory::NGC_Example");
        assert!(app.ready);
        assert_eq!(app.phase, crate::ServicePhase::Complete);
    }
}
