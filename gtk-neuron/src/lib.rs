//! gtk-neuron — AI Self Driving layer for Neuronix GTK apps.
//!
//! Apps open **Self Driving** via a header toggle and a right-hand side panel.
//! `gtk-neurond` holds provider credentials and talks to Cursor / Gemini / Claude.

pub mod capabilities;
pub mod client;
pub mod credentials;
pub mod drive_ui;
pub mod protocol;
pub mod providers;
pub mod socket;

pub use client::{CapabilityHandler, NeuronClient};
pub use drive_ui::{
    append_driving_menu_item, attach_drive_button, attach_driving_mode, create_driving_mode,
    edit_drive_context, files_drive_context, image_drive_context, make_drive_button,
    term_drive_context, wrap_with_driving_panel, DriveContext, DrivingMode,
};
pub use client::DRIVE_ICON_NAME;
pub use protocol::{CapabilitySpec, ProviderId};
