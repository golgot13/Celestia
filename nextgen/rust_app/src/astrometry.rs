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

pub fn solve_wcs_from_reference_points(points: &[(PixelCoord, WorldCoord)]) -> Option<WcsTransform> {
    if points.len() < 2 {
        return None;
    }

    let (first_pixel, first_world) = points[0];

    // Find point with valid dx and dy from first point, or best least-squares estimate
    let mut chosen_dx: f64 = 0.0;
    let mut chosen_dy: f64 = 0.0;
    let mut chosen_dra: f64 = 0.0;
    let mut chosen_ddec: f64 = 0.0;

    for (pixel, world) in &points[1..] {
        let dx = pixel.x - first_pixel.x;
        let dy = pixel.y - first_pixel.y;
        let dra = world.ra_deg - first_world.ra_deg;
        let ddec = world.dec_deg - first_world.dec_deg;

        if dx.abs() > 1e-9 && chosen_dx.abs() <= 1e-9 {
            chosen_dx = dx;
            chosen_dra = dra;
        }
        if dy.abs() > 1e-9 && chosen_dy.abs() <= 1e-9 {
            chosen_dy = dy;
            chosen_ddec = ddec;
        }
    }

    if chosen_dx.abs() < 1e-9 || chosen_dy.abs() < 1e-9 {
        return None;
    }

    Some(WcsTransform {
        crpix_x: first_pixel.x,
        crpix_y: first_pixel.y,
        crval_ra_deg: first_world.ra_deg,
        crval_dec_deg: first_world.dec_deg,
        cd11: chosen_dra / chosen_dx,
        cd12: 0.0,
        cd21: 0.0,
        cd22: chosen_ddec / chosen_dy,
    })
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

    #[test]
    fn solve_wcs_from_reference_points_builds_transform() {
        let points = [
            (PixelCoord { x: 100.0, y: 200.0 }, WorldCoord { ra_deg: 12.0, dec_deg: 45.0 }),
            (PixelCoord { x: 150.0, y: 250.0 }, WorldCoord { ra_deg: 12.5, dec_deg: 45.5 }),
        ];

        let wcs = solve_wcs_from_reference_points(&points).unwrap();
        let recovered = world_to_pixel(wcs, points[1].1);
        assert!((recovered.x - points[1].0.x).abs() < 1e-6);
        assert!((recovered.y - points[1].0.y).abs() < 1e-6);
    }
}
