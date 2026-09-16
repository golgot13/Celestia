use crate::{
    compute_airmass, deg_to_rad, equatorial_to_horizontal, rad_to_deg, CampaignTarget,
    GeographicCoord, HorizontalCoordinates,
};
use std::f64::consts::PI;

#[derive(Clone, Debug, PartialEq)]
pub struct SiteLimits {
    pub min_altitude_deg: f64,
    pub max_airmass: f64,
}

impl Default for SiteLimits {
    fn default() -> Self {
        Self {
            min_altitude_deg: 20.0,
            max_airmass: 2.5,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TargetVisibility {
    pub target_name: &'static str,
    pub altitude_deg: f64,
    pub azimuth_deg: f64,
    pub airmass: f64,
    pub is_observable: bool,
    pub merit_score: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScheduledObservation {
    pub rank: usize,
    pub target_name: &'static str,
    pub ra_deg: f64,
    pub dec_deg: f64,
    pub priority: u32,
    pub altitude_deg: f64,
    pub azimuth_deg: f64,
    pub airmass: f64,
    pub merit_score: f64,
    pub estimated_duration_s: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SchedulePlan {
    pub site_latitude_deg: f64,
    pub site_longitude_deg: f64,
    pub total_targets: usize,
    pub observable_targets: usize,
    pub total_estimated_duration_s: f64,
    pub queue: Vec<ScheduledObservation>,
}

pub fn compute_local_sidereal_time_rad(jd: f64, longitude_deg: f64) -> f64 {
    // Greenwich Mean Sidereal Time (GMST) approximation from JD
    let d = jd - 2451545.0;
    let gmst_hours = 18.697374558 + 24.06570982441908 * d;
    let gmst_rad = (gmst_hours % 24.0) * (PI / 12.0);
    let lst_rad = gmst_rad + deg_to_rad(longitude_deg);
    lst_rad.rem_euclid(2.0 * PI)
}

pub fn compute_target_visibility(
    target: &CampaignTarget,
    site: &GeographicCoord,
    limits: &SiteLimits,
    lst_rad: f64,
) -> TargetVisibility {
    let ra_rad = deg_to_rad(target.ra_deg);
    let dec_rad = deg_to_rad(target.dec_deg);
    let lat_rad = deg_to_rad(site.latitude_deg);

    let hour_angle_rad = (lst_rad - ra_rad).rem_euclid(2.0 * PI);
    let ha_signed = if hour_angle_rad > PI {
        hour_angle_rad - 2.0 * PI
    } else {
        hour_angle_rad
    };

    let horizontal: HorizontalCoordinates = equatorial_to_horizontal(ha_signed, dec_rad, lat_rad);
    let altitude_deg = rad_to_deg(horizontal.altitude_rad);
    let azimuth_deg = rad_to_deg(horizontal.azimuth_rad);

    let airmass = compute_airmass(horizontal.altitude_rad);
    let is_observable = altitude_deg >= limits.min_altitude_deg && airmass <= limits.max_airmass;

    // Merit score balances target priority and transit proximity (lower airmass is better)
    let airmass_weight = if airmass.is_finite() && airmass >= 1.0 {
        1.0 / airmass
    } else {
        0.0
    };
    let merit_score = if is_observable {
        (target.priority as f64) * airmass_weight * 100.0
    } else {
        0.0
    };

    TargetVisibility {
        target_name: target.name,
        altitude_deg,
        azimuth_deg,
        airmass,
        is_observable,
        merit_score,
    }
}

pub fn schedule_observation_queue(
    targets: &[CampaignTarget],
    site: &GeographicCoord,
    limits: &SiteLimits,
    jd: f64,
    exposure_per_target_s: f64,
    repeats: usize,
) -> SchedulePlan {
    let lst_rad = compute_local_sidereal_time_rad(jd, site.longitude_deg);
    let duration_per_target = exposure_per_target_s * repeats as f64;

    let mut evaluated: Vec<(CampaignTarget, TargetVisibility)> = targets
        .iter()
        .map(|t| (*t, compute_target_visibility(t, site, limits, lst_rad)))
        .collect();

    // Sort by merit score descending (highest priority & highest elevation first)
    evaluated.sort_by(|a, b| {
        b.1.merit_score
            .partial_cmp(&a.1.merit_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut queue = Vec::new();
    let mut total_duration = 0.0;
    let mut rank = 1;

    for (target, vis) in evaluated {
        if vis.is_observable {
            queue.push(ScheduledObservation {
                rank,
                target_name: target.name,
                ra_deg: target.ra_deg,
                dec_deg: target.dec_deg,
                priority: target.priority as u32,
                altitude_deg: vis.altitude_deg,
                azimuth_deg: vis.azimuth_deg,
                airmass: vis.airmass,
                merit_score: vis.merit_score,
                estimated_duration_s: duration_per_target,
            });
            total_duration += duration_per_target;
            rank += 1;
        }
    }

    SchedulePlan {
        site_latitude_deg: site.latitude_deg,
        site_longitude_deg: site.longitude_deg,
        total_targets: targets.len(),
        observable_targets: queue.len(),
        total_estimated_duration_s: total_duration,
        queue,
    }
}

pub fn schedule_plan_to_json(plan: &SchedulePlan) -> String {
    let mut json = String::new();
    json.push_str("{\n");
    json.push_str("  \"site_latitude_deg\": ");
    json.push_str(&format!("{:.4}", plan.site_latitude_deg));
    json.push_str(",\n");
    json.push_str("  \"site_longitude_deg\": ");
    json.push_str(&format!("{:.4}", plan.site_longitude_deg));
    json.push_str(",\n");
    json.push_str("  \"total_targets\": ");
    json.push_str(&plan.total_targets.to_string());
    json.push_str(",\n");
    json.push_str("  \"observable_targets\": ");
    json.push_str(&plan.observable_targets.to_string());
    json.push_str(",\n");
    json.push_str("  \"total_estimated_duration_s\": ");
    json.push_str(&format!("{:.2}", plan.total_estimated_duration_s));
    json.push_str(",\n");
    json.push_str("  \"queue\": [\n");

    for (index, obs) in plan.queue.iter().enumerate() {
        json.push_str("    {\n");
        json.push_str("      \"rank\": ");
        json.push_str(&obs.rank.to_string());
        json.push_str(",\n");
        json.push_str("      \"target_name\": \"");
        json.push_str(obs.target_name);
        json.push_str("\",\n");
        json.push_str("      \"ra_deg\": ");
        json.push_str(&format!("{:.4}", obs.ra_deg));
        json.push_str(",\n");
        json.push_str("      \"dec_deg\": ");
        json.push_str(&format!("{:.4}", obs.dec_deg));
        json.push_str(",\n");
        json.push_str("      \"priority\": ");
        json.push_str(&obs.priority.to_string());
        json.push_str(",\n");
        json.push_str("      \"altitude_deg\": ");
        json.push_str(&format!("{:.2}", obs.altitude_deg));
        json.push_str(",\n");
        json.push_str("      \"azimuth_deg\": ");
        json.push_str(&format!("{:.2}", obs.azimuth_deg));
        json.push_str(",\n");
        json.push_str("      \"airmass\": ");
        json.push_str(&format!("{:.3}", obs.airmass));
        json.push_str(",\n");
        json.push_str("      \"merit_score\": ");
        json.push_str(&format!("{:.2}", obs.merit_score));
        json.push_str(",\n");
        json.push_str("      \"estimated_duration_s\": ");
        json.push_str(&format!("{:.1}", obs.estimated_duration_s));
        json.push_str("\n    }");
        if index + 1 < plan.queue.len() {
            json.push(',');
        }
        json.push('\n');
    }

    json.push_str("  ]\n");
    json.push_str("}\n");
    json
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_target_visibility_and_merit() {
        let target = CampaignTarget {
            name: "M31",
            ra_deg: 10.6847,
            dec_deg: 41.2687,
            priority: 5,
        };
        let site = GeographicCoord {
            latitude_deg: 45.0,
            longitude_deg: 5.0,
            elevation_m: 1500.0,
        };
        let limits = SiteLimits::default();
        let lst_rad = deg_to_rad(10.6847); // Zenith transit for this RA

        let vis = compute_target_visibility(&target, &site, &limits, lst_rad);
        assert!(vis.is_observable);
        assert!(vis.altitude_deg > 80.0);
        assert!(vis.airmass < 1.1);
        assert!(vis.merit_score > 400.0);
    }

    #[test]
    fn schedules_observable_queue_sorted_by_merit() {
        let targets = vec![
            CampaignTarget {
                name: "LowPriorityAtZenith",
                ra_deg: 10.0,
                dec_deg: 45.0,
                priority: 1,
            },
            CampaignTarget {
                name: "HighPriorityAtZenith",
                ra_deg: 10.0,
                dec_deg: 45.0,
                priority: 5,
            },
            CampaignTarget {
                name: "BelowHorizon",
                ra_deg: 190.0,
                dec_deg: -60.0,
                priority: 5,
            },
        ];

        let site = GeographicCoord {
            latitude_deg: 45.0,
            longitude_deg: 0.0,
            elevation_m: 500.0,
        };
        let limits = SiteLimits::default();
        let jd = 2451545.0 + (10.0 / 360.0); // Align LST to ~10 deg

        let plan = schedule_observation_queue(&targets, &site, &limits, jd, 60.0, 2);
        assert_eq!(plan.total_targets, 3);
        assert_eq!(plan.observable_targets, 2);
        assert_eq!(plan.queue[0].target_name, "HighPriorityAtZenith");
        assert_eq!(plan.queue[1].target_name, "LowPriorityAtZenith");
        assert_eq!(plan.queue[0].rank, 1);
        assert_eq!(plan.queue[1].rank, 2);
    }
}
