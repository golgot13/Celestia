#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeridianSide {
    East,
    West,
    PierCrossing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MountSafetyStatus {
    Safe,
    WarningNearLimit,
    EmergencyStopHorizonLimit,
    EmergencyStopMeridianLimit,
    CableWrapRisk,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MountSafetyLimits {
    pub min_altitude_deg: f64,
    pub max_altitude_deg: f64,
    pub meridian_overtravel_limit_deg: f64, // e.g., 5.0 deg past meridian before collision
    pub cable_wrap_max_azimuth_turns: f64, // e.g., 1.5 turns
}

impl Default for MountSafetyLimits {
    fn default() -> Self {
        Self {
            min_altitude_deg: 15.0,
            max_altitude_deg: 88.0, // Avoid zenith gimbal lock
            meridian_overtravel_limit_deg: 7.5, // 30 minutes of tracking past meridian
            cable_wrap_max_azimuth_turns: 1.25,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MeridianFlipEvaluation {
    pub current_ha_deg: f64,
    pub current_pier_side: MeridianSide,
    pub target_pier_side: MeridianSide,
    pub flip_required: bool,
    pub time_until_limit_seconds: f64,
    pub safety_status: MountSafetyStatus,
    pub safe_to_expose_duration_s: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PierFlipSequencePlan {
    pub start_ra_deg: f64,
    pub start_dec_deg: f64,
    pub post_flip_ra_deg: f64,
    pub post_flip_dec_deg: f64,
    pub initial_pier_side: MeridianSide,
    pub final_pier_side: MeridianSide,
    pub settle_time_seconds: f64,
    pub reacquisition_required: bool,
}

pub fn evaluate_meridian_safety(
    target_ra_deg: f64,
    target_dec_deg: f64,
    lst_deg: f64,
    site_latitude_deg: f64,
    current_pier_side: MeridianSide,
    planned_exposure_s: f64,
    limits: &MountSafetyLimits,
) -> MeridianFlipEvaluation {
    // Hour Angle in degrees: HA = LST - RA
    let ha_raw = (lst_deg - target_ra_deg).rem_euclid(360.0);
    let ha_signed = if ha_raw > 180.0 { ha_raw - 360.0 } else { ha_raw };

    // Calculate altitude to check horizon and zenith limits
    let lat_rad = site_latitude_deg.to_radians();
    let dec_rad = target_dec_deg.to_radians();
    let ha_rad = ha_signed.to_radians();

    let sin_alt = (lat_rad.sin() * dec_rad.sin() + lat_rad.cos() * dec_rad.cos() * ha_rad.cos()).clamp(-1.0, 1.0);
    let alt_deg = sin_alt.asin().to_degrees();

    // Check Altitude safety limits
    if alt_deg < limits.min_altitude_deg {
        return MeridianFlipEvaluation {
            current_ha_deg: ha_signed,
            current_pier_side,
            target_pier_side: current_pier_side,
            flip_required: false,
            time_until_limit_seconds: 0.0,
            safety_status: MountSafetyStatus::EmergencyStopHorizonLimit,
            safe_to_expose_duration_s: 0.0,
        };
    }

    if alt_deg > limits.max_altitude_deg {
        return MeridianFlipEvaluation {
            current_ha_deg: ha_signed,
            current_pier_side,
            target_pier_side: current_pier_side,
            flip_required: false,
            time_until_limit_seconds: 0.0,
            safety_status: MountSafetyStatus::WarningNearLimit,
            safe_to_expose_duration_s: 0.0,
        };
    }

    // Determine ideal pier side (for German Equatorial Mount):
    // If target is in East (HA < 0), telescope should be on West of pier looking East.
    // If target is in West (HA > 0), telescope should be on East of pier looking West.
    let ideal_pier_side = if ha_signed < 0.0 {
        MeridianSide::West
    } else {
        MeridianSide::East
    };

    // Tracking past meridian: Earth rotates 15 degrees per hour (1 deg = 240 seconds)
    let degrees_to_limit = limits.meridian_overtravel_limit_deg - ha_signed.max(0.0);
    let time_until_limit_s = (degrees_to_limit * 240.0).max(0.0);

    let flip_required = current_pier_side != ideal_pier_side && ha_signed > 0.0;

    let safety_status = if ha_signed > limits.meridian_overtravel_limit_deg {
        MountSafetyStatus::EmergencyStopMeridianLimit
    } else if ha_signed > (limits.meridian_overtravel_limit_deg - 2.5) {
        MountSafetyStatus::WarningNearLimit
    } else {
        MountSafetyStatus::Safe
    };

    let safe_exposure = if flip_required {
        time_until_limit_s.min(planned_exposure_s)
    } else {
        planned_exposure_s
    };

    MeridianFlipEvaluation {
        current_ha_deg: ha_signed,
        current_pier_side,
        target_pier_side: ideal_pier_side,
        flip_required,
        time_until_limit_seconds: time_until_limit_s,
        safety_status,
        safe_to_expose_duration_s: safe_exposure,
    }
}

pub fn plan_meridian_flip_sequence(
    target_ra_deg: f64,
    target_dec_deg: f64,
    initial_pier_side: MeridianSide,
    settle_seconds: f64,
) -> PierFlipSequencePlan {
    let final_pier_side = match initial_pier_side {
        MeridianSide::East => MeridianSide::West,
        MeridianSide::West => MeridianSide::East,
        MeridianSide::PierCrossing => MeridianSide::West,
    };

    PierFlipSequencePlan {
        start_ra_deg: target_ra_deg,
        start_dec_deg: target_dec_deg,
        post_flip_ra_deg: target_ra_deg,
        post_flip_dec_deg: target_dec_deg,
        initial_pier_side,
        final_pier_side,
        settle_time_seconds: settle_seconds.max(5.0),
        reacquisition_required: true,
    }
}

pub fn meridian_evaluation_to_json(eval: &MeridianFlipEvaluation) -> String {
    format!(
        "{{\n  \"current_ha_deg\": {:.4},\n  \"current_pier_side\": \"{:?}\",\n  \"target_pier_side\": \"{:?}\",\n  \"flip_required\": {},\n  \"time_until_limit_seconds\": {:.1},\n  \"safety_status\": \"{:?}\",\n  \"safe_to_expose_duration_s\": {:.1}\n}}\n",
        eval.current_ha_deg,
        eval.current_pier_side,
        eval.target_pier_side,
        eval.flip_required,
        eval.time_until_limit_seconds,
        eval.safety_status,
        eval.safe_to_expose_duration_s
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_safe_pre_meridian_tracking() {
        let limits = MountSafetyLimits::default();
        // Target 1 hour east of meridian (HA = -15 deg)
        let eval = evaluate_meridian_safety(
            15.0,
            45.0,
            0.0,
            44.0,
            MeridianSide::West, // Looking East
            300.0,
            &limits,
        );

        assert!(!eval.flip_required);
        assert_eq!(eval.safety_status, MountSafetyStatus::Safe);
        assert_eq!(eval.safe_to_expose_duration_s, 300.0);
    }

    #[test]
    fn triggers_meridian_flip_when_target_crosses_meridian() {
        let limits = MountSafetyLimits::default();
        // Target at Dec 45, Lat 44 -> at meridian transit, alt is ~89 deg (near zenith limit of 88 deg)
        // Set Dec to 20 deg so transit alt = 90 - (44 - 20) = 66 deg (well within 15-88 deg safe zone)
        // Target 2 degrees past meridian (HA = +2 deg) with telescope still on West pier side
        let eval = evaluate_meridian_safety(
            0.0,
            20.0,
            2.0,
            44.0,
            MeridianSide::West, // Needs flip to East
            600.0,
            &limits,
        );

        assert!(eval.flip_required);
        assert_eq!(eval.target_pier_side, MeridianSide::East);
        assert!(eval.time_until_limit_seconds > 0.0);
    }

    #[test]
    fn plans_pier_flip_sequence() {
        let plan = plan_meridian_flip_sequence(180.0, 30.0, MeridianSide::West, 15.0);
        assert_eq!(plan.initial_pier_side, MeridianSide::West);
        assert_eq!(plan.final_pier_side, MeridianSide::East);
        assert!(plan.reacquisition_required);
        assert_eq!(plan.settle_time_seconds, 15.0);
    }
}
