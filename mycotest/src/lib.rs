//! Shared defs between the kernel and the test runner.
//!
//! This is in its own crate mainly so the constants are the same and I can't
//! have the kernel write the wrong strings (which I did lol).

#![no_std]
pub const TEST_COUNT: &str = "MYCELIUM_TEST_COUNT:";
pub const START_TEST: &str = "MYCELIUM_TEST_START:";
pub const FAIL_TEST: &str = "MYCELIUM_TEST_FAIL:";
pub const PASS_TEST: &str = "MYCELIUM_TEST_PASS:";

#[cfg(feature = "alloc")]
extern crate alloc;

use core::{cmp, fmt, marker::PhantomData};
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Test<'a, S = &'a str> {
    name: S,
    module: S,
    _lt: PhantomData<&'a str>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Outcome {
    Pass,
    Fail,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Failure {
    Fail,
    Panic,
    Fault,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ParseError(&'static str);

impl<'a, S> Test<'a, S>
where
    S: AsRef<str>,
{
    pub fn name(&self) -> &str {
        self.name.as_ref()
    }

    pub fn module(&self) -> &str {
        self.module.as_ref()
    }
}

impl<'a> Test<'a> {
    pub fn parse_start(line: &'a str) -> Option<Self> {
        Self::parse(line.strip_prefix(START_TEST)?)
    }

    pub fn parse_outcome(line: &'a str) -> Option<(Self, Result<(), Failure>)> {
        let line = line.strip_prefix("MYCELIUM_TEST_")?;
        let (line, result) = if let Some(line) = line.strip_prefix("PASS:") {
            (line, Ok(()))
        } else if let Some(line) = line.strip_prefix("FAIL:") {
            let failure = line.parse::<Failure>().ok()?;
            let line = line.strip_prefix(failure.as_str())?;
            (line, Err(failure))
        } else {
            return None;
        };

        Some((Self::parse(line)?, result))
    }

    #[cfg(feature = "alloc")]
    pub fn to_static(self) -> Test<'static, alloc::string::String> {
        use alloc::borrow::ToOwned;
        Test {
            name: self.name.to_owned(),
            module: self.module.to_owned(),
            _lt: PhantomData,
        }
    }

    fn parse(line: &'a str) -> Option<Self> {
        let mut line = line.trim().split_whitespace();
        let module = line.next()?;
        let name = line.next()?;
        Some(Self {
            name,
            module,
            _lt: PhantomData,
        })
    }
}

impl<S: Ord> cmp::PartialOrd for Test<'_, S> {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<S: Ord> cmp::Ord for Test<'_, S> {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        self.module
            .cmp(&other.module)
            .then_with(|| self.name.cmp(&other.name))
    }
}

impl<S: fmt::Display> fmt::Display for Test<'_, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}::{}", self.module, self.name)
    }
}

// === impl Failure ===

impl core::str::FromStr for Failure {
    type Err = ParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim() {
            s if s.eq_ignore_ascii_case("panic") => Ok(Self::Panic),
            s if s.eq_ignore_ascii_case("fail") => Ok(Self::Fail),
            s if s.eq_ignore_ascii_case("fault") => Ok(Self::Fault),
            _ => Err(ParseError(
                "invalid failure kind: expected one of `panic`, `fail`, or `fault`",
            )),
        }
    }
}

impl Failure {
    pub fn as_str(&self) -> &'static str {
        match self {
            Failure::Fail => "fail",
            Failure::Fault => "fault",
            Failure::Panic => "panic",
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(self.as_str())
    }
}
