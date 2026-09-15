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
    unimplemented!("campaign report not implemented yet")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CampaignTarget, CampaignExecutionReport, ExecutionSummary, SequenceReductionResult};

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
}
