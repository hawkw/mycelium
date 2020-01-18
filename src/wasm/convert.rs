use core::fmt;

#[derive(Debug, Copy, Clone)]
pub struct ConvertError;
impl fmt::Display for ConvertError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "conversion error")
    }
}
impl From<ConvertError> for wasmi::Trap {
    fn from(_: ConvertError) -> wasmi::Trap {
        wasmi::TrapKind::UnexpectedSignature.into()
    }
}

/// Trait describing a type which can be used in wasm method signatures.
// XXX: Should this require `wasmi::FromRuntimeValue` and
// `Into<wasmi::RuntimeValue>`?
pub trait WasmPrimitive: Sized {
    const TYPE: wasmi::ValueType;

    fn from_wasm_value(value: wasmi::RuntimeValue) -> Result<Self, ConvertError>;
    fn into_wasm_value(self) -> wasmi::RuntimeValue;
}

macro_rules! impl_wasm_primitive {
    ($($rust:ty = $wasm:ident;)*) => {
        $(
            impl WasmPrimitive for $rust {
                const TYPE: wasmi::ValueType = wasmi::ValueType::$wasm;

                fn from_wasm_value(value: wasmi::RuntimeValue) -> Result<Self, ConvertError> {
                    value.try_into().ok_or(ConvertError)
                }

                fn into_wasm_value(self) -> wasmi::RuntimeValue {
                    self.into()
                }
            }
        )*
    }
}

impl_wasm_primitive! {
    i8 = I32;
    i16 = I32;
    i32 = I32;
    i64 = I64;
    u8 = I32;
    u16 = I32;
    u32 = I32;
    u64 = I64;
    wasmi::nan_preserving_float::F32 = F32;
    wasmi::nan_preserving_float::F64 = F64;
}
