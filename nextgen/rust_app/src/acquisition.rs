#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MountState {
    pub ra_deg: f64,
    pub dec_deg: f64,
    pub tracking: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CaptureSession {
    pub target: &'static str,
    pub exposure_s: f64,
    pub filter: &'static str,
    pub count: usize,
    pub enabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CaptureResult {
    pub session: CaptureSession,
    pub frames_acquired: usize,
    pub sync_ok: bool,
}

pub fn initialize_mount() -> MountState {
    MountState {
        ra_deg: 0.0,
        dec_deg: 0.0,
        tracking: true,
    }
}

pub fn start_capture(session: CaptureSession) -> CaptureResult {
    let valid = session.exposure_s > 0.0 && session.count > 0 && session.enabled;
    CaptureResult {
        session,
        frames_acquired: if valid { session.count } else { 0 },
        sync_ok: valid,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mount_starts_in_tracking_mode() {
        let mount = initialize_mount();
        assert!(mount.tracking);
        assert_eq!(mount.ra_deg, 0.0);
        assert_eq!(mount.dec_deg, 0.0);
    }

    #[test]
    fn capture_session_validates_exposure_and_count() {
        let session = CaptureSession {
            target: "M31",
            exposure_s: 60.0,
            filter: "R",
            count: 3,
            enabled: true,
        };

        let result = start_capture(session);
        assert!(result.sync_ok);
        assert_eq!(result.frames_acquired, 3);
    }

    #[test]
    fn invalid_capture_is_rejected() {
        let session = CaptureSession {
            target: "M31",
            exposure_s: 0.0,
            filter: "R",
            count: 5,
            enabled: true,
        };

        let result = start_capture(session);
        assert!(!result.sync_ok);
        assert_eq!(result.frames_acquired, 0);
    }
}
