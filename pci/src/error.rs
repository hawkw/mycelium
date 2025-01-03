use core::fmt;

pub(crate) fn unexpected<T>(value: T) -> UnexpectedValue<T>
where
    T: fmt::LowerHex + fmt::Debug,
{
    UnexpectedValue { value, name: None }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnexpectedValue<T> {
    value: T,
    name: Option<&'static str>,
}

impl<T> UnexpectedValue<T> {
    pub(crate) fn named(self, name: &'static str) -> Self {
        Self {
            name: Some(name),
            ..self
        }
    }
}

impl<T> fmt::Display for UnexpectedValue<T>
where
    T: fmt::LowerHex + fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ty = core::any::type_name::<T>();
        match self {
            Self {
                value,
                name: Some(name),
            } => write!(f, "unexpected {ty} value for {name}: {value:#x}",),
            Self { value, name: None } => write!(f, "unexpected {ty} value: {value:#x}",),
        }
    }
}

impl<T> core::error::Error for UnexpectedValue<T> where T: fmt::LowerHex + fmt::Debug {}
