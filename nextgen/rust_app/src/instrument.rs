#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InstrumentConfig {
    pub name: &'static str,
    pub pixel_width: usize,
    pub pixel_height: usize,
    pub gain_e_per_adu: f64,
    pub read_noise_e: f64,
    pub temperature_c: f64,
    pub enabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InstrumentStatus {
    pub name: &'static str,
    pub ready: bool,
    pub temperature_c: f64,
    pub exposure_ready: bool,
}

pub fn initialize_instrument(config: InstrumentConfig) -> InstrumentStatus {
    InstrumentStatus {
        name: config.name,
        ready: config.enabled && config.pixel_width > 0 && config.pixel_height > 0,
        temperature_c: config.temperature_c,
        exposure_ready: config.enabled && config.gain_e_per_adu > 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initializes_ready_instrument_when_enabled() {
        let config = InstrumentConfig {
            name: "MainCam",
            pixel_width: 4096,
            pixel_height: 4096,
            gain_e_per_adu: 1.2,
            read_noise_e: 6.5,
            temperature_c: -10.0,
            enabled: true,
        };

        let status = initialize_instrument(config);
        assert!(status.ready);
        assert!(status.exposure_ready);
        assert_eq!(status.name, "MainCam");
    }

    #[test]
    fn disabled_instrument_is_not_ready() {
        let config = InstrumentConfig {
            name: "GuidingCam",
            pixel_width: 0,
            pixel_height: 0,
            gain_e_per_adu: 0.0,
            read_noise_e: 0.0,
            temperature_c: 20.0,
            enabled: false,
        };

        let status = initialize_instrument(config);
        assert!(!status.ready);
        assert!(!status.exposure_ready);
    }
}
