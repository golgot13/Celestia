#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldCoord {
    pub ra_deg: f64,
    pub dec_deg: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PixelCoord {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WcsTransform {
    pub crpix_x: f64,
    pub crpix_y: f64,
    pub crval_ra_deg: f64,
    pub crval_dec_deg: f64,
    pub cd11: f64,
    pub cd12: f64,
    pub cd21: f64,
    pub cd22: f64,
}

pub fn world_to_pixel(transform: WcsTransform, world: WorldCoord) -> PixelCoord {
    let dx_ra = world.ra_deg - transform.crval_ra_deg;
    let dy_dec = world.dec_deg - transform.crval_dec_deg;

    let x = transform.crpix_x + dx_ra / transform.cd11;
    let y = transform.crpix_y + dy_dec / transform.cd22;

    PixelCoord { x, y }
}

pub fn pixel_to_world(transform: WcsTransform, pixel: PixelCoord) -> WorldCoord {
    let dx = pixel.x - transform.crpix_x;
    let dy = pixel.y - transform.crpix_y;

    let ra_deg = transform.crval_ra_deg + dx * transform.cd11;
    let dec_deg = transform.crval_dec_deg + dy * transform.cd22;

    WorldCoord { ra_deg, dec_deg }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_to_pixel_returns_zero_at_reference_point() {
        let wcs = WcsTransform {
            crpix_x: 512.0,
            crpix_y: 512.0,
            crval_ra_deg: 12.0,
            crval_dec_deg: 45.0,
            cd11: 0.2,
            cd12: 0.0,
            cd21: 0.0,
            cd22: 0.2,
        };

        let pixel = world_to_pixel(wcs, WorldCoord { ra_deg: 12.0, dec_deg: 45.0 });
        assert!((pixel.x - 512.0).abs() < 1e-9);
        assert!((pixel.y - 512.0).abs() < 1e-9);
    }

    #[test]
    fn round_trip_world_pixel_round_trip_is_stable() {
        let wcs = WcsTransform {
            crpix_x: 1000.0,
            crpix_y: 800.0,
            crval_ra_deg: 10.0,
            crval_dec_deg: 20.0,
            cd11: 0.1,
            cd12: 0.0,
            cd21: 0.0,
            cd22: 0.1,
        };

        let world = WorldCoord { ra_deg: 10.5, dec_deg: 20.5 };
        let pixel = world_to_pixel(wcs, world);
        let recovered = pixel_to_world(wcs, pixel);
        assert!((recovered.ra_deg - world.ra_deg).abs() < 1e-6);
        assert!((recovered.dec_deg - world.dec_deg).abs() < 1e-6);
    }
}
