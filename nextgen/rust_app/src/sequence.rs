#[derive(Clone, Debug, PartialEq)]
pub struct SequenceStep {
    pub target: &'static str,
    pub filter: &'static str,
    pub exposure_s: f64,
    pub repeat_count: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SequencePlan {
    pub steps: Vec<SequenceStep>,
    pub total_exposure_s: f64,
    pub valid: bool,
}

pub fn build_sequence(target: &'static str, filter: &'static str, steps: &[SequenceStep]) -> SequencePlan {
    let mut total = 0.0_f64;
    for step in steps {
        if step.target.is_empty() || step.filter.is_empty() || step.exposure_s <= 0.0 || step.repeat_count == 0 {
            return SequencePlan {
                steps: steps.to_vec(),
                total_exposure_s: 0.0,
                valid: false,
            };
        }
        total += step.exposure_s * step.repeat_count as f64;
    }

    SequencePlan {
        steps: steps.to_vec(),
        total_exposure_s: total,
        valid: !steps.is_empty() && !target.is_empty() && !filter.is_empty(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_valid_sequence_plan() {
        let steps = vec![
            SequenceStep { target: "M31", filter: "R", exposure_s: 60.0, repeat_count: 2 },
            SequenceStep { target: "M31", filter: "G", exposure_s: 45.0, repeat_count: 3 },
        ];

        let plan = build_sequence("M31", "R", &steps);
        assert!(plan.valid);
        assert!((plan.total_exposure_s - 255.0).abs() < 1e-9);
        assert_eq!(plan.steps.len(), 2);
    }

    #[test]
    fn rejects_invalid_step() {
        let steps = vec![
            SequenceStep { target: "M31", filter: "R", exposure_s: 0.0, repeat_count: 2 },
        ];

        let plan = build_sequence("M31", "R", &steps);
        assert!(!plan.valid);
        assert_eq!(plan.total_exposure_s, 0.0);
    }
}
