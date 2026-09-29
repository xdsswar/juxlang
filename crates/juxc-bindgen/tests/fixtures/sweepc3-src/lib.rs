//! A crate whose type has associated functions and constants only through
//! trait impls: its own trait, and the standard library's construction and
//! conversion traits (ERRATA E1XX-SWEEPC3).

/// A crate trait with an associated function and an associated constant.
pub trait Tick {
    /// How far one tick moves.
    const STEP: u32;
    /// Where counting starts.
    fn origin() -> Self;
    /// The next position.
    fn next(&self) -> Self;
}

/// A counter.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Clock {
    t: u32,
}

impl Clock {
    /// A clock at `t`.
    pub fn at(t: u32) -> Clock {
        Clock { t }
    }

    /// Where it is.
    pub fn now(&self) -> u32 {
        self.t
    }
}

impl Tick for Clock {
    const STEP: u32 = 5;

    fn origin() -> Clock {
        Clock { t: 100 }
    }

    fn next(&self) -> Clock {
        Clock { t: self.t + Self::STEP }
    }
}

impl std::str::FromStr for Clock {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> Result<Clock, Self::Err> {
        Ok(Clock { t: s.trim().parse()? })
    }
}

impl From<u32> for Clock {
    fn from(t: u32) -> Clock {
        Clock { t }
    }
}

impl From<bool> for Clock {
    fn from(b: bool) -> Clock {
        Clock { t: u32::from(b) }
    }
}
