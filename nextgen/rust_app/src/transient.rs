#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransientClassification {
    NewPointSource,     // Supernova / Nova / Optical Transient
    MovingObject,       // Asteroid / Comet
    VariableStar,       // Flare / Eclipsing Binary
    SubtractionArtifact,// Saturated core or bad alignment residual
}

#[derive(Clone, Debug, PartialEq)]
pub struct TransientCandidate {
    pub candidate_id: usize,
    pub x: f64,
    pub y: f64,
    pub peak_flux: f64,
    pub total_flux: f64,
    pub snr: f64,
    pub significance_sigmas: f64,
    pub classification: TransientClassification,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SubtractionResult {
    pub width: usize,
    pub height: usize,
    pub difference_image: Vec<f64>,
    pub background_rms: f64,
    pub detected_transients: Vec<TransientCandidate>,
    pub total_candidate_count: usize,
}

pub fn estimate_scale_factor_2d(template: &[f64], science: &[f64]) -> f64 {
    let mut sum_tt = 0.0;
    let mut sum_ts = 0.0;

    for (&t, &s) in template.iter().zip(science) {
        if t > 0.0 && s > 0.0 {
            sum_tt += t * t;
            sum_ts += t * s;
        }
    }

    if sum_tt > 1e-12 {
        sum_ts / sum_tt
    } else {
        1.0
    }
}

pub fn compute_difference_image_2d(
    template: &[f64],
    science: &[f64],
    width: usize,
    height: usize,
    detection_threshold_sigma: f64,
) -> Result<SubtractionResult, String> {
    if template.len() != width * height || science.len() != width * height {
        return Err("template and science image dimensions mismatch".to_string());
    }

    let total_pixels = width * height;
    let scale = estimate_scale_factor_2d(template, science);

    let mut diff = vec![0.0; total_pixels];
    for i in 0..total_pixels {
        diff[i] = science[i] - scale * template[i];
    }

    // Robust background noise estimation on difference image (MAD)
    let mut sorted_diff = diff.clone();
    sorted_diff.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = if total_pixels % 2 == 1 {
        sorted_diff[total_pixels / 2]
    } else {
        0.5 * (sorted_diff[total_pixels / 2 - 1] + sorted_diff[total_pixels / 2])
    };

    let mut abs_devs: Vec<f64> = diff.iter().map(|&v| (v - median).abs()).collect();
    abs_devs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mad = if total_pixels % 2 == 1 {
        abs_devs[total_pixels / 2]
    } else {
        0.5 * (abs_devs[total_pixels / 2 - 1] + abs_devs[total_pixels / 2])
    };

    let bg_rms = (1.4826 * mad).max(1e-6);
    let threshold = median + detection_threshold_sigma * bg_rms;

    // Estimate template background median
    let mut sorted_tmpl = template.to_vec();
    sorted_tmpl.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let tmpl_median = if total_pixels % 2 == 1 {
        sorted_tmpl[total_pixels / 2]
    } else {
        0.5 * (sorted_tmpl[total_pixels / 2 - 1] + sorted_tmpl[total_pixels / 2])
    };

    let mut candidates = Vec::new();
    let mut candidate_id = 1;

    // Detect local positive peaks on difference image
    for y in 1..(height - 1) {
        for x in 1..(width - 1) {
            let idx = y * width + x;
            let val = diff[idx];

            if val > threshold {
                // Check 8-connected local maximum
                let is_local_max = val >= diff[(y - 1) * width + (x - 1)]
                    && val >= diff[(y - 1) * width + x]
                    && val >= diff[(y - 1) * width + (x + 1)]
                    && val >= diff[y * width + (x - 1)]
                    && val >= diff[y * width + (x + 1)]
                    && val >= diff[(y + 1) * width + (x - 1)]
                    && val >= diff[(y + 1) * width + x]
                    && val >= diff[(y + 1) * width + (x + 1)];

                if is_local_max {
                    let sigmas = (val - median) / bg_rms;
                    let template_val = template[idx];

                    // Classify candidate: compare with template background level
                    let classification = if template_val <= tmpl_median + 3.0 * bg_rms {
                        // New source not present in template (Supernova or Asteroid)
                        TransientClassification::NewPointSource
                    } else if (science[idx] - template_val).abs() > 3.0 * bg_rms {
                        // Source present in template but flux changed
                        TransientClassification::VariableStar
                    } else {
                        TransientClassification::SubtractionArtifact
                    };

                    // Compute 3x3 aperture net flux
                    let mut aperture_flux = 0.0;
                    for dy in -1..=1 {
                        for dx in -1..=1 {
                            let ny = (y as isize + dy) as usize;
                            let nx = (x as isize + dx) as usize;
                            aperture_flux += (diff[ny * width + nx] - median).max(0.0);
                        }
                    }

                    candidates.push(TransientCandidate {
                        candidate_id,
                        x: x as f64,
                        y: y as f64,
                        peak_flux: val,
                        total_flux: aperture_flux,
                        snr: sigmas,
                        significance_sigmas: sigmas,
                        classification,
                    });
                    candidate_id += 1;
                }
            }
        }
    }

    let count = candidates.len();
    Ok(SubtractionResult {
        width,
        height,
        difference_image: diff,
        background_rms: bg_rms,
        detected_transients: candidates,
        total_candidate_count: count,
    })
}

pub fn transient_result_to_json(result: &SubtractionResult) -> String {
    let mut json = String::new();
    json.push_str("{\n");
    json.push_str("  \"width\": ");
    json.push_str(&result.width.to_string());
    json.push_str(",\n");
    json.push_str("  \"height\": ");
    json.push_str(&result.height.to_string());
    json.push_str(",\n");
    json.push_str("  \"background_rms\": ");
    json.push_str(&format!("{:.4}", result.background_rms));
    json.push_str(",\n");
    json.push_str("  \"total_candidate_count\": ");
    json.push_str(&result.total_candidate_count.to_string());
    json.push_str(",\n");
    json.push_str("  \"candidates\": [\n");

    for (i, c) in result.detected_transients.iter().enumerate() {
        json.push_str("    {\n");
        json.push_str("      \"candidate_id\": ");
        json.push_str(&c.candidate_id.to_string());
        json.push_str(",\n");
        json.push_str("      \"x\": ");
        json.push_str(&format!("{:.2}", c.x));
        json.push_str(",\n");
        json.push_str("      \"y\": ");
        json.push_str(&format!("{:.2}", c.y));
        json.push_str(",\n");
        json.push_str("      \"peak_flux\": ");
        json.push_str(&format!("{:.2}", c.peak_flux));
        json.push_str(",\n");
        json.push_str("      \"total_flux\": ");
        json.push_str(&format!("{:.2}", c.total_flux));
        json.push_str(",\n");
        json.push_str("      \"significance_sigmas\": ");
        json.push_str(&format!("{:.2}", c.significance_sigmas));
        json.push_str(",\n");
        json.push_str("      \"classification\": \"");
        json.push_str(&format!("{:?}", c.classification));
        json.push_str("\"\n    }");
        if i + 1 < result.detected_transients.len() {
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
    fn detects_new_supernova_transient_source() {
        let width = 16;
        let height = 16;
        let mut template = vec![10.0; 256];
        let mut science = vec![10.0; 256];

        // Reference galaxy core at (8,8) in both template and science
        template[8 * 16 + 8] = 300.0;
        science[8 * 16 + 8] = 300.0;

        // New Supernova appeared in science frame at (12,12) -> index 12*16 + 12 = 204
        science[12 * 16 + 12] = 250.0;

        let result = compute_difference_image_2d(&template, &science, width, height, 5.0).unwrap();

        assert_eq!(result.total_candidate_count, 1);
        let cand = &result.detected_transients[0];
        assert_eq!(cand.x as usize, 12);
        assert_eq!(cand.y as usize, 12);
        assert_eq!(cand.classification, TransientClassification::NewPointSource);
        assert!(cand.significance_sigmas > 10.0);
    }

    #[test]
    fn detects_variable_star_flux_change() {
        let width = 16;
        let height = 16;
        let mut template = vec![15.0; 256];
        let mut science = vec![15.0; 256];

        // Flare star brightening at (5,5)
        template[5 * 16 + 5] = 100.0;
        science[5 * 16 + 5] = 450.0; // brightened by 350 ADU

        let result = compute_difference_image_2d(&template, &science, width, height, 5.0).unwrap();

        assert_eq!(result.total_candidate_count, 1);
        let cand = &result.detected_transients[0];
        assert_eq!(cand.x as usize, 5);
        assert_eq!(cand.y as usize, 5);
        assert_eq!(cand.classification, TransientClassification::VariableStar);
    }
}
