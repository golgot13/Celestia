#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ObservationMeta {
    pub target_name: &'static str,
    pub ra_deg: f64,
    pub dec_deg: f64,
    pub exposure_s: f64,
    pub filter: &'static str,
    pub gain_e_per_adu: f64,
    pub read_noise_e: f64,
    pub temperature_c: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ObservationPlan {
    pub target_name: &'static str,
    pub ra_deg: f64,
    pub dec_deg: f64,
    pub exposure_s: f64,
    pub filter: &'static str,
    pub repeat_count: usize,
    pub usable: bool,
}

pub fn plan_observation(meta: ObservationMeta) -> ObservationPlan {
    ObservationPlan {
        target_name: meta.target_name,
        ra_deg: meta.ra_deg,
        dec_deg: meta.dec_deg,
        exposure_s: meta.exposure_s,
        filter: meta.filter,
        repeat_count: 1,
        usable: meta.exposure_s > 0.0 && meta.gain_e_per_adu > 0.0 && meta.read_noise_e >= 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_observation_marks_valid_exposure_as_usable() {
        let meta = ObservationMeta {
            target_name: "M31",
            ra_deg: 10.6847,
            dec_deg: 41.2687,
            exposure_s: 120.0,
            filter: "R",
            gain_e_per_adu: 1.5,
            read_noise_e: 7.0,
            temperature_c: -5.0,
        };

        let plan = plan_observation(meta);
        assert!(plan.usable);
        assert_eq!(plan.target_name, "M31");
        assert_eq!(plan.filter, "R");
        assert_eq!(plan.repeat_count, 1);
    }

    #[test]
    fn plan_observation_rejects_invalid_exposure() {
        let meta = ObservationMeta {
            target_name: "NGC 253",
            ra_deg: 11.0,
            dec_deg: 25.0,
            exposure_s: 0.0,
            filter: "G",
            gain_e_per_adu: 1.0,
            read_noise_e: 5.0,
            temperature_c: 0.0,
        };

        let plan = plan_observation(meta);
        assert!(!plan.usable);
    }
}
