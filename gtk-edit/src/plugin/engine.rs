//! Plugin engine: builtin + external cdylib discovery and activation.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use libloading::Library;

use crate::config::{plugins_user_dir, Config};
use crate::plugins;

use super::activatable::{Plugin, PluginFactory, PluginInfo, WindowContext};
use super::bus::MessageBus;
use super::discovery::{scan_plugin_dir, DiscoveredPlugin};

struct LoadedExternal {
    _lib: Library,
}

pub struct PluginEngine {
    bus: MessageBus,
    factories: HashMap<String, PluginFactory>,
    infos: Vec<PluginInfo>,
    instances: RefCell<HashMap<String, Box<dyn Plugin>>>,
    active: RefCell<Vec<String>>,
    _external: RefCell<Vec<LoadedExternal>>,
    discovered: Vec<DiscoveredPlugin>,
}

impl PluginEngine {
    pub fn new(config: &Config) -> Rc<Self> {
        let bus = MessageBus::new();
        let mut factories: HashMap<String, PluginFactory> = HashMap::new();
        let mut infos = Vec::new();

        for (info, factory) in plugins::builtin_plugins() {
            factories.insert(info.module.clone(), factory);
            infos.push(info);
        }

        let mut discovered = Vec::new();
        let user_dir = plugins_user_dir();
        let _ = std::fs::create_dir_all(&user_dir);
        discovered.extend(scan_plugin_dir(&user_dir));

        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                let p = dir.join("plugins");
                if p.is_dir() {
                    discovered.extend(scan_plugin_dir(&p));
                }
            }
        }

        for d in &discovered {
            if !infos.iter().any(|i| i.module == d.info.module) {
                infos.push(d.info.clone());
            }
        }

        let engine = Rc::new(Self {
            bus,
            factories,
            infos,
            instances: RefCell::new(HashMap::new()),
            active: RefCell::new(config.plugins.active_plugins.clone()),
            _external: RefCell::new(Vec::new()),
            discovered,
        });

        engine.ensure_instances();
        engine.activate_app_plugins();
        engine
    }

    pub fn bus(&self) -> &MessageBus {
        &self.bus
    }

    pub fn list_plugins(&self) -> &[PluginInfo] {
        &self.infos
    }

    pub fn is_active(&self, module: &str) -> bool {
        self.active.borrow().iter().any(|m| m == module)
    }

    pub fn set_active(&self, module: &str, active: bool, config: &mut Config) {
        {
            let mut a = self.active.borrow_mut();
            if active {
                if !a.iter().any(|m| m == module) {
                    a.push(module.to_string());
                }
            } else {
                a.retain(|m| m != module);
            }
            config.plugins.active_plugins = a.clone();
        }
        let _ = config.save();

        if active {
            self.ensure_instance(module);
            if let Some(p) = self.instances.borrow_mut().get_mut(module) {
                if let Some(app) = p.as_app() {
                    app.activate();
                }
            }
        } else if let Some(p) = self.instances.borrow_mut().get_mut(module) {
            if let Some(w) = p.as_window() {
                w.deactivate();
            }
            if let Some(app) = p.as_app() {
                app.deactivate();
            }
        }
    }

    fn ensure_instances(&self) {
        let active: Vec<String> = self.active.borrow().clone();
        for module in active {
            self.ensure_instance(&module);
        }
    }

    fn ensure_instance(&self, module: &str) {
        if self.instances.borrow().contains_key(module) {
            return;
        }
        if let Some(factory) = self.factories.get(module) {
            self.instances
                .borrow_mut()
                .insert(module.to_string(), factory());
            return;
        }
        if let Some(d) = self.discovered.iter().find(|d| d.info.module == module) {
            if let Some(lib_path) = &d.library_path {
                if let Ok(lib) = unsafe { Library::new(lib_path) } {
                    // Keep library loaded for future ABI hooks.
                    self._external
                        .borrow_mut()
                        .push(LoadedExternal { _lib: lib });
                }
            }
        }
    }

    fn activate_app_plugins(&self) {
        let active: Vec<String> = self.active.borrow().clone();
        for module in active {
            if let Some(p) = self.instances.borrow_mut().get_mut(&module) {
                if let Some(app) = p.as_app() {
                    app.activate();
                }
            }
        }
    }

    pub fn activate_window_plugins(&self, ctx: &WindowContext) {
        let active: Vec<String> = self.active.borrow().clone();
        for module in active {
            self.ensure_instance(&module);
            if let Some(p) = self.instances.borrow_mut().get_mut(&module) {
                if let Some(w) = p.as_window() {
                    w.activate(ctx);
                }
            }
        }
    }

    pub fn update_window_plugins(&self) {
        let active: Vec<String> = self.active.borrow().clone();
        for module in active {
            if let Some(p) = self.instances.borrow_mut().get_mut(&module) {
                if let Some(w) = p.as_window() {
                    w.update_state();
                }
            }
        }
    }

    pub fn activate_view_plugins(&self, view: &sourceview5::View) {
        let active: Vec<String> = self.active.borrow().clone();
        for module in active {
            self.ensure_instance(&module);
            if let Some(p) = self.instances.borrow_mut().get_mut(&module) {
                if let Some(v) = p.as_view() {
                    v.activate(view);
                }
            }
        }
    }
}

pub fn plugin_dirs() -> Vec<PathBuf> {
    vec![plugins_user_dir()]
}
