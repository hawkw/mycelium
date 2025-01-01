//! Advanced Programmable Interrupt Controller (APIC).
pub mod ioapic;
pub mod local;
pub use ioapic::{IoApic, IoApicSet};
pub use local::LocalApic;

use mycelium_util::bits::enum_from_bits;
use raw_cpuid::CpuId;

pub fn is_supported() -> bool {
    CpuId::new()
        .get_feature_info()
        .map(|features| features.has_apic())
        .unwrap_or(false)
}

enum_from_bits! {
    #[derive(Debug, PartialEq, Eq)]
    pub enum PinPolarity<u8> {
        High = 0,
        Low = 1,
    }
}

enum_from_bits! {
    #[derive(Debug, PartialEq, Eq)]
    pub enum TriggerMode<u8> {
        Edge = 0,
        Level = 1,
    }
}
