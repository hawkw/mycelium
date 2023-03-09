use crate::segment;
use alloc::{boxed::Box, vec::Vec};
use core::fmt;

pub const MAX_CPUS: usize = 256;

pub type Id = usize;

#[derive(Debug)]
pub struct Topology {
    boot_processor: Processor,
    application_processors: Vec<Processor>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
#[non_exhaustive]
pub struct Processor {
    pub id: Id,
    pub device_uid: u32,
    pub lapic_id: u32,
    pub is_boot_processor: bool,
    pub state: State,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TopologyError {
    NoTopology,
    Weird(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Running,
    Idle,
    FirmwareDisabled,
    Error(&'static str),
}

impl Topology {
    // TODO(eliza): good error type
    #[tracing::instrument(name = "Topology::from_acpi", skip(acpi), err(Display))]
    pub fn from_acpi(acpi: &acpi::PlatformInfo) -> Result<Self, TopologyError> {
        use acpi::platform;

        let platform::ProcessorInfo {
            ref application_processors,
            ref boot_processor,
        } = acpi
            .processor_info
            .as_ref()
            .ok_or(TopologyError::NoTopology)?;

        let bsp = Processor {
            id: 0,
            device_uid: boot_processor.processor_uid,
            lapic_id: boot_processor.local_apic_id,
            is_boot_processor: true,
            state: State::Idle,
        };

        if boot_processor.is_ap {
            return Err(TopologyError::Weird(
                "boot processor claimed to be an application processor",
            ))?;
        }

        if boot_processor.state != platform::ProcessorState::Running {
            return Err(TopologyError::Weird(
                "boot processor claimed to not be running",
            ))?;
        }

        tracing::info!(
            bsp.id,
            bsp.device_uid,
            bsp.lapic_id,
            "boot processor seems normalish"
        );

        let mut id = 1;
        let mut disabled = 0;
        let mut errors = 0;
        let mut aps = Vec::with_capacity(application_processors.len());
        for ap in application_processors {
            if !ap.is_ap {
                return Err(TopologyError::Weird(
                    "application processor claimed to be the boot processor",
                ))?;
            }

            let state = match ap.state {
                // if the firmware disabled a processor, just skip it
                platform::ProcessorState::Disabled => {
                    tracing::warn!(
                        ap.device_uid = ap.processor_uid,
                        "application processor disabled by firmware, skipping it"
                    );
                    disabled += 1;
                    State::FirmwareDisabled
                }
                // if a processor claims it's already running, that seems messed up!
                platform::ProcessorState::Running => {
                    errors += 1;
                    State::Error("application processor claimed to be running before SIPI!")
                }
                // otherwise, add it to the topology
                platform::ProcessorState::WaitingForSipi => State::Idle,
            };

            let ap = Processor {
                id,
                device_uid: ap.processor_uid,
                lapic_id: ap.local_apic_id,
                is_boot_processor: false,
                state,
            };
            tracing::debug!(
                ap.id,
                ap.device_uid,
                ap.lapic_id,
                ?ap.state,
                "found application processor"
            );

            aps.push(ap);
            id += 1;
        }

        tracing::info!(
            "found {} application processors ({disabled} disabled, {errors} errors)",
            application_processors.len(),
        );

        Ok(Self {
            application_processors: aps,
            boot_processor: bsp,
        })
    }

    pub fn init_boot_processor(&mut self, gdt: &mut segment::Gdt) {
        self.boot_processor.init_processor(gdt);
    }

    pub fn by_id(&self, id: Id) -> Option<&Processor> {
        if id == 0 {
            Some(&self.boot_processor)
        } else {
            self.application_processors.get(id - 1)
        }
    }

    pub fn total_cpus(&self) -> usize {
        self.application_processors.len() + 1
    }

    pub fn running_cpus(&self) -> usize {
        self.all_cpus()
            .filter(|p| p.state == State::Running)
            .count()
    }

    pub fn by_device_uid(&self, uid: u32) -> Option<&Processor> {
        self.all_cpus().find(|p| p.device_uid == uid as u32)
    }

    pub fn by_local_apic_id(&self, lapic_id: u32) -> Option<&Processor> {
        self.all_cpus().find(|p| p.lapic_id == lapic_id as u32)
    }

    pub fn boot_cpu(&self) -> &Processor {
        &self.boot_processor
    }

    pub fn application_cpus(&self) -> &[Processor] {
        &&self.application_processors[..]
    }

    pub fn all_cpus(&self) -> impl Iterator<Item = &Processor> {
        core::iter::once(&self.boot_processor).chain(self.application_processors.iter())
    }
}

impl Processor {
    pub(crate) fn init_processor(&mut self, gdt: &mut segment::Gdt) {
        tracing::info!(self.id, ?self.state, "initializing processor");
        assert_eq!(self.state, State::Idle);

        use super::local::GsLocalData;
        Box::pin(GsLocalData::new(self.clone())).init();
        self.state = State::Running;
    }
}

impl fmt::Display for TopologyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TopologyError::NoTopology => f.pad("no topology information found in MADT"),
            TopologyError::Weird(msg) => {
                write!(f, "found something weird: {msg}, is the MADT corrupted?")
            }
        }
    }
}
