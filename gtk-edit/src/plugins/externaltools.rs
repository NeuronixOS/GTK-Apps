use std::process::{Command, Stdio};

use gtk4 as gtk;
use gtk::gio;
use gtk::prelude::*;

use crate::plugin::activatable::{Plugin, PluginInfo, WindowActivatable, WindowContext};
use crate::plugins::{info, make_factory};

struct ExternalToolsPlugin {
    info: PluginInfo,
    actions: Vec<gio::SimpleAction>,
}

impl Plugin for ExternalToolsPlugin {
    fn info(&self) -> &PluginInfo {
        &self.info
    }
    fn as_window(&mut self) -> Option<&mut dyn WindowActivatable> {
        Some(self)
    }
}

impl WindowActivatable for ExternalToolsPlugin {
    fn activate(&mut self, ctx: &WindowContext) {
        let win = ctx.window.clone();

        let tools = [
            ("ext-strip", "Remove Trailing Spaces", Tool::StripTrailing),
            ("ext-run", "Run Command…", Tool::RunCommand),
            ("ext-build", "Build", Tool::Build),
            ("ext-term", "Open Terminal Here", Tool::OpenTerminal),
        ];

        for (name, label, tool) in tools {
            let action = gio::SimpleAction::new(name, None);
            let win2 = win.clone();
            action.connect_activate(move |_, _| run_tool(&win2, tool));
            win.add_action(&action);
            let icon = match name {
                "ext-strip" => "edit-clear-symbolic",
                "ext-run" => "system-run-symbolic",
                "ext-build" => "system-run-symbolic",
                "ext-term" => "utilities-terminal-symbolic",
                _ => "emblem-system-symbolic",
            };
            ctx.menu_icons.borrow_mut().append(
                &ctx.tools_menu,
                label,
                &format!("win.{name}"),
                icon,
            );
            self.actions.push(action);
        }
    }

    fn deactivate(&mut self) {
        self.actions.clear();
    }
}

#[derive(Clone, Copy)]
enum Tool {
    StripTrailing,
    RunCommand,
    Build,
    OpenTerminal,
}

fn run_tool(win: &gtk::ApplicationWindow, tool: Tool) {
    let Some(tab) = crate::window::current_tab_from_window(win) else {
        return;
    };
    match tool {
        Tool::StripTrailing => {
            let text = tab.document.text();
            let cleaned: String = text
                .lines()
                .map(|l| l.trim_end())
                .collect::<Vec<_>>()
                .join("\n");
            let mut out = cleaned;
            if text.ends_with('\n') {
                out.push('\n');
            }
            tab.document.set_text(&out);
            tab.document.set_modified(true);
        }
        Tool::RunCommand => {
            let dialog = gtk::Window::builder()
                .title("Run Command")
                .transient_for(win)
                .modal(true)
                .default_width(400)
                .build();
            gtk_theme::style_dialog(&dialog);
            let entry = gtk::Entry::builder()
                .placeholder_text("command")
                .hexpand(true)
                .build();
            let run = gtk_theme::labeled_button("system-run-symbolic", "Run");
            let box_ = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            box_.set_margin_top(12);
            box_.set_margin_bottom(12);
            box_.set_margin_start(12);
            box_.set_margin_end(12);
            box_.append(&entry);
            box_.append(&run);
            dialog.set_child(Some(&box_));
            let d = dialog.clone();
            let win2 = win.clone();
            run.connect_clicked(move |_| {
                let cmd = entry.text().to_string();
                let cwd = tab
                    .document
                    .path()
                    .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                    .unwrap_or_else(|| dirs::home_dir().unwrap_or_default());
                let output = Command::new("sh")
                    .arg("-c")
                    .arg(&cmd)
                    .current_dir(&cwd)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .output();
                let detail = match output {
                    Ok(o) => format!(
                        "{}\n{}",
                        String::from_utf8_lossy(&o.stdout),
                        String::from_utf8_lossy(&o.stderr)
                    ),
                    Err(e) => e.to_string(),
                };
                let alert = gtk::AlertDialog::builder()
                    .modal(true)
                    .message("Command Output")
                    .detail(&detail)
                    .buttons(["Close"])
                    .build();
                alert.show(Some(&win2));
                d.close();
            });
            dialog.present();
        }
        Tool::Build => {
            let cwd = tab
                .document
                .path()
                .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                .unwrap_or_else(|| dirs::home_dir().unwrap_or_default());
            let cmd = if cwd.join("Cargo.toml").exists() {
                "cargo build"
            } else if cwd.join("Makefile").exists() {
                "make"
            } else {
                "echo 'No build system found'"
            };
            let output = Command::new("sh")
                .arg("-c")
                .arg(cmd)
                .current_dir(&cwd)
                .output();
            let detail = match output {
                Ok(o) => format!(
                    "{}\n{}",
                    String::from_utf8_lossy(&o.stdout),
                    String::from_utf8_lossy(&o.stderr)
                ),
                Err(e) => e.to_string(),
            };
            let alert = gtk::AlertDialog::builder()
                .modal(true)
                .message("Build")
                .detail(&detail)
                .buttons(["Close"])
                .build();
            alert.show(Some(win));
        }
        Tool::OpenTerminal => {
            let cwd = tab
                .document
                .path()
                .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                .unwrap_or_else(|| dirs::home_dir().unwrap_or_default());
            // Prefer suite gtk-term launcher under XDG, then common terminal names.
            let mut launched = false;
            if let Some(cfg) = dirs::config_dir() {
                let launch = cfg.join("gtk-apps/applications/gtk-term-launch.sh");
                if launch.is_file() {
                    launched = Command::new(&launch)
                        .arg("--working-directory")
                        .arg(&cwd)
                        .spawn()
                        .is_ok();
                }
            }
            if !launched {
                let _ = Command::new("gtk-term")
                    .current_dir(&cwd)
                    .spawn()
                    .or_else(|_| {
                        Command::new("x-terminal-emulator")
                            .arg("--working-directory")
                            .arg(&cwd)
                            .spawn()
                    })
                    .or_else(|_| Command::new("xterm").current_dir(&cwd).spawn());
            }
        }
    }
}

pub fn factory() -> (PluginInfo, crate::plugin::activatable::PluginFactory) {
    let i = info(
        "externaltools",
        "External Tools",
        "Run external tools and commands on the current document.",
    );
    make_factory(i.clone(), move || {
        Box::new(ExternalToolsPlugin {
            info: i.clone(),
            actions: Vec::new(),
        })
    })
}
