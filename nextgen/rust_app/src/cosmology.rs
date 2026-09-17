//! Homogeneous and isotropic FLRW cosmology for the large-scale simulator.

pub const SPEED_OF_LIGHT_KM_S: f64 = 299_792.458;
pub const MPC_KM: f64 = 3.085_677_581_491_367_3e19;
pub const SECONDS_PER_GIGAYEAR: f64 = 31_557_600.0e9;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpatialGeometry {
    Flat,
    Open,
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UniverseModel {
    PlanckLambdaCdm,
    EinsteinDeSitter,
    Milne,
    DeSitter,
    RadiationDominated,
    OpenLambdaCdm,
    ClosedLambdaCdm,
}

impl UniverseModel {
    pub const ALL: [Self; 7] = [
        Self::PlanckLambdaCdm,
        Self::EinsteinDeSitter,
        Self::Milne,
        Self::DeSitter,
        Self::RadiationDominated,
        Self::OpenLambdaCdm,
        Self::ClosedLambdaCdm,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::PlanckLambdaCdm => "Planck 2018 Lambda-CDM",
            Self::EinsteinDeSitter => "Einstein-de Sitter",
            Self::Milne => "Milne (vide, ouvert)",
            Self::DeSitter => "de Sitter (Lambda)",
            Self::RadiationDominated => "univers domine par le rayonnement",
            Self::OpenLambdaCdm => "Lambda-CDM ouvert",
            Self::ClosedLambdaCdm => "Lambda-CDM ferme",
        }
    }

    pub fn parameters(self) -> CosmologicalParameters {
        let h0 = CosmologicalParameters::PLANCK_2018.h0_km_s_mpc;
        match self {
            Self::PlanckLambdaCdm => CosmologicalParameters::PLANCK_2018,
            Self::EinsteinDeSitter => CosmologicalParameters {
                h0_km_s_mpc: h0,
                omega_matter: 1.0,
                omega_radiation: 0.0,
                omega_lambda: 0.0,
            },
            Self::Milne => CosmologicalParameters {
                h0_km_s_mpc: h0,
                omega_matter: 0.0,
                omega_radiation: 0.0,
                omega_lambda: 0.0,
            },
            Self::DeSitter => CosmologicalParameters {
                h0_km_s_mpc: h0,
                omega_matter: 0.0,
                omega_radiation: 0.0,
                omega_lambda: 1.0,
            },
            Self::RadiationDominated => CosmologicalParameters {
                h0_km_s_mpc: h0,
                omega_matter: 0.0,
                omega_radiation: 1.0,
                omega_lambda: 0.0,
            },
            Self::OpenLambdaCdm => CosmologicalParameters {
                h0_km_s_mpc: h0,
                omega_matter: 0.3,
                omega_radiation: 0.0,
                omega_lambda: 0.5,
            },
            Self::ClosedLambdaCdm => CosmologicalParameters {
                h0_km_s_mpc: h0,
                omega_matter: 0.8,
                omega_radiation: 0.0,
                omega_lambda: 0.5,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CosmologicalParameters {
    pub h0_km_s_mpc: f64,
    pub omega_matter: f64,
    pub omega_radiation: f64,
    pub omega_lambda: f64,
}

impl CosmologicalParameters {
    pub const PLANCK_2018: Self = Self {
        h0_km_s_mpc: 67.4,
        omega_matter: 0.315,
        omega_radiation: 9.0e-5,
        omega_lambda: 0.6849,
    };

    pub fn validate(self) -> Result<Self, String> {
        if !self.h0_km_s_mpc.is_finite() || self.h0_km_s_mpc <= 0.0 {
            return Err("H0 doit etre fini et strictement positif".to_string());
        }
        if !self.omega_matter.is_finite()
            || !self.omega_radiation.is_finite()
            || !self.omega_lambda.is_finite()
            || self.omega_matter < 0.0
            || self.omega_radiation < 0.0
            || self.omega_lambda < 0.0
        {
            return Err("les densites Omega doivent etre finies et non negatives".to_string());
        }
        if self.omega_curvature() < -1.0 {
            return Err("la somme des densites est physiquement invalide".to_string());
        }
        Ok(self)
    }

    pub fn omega_curvature(self) -> f64 {
        1.0 - self.omega_matter - self.omega_radiation - self.omega_lambda
    }

    pub fn geometry(self) -> SpatialGeometry {
        let curvature = self.omega_curvature();
        // Published parameter sets are rounded; classify residual curvature below 0.1%
        // as flat instead of turning catalogue rounding into a different universe type.
        if curvature > 1.0e-3 {
            SpatialGeometry::Open
        } else if curvature < -1.0e-3 {
            SpatialGeometry::Closed
        } else {
            SpatialGeometry::Flat
        }
    }

    pub fn hubble_at_redshift(self, redshift: f64) -> Result<f64, String> {
        self.validate()?;
        if !redshift.is_finite() || redshift < -1.0 {
            return Err("le redshift doit etre fini et superieur ou egal a -1".to_string());
        }
        let one_plus_z = 1.0 + redshift;
        let e2 = self.omega_radiation * one_plus_z.powi(4)
            + self.omega_matter * one_plus_z.powi(3)
            + self.omega_curvature() * one_plus_z.powi(2)
            + self.omega_lambda;
        if e2 <= 0.0 || !e2.is_finite() {
            return Err("H(z) est indefini pour cette configuration".to_string());
        }
        Ok(self.h0_km_s_mpc * e2.sqrt())
    }

    pub fn age_gyr(self) -> Result<f64, String> {
        self.validate()?;
        let integral = integrate_simpson(
            |a| 1.0 / (a * self.expansion_rate_at_scale_factor(a)),
            1.0e-8,
            1.0,
            4096,
        );
        Ok(integral * MPC_KM / self.h0_km_s_mpc / SECONDS_PER_GIGAYEAR)
    }

    pub fn lookback_time_gyr(self, redshift: f64) -> Result<f64, String> {
        self.validate()?;
        if !redshift.is_finite() || redshift < 0.0 {
            return Err("le redshift doit etre fini et non negatif".to_string());
        }
        let upper = 1.0 / (1.0 + redshift);
        let integral = integrate_simpson(
            |a| 1.0 / (a * self.expansion_rate_at_scale_factor(a)),
            upper,
            1.0,
            2048,
        );
        Ok(integral * MPC_KM / self.h0_km_s_mpc / SECONDS_PER_GIGAYEAR)
    }

    pub fn comoving_distance_mpc(self, redshift: f64) -> Result<f64, String> {
        self.validate()?;
        if !redshift.is_finite() || redshift < 0.0 {
            return Err("le redshift doit etre fini et non negatif".to_string());
        }
        let integral = integrate_simpson(
            |z| SPEED_OF_LIGHT_KM_S / self.hubble_at_redshift(z).unwrap_or(f64::INFINITY),
            0.0,
            redshift,
            4096,
        );
        Ok(integral)
    }

    pub fn transverse_comoving_distance_mpc(self, redshift: f64) -> Result<f64, String> {
        let distance = self.comoving_distance_mpc(redshift)?;
        let curvature = self.omega_curvature();
        if curvature.abs() < 1.0e-12 {
            return Ok(distance);
        }
        let scale = curvature.abs().sqrt() * self.h0_km_s_mpc / SPEED_OF_LIGHT_KM_S;
        let dimensionless = scale * distance;
        Ok(if curvature > 0.0 {
            dimensionless.sinh() / scale
        } else {
            dimensionless.sin() / scale
        })
    }

    pub fn luminosity_distance_mpc(self, redshift: f64) -> Result<f64, String> {
        Ok((1.0 + redshift) * self.transverse_comoving_distance_mpc(redshift)?)
    }

    pub fn angular_diameter_distance_mpc(self, redshift: f64) -> Result<f64, String> {
        Ok(self.transverse_comoving_distance_mpc(redshift)? / (1.0 + redshift))
    }

    pub fn distance_modulus(self, redshift: f64) -> Result<f64, String> {
        let distance_mpc = self.luminosity_distance_mpc(redshift)?;
        if distance_mpc <= 0.0 {
            return Err("le module de distance est indefini a z=0".to_string());
        }
        Ok(5.0 * distance_mpc.log10() + 25.0)
    }

    pub fn differential_comoving_volume_mpc3_per_sr(
        self,
        redshift: f64,
    ) -> Result<f64, String> {
        let transverse = self.transverse_comoving_distance_mpc(redshift)?;
        let hubble = self.hubble_at_redshift(redshift)?;
        Ok(SPEED_OF_LIGHT_KM_S * transverse * transverse / hubble)
    }

    pub fn deceleration_parameter(self, redshift: f64) -> Result<f64, String> {
        let one_plus_z = 1.0 + redshift;
        let hubble = self.hubble_at_redshift(redshift)?;
        let numerator = 0.5 * self.omega_matter * one_plus_z.powi(3)
            + self.omega_radiation * one_plus_z.powi(4)
            - self.omega_lambda;
        Ok(numerator / (hubble / self.h0_km_s_mpc).powi(2))
    }

    pub fn jerk_parameter(self, redshift: f64) -> Result<f64, String> {
        let one_plus_z = 1.0 + redshift;
        let hubble = self.hubble_at_redshift(redshift)?;
        let numerator = self.omega_matter * one_plus_z.powi(3)
            + 3.0 * self.omega_radiation * one_plus_z.powi(4)
            + self.omega_lambda;
        Ok(numerator / (hubble / self.h0_km_s_mpc).powi(2))
    }

    pub fn scale_factor_at_age_gyr(self, age_gyr: f64) -> Result<f64, String> {
        self.validate()?;
        if !age_gyr.is_finite() || age_gyr < 0.0 || age_gyr > self.age_gyr()? {
            return Err("l'age doit etre compris entre 0 et l'age actuel de l'univers".to_string());
        }
        let target_seconds = age_gyr * SECONDS_PER_GIGAYEAR;
        let h0_seconds = self.h0_km_s_mpc / MPC_KM;
        let mut low = 1.0e-8;
        let mut high = 1.0;
        for _ in 0..80 {
            let mid = 0.5 * (low + high);
            let age_mid = integrate_simpson(
                |a| 1.0 / (a * self.expansion_rate_at_scale_factor(a)),
                1.0e-8,
                mid,
                1024,
            ) / h0_seconds;
            if age_mid < target_seconds {
                low = mid;
            } else {
                high = mid;
            }
        }
        Ok(0.5 * (low + high))
    }

    fn expansion_rate_at_scale_factor(self, scale_factor: f64) -> f64 {
        let a = scale_factor.max(1.0e-8);
        let e2 = self.omega_radiation / a.powi(4)
            + self.omega_matter / a.powi(3)
            + self.omega_curvature() / a.powi(2)
            + self.omega_lambda;
        e2.max(0.0).sqrt()
    }
}

fn integrate_simpson(function: impl Fn(f64) -> f64, lower: f64, upper: f64, intervals: usize) -> f64 {
    if upper <= lower {
        return 0.0;
    }
    let intervals = intervals.max(2) + intervals.max(2) % 2;
    let step = (upper - lower) / intervals as f64;
    let mut sum = function(lower) + function(upper);
    for index in 1..intervals {
        let weight = if index % 2 == 0 { 2.0 } else { 4.0 };
        sum += weight * function(lower + index as f64 * step);
    }
    sum * step / 3.0
}

#[cfg(test)]
mod tests {
    use super::{CosmologicalParameters, SpatialGeometry, UniverseModel};

    #[test]
    fn universe_presets_cover_distinct_physical_regimes() {
        assert_eq!(UniverseModel::PlanckLambdaCdm.parameters().geometry(), SpatialGeometry::Flat);
        assert_eq!(UniverseModel::Milne.parameters().geometry(), SpatialGeometry::Open);
        assert_eq!(UniverseModel::ClosedLambdaCdm.parameters().geometry(), SpatialGeometry::Closed);
        assert_eq!(UniverseModel::DeSitter.parameters().omega_lambda, 1.0);
    }

    #[test]
    fn classifies_open_flat_and_closed_geometries() {
        let matter = CosmologicalParameters {
            omega_matter: 0.3,
            omega_radiation: 0.0,
            omega_lambda: 0.5,
            ..CosmologicalParameters::PLANCK_2018
        };
        let flat = CosmologicalParameters {
            omega_matter: 0.3,
            omega_radiation: 0.0,
            omega_lambda: 0.7,
            ..CosmologicalParameters::PLANCK_2018
        };
        let closed = CosmologicalParameters {
            omega_matter: 0.8,
            omega_radiation: 0.0,
            omega_lambda: 0.5,
            ..CosmologicalParameters::PLANCK_2018
        };
        assert_eq!(matter.geometry(), SpatialGeometry::Open);
        assert_eq!(flat.geometry(), SpatialGeometry::Flat);
        assert_eq!(closed.geometry(), SpatialGeometry::Closed);
    }

    #[test]
    fn hubble_law_matches_flat_lambda_cdm_order_of_magnitude() {
        let cosmology = CosmologicalParameters {
            h0_km_s_mpc: 70.0,
            omega_matter: 0.3,
            omega_radiation: 0.0,
            omega_lambda: 0.7,
        };
        assert!((cosmology.hubble_at_redshift(0.0).unwrap() - 70.0).abs() < 1.0e-12);
        assert!((cosmology.hubble_at_redshift(1.0).unwrap() - 123.2477).abs() < 1.0e-3);
    }

    #[test]
    fn distances_and_age_are_positive_and_scale_factor_is_invertible() {
        let cosmology = CosmologicalParameters::PLANCK_2018;
        let age = cosmology.age_gyr().unwrap();
        let distance = cosmology.luminosity_distance_mpc(1.0).unwrap();
        let scale = cosmology.scale_factor_at_age_gyr(age).unwrap();
        assert!((age - 13.8).abs() < 0.2);
        assert!(distance > 6000.0);
        assert!((scale - 1.0).abs() < 1.0e-6);
    }

    #[test]
    fn cosmography_observables_match_known_limits() {
        let de_sitter = UniverseModel::DeSitter.parameters();
        assert!((de_sitter.deceleration_parameter(0.0).unwrap() + 1.0).abs() < 1.0e-12);
        assert!((de_sitter.jerk_parameter(0.0).unwrap() - 1.0).abs() < 1.0e-12);

        let cosmology = CosmologicalParameters::PLANCK_2018;
        assert!(cosmology.angular_diameter_distance_mpc(1.0).unwrap() > 1000.0);
        assert!(cosmology.distance_modulus(1.0).unwrap() > 40.0);
        assert!(cosmology
            .differential_comoving_volume_mpc3_per_sr(1.0)
            .unwrap()
            > 1.0e10);
    }
}
