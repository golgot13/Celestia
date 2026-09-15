#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpuFeatureFlags {
    pub x86_64: bool,
    pub sse2: bool,
    pub avx2: bool,
}

#[inline]
pub fn detect_cpu_features() -> CpuFeatureFlags {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        CpuFeatureFlags {
            x86_64: true,
            sse2: is_x86_feature_detected!("sse2"),
            avx2: is_x86_feature_detected!("avx2"),
        }
    }

    #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
    {
        CpuFeatureFlags {
            x86_64: false,
            sse2: false,
            avx2: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_x86_execution_environment() {
        let flags = detect_cpu_features();
        assert!(flags.x86_64 || !cfg!(target_arch = "x86_64"));
    }
}
