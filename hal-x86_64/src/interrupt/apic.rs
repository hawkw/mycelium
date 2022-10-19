//! Advanced Programmable Interrupt Controller (APIC).
pub mod io;
pub mod local;
pub use local::LocalApic;

use raw_cpuid::CpuId;

pub fn is_supported() -> bool {
    CpuId::new()
        .get_feature_info()
        .map(|features| features.has_apic())
        .unwrap_or(false)
}
