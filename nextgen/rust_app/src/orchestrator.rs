use crate::{CampaignTarget, SequenceReductionResult};

#[derive(Clone, Debug, PartialEq)]
pub struct CampaignOutcome {
    pub target_count: usize,
    pub valid_targets: usize,
    pub total_sources: usize,
    pub total_flux: f64,
    pub average_flux: f64,
    pub ready: bool,
}

pub fn run_campaign(targets: &[CampaignTarget], reductions: &[SequenceReductionResult]) -> CampaignOutcome {
    let execution = crate::execute_campaign(targets, reductions);
    let report = crate::build_campaign_report(&execution);

    CampaignOutcome {
        target_count: report.target_count,
        valid_targets: report.valid_targets,
        total_sources: report.total_sources,
        total_flux: report.total_flux,
        average_flux: report.average_flux,
        ready: report.ready,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_campaign_from_targets_and_reductions() {
        let targets = [
            CampaignTarget { name: "M31", ra_deg: 10.6847, dec_deg: 41.2687, priority: 5 },
            CampaignTarget { name: "M45", ra_deg: 56.75, dec_deg: 24.1167, priority: 3 },
        ];

        let reductions = [
            SequenceReductionResult {
                median_signal: 12.5,
                source_count: 3,
                total_flux: 250.0,
                valid: true,
            },
            SequenceReductionResult {
                median_signal: 14.5,
                source_count: 2,
                total_flux: 170.0,
                valid: true,
            },
        ];

        let outcome = run_campaign(&targets, &reductions);
        assert!(outcome.ready);
        assert_eq!(outcome.target_count, 2);
        assert_eq!(outcome.valid_targets, 2);
        assert_eq!(outcome.total_sources, 5);
        assert!((outcome.total_flux - 420.0).abs() < 1e-9);
        assert!((outcome.average_flux - 210.0).abs() < 1e-9);
    }
}
