use std::f64::consts::PI;

#[derive(Clone, Debug, PartialEq)]
pub struct MosaicGridConfig {
    pub center_ra_deg: f64,
    pub center_dec_deg: f64,
    pub fov_width_arcmin: f64,
    pub fov_height_arcmin: f64,
    pub columns: usize,
    pub rows: usize,
    pub overlap_percentage: f64, // e.g. 15.0 for 15% overlap
    pub position_angle_deg: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MosaicTile {
    pub tile_index: usize,
    pub row: usize,
    pub column: usize,
    pub center_ra_deg: f64,
    pub center_dec_deg: f64,
    pub corner_coordinates: [(f64, f64); 4], // (RA, Dec) for top-left, top-right, bottom-right, bottom-left
    pub offset_from_center_arcmin: (f64, f64),
}

#[derive(Clone, Debug, PartialEq)]
pub struct MosaicPlan {
    pub center_ra_deg: f64,
    pub center_dec_deg: f64,
    pub total_tiles: usize,
    pub total_width_arcmin: f64,
    pub total_height_arcmin: f64,
    pub total_area_sq_deg: f64,
    pub overlap_percentage: f64,
    pub tiles: Vec<MosaicTile>,
}

pub fn generate_mosaic_grid(config: &MosaicGridConfig) -> Result<MosaicPlan, String> {
    if config.columns == 0 || config.rows == 0 {
        return Err("mosaic grid dimensions must be at least 1x1".to_string());
    }
    if config.fov_width_arcmin <= 0.0 || config.fov_height_arcmin <= 0.0 {
        return Err("camera FOV dimensions must be positive".to_string());
    }
    if config.overlap_percentage < 0.0 || config.overlap_percentage >= 90.0 {
        return Err("overlap percentage must be in range [0, 90)".to_string());
    }

    let overlap_factor = 1.0 - (config.overlap_percentage / 100.0);
    let step_x_arcmin = config.fov_width_arcmin * overlap_factor;
    let step_y_arcmin = config.fov_height_arcmin * overlap_factor;

    let half_cols = (config.columns as f64 - 1.0) / 2.0;
    let half_rows = (config.rows as f64 - 1.0) / 2.0;

    let pa_rad = config.position_angle_deg * PI / 180.0;
    let cos_pa = pa_rad.cos();
    let sin_pa = pa_rad.sin();

    let mut tiles = Vec::with_capacity(config.columns * config.rows);
    let mut tile_index = 1;

    for r in 0..config.rows {
        for c in 0..config.columns {
            let offset_x_unrot = (c as f64 - half_cols) * step_x_arcmin;
            let offset_y_unrot = (half_rows - r as f64) * step_y_arcmin;

            // Apply position angle rotation
            let dx_arcmin = offset_x_unrot * cos_pa - offset_y_unrot * sin_pa;
            let dy_arcmin = offset_x_unrot * sin_pa + offset_y_unrot * cos_pa;

            // Convert arcmin offset to RA/Dec taking into account cos(Dec) projection
            let dec_rad = config.center_dec_deg * PI / 180.0;
            let cos_dec = dec_rad.cos().abs().max(0.01);

            let tile_dec = config.center_dec_deg + (dy_arcmin / 60.0);
            let tile_ra = (config.center_ra_deg + (dx_arcmin / 60.0) / cos_dec).rem_euclid(360.0);

            // Calculate 4 corner coordinates (half FOV in each direction)
            let half_fw = config.fov_width_arcmin / 2.0;
            let half_fh = config.fov_height_arcmin / 2.0;

            let corner_offsets = [
                (-half_fw, half_fh),  // Top-left
                (half_fw, half_fh),   // Top-right
                (half_fw, -half_fh),  // Bottom-right
                (-half_fw, -half_fh), // Bottom-left
            ];

            let corners = corner_offsets.map(|(cx, cy)| {
                let r_cx = cx * cos_pa - cy * sin_pa;
                let r_cy = cx * sin_pa + cy * cos_pa;
                let c_dec = tile_dec + (r_cy / 60.0);
                let c_ra = (tile_ra + (r_cx / 60.0) / cos_dec).rem_euclid(360.0);
                (c_ra, c_dec)
            });

            tiles.push(MosaicTile {
                tile_index,
                row: r + 1,
                column: c + 1,
                center_ra_deg: tile_ra,
                center_dec_deg: tile_dec,
                corner_coordinates: corners,
                offset_from_center_arcmin: (dx_arcmin, dy_arcmin),
            });

            tile_index += 1;
        }
    }

    let total_width = (config.columns as f64 - 1.0) * step_x_arcmin + config.fov_width_arcmin;
    let total_height = (config.rows as f64 - 1.0) * step_y_arcmin + config.fov_height_arcmin;
    let area_sq_deg = (total_width / 60.0) * (total_height / 60.0);

    Ok(MosaicPlan {
        center_ra_deg: config.center_ra_deg,
        center_dec_deg: config.center_dec_deg,
        total_tiles: tiles.len(),
        total_width_arcmin: total_width,
        total_height_arcmin: total_height,
        total_area_sq_deg: area_sq_deg,
        overlap_percentage: config.overlap_percentage,
        tiles,
    })
}

pub fn mosaic_plan_to_json(plan: &MosaicPlan) -> String {
    let mut json = String::new();
    json.push_str("{\n");
    json.push_str("  \"center_ra_deg\": ");
    json.push_str(&format!("{:.6}", plan.center_ra_deg));
    json.push_str(",\n");
    json.push_str("  \"center_dec_deg\": ");
    json.push_str(&format!("{:.6}", plan.center_dec_deg));
    json.push_str(",\n");
    json.push_str("  \"total_tiles\": ");
    json.push_str(&plan.total_tiles.to_string());
    json.push_str(",\n");
    json.push_str("  \"total_width_arcmin\": ");
    json.push_str(&format!("{:.2}", plan.total_width_arcmin));
    json.push_str(",\n");
    json.push_str("  \"total_height_arcmin\": ");
    json.push_str(&format!("{:.2}", plan.total_height_arcmin));
    json.push_str(",\n");
    json.push_str("  \"total_area_sq_deg\": ");
    json.push_str(&format!("{:.4}", plan.total_area_sq_deg));
    json.push_str(",\n");
    json.push_str("  \"overlap_percentage\": ");
    json.push_str(&format!("{:.1}", plan.overlap_percentage));
    json.push_str(",\n");
    json.push_str("  \"tiles\": [\n");

    for (i, t) in plan.tiles.iter().enumerate() {
        json.push_str("    {\n");
        json.push_str("      \"tile_index\": ");
        json.push_str(&t.tile_index.to_string());
        json.push_str(",\n");
        json.push_str("      \"row\": ");
        json.push_str(&t.row.to_string());
        json.push_str(",\n");
        json.push_str("      \"column\": ");
        json.push_str(&t.column.to_string());
        json.push_str(",\n");
        json.push_str("      \"center_ra_deg\": ");
        json.push_str(&format!("{:.6}", t.center_ra_deg));
        json.push_str(",\n");
        json.push_str("      \"center_dec_deg\": ");
        json.push_str(&format!("{:.6}", t.center_dec_deg));
        json.push_str(",\n");
        json.push_str("      \"offset_arcmin\": [");
        json.push_str(&format!("{:.2}, {:.2}", t.offset_from_center_arcmin.0, t.offset_from_center_arcmin.1));
        json.push_str("]\n    }");
        if i + 1 < plan.tiles.len() {
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
    fn generates_2x2_mosaic_grid_with_exact_overlap() {
        let config = MosaicGridConfig {
            center_ra_deg: 10.0,
            center_dec_deg: 45.0,
            fov_width_arcmin: 60.0,  // 1 degree FOV
            fov_height_arcmin: 60.0,
            columns: 2,
            rows: 2,
            overlap_percentage: 20.0, // 20% overlap -> 48 arcmin step
            position_angle_deg: 0.0,
        };

        let plan = generate_mosaic_grid(&config).unwrap();
        assert_eq!(plan.total_tiles, 4);
        assert_eq!(plan.tiles.len(), 4);
        assert!((plan.total_width_arcmin - 108.0).abs() < 1e-4);
        assert!((plan.total_height_arcmin - 108.0).abs() < 1e-4);

        // Center of mosaic should average to (10.0, 45.0)
        let avg_ra: f64 = plan.tiles.iter().map(|t| t.center_ra_deg).sum::<f64>() / 4.0;
        let avg_dec: f64 = plan.tiles.iter().map(|t| t.center_dec_deg).sum::<f64>() / 4.0;
        assert!((avg_ra - 10.0).abs() < 1e-4);
        assert!((avg_dec - 45.0).abs() < 1e-4);
    }
}
