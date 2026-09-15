use crate::{CampaignTarget, SequenceReductionResult};

#[derive(Clone, Debug, PartialEq)]
pub struct ExecutionSummary {
    pub target_name: &'static str,
    pub successful_reductions: usize,
    pub total_flux: f64,
    pub valid: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CampaignExecutionReport {
    pub target_count: usize,
    pub total_sources: usize,
    pub total_flux: f64,
    pub ready: bool,
    pub summaries: Vec<ExecutionSummary>,
}

pub fn execute_campaign(
    targets: &[CampaignTarget],
    results: &[SequenceReductionResult],
) -> CampaignExecutionReport {
    if targets.is_empty() {
        return CampaignExecutionReport {
            target_count: 0,
            total_sources: 0,
            total_flux: 0.0,
            ready: false,
            summaries: Vec::new(),
        };
    }

    let mut total_sources = 0usize;
    let mut total_flux = 0.0_f64;
    let mut summaries = Vec::with_capacity(targets.len());

    for (index, target) in targets.iter().enumerate() {
        let result = results.get(index).copied().unwrap_or(SequenceReductionResult {
            median_signal: 0.0,
            source_count: 0,
            total_flux: 0.0,
            valid: false,
        });

        let valid = !target.name.is_empty()
            && target.ra_deg.is_finite()
            && target.dec_deg.is_finite()
            && result.valid;

        let success_count = usize::from(result.valid);
        let flux = if result.valid { result.total_flux } else { 0.0 };

        if result.valid {
            total_sources += result.source_count;
            total_flux += result.total_flux;
        }

        summaries.push(ExecutionSummary {
            target_name: target.name,
            successful_reductions: success_count,
            total_flux: flux,
            valid,
        });
    }

    let ready = targets
        .iter()
        .all(|target| !target.name.is_empty() && target.ra_deg.is_finite() && target.dec_deg.is_finite())
        && summaries.iter().all(|summary| summary.valid || summary.successful_reductions == 0);

    CampaignExecutionReport {
        target_count: targets.len(),
        total_sources,
        total_flux,
        ready,
        summaries,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executes_valid_campaign_and_sums_results() {
        let targets = [
            CampaignTarget { name: "M31", ra_deg: 10.6847, dec_deg: 41.2687, priority: 5 },
            CampaignTarget { name: "M45", ra_deg: 56.75, dec_deg: 24.1167, priority: 3 },
        ];

        let results = [
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

        let report = execute_campaign(&targets, &results);
        assert!(report.ready);
        assert_eq!(report.target_count, 2);
        assert_eq!(report.total_sources, 5);
        assert!((report.total_flux - 420.0).abs() < 1e-9);
        assert_eq!(report.summaries.len(), 2);
    }
}
