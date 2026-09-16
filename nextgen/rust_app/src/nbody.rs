pub const GRAVITATIONAL_CONSTANT: f64 = 6.67430e-11; // m^3 kg^-1 s^-2
pub const SPEED_OF_LIGHT: f64 = 299792458.0; // m s^-1
pub const ASTRONOMICAL_UNIT_M: f64 = 1.495978707e11; // m

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntegratorMethod {
    SymplecticEuler,
    Verlet,
    SymplecticYoshida4th,
    RungeKutta4th,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CelestialBody {
    pub name: &'static str,
    pub mass_kg: f64,
    pub position_m: [f64; 3],
    pub velocity_m_s: [f64; 3],
}

#[derive(Clone, Debug, PartialEq)]
pub struct SystemEnergy {
    pub kinetic_energy_j: f64,
    pub potential_energy_j: f64,
    pub total_energy_j: f64,
    pub angular_momentum_kg_m2_s: [f64; 3],
}

#[derive(Clone, Debug, PartialEq)]
pub struct NBodyPropagationReport {
    pub initial_time_s: f64,
    pub final_time_s: f64,
    pub step_count: usize,
    pub initial_energy: SystemEnergy,
    pub final_energy: SystemEnergy,
    pub energy_relative_drift: f64,
    pub bodies: Vec<CelestialBody>,
}

fn compute_accelerations(bodies: &[CelestialBody], include_relativity: bool) -> Vec<[f64; 3]> {
    let n = bodies.len();
    let mut acc = vec![[0.0, 0.0, 0.0]; n];

    for i in 0..n {
        for j in 0..n {
            if i == j {
                continue;
            }

            let dx = bodies[j].position_m[0] - bodies[i].position_m[0];
            let dy = bodies[j].position_m[1] - bodies[i].position_m[1];
            let dz = bodies[j].position_m[2] - bodies[i].position_m[2];

            let dist_sq = dx * dx + dy * dy + dz * dz;
            let dist = dist_sq.sqrt();
            if dist < 1.0 {
                continue; // soften near-zero distance to prevent singularity
            }

            let dist_cube = dist * dist_sq;
            let newton_factor = GRAVITATIONAL_CONSTANT * bodies[j].mass_kg / dist_cube;

            acc[i][0] += newton_factor * dx;
            acc[i][1] += newton_factor * dy;
            acc[i][2] += newton_factor * dz;

            // 1PN Post-Newtonian relativistic correction (Schwarzschild effect)
            if include_relativity && bodies[j].mass_kg > 1e28 {
                let v2 = bodies[i].velocity_m_s[0].powi(2)
                    + bodies[i].velocity_m_s[1].powi(2)
                    + bodies[i].velocity_m_s[2].powi(2);
                let r_dot_v = dx * bodies[i].velocity_m_s[0]
                    + dy * bodies[i].velocity_m_s[1]
                    + dz * bodies[i].velocity_m_s[2];

                let gm = GRAVITATIONAL_CONSTANT * bodies[j].mass_kg;
                let c2 = SPEED_OF_LIGHT * SPEED_OF_LIGHT;
                let factor_pn = gm / (c2 * dist_cube);

                let term1 = (4.0 * gm / dist) - v2;
                acc[i][0] += factor_pn * (term1 * dx + 4.0 * r_dot_v * bodies[i].velocity_m_s[0]);
                acc[i][1] += factor_pn * (term1 * dy + 4.0 * r_dot_v * bodies[i].velocity_m_s[1]);
                acc[i][2] += factor_pn * (term1 * dz + 4.0 * r_dot_v * bodies[i].velocity_m_s[2]);
            }
        }
    }

    acc
}

pub fn compute_system_energy(bodies: &[CelestialBody]) -> SystemEnergy {
    let mut kinetic = 0.0;
    let mut potential = 0.0;
    let mut lx = 0.0;
    let mut ly = 0.0;
    let mut lz = 0.0;

    for (i, body) in bodies.iter().enumerate() {
        let vx = body.velocity_m_s[0];
        let vy = body.velocity_m_s[1];
        let vz = body.velocity_m_s[2];
        let v2 = vx * vx + vy * vy + vz * vz;

        kinetic += 0.5 * body.mass_kg * v2;

        let rx = body.position_m[0];
        let ry = body.position_m[1];
        let rz = body.position_m[2];

        // Angular momentum: L = r x (m * v)
        lx += body.mass_kg * (ry * vz - rz * vy);
        ly += body.mass_kg * (rz * vx - rx * vz);
        lz += body.mass_kg * (rx * vy - ry * vx);

        for j in (i + 1)..bodies.len() {
            let dx = bodies[j].position_m[0] - rx;
            let dy = bodies[j].position_m[1] - ry;
            let dz = bodies[j].position_m[2] - rz;
            let dist = (dx * dx + dy * dy + dz * dz).sqrt();
            if dist > 1.0 {
                potential -= (GRAVITATIONAL_CONSTANT * body.mass_kg * bodies[j].mass_kg) / dist;
            }
        }
    }

    SystemEnergy {
        kinetic_energy_j: kinetic,
        potential_energy_j: potential,
        total_energy_j: kinetic + potential,
        angular_momentum_kg_m2_s: [lx, ly, lz],
    }
}

pub fn step_nbody_symplectic_4th(bodies: &mut [CelestialBody], dt_s: f64, relativity: bool) {
    // 4th order Yoshida symplectic integrator coefficients
    let c2 = 2.0_f64.powf(1.0 / 3.0);
    let w1 = 1.0 / (2.0 - c2);
    let w0 = -c2 / (2.0 - c2);

    let c_coeff = [0.5 * w1, 0.5 * (w0 + w1), 0.5 * (w0 + w1), 0.5 * w1];
    let d_coeff = [w1, w0, w1];

    for step in 0..3 {
        // Position kick
        let c = c_coeff[step];
        for body in bodies.iter_mut() {
            body.position_m[0] += c * body.velocity_m_s[0] * dt_s;
            body.position_m[1] += c * body.velocity_m_s[1] * dt_s;
            body.position_m[2] += c * body.velocity_m_s[2] * dt_s;
        }

        // Acceleration and velocity kick
        let acc = compute_accelerations(bodies, relativity);
        let d = d_coeff[step];
        for (body, a) in bodies.iter_mut().zip(&acc) {
            body.velocity_m_s[0] += d * a[0] * dt_s;
            body.velocity_m_s[1] += d * a[1] * dt_s;
            body.velocity_m_s[2] += d * a[2] * dt_s;
        }
    }

    // Final position drift
    let c = c_coeff[3];
    for body in bodies.iter_mut() {
        body.position_m[0] += c * body.velocity_m_s[0] * dt_s;
        body.position_m[1] += c * body.velocity_m_s[1] * dt_s;
        body.position_m[2] += c * body.velocity_m_s[2] * dt_s;
    }
}

pub fn propagate_nbody_system(
    mut bodies: Vec<CelestialBody>,
    duration_s: f64,
    dt_s: f64,
    method: IntegratorMethod,
    relativity: bool,
) -> NBodyPropagationReport {
    let initial_energy = compute_system_energy(&bodies);
    let steps = (duration_s / dt_s).ceil() as usize;

    for _ in 0..steps {
        match method {
            IntegratorMethod::SymplecticYoshida4th => {
                step_nbody_symplectic_4th(&mut bodies, dt_s, relativity);
            }
            IntegratorMethod::Verlet | IntegratorMethod::SymplecticEuler => {
                let acc = compute_accelerations(&bodies, relativity);
                for (body, a) in bodies.iter_mut().zip(&acc) {
                    body.velocity_m_s[0] += a[0] * dt_s;
                    body.velocity_m_s[1] += a[1] * dt_s;
                    body.velocity_m_s[2] += a[2] * dt_s;
                    body.position_m[0] += body.velocity_m_s[0] * dt_s;
                    body.position_m[1] += body.velocity_m_s[1] * dt_s;
                    body.position_m[2] += body.velocity_m_s[2] * dt_s;
                }
            }
            IntegratorMethod::RungeKutta4th => {
                step_nbody_symplectic_4th(&mut bodies, dt_s, relativity); // Fallback to high precision symplectic
            }
        }
    }

    let final_energy = compute_system_energy(&bodies);
    let energy_relative_drift = ((final_energy.total_energy_j - initial_energy.total_energy_j)
        / initial_energy.total_energy_j.abs().max(1e-12))
    .abs();

    NBodyPropagationReport {
        initial_time_s: 0.0,
        final_time_s: duration_s,
        step_count: steps,
        initial_energy,
        final_energy,
        energy_relative_drift,
        bodies,
    }
}

pub fn nbody_report_to_json(report: &NBodyPropagationReport) -> String {
    let mut json = String::new();
    json.push_str("{\n");
    json.push_str("  \"initial_time_s\": ");
    json.push_str(&format!("{:.2}", report.initial_time_s));
    json.push_str(",\n");
    json.push_str("  \"final_time_s\": ");
    json.push_str(&format!("{:.2}", report.final_time_s));
    json.push_str(",\n");
    json.push_str("  \"step_count\": ");
    json.push_str(&report.step_count.to_string());
    json.push_str(",\n");
    json.push_str("  \"initial_total_energy_j\": ");
    json.push_str(&format!("{:.6e}", report.initial_energy.total_energy_j));
    json.push_str(",\n");
    json.push_str("  \"final_total_energy_j\": ");
    json.push_str(&format!("{:.6e}", report.final_energy.total_energy_j));
    json.push_str(",\n");
    json.push_str("  \"energy_relative_drift\": ");
    json.push_str(&format!("{:.6e}", report.energy_relative_drift));
    json.push_str(",\n");
    json.push_str("  \"bodies\": [\n");

    for (i, body) in report.bodies.iter().enumerate() {
        json.push_str("    {\n");
        json.push_str("      \"name\": \"");
        json.push_str(body.name);
        json.push_str("\",\n");
        json.push_str("      \"mass_kg\": ");
        json.push_str(&format!("{:.4e}", body.mass_kg));
        json.push_str(",\n");
        json.push_str("      \"position_au\": [");
        json.push_str(&format!(
            "{:.6}, {:.6}, {:.6}",
            body.position_m[0] / ASTRONOMICAL_UNIT_M,
            body.position_m[1] / ASTRONOMICAL_UNIT_M,
            body.position_m[2] / ASTRONOMICAL_UNIT_M
        ));
        json.push_str("],\n");
        json.push_str("      \"velocity_km_s\": [");
        json.push_str(&format!(
            "{:.4}, {:.4}, {:.4}",
            body.velocity_m_s[0] / 1000.0,
            body.velocity_m_s[1] / 1000.0,
            body.velocity_m_s[2] / 1000.0
        ));
        json.push_str("]\n    }");
        if i + 1 < report.bodies.len() {
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
    fn propagates_earth_sun_orbit_conserving_symplectic_energy() {
        let sun = CelestialBody {
            name: "Sun",
            mass_kg: 1.98847e30,
            position_m: [0.0, 0.0, 0.0],
            velocity_m_s: [0.0, 0.0, 0.0],
        };

        // Earth at 1 AU with circular orbit speed ~29.78 km/s
        let r0 = ASTRONOMICAL_UNIT_M;
        let v0 = (GRAVITATIONAL_CONSTANT * sun.mass_kg / r0).sqrt();

        let earth = CelestialBody {
            name: "Earth",
            mass_kg: 5.9722e24,
            position_m: [r0, 0.0, 0.0],
            velocity_m_s: [0.0, v0, 0.0],
        };

        let bodies = vec![sun, earth];
        let one_year_s = 365.25 * 86400.0;
        let dt_s = 3600.0; // 1 hour steps

        let report = propagate_nbody_system(
            bodies,
            one_year_s,
            dt_s,
            IntegratorMethod::SymplecticYoshida4th,
            false,
        );

        assert_eq!(report.step_count, 8766);
        // Symplectic 4th order integrator must preserve energy to < 1e-7 over 1 year orbit
        assert!(report.energy_relative_drift < 1e-7);

        // After 1 full year, Earth must be back at position ~ (1 AU, 0, 0)
        let final_earth_r = report.bodies[1].position_m[0] / ASTRONOMICAL_UNIT_M;
        assert!((final_earth_r - 1.0).abs() < 0.005);
    }
}
