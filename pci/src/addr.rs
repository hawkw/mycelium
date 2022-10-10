#[derive(Debug, Copy, Clone, Eq, PartialEq, PartialOrd, Ord)]
pub enum Segment {
    /// A conventional PCI bus (not PCIe) supports up to 256 bus segments.
    Conventional(u8),
    /// A PCI Express bus supports up to 65535 "segment groups", which may each
    /// contain up to 256 bus segments.
    Extended { group: u16, segment: u8 },
}
