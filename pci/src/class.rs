use crate::{device, error};
use core::{cmp, fmt};
use pci_ids::FromId;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Classes {
    class: Class,
    subclass: Subclass,
}

/// A code describing a PCI device's device class.
///
/// This type represents a class code that exists in the PCI class database.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Class(&'static pci_ids::Class);

/// A code describing a PCI device's subclass within its [`Class`].
///
/// This type represents a subclass code that exists in the PCI class database.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Subclass(&'static pci_ids::Subclass);

#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct RawClasses {
    pub(crate) subclass: u8,
    pub(crate) class: u8,
}

// === impl Classes ===

impl Classes {
    #[inline]
    #[must_use]
    pub fn class(&self) -> Class {
        self.class
    }

    #[inline]
    #[must_use]
    pub fn subclass(&self) -> Subclass {
        self.subclass
    }

    #[inline]
    #[must_use]
    pub fn class_id(&self) -> u8 {
        self.class.0.id()
    }

    #[inline]
    #[must_use]
    pub fn subclass_id(&self) -> u8 {
        self.subclass.0.id()
    }
}

impl fmt::Display for Classes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { class, subclass } = self;
        write!(f, "{}: {}", class.name(), subclass.name())
    }
}

// === impl Class ===

impl Class {
    pub fn from_id(id: u8) -> Result<Self, error::UnexpectedValue<u8>> {
        let inner =
            pci_ids::Class::from_id(id).ok_or_else(|| error::unexpected(id).named("PCI class"))?;
        Ok(Self(inner))
    }

    pub fn subclass(self, id: u8) -> Result<Subclass, error::UnexpectedValue<u8>> {
        let inner = pci_ids::Subclass::from_cid_sid(self.id(), id)
            .ok_or_else(|| error::unexpected(id).named("PCI subclass"))?;
        Ok(Subclass(inner))
    }

    #[inline]
    #[must_use]
    pub fn name(self) -> &'static str {
        self.0.name()
    }

    #[inline]
    #[must_use]
    pub fn id(self) -> u8 {
        self.0.id()
    }
}

impl PartialOrd for Class {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Class {
    #[inline]
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        self.id().cmp(&other.id())
    }
}

// === impl Subclass ===

impl Subclass {
    #[inline]
    #[must_use]
    pub fn name(self) -> &'static str {
        self.0.name()
    }

    #[inline]
    #[must_use]
    pub fn id(self) -> u8 {
        self.0.id()
    }

    #[inline]
    #[must_use]
    pub fn class(self) -> Class {
        Class(self.0.class())
    }

    /// Resolves a register-level programming interface code ("prog IF") for
    /// this subclass.
    ///
    /// Note that this is an O(_N_) operation.
    #[must_use]
    pub fn prog_if(self, code: u8) -> device::ProgIf {
        self.0
            .prog_ifs()
            .find(|prog_if| prog_if.id() == code)
            .map(device::ProgIf::Known)
            .unwrap_or(device::ProgIf::Unknown(code))
    }
}

impl PartialOrd for Subclass {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Subclass {
    #[inline]
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        self.0
            .class()
            .id()
            .cmp(&other.class().id())
            .then_with(|| self.id().cmp(&other.id()))
    }
}

// === impl RawClasses ===

impl RawClasses {
    pub fn resolve(&self) -> Result<Classes, error::UnexpectedValue<Self>> {
        let class = self
            .resolve_class()
            .map_err(|_| error::unexpected(*self).named("PCI device class"))?;
        let subclass = self
            .resolve_subclass()
            .map_err(|_| error::unexpected(*self).named("PCI device subclass"))?;
        Ok(crate::Classes { class, subclass })
    }

    pub fn resolve_class(&self) -> Result<Class, error::UnexpectedValue<u8>> {
        pci_ids::Class::from_id(self.class)
            .ok_or_else(|| error::unexpected(self.class).named("PCI device class"))
            .map(Class)
    }

    pub fn resolve_subclass(&self) -> Result<Subclass, error::UnexpectedValue<u8>> {
        pci_ids::Subclass::from_cid_sid(self.class, self.subclass)
            .ok_or_else(|| error::unexpected(self.subclass).named("PCI device subclass"))
            .map(Subclass)
    }
}

impl fmt::LowerHex for RawClasses {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { class, subclass } = self;
        // should the formatted ID be prefaced with a leading `0x`?
        let leading = if f.alternate() { "0x" } else { "" };
        write!(f, "{leading}{class:x}:{subclass:x}")
    }
}

// macro_rules! replace_tt {
//     ($old:tt $new:tt) => {
//         $new
//     };
// }

// macro_rules! class_enum {
//     (
//         $(#[$m:meta])*
//         $v:vis enum $name:ident<NoProgIf> {
//             $(
//                 $(#[$($mm:tt)*])*
//                 $variant:ident = $value:expr
//             ),+
//             $(,)?
//         }
//     ) => {
//         class_enum! {
//             $(#[$m])*
//             $v enum $name {
//                 $(
//                     $(#[$($mm)*])*
//                     $variant = $value
//                 ),+
//             }
//         }

//         impl TryFrom<(u8, u8)> for $name {
//             type Error = error::UnexpectedValue<u8>;
//             fn try_from((u, rest): (u8, u8)) -> Result<Self, Self::Error> {
//                 if rest != 0 {
//                     return Err(error::unexpected(rest));
//                 }

//                 Self::try_from(u)
//             }
//         }
//     };

//     (
//         $(#[$m:meta])*
//         $v:vis enum $name:ident<$kind:ident, $rest:ty> {
//             $(
//                 $(#[$($mm:tt)*])*
//                 $variant:ident $(($next:ty))? = $value:expr
//             ),+
//             $(,)?
//         }
//     ) => {
//         $(#[$m])*
//         #[derive(Copy, Clone, Debug, PartialEq, Eq, Ord, PartialOrd)]
//         $v enum $name {
//             $(
//                 $(#[$($mm)*])*
//                 $variant $( ($next) )?
//             ),+
//         }

//         impl TryFrom<(u8, $rest)> for $name {
//             type Error = error::UnexpectedValue<u8>;
//             fn try_from((u, rest): (u8, $rest)) -> Result<Self, Self::Error> {
//                 match $kind::try_from(u)? {
//                     $(
//                         $kind::$variant => Ok($name::$variant $((<$next>::try_from(rest)?) )?)
//                     ),+
//                 }
//             }
//         }

//         impl fmt::Display for $name {
//             fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
//                 match self {
//                     $(
//                         $name::$variant $((replace_tt!($next next)))? => {
//                             fmt::Display::fmt(&$kind::$variant, f)?;
//                             $(next_display_helper(f, replace_tt!($next next))?;)?
//                         }
//                     ),*
//                 }
//                 Ok(())
//             }
//         }

//         class_enum!{
//             enum $kind {
//                 $(
//                     $(#[$($mm)*])*
//                     $variant = $value
//                 ),+
//             }
//         }
//     };

//     (
//         $(#[$m:meta])*
//         $v:vis enum $name:ident {
//             $(
//                 #[doc = $doc:expr]
//                 $(#[$mm:meta])*
//                 $variant:ident = $value:expr
//             ),+
//             $(,)?
//         }
//     ) => {
//         $(#[$m])*
//         #[derive(Copy, Clone, Debug, PartialEq, Eq, Ord, PartialOrd)]
//         #[repr(u8)]
//         $v enum $name {
//             $(
//                 #[doc = $doc]
//                 $(#[$mm])*
//                 $variant = $value
//             ),+
//         }

//         impl TryFrom<u8> for $name {
//             type Error = error::UnexpectedValue<u8>;
//             fn try_from(num: u8) -> Result<Self, Self::Error> {
//                 match num {
//                     $(
//                         $value => Ok($name::$variant),
//                     )+
//                     num => Err(error::unexpected(num)),
//                 }
//             }
//         }

//         impl fmt::Display for $name {
//             fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
//                 match self {
//                     $(
//                         $name::$variant => f.write_str($doc.trim())
//                     ),*
//                 }
//             }
//         }
//     };
// }

// fn next_display_helper(f: &mut fmt::Formatter, next: &impl fmt::Display) -> fmt::Result {
//     // Precision of `0` means we're done, no precision implies infinite precision.
//     match f.precision() {
//         Some(0) => Ok(()),
//         Some(precision) => write!(f, ": {:.*}", precision - 1, next),
//         None => write!(f, ": {}", next),
//     }
// }

// class_enum! {
//     pub enum Class<ClassValue, (u8, u8)> {
//         /// Unclassified
//         Unclassified(Unclassified) = 0x00,
//         /// Mass Storage
//         MassStorage(MassStorage) = 0x01,
//         /// Network
//         Network(Network) = 0x02,
//         /// Display
//         Display(Display) = 0x03,
//         /// Multimedia
//         Multimedia(Multimedia) = 0x04,
//         /// Memory Controller
//         Memory = 0x05,
//         /// Bridge Device
//         Bridge = 0x06,
//         /// Simple Communication Controller
//         SimpleComm = 0x07,
//         /// Base System Peripheral
//         BaseSystemPeripheral = 0x08,
//         /// Input Device Controller
//         Input = 0x09,
//         /// Docking Station
//         DockingStation = 0x0A,
//         /// Processor
//         Processor = 0x0B,
//         /// Serial Bus Controller
//         SerialBus = 0x0C,
//         /// Wireless Controller
//         Wireless = 0x0D,
//         /// Intelligent Controller
//         Intelligent = 0x0E,
//         /// Satellite Communication Controller
//         SatelliteComm = 0x0F,
//         /// Encryption Controller
//         Encryption = 0x10,
//         /// Signal Processing Controller
//         SignalProcessing = 0x11,
//         /// Processing Accelerator
//         ProcessingAccelerator = 0x12,
//         /// Non-Essential Instrumentation
//         NonEssentialInstrumentation = 0x13
//     }
// }

// class_enum! {
//     pub enum Unclassified<NoProgIf> {
//         /// Non-VGA-Compatible Device
//         NonVga = 0x00,
//         /// VGA-Compatible Device
//         Vga = 0x01
//     }
// }

// class_enum! {
//     pub enum MassStorage<MassStorageKind, u8> {
//         /// SCSI Bus Controller
//         ScsiBus = 0x00,
//         /// IDE Controller
//         Ide(iface::Ide) = 0x01,
//         /// Floppy Disk Controller
//         Floppy = 0x02,
//         /// IPI Bus Controller
//         IpiBus = 0x03,
//         /// RAID Controller
//         Raid = 0x04,
//         /// ATA Controller
//         Ata(iface::Ata) = 0x05,
//         /// Serial ATA Controller
//         Sata(iface::Sata) = 0x06,
//         /// Serial Attached SCSI
//         SerialAttachedScsi(iface::SerialAttachedScsi) = 0x07,
//         /// Non-Volatile Memory Controller
//         NonVolatileMem(iface::Nvm) = 0x08,
//         /// Other
//         Other = 0x80
//     }
// }

// class_enum! {
//     pub enum Network<NoProgIf> {
//         /// Ethernet Controller
//         Ethernet = 0x00,
//         /// Token Ring Controller
//         TokenRing = 0x01,
//         /// FDDI Controller
//         Fddi = 0x02,
//         /// ATM Controller
//         Atm = 0x03,
//         /// ISDN Controller
//         Isdn = 0x04,
//         /// WorldFlip Controller
//         WorldFip = 0x05,
//         /// PICMG 2.14 Multi Computing
//         Picmig2_14 = 0x06,
//         /// Infiniband Controller
//         Infiniband = 0x07,
//         /// Fabric Controller
//         Fabric = 0x08,
//         /// Other
//         Other = 0x80,
//     }
// }

// class_enum! {
//     pub enum Display<DisplayValue, u8> {
//         /// VGA Compatible
//         VgaCompatible(iface::VgaCompatible) = 0x00,
//         /// XGA Controller
//         Xga = 0x01,
//         /// 3D Controller (Not VGA-Compatible)
//         ThreeD = 0x02,
//         /// Other
//         Other = 0x80,
//     }
// }

// class_enum! {
//     pub enum Multimedia<NoProgIf> {
//         /// Multimedia Video Controller
//         MultimediaVideo = 0x00,
//         /// Multimedia Audio Controller
//         MultimediaAudio = 0x01,
//         /// Computer Telephony Device
//         ComputerTelephony = 0x02,
//         /// Audio Device
//         Audio = 0x03,
//         /// Other
//         Other = 0x80,
//     }
// }

// class_enum! {
//     pub enum Memory<NoProgIf> {
//         /// RAM Controller
//         Ram = 0x00,
//         /// Flash Controller
//         Flash = 0x01,
//         /// Other
//         Other = 0x80,
//     }
// }

// impl TryFrom<(RawClasses, u8)> for Class {
//     type Error = error::UnexpectedValue<u8>;
//     fn try_from(
//         (RawClasses { class, subclass }, prog_if): (RawClasses, u8),
//     ) -> Result<Self, Self::Error> {
//         Self::try_from((class, (subclass, prog_if)))
//     }
// }

// pub mod iface {
//     use super::*;

//     #[derive(Debug, Eq, PartialEq, Copy, Clone, Ord, PartialOrd)]
//     #[repr(transparent)]
//     pub struct Ide(u8);

//     impl Ide {
//         const PCI_NATIVE: u8 = 0b0101;
//         const SWITCHABLE: u8 = 0b1010;
//         const BUS_MASTERING: u8 = 0x8;

//         pub fn supports_bus_mastering(&self) -> bool {
//             self.0 & Self::BUS_MASTERING == Self::BUS_MASTERING
//         }

//         pub fn is_switchable(&self) -> bool {
//             self.0 & Self::SWITCHABLE == Self::SWITCHABLE
//         }

//         pub fn is_isa_native(&self) -> bool {
//             !self.is_pci_native()
//         }

//         pub fn is_pci_native(&self) -> bool {
//             self.0 & Self::PCI_NATIVE == Self::PCI_NATIVE
//         }
//     }

//     impl TryFrom<u8> for Ide {
//         type Error = error::UnexpectedValue<u8>;
//         fn try_from(u: u8) -> Result<Self, Self::Error> {
//             if u > 0x8f {
//                 return Err(error::unexpected(u));
//             }
//             Ok(Self(u))
//         }
//     }

//     impl fmt::Display for Ide {
//         fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
//             let mode = match (self.is_pci_native(), self.is_switchable()) {
//                 (false, false) => "ISA compatibility mode-only",
//                 (false, true) => {
//                     "ISA compatibility mode, supports both channels switched to PCI native mode"
//                 }
//                 (true, false) => "PCI native mode-only",
//                 (true, true) => {
//                     "PCI native mode, supports both channels switched to ISA compatibility mode"
//                 }
//             };

//             if self.supports_bus_mastering() {
//                 write!(f, "{}, supports bus mastering", mode)?;
//             } else {
//                 write!(f, "{}", mode)?;
//             }
//             Ok(())
//         }
//     }

//     class_enum! {
//         pub enum Ata {
//             /// Single DMA
//             SingleDma = 0x20,
//             /// Chained DMA
//             ChainedDma = 0x30,
//         }
//     }

//     class_enum! {
//         pub enum Sata {
//             /// Vendor Specific Interface
//             VendorSpecific = 0x00,
//             /// AHCI 1.0
//             Achi1 = 0x01,
//             /// Serial Storage Bus
//             SerialStorageBus = 0x02,
//         }
//     }

//     class_enum! {
//         pub enum SerialAttachedScsi {
//             /// SAS
//             Sas = 0x00,
//             /// Serial Storage Bus
//             SerialStorageBus = 0x02,
//         }
//     }

//     class_enum! {
//         pub enum Nvm {
//             /// NVMHCI
//             Nvmhci = 0x01,
//             /// NVM Express
//             NvmExpress = 0x02,
//         }
//     }

//     class_enum! {
//         pub enum VgaCompatible {
//             /// VGA Controller
//             VgaController = 0x00,
//             /// 8514-Compatible Controller
//             Compat8514 = 0x01,
//         }
//     }
// }

// #[cfg(test)]
// mod test {
//     use super::*;

//     #[test]
//     fn test_parsing() {
//         let mass_storage_sata_achi = (
//             RawClasses {
//                 class: 0x01,
//                 subclass: 0x06,
//             },
//             0x01,
//         );
//         let class = Class::try_from(mass_storage_sata_achi);
//         assert_eq!(
//             class,
//             Ok(Class::MassStorage(MassStorage::Sata(iface::Sata::Achi1))),
//         );
//     }

//     #[test]
//     fn test_display() {
//         assert_eq!(
//             Class::MassStorage(MassStorage::Sata(iface::Sata::Achi1)).to_string(),
//             "Mass Storage: Serial ATA Controller: AHCI 1.0"
//         );

//         let ide_iface = iface::Ide::try_from(0x8F).unwrap();
//         assert_eq!(
//             Class::MassStorage(MassStorage::Ide(ide_iface)).to_string(),
//             "Mass Storage: IDE Controller: PCI native mode, supports both channels switched to ISA compatibility mode, supports bus mastering"
//         );
//         assert_eq!(
//             format!("{:.1}", Class::MassStorage(MassStorage::Ide(ide_iface))),
//             "Mass Storage: IDE Controller"
//         );
//     }
// }
