//! Every shape of LEAKS L2-L21 that bindgen maps, in one small crate.
use std::collections::{BTreeMap, HashMap};
use std::hash::Hash;
use std::sync::Arc;

/// A text-like value with conversions into it (L2).
#[derive(Clone, Debug, PartialEq, Hash)]
pub struct Label(String);
impl From<String> for Label { fn from(s: String) -> Self { Label(s) } }
impl From<&str> for Label { fn from(s: &str) -> Self { Label(s.to_string()) } }
impl From<Rich> for Label { fn from(r: Rich) -> Self { Label(r.0) } }

/// Rich text.
#[derive(Clone, Debug, PartialEq)]
pub struct Rich(pub String);

/// A payload that is shared behind an Arc (L11).
#[derive(Clone, Debug)]
pub enum Text { Plain(String), Shared(Arc<Rich>) }

/// Anything that converts into a `Label` is an atom (L3 blanket).
pub trait IntoAtom { fn describe(&self) -> String { String::new() } }
impl<T: Into<Label>> IntoAtom for T {}

/// Anything hashable and printable is an id source (L4 multi-bound).
pub trait AsKey {}
impl<T: Hash + std::fmt::Debug> AsKey for T {}

/// Text buffers, implemented for a type of another crate (L13).
pub trait Buffer { fn text(&self) -> String; }
impl Buffer for String { fn text(&self) -> String { self.clone() } }

/// A widget (L3 trait-object parameter).
pub trait Widget { fn show(self, ui: &mut Panel) -> bool; }

/// Tuple struct (L19).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Mm(pub f32);

/// Struct with associated constants (L17).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color { pub r: u8 }
impl Color {
    pub const RED: Color = Color { r: 255 };
    pub fn new(r: u8) -> Self { Color { r } }
}

/// Struct-like variants (L19).
#[derive(Clone, Debug)]
pub enum Op { Fill { col: Color }, Move { x: Mm, y: Mm }, End }

/// A value that merges into itself but is not a collection (L21).
#[derive(Clone, Debug, Default)]
pub struct Style { bold: bool }
impl<T: Into<Style>> Extend<T> for Style { fn extend<I: IntoIterator<Item = T>>(&mut self, _i: I) {} }
impl From<Color> for Style { fn from(_c: Color) -> Self { Style { bold: false } } }

/// A real collection (for contrast).
#[derive(Clone, Debug, Default)]
pub struct Bag(Vec<u8>);
impl Extend<u8> for Bag { fn extend<I: IntoIterator<Item = u8>>(&mut self, i: I) { self.0.extend(i) } }
impl IntoIterator for Bag { type Item = u8; type IntoIter = std::vec::IntoIter<u8>; fn into_iter(self) -> Self::IntoIter { self.0.into_iter() } }

/// The receiver of the calls.
#[derive(Default)]
pub struct Panel { style: Arc<Style> }
impl Panel {
    pub fn label(&mut self, text: impl Into<Label>) {}
    pub fn add(&mut self, w: impl Widget) -> bool { w.show(self) }
    pub fn atom(&mut self, a: impl IntoAtom) {}
    pub fn edit(&mut self, b: &mut dyn Buffer) {}
    pub fn key(&mut self, k: impl AsKey) {}
    pub fn style(&self) -> &Arc<Style> { &self.style }
    pub fn set_style(&mut self, s: Arc<Style>) { self.style = s }
    pub fn maps(&mut self, a: &BTreeMap<String, u8>, b: HashMap<String, u8>) {}
    pub fn read<R>(&self, f: impl FnOnce(&Style) -> R) -> R { f(&self.style) }
    pub fn write<R>(&mut self, f: impl FnOnce(&mut Style) -> R) -> R { f(Arc::make_mut(&mut self.style)) }
}

/// A record with a field named like a Jux keyword (L20).
#[derive(Clone, Debug)]
pub struct Operation { pub operator: String, pub operands: Vec<u8> }
