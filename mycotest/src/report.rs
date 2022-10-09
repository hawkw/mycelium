use crate::{Outcome, TestName};
use core::{fmt, str::FromStr};

pub const TEST_COUNT: &str = "MYCELIUM_TEST_COUNT:";
pub const START_TEST: &str = "MYCELIUM_TEST_START:";
pub const FAIL_TEST: &str = "MYCELIUM_TEST_FAIL:";
pub const PASS_TEST: &str = "MYCELIUM_TEST_PASS:";

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Failure {
    Fail,
    Panic,
    Fault,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ParseError(&'static str);

// === impl Failure ===

impl FromStr for Failure {
    type Err = ParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim() {
            s if s.starts_with("panic") => Ok(Self::Panic),
            s if s.starts_with("fail") => Ok(Self::Fail),
            s if s.starts_with("fault") => Ok(Self::Fault),
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

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.0, f)
    }
}

impl<'a> TestName<'a> {
    pub fn parse_start(line: &'a str) -> Option<Self> {
        Self::parse(line.strip_prefix(START_TEST)?)
    }

    #[tracing::instrument(level = "trace")]
    pub fn parse_outcome(line: &'a str) -> Result<Option<(Self, Outcome)>, ParseError> {
        let line = match line.strip_prefix("MYCELIUM_TEST_") {
            None => {
                tracing::trace!("not a test outcome");
                return Ok(None);
            }
            Some(line) => line,
        };
        tracing::trace!(?line);
        let (line, result) = if let Some(line) = line.strip_prefix("PASS:") {
            (line, Ok(()))
        } else if let Some(line) = line.strip_prefix("FAIL:") {
            let line = line.trim();
            tracing::trace!(?line);
            let failure = line.parse::<Failure>();
            tracing::trace!(?failure);
            let failure = failure?;
            let line = line.strip_prefix(failure.as_str()).unwrap_or(line);
            (line, Err(failure))
        } else {
            tracing::trace!("this is a test start, not an outcome");
            return Ok(None);
        };
        let test = Self::parse(line.trim()).ok_or(ParseError("failed to parse test"));
        tracing::trace!(?test);
        Ok(Some((test?, result)))
    }

    #[cfg(feature = "alloc")]
    pub fn to_static(self) -> TestName<'static, alloc::string::String> {
        use alloc::borrow::ToOwned;
        TestName::new(self.module.to_owned(), self.name.to_owned())
    }

    fn parse(line: &'a str) -> Option<Self> {
        let mut line = line.split_whitespace();
        let module = line.next()?;
        let name = line.next()?;
        Some(Self::new(module, name))
    }
}
