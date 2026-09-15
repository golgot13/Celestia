#[repr(C, align(32))]
pub struct AstroBatchHeader {
    pub element_count: u32,
    pub reserved: u32,
    pub timestamp_tag: u64,
}

#[repr(C, align(32))]
pub struct AstroStatus {
    pub code: i32,
    pub processed: u32,
    pub first_error_index: u32,
}

pub const ASTRO_ABI_VERSION_MAJOR: i32 = 1;
pub const ASTRO_ABI_VERSION_MINOR: i32 = 0;
pub const ASTRO_REQUIRED_CPU_FEATURES_AVX2: u64 = 1 << 1;

#[allow(dead_code)]
extern "C" {
    pub fn get_abi_version_major() -> i32;
    pub fn get_abi_version_minor() -> i32;
    pub fn get_required_cpu_features() -> u64;
    pub fn self_test_kernel_set(status_out: *mut AstroStatus) -> i32;
    pub fn kernel_ephem_interp_f64_avx2(
        hdr: *const AstroBatchHeader,
        t: *const f64,
        coeff: *const f64,
        coeff_stride: u32,
        pos_out: *mut f64,
        vel_out: *mut f64,
        status_out: *mut AstroStatus,
    ) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abi_contract_layout_is_stable() {
        let header_size = std::mem::size_of::<AstroBatchHeader>();
        let status_size = std::mem::size_of::<AstroStatus>();
        assert!(header_size >= 32);
        assert!(status_size >= 16);
        assert!(ASTRO_ABI_VERSION_MAJOR == 1);
    }
}
