use core::fmt;
use mycelium_util::error::Error;

pub(crate) fn unexpected<T>(value: T) -> UnexpectedValue<T>
where
    T: fmt::LowerHex + fmt::Debug,
{
    UnexpectedValue { value }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnexpectedValue<T> {
    value: T,
}

impl<T> fmt::Display for UnexpectedValue<T>
where
    T: fmt::LowerHex + fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unexpected `{}` value: {:#x}",
            core::any::type_name::<T>(),
            self.value
        )
    }
}

impl<T> Error for UnexpectedValue<T> where T: fmt::LowerHex + fmt::Debug {}
