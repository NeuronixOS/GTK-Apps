//! Plugin activatable interfaces (libpeas-style).

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use gtk4 as gtk;
use gtk4::gio;

/// Shared handles plugins receive when activated on a window.
pub struct WindowContext {
    pub window: gtk::ApplicationWindow,
    pub side_panel: gtk::Box,
    pub bottom_panel: gtk::Box,
    pub tools_menu: gio::Menu,
    pub edit_menu: gio::Menu,
    pub search_menu: gio::Menu,
    /// Shared with the header hamburger popover; bind after all plugins activate.
    pub menu_icons: Rc<RefCell<gtk_theme::IconMenu>>,
    pub status_label: gtk::Label,
}

/// App-level plugin lifecycle.
pub trait AppActivatable {
    fn activate(&mut self);
    fn deactivate(&mut self);
}

/// Window-level plugin lifecycle.
pub trait WindowActivatable {
    fn activate(&mut self, ctx: &WindowContext);
    fn deactivate(&mut self);
    fn update_state(&mut self) {}
}

/// View-level plugin lifecycle (per document view).
pub trait ViewActivatable {
    fn activate(&mut self, view: &sourceview5::View);
    fn deactivate(&mut self);
    fn update_state(&mut self) {}
}

/// A loaded plugin instance that may implement any activatable level.
pub trait Plugin: Any {
    fn info(&self) -> &PluginInfo;
    fn as_app(&mut self) -> Option<&mut dyn AppActivatable> {
        None
    }
    fn as_window(&mut self) -> Option<&mut dyn WindowActivatable> {
        None
    }
    fn as_view(&mut self) -> Option<&mut dyn ViewActivatable> {
        None
    }
}

#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub module: String,
    pub name: String,
    pub description: String,
    pub authors: String,
    pub copyright: String,
    pub website: String,
    pub builtin: bool,
}

pub type PluginFactory = Rc<dyn Fn() -> Box<dyn Plugin>>;
