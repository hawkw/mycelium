use alloc::borrow::ToOwned;
use core::convert::TryFrom;
use core::fmt;

mod wasi;
mod convert;

use self::convert::WasmPrimitive;

macro_rules! option_helper {
    (Some $rt:expr) => { Some($rt) };
    (Some) => { None };
}

#[derive(Debug)]
pub struct Host {
    pub module: wasmi::ModuleRef,
    // FIXME: The wasmi crate currently provides no way to determine the
    // caller's module or memory from a host function. This effectively means
    // that if a module imports and calls a function from another module, and
    // that module calls a host function, the memory region of the first module
    // will be used.
    //
    // We will need to either modify the wasmi interpreter and host function API
    // to pass in function context when calling host methods, or use host
    // function trampolines which change the `Host` value, to avoid this issue.
    pub memory: wasmi::MemoryRef,
}

impl Host {
    // Create a new host for the given instance.
    // NOTE: The instance may not have been started yet.
    fn new(instance: &wasmi::ModuleRef) -> Result<Self, wasmi::Error> {
        let memory = match instance.export_by_name("memory") {
            Some(wasmi::ExternVal::Memory(memory)) => memory,
            _ => return Err(wasmi::Error::Instantiation("required memory export".to_owned())),
        };

        Ok(Host { module: instance.clone(), memory })
    }
}

macro_rules! host_funcs {
    ($(
        fn $module:literal :: $name:literal ($($p:ident : $t:ident),*) $( -> $rt:ident)?
            as $variant:ident impl $method:path;
    )*) => {
        #[repr(usize)]
        #[derive(Copy, Clone, Debug, Eq, PartialEq)]
        enum HostFunc {
            $($variant),*
        }

        impl HostFunc {
            fn resolve_func(
                module_name: &str,
                field_name: &str,
                _signature: &wasmi::Signature,
            ) -> Result<HostFunc, wasmi::Error> {
                match (module_name, field_name) {
                    $(($module, $name) => Ok(HostFunc::$variant),)*
                    _ => Err(wasmi::Error::Instantiation("unresolved func import".to_owned()))
                }
            }

            fn signature(self) -> wasmi::Signature {
                match self {
                    $(
                        HostFunc::$variant => wasmi::Signature::new(
                            &[$(<$t as WasmPrimitive>::TYPE),*][..],
                            option_helper!(Some $(<$rt as WasmPrimitive>::TYPE)?),
                        )
                    ),*
                }
            }

            fn func_ref(self) -> wasmi::FuncRef {
                wasmi::FuncInstance::alloc_host(self.signature(), self as usize)
            }

            fn module_name(self) -> &'static str {
                match self {
                    $(HostFunc::$variant => $module),*
                }
            }

            fn field_name(self) -> &'static str {
                match self {
                    $(HostFunc::$variant => $name),*
                }
            }
        }

        impl TryFrom<usize> for HostFunc {
            type Error = wasmi::Trap;
            fn try_from(x: usize) -> Result<Self, Self::Error> {
                $(
                    if x == (HostFunc::$variant as usize) {
                        return Ok(HostFunc::$variant);
                    }
                )*
                Err(wasmi::TrapKind::UnexpectedSignature.into())
            }
        }

        impl fmt::Display for HostFunc {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "{}::{}", self.module_name(), self.field_name())
            }
        }

        impl wasmi::Externals for Host {
            fn invoke_index(
                &mut self,
                index: usize,
                args: wasmi::RuntimeArgs,
            ) -> Result<Option<wasmi::RuntimeValue>, wasmi::Trap> {
                let span = tracing::trace_span!("invoke_index", index, ?args);
                let _enter = span.enter();

                match HostFunc::try_from(index)? {
                    $(
                        HostFunc::$variant => match args.as_ref() {
                            [$($p),*] => {
                                let _result = $method(
                                    self,
                                    $(<$t as WasmPrimitive>::from_wasm_value(*$p)?),*
                                )?;
                                Ok(option_helper!(
                                    Some $(<$rt as WasmPrimitive>::into_wasm_value(_result))?
                                ))
                            }
                            _ => Err(wasmi::TrapKind::UnexpectedSignature.into()),
                        }
                    ),*
                }
            }
        }
    }
}

host_funcs! {
    fn "wasi_unstable"::"fd_write"(fd: u32, iovs: u32, iovs_len: u32, nwritten: u32) -> u16
        as FdWrite impl wasi::fd_write;
}

struct HostResolver;
impl wasmi::ImportResolver for HostResolver {
    fn resolve_func(
        &self,
        module_name: &str,
        field_name: &str,
        signature: &wasmi::Signature,
    ) -> Result<wasmi::FuncRef, wasmi::Error> {
        let host_fn = HostFunc::resolve_func(module_name, field_name, signature)?;
        Ok(host_fn.func_ref())
    }

    fn resolve_global(
        &self,
        module_name: &str,
        field_name: &str,
        descriptor: &wasmi::GlobalDescriptor,
    ) -> Result<wasmi::GlobalRef, wasmi::Error> {
        tracing::error!(module_name, field_name, ?descriptor, "unresolved global import");
        Err(wasmi::Error::Instantiation("unresolved global import".to_owned()))
    }

    fn resolve_memory(
        &self,
        module_name: &str,
        field_name: &str,
        descriptor: &wasmi::MemoryDescriptor,
    ) -> Result<wasmi::MemoryRef, wasmi::Error> {
        tracing::error!(module_name, field_name, ?descriptor, "unresolved memory import");
        Err(wasmi::Error::Instantiation("unresolved memory import".to_owned()))
    }

    fn resolve_table(
        &self,
        module_name: &str,
        field_name: &str,
        descriptor: &wasmi::TableDescriptor,
    ) -> Result<wasmi::TableRef, wasmi::Error> {
        tracing::error!(module_name, field_name, ?descriptor, "unresolved table import");
        Err(wasmi::Error::Instantiation("unresolved table import".to_owned()))
    }
}

pub fn run_wasm(binary: &[u8]) -> Result<(), wasmi::Error> {
    let module = wasmi::Module::from_buffer(binary)?;

    // Instantiate the module and it's corresponding `Host` instance.
    let instance = wasmi::ModuleInstance::new(&module, &HostResolver)?;
    let mut host = Host::new(instance.not_started_instance())?;
    let instance = instance.run_start(&mut host)?;

    // FIXME: We should probably use resumable calls here.
    instance.invoke_export("_start", &[], &mut host)?;
    Ok(())
}

