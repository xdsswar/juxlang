//! The host crate of a two-crate family in eframe's shape (LEAKS L29): it
//! re-exports the member crate whole (`pub use g37ui;`, as eframe does
//! `pub use egui;`) and declares a `Frame` of its own, the window's
//! surroundings. Its signatures mention both crates' types.

pub use g37ui;

/// The window's surroundings (eframe's `Frame`).
#[derive(Debug, Default)]
pub struct Frame {
    title: String,
}

impl Frame {
    pub fn set_title(&mut self, title: &str) {
        self.title = title.to_string();
    }
    pub fn title(&self) -> String {
        self.title.clone()
    }
}

/// eframe's `run_ui_native`: the closure is lent the member's `Ui` and the
/// host's own `Frame`.
pub fn run(app: impl FnOnce(&mut g37ui::Ui, &mut Frame)) -> String {
    let mut ui = g37ui::Ui::default();
    let mut frame = Frame::default();
    app(&mut ui, &mut frame);
    format!("{}\ntitle={}", ui.dump(), frame.title())
}

/// A host signature that names the MEMBER's frame, by its full path.
pub fn default_panel_frame() -> g37ui::Frame {
    g37ui::Frame::new().inner_margin(4)
}
