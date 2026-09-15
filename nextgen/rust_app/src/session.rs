use crate::{CampaignSummary, SequencePlan};

#[derive(Clone, Debug, PartialEq)]
pub struct SessionSummary {
    pub campaign_target_count: usize,
    pub total_priority: u32,
    pub sequence_total_exposure_s: f64,
    pub ready: bool,
    pub sequence_valid: bool,
}

pub fn build_session_summary(campaign: &CampaignSummary, sequence: &SequencePlan) -> SessionSummary {
    SessionSummary {
        campaign_target_count: campaign.target_count,
        total_priority: campaign.total_priority,
        sequence_total_exposure_s: sequence.total_exposure_s,
        ready: campaign.ready && sequence.valid,
        sequence_valid: sequence.valid,
    }
}

pub fn session_summary_to_json(summary: &SessionSummary) -> String {
    format!(
        "{{\n  \"campaign_target_count\": {},\n  \"total_priority\": {},\n  \"sequence_total_exposure_s\": {},\n  \"ready\": {},\n  \"sequence_valid\": {}\n}}\n",
        summary.campaign_target_count,
        summary.total_priority,
        summary.sequence_total_exposure_s,
        summary.ready,
        summary.sequence_valid
    )
}

pub fn write_session_summary_json(path: &str, summary: &SessionSummary) -> Result<(), String> {
    std::fs::write(path, session_summary_to_json(summary))
        .map_err(|error| format!("failed to write session summary '{path}': {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_campaign, build_sequence, CampaignTarget, SequenceStep};

    #[test]
    fn builds_session_summary_from_campaign_and_sequence() {
        let targets = [
            CampaignTarget { name: "M31", ra_deg: 10.6847, dec_deg: 41.2687, priority: 5 },
            CampaignTarget { name: "M45", ra_deg: 56.75, dec_deg: 24.1167, priority: 3 },
        ];

        let campaign = build_campaign(&targets);
        let steps = vec![
            SequenceStep { target: "M31", filter: "R", exposure_s: 60.0, repeat_count: 2 },
            SequenceStep { target: "M31", filter: "G", exposure_s: 45.0, repeat_count: 3 },
        ];
        let sequence = build_sequence("M31", "R", &steps);

        let summary = build_session_summary(&campaign, &sequence);
        assert!(summary.ready);
        assert_eq!(summary.campaign_target_count, 2);
        assert_eq!(summary.total_priority, 8);
        assert!((summary.sequence_total_exposure_s - 255.0).abs() < 1e-9);
    }

    #[test]
    fn serializes_session_summary_to_json() {
        let summary = SessionSummary {
            campaign_target_count: 2,
            total_priority: 8,
            sequence_total_exposure_s: 255.0,
            ready: true,
            sequence_valid: true,
        };

        let json = session_summary_to_json(&summary);
        assert!(json.contains("\"campaign_target_count\": 2"));
        assert!(json.contains("\"total_priority\": 8"));
        assert!(json.contains("\"ready\": true"));
        assert!(json.contains("\"sequence_valid\": true"));
    }
}
