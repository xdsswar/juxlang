//! The crate-boundary sweep's fixture (ERRATA E1XX-SWEEPB), in naga's shape:
//! ONE crate declaring several items of one simple name in different modules
//! (`front::wgsl::Error`, `front::spv::Error`, `back::spv::Error`, the trait
//! `de::Error`, and two `parse` functions), a type whose `From` sources share
//! a simple name, and error types that a Jux program should see as the
//! closest Jux exception, classified by what each one is and implements.

use std::fmt;
use std::str::FromStr;

/// A shader module.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Module {
    pub name: String,
}

impl Module {
    /// A signature naming the WGSL front end's error exactly.
    pub fn describe_parse_error(error: &front::wgsl::Error) -> String {
        format!("{} at line {}", error.message(), error.line())
    }

    /// A signature naming the SPIR-V back end's error exactly.
    pub fn is_capability_error(error: &back::spv::Error) -> bool {
        matches!(error, back::spv::Error::UnsupportedCapability(_))
    }
}

pub mod front {
    pub mod wgsl {
        use std::fmt;

        /// A WGSL parse failure.
        #[derive(Clone, Debug, PartialEq)]
        pub struct Error {
            line: u32,
            message: String,
        }

        impl Error {
            pub fn new(line: u32, message: &str) -> Error {
                Error { line, message: message.to_string() }
            }
            pub fn line(&self) -> u32 {
                self.line
            }
            pub fn message(&self) -> String {
                self.message.clone()
            }
        }

        impl fmt::Display for Error {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{} (line {})", self.message, self.line)
            }
        }

        impl std::error::Error for Error {}

        /// Parse WGSL source: a `!` anywhere is an unexpected token.
        pub fn parse(source: &str) -> Result<crate::Module, Error> {
            match source.find('!') {
                Some(at) => Err(Error::new(1 + source[..at].matches('\n').count() as u32, "unexpected token")),
                None => Ok(crate::Module { name: source.trim().to_string() }),
            }
        }
    }

    pub mod spv {
        use std::fmt;

        /// A SPIR-V parse failure.
        #[derive(Clone, Debug, PartialEq)]
        pub enum Error {
            BadMagic(u32),
            Truncated,
        }

        impl fmt::Display for Error {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                match self {
                    Error::BadMagic(m) => write!(f, "bad magic number {m:#x}"),
                    Error::Truncated => write!(f, "the module is truncated"),
                }
            }
        }

        impl std::error::Error for Error {}

        /// Parse SPIR-V words: the first must be the magic number.
        pub fn parse(words: Vec<u32>) -> Result<crate::Module, Error> {
            match words.first() {
                None => Err(Error::Truncated),
                Some(&0x0723_0203) => Ok(crate::Module { name: format!("spv:{}", words.len()) }),
                Some(&other) => Err(Error::BadMagic(other)),
            }
        }
    }
}

pub mod back {
    pub mod spv {
        use std::fmt;

        /// A SPIR-V writer failure.
        #[derive(Debug)]
        pub enum Error {
            /// A capability the target lacks: no Jux exception is closer.
            UnsupportedCapability(String),
            /// An entry point the module does not have.
            EntryPointNotFound(String),
            /// Writing the output failed.
            Io(std::io::Error),
            /// The writer ran out of time.
            TimedOut,
        }

        impl From<std::io::Error> for Error {
            fn from(e: std::io::Error) -> Error {
                Error::Io(e)
            }
        }

        impl fmt::Display for Error {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                match self {
                    Error::UnsupportedCapability(c) => write!(f, "capability {c} is not supported"),
                    Error::EntryPointNotFound(e) => write!(f, "no entry point named {e}"),
                    Error::Io(e) => write!(f, "writing failed: {e}"),
                    Error::TimedOut => write!(f, "the writer timed out"),
                }
            }
        }

        impl std::error::Error for Error {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                match self {
                    Error::Io(e) => Some(e),
                    _ => None,
                }
            }
        }

        /// Write a module for one entry point with one capability.
        pub fn write(module: &crate::Module, entry: &str, capability: &str) -> Result<String, Error> {
            match (entry, capability) {
                ("missing", _) => Err(Error::EntryPointNotFound(entry.to_string())),
                ("slow", _) => Err(Error::TimedOut),
                (_, "f64") => Err(Error::UnsupportedCapability(capability.to_string())),
                (_, "locked") => Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "output locked").into()),
                _ => Ok(format!("{}:{entry}:{capability}", module.name)),
            }
        }
    }
}

/// Anything a front end rejects, in one type. Its two `From` sources share
/// the simple name `Error`, so what converts into it has to say which is which.
#[derive(Clone, Debug)]
pub struct Diagnostic {
    text: String,
}

impl Diagnostic {
    pub fn text(&self) -> String {
        self.text.clone()
    }
}

impl From<front::wgsl::Error> for Diagnostic {
    fn from(e: front::wgsl::Error) -> Diagnostic {
        Diagnostic { text: format!("wgsl: {e}") }
    }
}

impl From<front::spv::Error> for Diagnostic {
    fn from(e: front::spv::Error) -> Diagnostic {
        Diagnostic { text: format!("spv: {e}") }
    }
}

/// Report anything that converts into a `Diagnostic`.
pub fn report(diagnostic: impl Into<Diagnostic>) -> String {
    diagnostic.into().text
}

pub mod de {
    /// An error a deserializer can make up, in serde's shape.
    pub trait Error: Sized + std::error::Error {
        fn custom(message: &str) -> Self;
    }
}

/// Input that is not in the format asked for (serde's shape: it implements
/// `de::Error`).
#[derive(Debug)]
pub struct DataError {
    message: String,
}

impl fmt::Display for DataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for DataError {}

impl de::Error for DataError {
    fn custom(message: &str) -> DataError {
        DataError { message: message.to_string() }
    }
}

/// Decode a module from `name:<text>`.
pub fn decode(text: &str) -> Result<Module, DataError> {
    match text.strip_prefix("name:") {
        Some(name) => Ok(Module { name: name.to_string() }),
        None => Err(<DataError as de::Error>::custom("expected `name:` at column 1")),
    }
}

/// A settings file that could not be read (it wraps and `From`s an
/// `io::Error`).
#[derive(Debug)]
pub struct ConfigError {
    inner: std::io::Error,
}

impl From<std::io::Error> for ConfigError {
    fn from(inner: std::io::Error) -> ConfigError {
        ConfigError { inner }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cannot load the settings: {}", self.inner)
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.inner)
    }
}

/// Read a settings file.
pub fn load_config(path: &str) -> Result<String, ConfigError> {
    Ok(std::fs::read_to_string(path)?)
}

/// A version number, `major.minor`.
#[derive(Clone, Debug, PartialEq)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
}

/// Text that is not a version (a `FromStr` error of a type that is not a number).
#[derive(Debug)]
pub struct VersionError {
    text: String,
}

impl fmt::Display for VersionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "`{}` is not a version", self.text)
    }
}

impl std::error::Error for VersionError {}

impl FromStr for Version {
    type Err = VersionError;
    fn from_str(s: &str) -> Result<Version, VersionError> {
        let bad = || VersionError { text: s.to_string() };
        let (a, b) = s.split_once('.').ok_or_else(bad)?;
        Ok(Version { major: a.parse().map_err(|_| bad())?, minor: b.parse().map_err(|_| bad())? })
    }
}

/// Parse a version.
pub fn parse_version(text: &str) -> Result<Version, VersionError> {
    text.parse()
}

/// A fixed-point decimal number.
#[derive(Clone, Debug, PartialEq)]
pub struct Decimal {
    pub hundredths: i64,
}

/// Text that is not a decimal number (the `FromStr` error of a number type).
#[derive(Debug)]
pub struct DecimalError;

impl fmt::Display for DecimalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid decimal literal")
    }
}

impl std::error::Error for DecimalError {}

impl FromStr for Decimal {
    type Err = DecimalError;
    fn from_str(s: &str) -> Result<Decimal, DecimalError> {
        let v: f64 = s.parse().map_err(|_| DecimalError)?;
        Ok(Decimal { hundredths: (v * 100.0).round() as i64 })
    }
}

/// Parse a decimal.
pub fn parse_decimal(text: &str) -> Result<Decimal, DecimalError> {
    text.parse()
}

/// A deadline that passed (tokio's name for a timeout).
#[derive(Debug)]
pub struct Elapsed;

impl fmt::Display for Elapsed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "deadline has elapsed")
    }
}

impl std::error::Error for Elapsed {}

/// Count to `ticks` within `budget`.
pub fn wait_for(ticks: u32, budget: u32) -> Result<u32, Elapsed> {
    if ticks > budget {
        Err(Elapsed)
    } else {
        Ok(ticks)
    }
}

/// A failed lookup: one variant is a missing key.
#[derive(Debug)]
pub enum LookupError {
    KeyNotFound(String),
    Poisoned,
}

impl fmt::Display for LookupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LookupError::KeyNotFound(k) => write!(f, "no key {k}"),
            LookupError::Poisoned => write!(f, "the table is poisoned"),
        }
    }
}

impl std::error::Error for LookupError {}

/// Look a key up.
pub fn lookup(key: &str) -> Result<i32, LookupError> {
    match key {
        "one" => Ok(1),
        "poison" => Err(LookupError::Poisoned),
        _ => Err(LookupError::KeyNotFound(key.to_string())),
    }
}

/// An error with nothing about it a Jux exception matches.
#[derive(Debug)]
pub struct Refusal;

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the widget refused")
    }
}

impl std::error::Error for Refusal {}

/// Always refuses.
pub fn refuse() -> Result<(), Refusal> {
    Err(Refusal)
}
