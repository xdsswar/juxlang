//! The member crate of a two-crate family in egui's shape (LEAKS L29, L39):
//! it declares a `Frame` of its own, and the host (`g37app`, eframe's shape)
//! declares another. Every signature here that says `Frame` means THIS one.

/// A color, with an associated constant.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub const DARK: Color = Color { r: 27, g: 27, b: 27 };
    pub fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color { r, g, b }
    }
}

pub mod containers {
    pub mod frame {
        use crate::Color;

        /// A panel's fill and margins.
        #[derive(Clone, Copy, Debug, Default, PartialEq)]
        pub struct Frame {
            fill: Color,
            margin: i8,
        }

        impl Frame {
            pub fn new() -> Frame {
                Frame::default()
            }
            pub fn fill(mut self, fill: Color) -> Frame {
                self.fill = fill;
                self
            }
            pub fn inner_margin(mut self, margin: i8) -> Frame {
                self.margin = margin;
                self
            }
            pub fn describe(&self) -> String {
                format!("fill={},{},{} margin={}", self.fill.r, self.fill.g, self.fill.b, self.margin)
            }
        }
    }
}

pub use containers::frame::Frame;

/// Where widgets go.
#[derive(Debug, Default)]
pub struct Ui {
    lines: Vec<String>,
}

impl Ui {
    pub fn label(&mut self, text: &str) {
        self.lines.push(text.to_string());
    }
    /// egui's `dnd_drop_zone(frame, add_contents)`: takes the MEMBER's frame.
    pub fn drop_zone<R>(&mut self, frame: Frame, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
        self.lines.push(format!("zone {}", frame.describe()));
        add_contents(self)
    }
    pub fn dump(&self) -> String {
        self.lines.join("\n")
    }
}

/// A side panel with an optional frame of its own.
pub struct Panel {
    id: String,
    frame: Option<Frame>,
}

impl Panel {
    pub fn left(id: &str) -> Panel {
        Panel { id: id.to_string(), frame: None }
    }
    pub fn frame(mut self, frame: Frame) -> Panel {
        self.frame = Some(frame);
        self
    }
    pub fn show<R>(self, ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
        let frame = self.frame.unwrap_or_default();
        ui.lines.push(format!("panel {} {}", self.id, frame.describe()));
        add_contents(ui)
    }
    #[deprecated(since = "0.2.0", note = "Renamed to `show`")]
    pub fn show_inside<R>(self, ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
        self.show(ui, add_contents)
    }
}
