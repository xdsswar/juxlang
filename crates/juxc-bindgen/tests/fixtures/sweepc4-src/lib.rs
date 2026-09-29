//! Blanket impls of a crate's own traits, whose associated functions and
//! constants reach every type the bound covers, and `FromIterator`
//! (ERRATA E152).

/// A crate trait implemented directly.
pub trait Named {
    /// The value's name.
    fn name(&self) -> String;
}

/// Blanket over `Default + Clone`: decidable from a stub's derive facts.
pub trait Maker {
    /// A kind number.
    const KIND: u32;
    /// A fresh value.
    fn make_default() -> Self;
}

impl<T: Default + Clone> Maker for T {
    const KIND: u32 = 7;

    fn make_default() -> T {
        T::default()
    }
}

/// Blanket over one of the crate's own traits.
pub trait Labelled {
    /// A label for the type.
    fn label() -> String;
}

impl<T: Named> Labelled for T {
    fn label() -> String {
        "named".to_string()
    }
}

/// Blanket over `Send`, which no stub records: covers nothing in the stub.
pub trait Portable {
    /// Always one.
    fn portable() -> u32;
}

impl<T: Send> Portable for T {
    fn portable() -> u32 {
        1
    }
}

/// A counter: `Default + Clone`, `Named`, and built from numbers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Clock {
    t: u32,
}

impl Clock {
    /// Where it is.
    pub fn now(&self) -> u32 {
        self.t
    }
}

impl Named for Clock {
    fn name(&self) -> String {
        format!("clock@{}", self.t)
    }
}

impl FromIterator<u32> for Clock {
    fn from_iter<I: IntoIterator<Item = u32>>(iter: I) -> Clock {
        Clock { t: iter.into_iter().sum() }
    }
}

/// Neither `Default` nor `Named`: no blanket impl's static reaches it.
#[derive(Debug)]
pub struct Plain {
    /// A value.
    pub v: u32,
}
