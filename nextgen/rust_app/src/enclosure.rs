#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnclosureState {
    Closed,
    Opening,
    Open,
    Closing,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeatherSafetyStatus {
    SafeToOpen,
    WarningApproachingLimit,
    UnsafeMustClose,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WeatherTelemetry {
    pub ambient_temp_c: f64,
    pub sky_temp_c: f64, // Infrared cloud sensor (clear sky: sky_temp << ambient_temp)
    pub relative_humidity_pct: f64,
    pub wind_speed_km_h: f64,
    pub wind_gust_km_h: f64,
    pub rain_detected: bool,
    pub sky_brightness_mpsas: f64, // Mag per square arcsec (e.g. 21.5 for dark sky)
}

impl Default for WeatherTelemetry {
    fn default() -> Self {
        Self {
            ambient_temp_c: 12.0,
            sky_temp_c: -25.0, // Clear sky
            relative_humidity_pct: 55.0,
            wind_speed_km_h: 15.0,
            wind_gust_km_h: 22.0,
            rain_detected: false,
            sky_brightness_mpsas: 21.4,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct WeatherLimits {
    pub max_humidity_pct: f64,
    pub min_dew_point_margin_c: f64,
    pub max_wind_speed_km_h: f64,
    pub max_wind_gust_km_h: f64,
    pub max_cloud_delta_t_c: f64, // sky_temp - ambient_temp (cloudy if > -15 C)
}

impl Default for WeatherLimits {
    fn default() -> Self {
        Self {
            max_humidity_pct: 85.0,
            min_dew_point_margin_c: 2.5,
            max_wind_speed_km_h: 40.0,
            max_wind_gust_km_h: 55.0,
            max_cloud_delta_t_c: -15.0, // e.g. -25 - 12 = -37 (very clear)
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DomeSlitGeometry {
    pub dome_radius_m: f64,
    pub mount_offset_north_m: f64,
    pub mount_offset_east_m: f64,
    pub mount_offset_up_m: f64,
    pub slit_width_m: f64,
}

impl Default for DomeSlitGeometry {
    fn default() -> Self {
        Self {
            dome_radius_m: 2.5,
            mount_offset_north_m: 0.0,
            mount_offset_east_m: 0.0,
            mount_offset_up_m: 0.2,
            slit_width_m: 0.8,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DomeSlitPosition {
    pub required_dome_azimuth_deg: f64,
    pub telescope_azimuth_deg: f64,
    pub telescope_altitude_deg: f64,
    pub slit_centered: bool,
    pub vignetting_risk: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EnclosureSafetyReport {
    pub dew_point_c: f64,
    pub dew_point_margin_c: f64,
    pub cloud_cover_delta_c: f64,
    pub weather_status: WeatherSafetyStatus,
    pub enclosure_state: EnclosureState,
    pub can_open_shutter: bool,
    pub safety_reasons: Vec<String>,
}

pub fn compute_dew_point_c(temperature_c: f64, relative_humidity_pct: f64) -> f64 {
    // Magnus-Tetens formula for dew point calculation
    let a = 17.27;
    let b = 237.7;
    let rh = (relative_humidity_pct.clamp(1.0, 100.0)) / 100.0;
    let alpha = (a * temperature_c) / (b + temperature_c) + rh.ln();
    (b * alpha) / (a - alpha)
}

pub fn evaluate_weather_safety(
    telemetry: &WeatherTelemetry,
    limits: &WeatherLimits,
    current_enclosure: EnclosureState,
) -> EnclosureSafetyReport {
    let mut reasons = Vec::new();
    let mut is_unsafe = false;
    let mut is_warning = false;

    // 1. Rain sensor check (immediate critical stop)
    if telemetry.rain_detected {
        reasons.push("Precipitation/Rain detected".to_string());
        is_unsafe = true;
    }

    // 2. Humidity and Dew Point margin check
    let dew_point = compute_dew_point_c(telemetry.ambient_temp_c, telemetry.relative_humidity_pct);
    let dew_margin = telemetry.ambient_temp_c - dew_point;

    if telemetry.relative_humidity_pct >= limits.max_humidity_pct {
        reasons.push(format!(
            "High humidity: {:.1}% >= {:.1}% limit",
            telemetry.relative_humidity_pct, limits.max_humidity_pct
        ));
        is_unsafe = true;
    }

    if dew_margin < limits.min_dew_point_margin_c {
        reasons.push(format!(
            "Dew point risk: margin {:.1}C < {:.1}C limit",
            dew_margin, limits.min_dew_point_margin_c
        ));
        is_unsafe = true;
    } else if dew_margin < (limits.min_dew_point_margin_c + 1.5) {
        is_warning = true;
    }

    // 3. Wind speed & gusts
    if telemetry.wind_speed_km_h >= limits.max_wind_speed_km_h {
        reasons.push(format!(
            "High wind speed: {:.1} km/h >= {:.1} km/h limit",
            telemetry.wind_speed_km_h, limits.max_wind_speed_km_h
        ));
        is_unsafe = true;
    }
    if telemetry.wind_gust_km_h >= limits.max_wind_gust_km_h {
        reasons.push(format!(
            "High wind gust: {:.1} km/h >= {:.1} km/h limit",
            telemetry.wind_gust_km_h, limits.max_wind_gust_km_h
        ));
        is_unsafe = true;
    }

    // 4. Infrared Sky Cloud Cover
    let delta_sky = telemetry.sky_temp_c - telemetry.ambient_temp_c;
    if delta_sky > limits.max_cloud_delta_t_c {
        reasons.push(format!(
            "Cloudy/Overcast sky: delta T {:.1}C > {:.1}C limit",
            delta_sky, limits.max_cloud_delta_t_c
        ));
        is_warning = true;
    }

    let weather_status = if is_unsafe {
        WeatherSafetyStatus::UnsafeMustClose
    } else if is_warning {
        WeatherSafetyStatus::WarningApproachingLimit
    } else {
        WeatherSafetyStatus::SafeToOpen
    };

    let can_open = weather_status != WeatherSafetyStatus::UnsafeMustClose;

    EnclosureSafetyReport {
        dew_point_c: dew_point,
        dew_point_margin_c: dew_margin,
        cloud_cover_delta_c: delta_sky,
        weather_status,
        enclosure_state: current_enclosure,
        can_open_shutter: can_open,
        safety_reasons: reasons,
    }
}

pub fn compute_dome_azimuth_sync(
    telescope_azimuth_deg: f64,
    telescope_altitude_deg: f64,
    geometry: &DomeSlitGeometry,
) -> DomeSlitPosition {
    let az_rad = telescope_azimuth_deg.to_radians();
    let alt_rad = telescope_altitude_deg.to_radians();

    // Telescope optical center vector in dome coordinates
    let tx = alt_rad.cos() * az_rad.sin();
    let ty = alt_rad.cos() * az_rad.cos();
    let tz = alt_rad.sin();

    // Geometric ray intersection with spherical dome shell of radius R
    let ox = geometry.mount_offset_east_m;
    let oy = geometry.mount_offset_north_m;
    let oz = geometry.mount_offset_up_m;

    // Vector equation: || O + s * T ||^2 = R^2
    let b = 2.0 * (ox * tx + oy * ty + oz * tz);
    let c = ox * ox + oy * oy + oz * oz - geometry.dome_radius_m * geometry.dome_radius_m;
    let discr = (b * b - 4.0 * c).max(0.0);
    let s = (-b + discr.sqrt()) / 2.0;

    let ix = ox + s * tx;
    let iy = oy + s * ty;

    // Astronomical azimuth is measured from North (y) towards East (x)
    let required_dome_azimuth_deg = (ix.atan2(iy).to_degrees()).rem_euclid(360.0);
    let angle_diff = ((required_dome_azimuth_deg - telescope_azimuth_deg + 180.0).rem_euclid(360.0) - 180.0).abs();

    let max_allowed_offset_deg = (geometry.slit_width_m / (2.0 * geometry.dome_radius_m)).asin().to_degrees();
    let slit_centered = angle_diff < 1.0;
    let vignetting_risk = angle_diff > max_allowed_offset_deg;

    DomeSlitPosition {
        required_dome_azimuth_deg,
        telescope_azimuth_deg,
        telescope_altitude_deg,
        slit_centered,
        vignetting_risk,
    }
}

pub fn enclosure_report_to_json(report: &EnclosureSafetyReport) -> String {
    let mut json = String::new();
    json.push_str("{\n");
    json.push_str("  \"dew_point_c\": ");
    json.push_str(&format!("{:.2}", report.dew_point_c));
    json.push_str(",\n");
    json.push_str("  \"dew_point_margin_c\": ");
    json.push_str(&format!("{:.2}", report.dew_point_margin_c));
    json.push_str(",\n");
    json.push_str("  \"cloud_cover_delta_c\": ");
    json.push_str(&format!("{:.2}", report.cloud_cover_delta_c));
    json.push_str(",\n");
    json.push_str("  \"weather_status\": \"");
    json.push_str(&format!("{:?}", report.weather_status));
    json.push_str("\",\n");
    json.push_str("  \"enclosure_state\": \"");
    json.push_str(&format!("{:?}", report.enclosure_state));
    json.push_str("\",\n");
    json.push_str("  \"can_open_shutter\": ");
    json.push_str(if report.can_open_shutter { "true" } else { "false" });
    json.push_str(",\n");
    json.push_str("  \"safety_reasons\": [\n");

    for (i, r) in report.safety_reasons.iter().enumerate() {
        json.push_str("    \"");
        json.push_str(&r.replace('"', "\\\""));
        json.push_str("\"");
        if i + 1 < report.safety_reasons.len() {
            json.push_str(",");
        }
        json.push_str("\n");
    }

    json.push_str("  ]\n");
    json.push_str("}\n");
    json
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculates_dew_point_accurately_with_magnus_formula() {
        // At 20 C and 50% RH, dew point is ~9.3 C
        let dp = compute_dew_point_c(20.0, 50.0);
        assert!((dp - 9.3).abs() < 0.2);
    }

    #[test]
    fn detects_safe_weather_conditions() {
        let telemetry = WeatherTelemetry::default(); // 12 C, 55% RH, -25 C sky, no rain, light wind
        let limits = WeatherLimits::default();
        let report = evaluate_weather_safety(&telemetry, &limits, EnclosureState::Closed);

        assert_eq!(report.weather_status, WeatherSafetyStatus::SafeToOpen);
        assert!(report.can_open_shutter);
        assert!(report.safety_reasons.is_empty());
    }

    #[test]
    fn triggers_safety_interlock_on_rain_or_humidity() {
        let telemetry = WeatherTelemetry {
            rain_detected: true,
            relative_humidity_pct: 92.0,
            ..Default::default()
        };
        let limits = WeatherLimits::default();
        let report = evaluate_weather_safety(&telemetry, &limits, EnclosureState::Open);

        assert_eq!(report.weather_status, WeatherSafetyStatus::UnsafeMustClose);
        assert!(!report.can_open_shutter);
        assert!(report.safety_reasons.len() >= 2);
    }

    #[test]
    fn synchronizes_dome_slit_azimuth() {
        let geometry = DomeSlitGeometry::default();
        let pos = compute_dome_azimuth_sync(180.0, 45.0, &geometry);

        assert!((pos.required_dome_azimuth_deg - 180.0).abs() < 2.0);
        assert!(!pos.vignetting_risk);
    }
}
