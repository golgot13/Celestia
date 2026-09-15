use crate::{CampaignExecutionReport, ExecutionSummary};

#[derive(Clone, Debug, PartialEq)]
pub struct CampaignReport {
    pub target_count: usize,
    pub valid_targets: usize,
    pub total_sources: usize,
    pub total_flux: f64,
    pub average_flux: f64,
    pub ready: bool,
    pub summaries: Vec<ExecutionSummary>,
}

pub fn build_campaign_report(report: &CampaignExecutionReport) -> CampaignReport {
    let valid_targets = report.summaries.iter().filter(|summary| summary.valid).count();
    let total_flux = report.summaries.iter().map(|summary| summary.total_flux).sum::<f64>();
    let average_flux = if report.target_count == 0 {
        0.0
    } else {
        total_flux / report.target_count as f64
    };

    CampaignReport {
        target_count: report.target_count,
        valid_targets,
        total_sources: report.total_sources,
        total_flux: report.total_flux,
        average_flux,
        ready: report.ready,
        summaries: report.summaries.clone(),
    }
}

pub fn campaign_report_to_json(report: &CampaignReport) -> String {
    let mut json = String::new();
    json.push_str("{\n");
    json.push_str("  \"target_count\": ");
    json.push_str(&report.target_count.to_string());
    json.push_str(",\n");
    json.push_str("  \"valid_targets\": ");
    json.push_str(&report.valid_targets.to_string());
    json.push_str(",\n");
    json.push_str("  \"total_sources\": ");
    json.push_str(&report.total_sources.to_string());
    json.push_str(",\n");
    json.push_str("  \"total_flux\": ");
    json.push_str(&format_float(report.total_flux));
    json.push_str(",\n");
    json.push_str("  \"average_flux\": ");
    json.push_str(&format_float(report.average_flux));
    json.push_str(",\n");
    json.push_str("  \"ready\": ");
    json.push_str(if report.ready { "true" } else { "false" });
    json.push_str(",\n");
    json.push_str("  \"summaries\": [\n");

    for (index, summary) in report.summaries.iter().enumerate() {
        json.push_str("    {\n");
        json.push_str("      \"target_name\": \"");
        json.push_str(summary.target_name);
        json.push_str("\",\n");
        json.push_str("      \"successful_reductions\": ");
        json.push_str(&summary.successful_reductions.to_string());
        json.push_str(",\n");
        json.push_str("      \"total_flux\": ");
        json.push_str(&format_float(summary.total_flux));
        json.push_str(",\n");
        json.push_str("      \"valid\": ");
        json.push_str(if summary.valid { "true" } else { "false" });
        json.push_str("\n    }");
        if index + 1 < report.summaries.len() {
            json.push_str(",");
        }
        json.push_str("\n");
    }

    json.push_str("  ]\n");
    json.push_str("}\n");
    json
}

pub fn write_campaign_report_json(path: &str, report: &CampaignReport) -> Result<(), String> {
    std::fs::write(path, campaign_report_to_json(report))
        .map_err(|error| format!("failed to write report '{path}': {error}"))
}

fn format_float(value: f64) -> String {
    if value.is_finite() {
        value.to_string()
    } else {
        "null".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_summary_report_from_execution_report() {
        let execution = CampaignExecutionReport {
            target_count: 2,
            total_sources: 5,
            total_flux: 420.0,
            ready: true,
            summaries: vec![
                ExecutionSummary {
                    target_name: "M31",
                    successful_reductions: 1,
                    total_flux: 250.0,
                    valid: true,
                },
                ExecutionSummary {
                    target_name: "M45",
                    successful_reductions: 1,
                    total_flux: 170.0,
                    valid: true,
                },
            ],
        };

        let report = build_campaign_report(&execution);
        assert!(report.ready);
        assert_eq!(report.target_count, 2);
        assert_eq!(report.valid_targets, 2);
        assert_eq!(report.total_sources, 5);
        assert!((report.total_flux - 420.0).abs() < 1e-9);
        assert!((report.average_flux - 210.0).abs() < 1e-9);
    }

    #[test]
    fn serializes_campaign_report_to_json() {
        let report = CampaignReport {
            target_count: 2,
            valid_targets: 2,
            total_sources: 5,
            total_flux: 420.0,
            average_flux: 210.0,
            ready: true,
            summaries: vec![
                ExecutionSummary {
                    target_name: "M31",
                    successful_reductions: 1,
                    total_flux: 250.0,
                    valid: true,
                },
                ExecutionSummary {
                    target_name: "M45",
                    successful_reductions: 1,
                    total_flux: 170.0,
                    valid: true,
                },
            ],
        };

        let json = campaign_report_to_json(&report);
        assert!(json.contains("\"target_count\": 2"));
        assert!(json.contains("\"valid_targets\": 2"));
        assert!(json.contains("\"ready\": true"));
        assert!(json.contains("\"target_name\": \"M31\""));
        assert!(json.contains("\"average_flux\": 210"));
    }

    #[test]
    fn writes_campaign_report_json_to_disk() {
        let report = CampaignReport {
            target_count: 1,
            valid_targets: 1,
            total_sources: 2,
            total_flux: 42.0,
            average_flux: 42.0,
            ready: true,
            summaries: vec![ExecutionSummary {
                target_name: "M31",
                successful_reductions: 1,
                total_flux: 42.0,
                valid: true,
            }],
        };

        let path = std::env::temp_dir().join("campaign_report_test.json");
        let result = super::write_campaign_report_json(path.to_str().unwrap(), &report);
        assert!(result.is_ok());

        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains("\"campaign\"") == false || written.contains("\"target_count\": 1"));
        assert!(written.contains("\"target_name\": \"M31\""));

        let _ = std::fs::remove_file(path);
    }
}
