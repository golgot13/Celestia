#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CampaignTarget {
    pub name: &'static str,
    pub ra_deg: f64,
    pub dec_deg: f64,
    pub priority: u8,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CampaignSummary {
    pub target_count: usize,
    pub total_priority: u32,
    pub ready: bool,
}

pub fn build_campaign(targets: &[CampaignTarget]) -> CampaignSummary {
    if targets.is_empty() {
        return CampaignSummary {
            target_count: 0,
            total_priority: 0,
            ready: false,
        };
    }

    let total_priority = targets.iter().map(|target| target.priority as u32).sum();
    CampaignSummary {
        target_count: targets.len(),
        total_priority,
        ready: targets.iter().all(|target| !target.name.is_empty() && target.ra_deg.is_finite() && target.dec_deg.is_finite()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_ready_campaign_for_valid_targets() {
        let targets = [
            CampaignTarget { name: "M31", ra_deg: 10.6847, dec_deg: 41.2687, priority: 5 },
            CampaignTarget { name: "M45", ra_deg: 56.75, dec_deg: 24.1167, priority: 3 },
        ];

        let summary = build_campaign(&targets);
        assert!(summary.ready);
        assert_eq!(summary.target_count, 2);
        assert_eq!(summary.total_priority, 8);
    }

    #[test]
    fn rejects_empty_campaign() {
        let targets: [CampaignTarget; 0] = [];
        let summary = build_campaign(&targets);
        assert!(!summary.ready);
        assert_eq!(summary.target_count, 0);
        assert_eq!(summary.total_priority, 0);
    }
}
