//! Launch gtk-sync installer and probe local server/client status for the sidebar.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use gtk4 as gtk;
use gtk::glib;
use gtk::prelude::*;
use serde::Deserialize;

use crate::util::{confirm_dialog, show_error};

/// What the installer is currently setting up (sidebar loading row).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupKind {
    Server,
    Client,
}

/// In-progress Setup Sync install (survives across sidebar rebuilds).
#[derive(Debug, Clone)]
pub struct SetupProgress {
    pub kind: SetupKind,
    pub started: Instant,
    /// Installer process still running.
    pub running: bool,
    /// Soft status line under the spinner.
    pub detail: String,
}

static SETUP_WATCH_ACTIVE: AtomicBool = AtomicBool::new(false);

thread_local! {
    static SETUP_PROGRESS: RefCell<Option<SetupProgress>> = const { RefCell::new(None) };
    static ON_SETUP_PROGRESS: RefCell<Option<Rc<dyn Fn()>>> = const { RefCell::new(None) };
}

pub fn set_on_setup_progress<F: Fn() + 'static>(f: F) {
    ON_SETUP_PROGRESS.with(|c| *c.borrow_mut() = Some(Rc::new(f)));
}

fn notify_setup_progress() {
    ON_SETUP_PROGRESS.with(|c| {
        if let Some(cb) = c.borrow().as_ref() {
            cb();
        }
    });
}

pub fn setup_progress() -> Option<SetupProgress> {
    SETUP_PROGRESS.with(|c| c.borrow().clone())
}

pub fn clear_setup_progress() {
    SETUP_WATCH_ACTIVE.store(false, Ordering::SeqCst);
    SETUP_PROGRESS.with(|c| *c.borrow_mut() = None);
    notify_setup_progress();
}

fn begin_setup_progress(kind: SetupKind) {
    SETUP_WATCH_ACTIVE.store(true, Ordering::SeqCst);
    let detail = match kind {
        SetupKind::Server => "Starting server…".into(),
        SetupKind::Client => "Connecting client…".into(),
    };
    SETUP_PROGRESS.with(|c| {
        *c.borrow_mut() = Some(SetupProgress {
            kind,
            started: Instant::now(),
            running: true,
            detail,
        });
    });
    notify_setup_progress();
}

fn set_setup_detail(detail: impl Into<String>) {
    let detail = detail.into();
    let changed = SETUP_PROGRESS.with(|c| {
        if let Some(p) = c.borrow_mut().as_mut() {
            if p.detail != detail {
                p.detail = detail;
                return true;
            }
        }
        false
    });
    if changed {
        notify_setup_progress();
    }
}

/// Refresh the soft status line from elapsed time while the installer runs.
pub fn refresh_setup_detail_for_elapsed() {
    let Some(progress) = setup_progress() else {
        return;
    };
    if !progress.running {
        return;
    }
    let secs = progress.started.elapsed().as_secs();
    let detail = match progress.kind {
        SetupKind::Server => {
            if secs < 15 {
                "Answer the setup prompts…"
            } else if secs < 60 {
                "Installing services…"
            } else if secs < 120 {
                "Still working — Docker / CouchDB can take a bit…"
            } else {
                "Almost there — starting the server…"
            }
        }
        SetupKind::Client => {
            if secs < 20 {
                "Answer the setup prompts…"
            } else {
                "Starting the sync client…"
            }
        }
    };
    set_setup_detail(detail);
}

fn mark_setup_process_finished() {
    SETUP_PROGRESS.with(|c| {
        if let Some(p) = c.borrow_mut().as_mut() {
            p.running = false;
            p.detail = match p.kind {
                SetupKind::Server => "Waiting for server…".into(),
                SetupKind::Client => "Waiting for client…".into(),
            };
        }
    });
    notify_setup_progress();
}

/// Call from the sidebar poll loop: clear pending once the service is up (or timed out).
pub fn tick_setup_progress() {
    let Some(progress) = setup_progress() else {
        return;
    };
    let sync = probe_sync_status();
    let ready = match progress.kind {
        SetupKind::Server => sync.server.is_some(),
        SetupKind::Client => sync.client_root.is_some(),
    };
    if ready {
        clear_setup_progress();
        return;
    }
    // Safety valve — don't spin forever if install hung after exit.
    if !progress.running && progress.started.elapsed() > Duration::from_secs(180) {
        clear_setup_progress();
    }
}

/// Snapshot of local gtk-sync services for the Sync sidebar.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncStatus {
    /// Active system `gtk-sync` unit (with optional config details).
    pub server: Option<ServerSyncInfo>,
    /// Active user `gtk-sync-client` sync root (navigate target).
    pub client_root: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerSyncInfo {
    pub port: u16,
    /// Hostname or LAN IP shown under "Active".
    pub host: String,
    pub root: Option<PathBuf>,
}

impl ServerSyncInfo {
    pub fn endpoint_label(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

impl SyncStatus {
    pub fn fingerprint(&self) -> String {
        let srv = self
            .server
            .as_ref()
            .map(|s| {
                format!(
                    "{}:{}:{}",
                    s.port,
                    s.host,
                    s.root
                        .as_ref()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_default()
                )
            })
            .unwrap_or_default();
        let cli = self
            .client_root
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        format!("{srv}|{cli}")
    }
}

#[derive(Debug, Deserialize)]
struct ServerToml {
    #[serde(default)]
    root: Option<PathBuf>,
    #[serde(default = "default_port")]
    port: u16,
    #[serde(default)]
    instance_name: String,
}

#[derive(Debug, Deserialize)]
struct ClientToml {
    root: PathBuf,
}

fn default_port() -> u16 {
    8443
}

fn systemctl_is_active(user: bool, unit: &str) -> bool {
    let mut cmd = Command::new("systemctl");
    if user {
        cmd.arg("--user");
    }
    cmd.args(["is-active", "--quiet", unit]);
    cmd.status().map(|s| s.success()).unwrap_or(false)
}

/// Prefer a LAN IPv4, else instance name / system hostname.
fn resolve_server_host(instance_name: &str) -> String {
    if let Some(ip) = primary_lan_ipv4() {
        return ip;
    }
    if !instance_name.is_empty() {
        return instance_name.to_string();
    }
    hostname_fallback()
}

fn primary_lan_ipv4() -> Option<String> {
    let out = Command::new("hostname").arg("-I").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    text.split_whitespace()
        .find(|t| {
            let ok_v4 = t.parse::<std::net::Ipv4Addr>().is_ok();
            ok_v4 && !t.starts_with("127.")
        })
        .map(|s| s.to_string())
}

fn hostname_fallback() -> String {
    Command::new("hostname")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "localhost".into())
}

fn load_server_info() -> ServerSyncInfo {
    let path = Path::new("/etc/gtk-sync/server.toml");
    if let Ok(text) = std::fs::read_to_string(path) {
        if let Ok(cfg) = toml::from_str::<ServerToml>(&text) {
            return ServerSyncInfo {
                port: cfg.port,
                host: resolve_server_host(&cfg.instance_name),
                root: cfg.root.filter(|p| !p.as_os_str().is_empty()),
            };
        }
    }
    ServerSyncInfo {
        port: default_port(),
        host: resolve_server_host(""),
        root: None,
    }
}

fn load_client_root() -> Option<PathBuf> {
    let path = dirs::config_dir()?.join("gtk-sync").join("client.toml");
    let text = std::fs::read_to_string(path).ok()?;
    let cfg: ClientToml = toml::from_str(&text).ok()?;
    if cfg.root.as_os_str().is_empty() {
        return None;
    }
    Some(cfg.root)
}

/// Probe systemd units + config files for Sync sidebar rows.
pub fn probe_sync_status() -> SyncStatus {
    let server = if systemctl_is_active(false, "gtk-sync") {
        Some(load_server_info())
    } else {
        None
    };

    let client_root = if systemctl_is_active(true, "gtk-sync-client") {
        load_client_root()
    } else {
        None
    };

    SyncStatus {
        server,
        client_root,
    }
}

fn suite_script(name: &str) -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        // Prefer the real binary path: /usr/local/bin/gtk-files is often a
        // symlink into …/lib/neuronix/gtk-apps/, where gtk-sync/ lives.
        let resolved = std::fs::canonicalize(&exe).unwrap_or(exe);
        for ancestor in resolved.ancestors().take(8) {
            let p = ancestor.join("gtk-sync").join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }

    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if let Some(suite) = crate_dir.parent() {
        let p = suite.join("gtk-sync").join(name);
        if p.is_file() {
            return Some(p);
        }
    }

    // Neuronix share fallback (if staged separately from the binary).
    for share in [
        PathBuf::from("/usr/local/lib/neuronix/gtk-apps/gtk-sync"),
        PathBuf::from("/usr/share/neuronix/gtk-sync"),
    ] {
        let p = share.join(name);
        if p.is_file() {
            return Some(p);
        }
    }

    None
}

/// Resolve `gtk-sync/install.sh` next to the suite or via `GTK_SYNC_INSTALL`.
pub fn install_script_path() -> Option<PathBuf> {
    if let Ok(custom) = std::env::var("GTK_SYNC_INSTALL") {
        let p = PathBuf::from(custom);
        if p.is_file() {
            return Some(p);
        }
    }
    suite_script("install.sh")
}

/// Resolve `gtk-sync/uninstall.sh` (or `GTK_SYNC_UNINSTALL`).
pub fn uninstall_script_path() -> Option<PathBuf> {
    if let Ok(custom) = std::env::var("GTK_SYNC_UNINSTALL") {
        let p = PathBuf::from(custom);
        if p.is_file() {
            return Some(p);
        }
    }
    suite_script("uninstall.sh")
}

/// Stop the user client service and remove its config (files on disk stay).
pub fn remove_client_service() -> Result<(), String> {
    let _ = Command::new("systemctl")
        .args(["--user", "disable", "--now", "gtk-sync-client"])
        .status();

    if let Some(cfg) = dirs::config_dir() {
        let unit = cfg.join("systemd/user/gtk-sync-client.service");
        let _ = std::fs::remove_file(unit);
        let _ = Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .status();
        let dir = cfg.join("gtk-sync");
        if dir.is_dir() {
            std::fs::remove_dir_all(&dir)
                .map_err(|e| format!("Could not remove {}:\n{e}", dir.display()))?;
        }
    }
    // Drop runtime status.json so the header does not stay on "Syncing…".
    crate::sync_status::clear_runtime_status_file();
    let local_bin = dirs::home_dir()
        .unwrap_or_default()
        .join(".local/bin/gtk-sync-client");
    let _ = std::fs::remove_file(local_bin);
    Ok(())
}

/// Confirm, then disconnect the local gtk-sync client.
pub fn confirm_remove_client(parent: Option<&impl IsA<gtk::Window>>, on_done: impl Fn() + 'static) {
    let parent_win = parent.map(|w| w.clone().upcast::<gtk::Window>());
    confirm_dialog(
        parent_win.as_ref(),
        "Disconnect sync folder?",
        "Stop the gtk-sync client service and remove its config.\n\n\
         Your files in the sync folder are not deleted.",
        "Disconnect",
        {
            let parent_win = parent_win.clone();
            move |ok| {
                if !ok {
                    return;
                }
                if let Err(e) = remove_client_service() {
                    show_error(parent_win.as_ref(), "Disconnect failed", &e);
                    return;
                }
                on_done();
            }
        },
    );
}

/// Confirm, then run uninstall.sh --server-only (sudo / zenity password).
pub fn confirm_remove_server(parent: Option<&impl IsA<gtk::Window>>, on_done: impl Fn() + 'static) {
    let parent_win = parent.map(|w| w.clone().upcast::<gtk::Window>());
    let Some(script) = uninstall_script_path() else {
        show_error(
            parent_win.as_ref(),
            "gtk-sync not found",
            "Could not find gtk-sync/uninstall.sh.\n\n\
             Expected it next to gtk-files in the GTK-Apps suite, or set GTK_SYNC_UNINSTALL.",
        );
        return;
    };

    confirm_dialog(
        parent_win.as_ref(),
        "Uninstall sync server?",
        "Stop the local GTK-Sync server and remove its system install \
         (/etc/gtk-sync, systemd unit, binary).\n\n\
         Storage under /var/lib/gtk-sync (or your chosen folder) is kept. \
         The CouchDB container is left running.",
        "Uninstall",
        {
            let parent_win = parent_win.clone();
            move |ok| {
                if !ok {
                    return;
                }
                let mut cmd = Command::new("bash");
                cmd.arg(&script).arg("--server-only");
                if let Err(e) = cmd.spawn() {
                    show_error(
                        parent_win.as_ref(),
                        "Could not uninstall server",
                        &format!("Failed to run {}:\n{e}", script.display()),
                    );
                    return;
                }
                on_done();
            }
        },
    );
}

/// Open Setup Sync: pick Server vs Client in a GTK dialog, then run install.sh.
///
/// When `start_folder` is set, the installer is told via `GTK_SYNC_START_FOLDER`
/// so the folder picker can open near the current gtk-files location.
pub fn launch_setup_sync(parent: Option<&impl IsA<gtk::Window>>, start_folder: Option<&Path>) {
    let Some(script) = install_script_path() else {
        show_error(
            parent,
            "gtk-sync not found",
            "Could not find gtk-sync/install.sh.\n\n\
             Expected it next to gtk-files in the GTK-Apps suite, or set GTK_SYNC_INSTALL.",
        );
        return;
    };

    if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
        show_error(
            parent,
            "No display",
            "gtk-sync setup needs a graphical session.",
        );
        return;
    }

    let start = start_folder.map(|p| p.to_path_buf());
    match parent {
        Some(win) => show_setup_role_dialog(win, script, start),
        None => spawn_install(None, &script, start.as_deref(), None),
    }
}

fn spawn_install(
    parent: Option<&gtk::Window>,
    script: &Path,
    start_folder: Option<&Path>,
    mode: Option<&str>,
) {
    let kind = match mode {
        Some("server") => Some(SetupKind::Server),
        Some("client") => Some(SetupKind::Client),
        _ => None,
    };

    let mut cmd = Command::new("bash");
    cmd.arg(script);
    cmd.stdin(Stdio::null());
    // Keep stdout/stderr so sudo/zenity still work; don't pipe (would block).
    if let Some(folder) = start_folder {
        cmd.env("GTK_SYNC_START_FOLDER", folder);
    }
    if let Some(m) = mode {
        cmd.env("GTK_SYNC_MODE", m);
    }

    if let Some(kind) = kind {
        begin_setup_progress(kind);
    }

    match cmd.spawn() {
        Ok(mut child) => {
            let Some(kind) = kind else {
                return;
            };
            // Watch installer exit on a worker thread; update UI on the GTK main loop.
            std::thread::spawn(move || {
                let status = child.wait();
                let ok = matches!(&status, Ok(s) if s.success());
                let err = match status {
                    Ok(s) if s.success() => None,
                    Ok(s) => Some(format!("Setup exited with status {s}")),
                    Err(e) => Some(format!("Setup failed: {e}")),
                };
                glib::MainContext::default().invoke(move || {
                    if !SETUP_WATCH_ACTIVE.load(Ordering::SeqCst) {
                        return;
                    }
                    if !ok {
                        // Includes zenity cancel — just clear the loading row.
                        clear_setup_progress();
                        let _ = err;
                        return;
                    }
                    mark_setup_process_finished();
                    set_setup_detail(match kind {
                        SetupKind::Server => "Waiting for server…",
                        SetupKind::Client => "Waiting for client…",
                    });
                    for ms in [500u64, 1500, 3000, 6000, 12000, 20000] {
                        glib::timeout_add_local_once(Duration::from_millis(ms), || {
                            tick_setup_progress();
                            notify_setup_progress();
                        });
                    }
                });
            });
        }
        Err(e) => {
            clear_setup_progress();
            show_error(
                parent,
                "Could not start gtk-sync",
                &format!("Failed to run {}:\n{e}", script.display()),
            );
        }
    }
}

/// Same visual language as `/usr/share/neuronix/neuronix_choice_dialog.py`
/// (Neuronix Settings hub — large card buttons).
const NEURONIX_CHOICE_CSS: &str = r#"
window.neuronix-choice {
  background-color: #2e2e2e;
  color: #f5f5f5;
  border-radius: 12px;
}
box.neuronix-root {
  background-color: #2e2e2e;
}
label.neuronix-title {
  color: #f5f5f5;
  font-size: 22px;
  font-weight: 700;
}
label.neuronix-subtitle {
  color: #b0b0b0;
  font-size: 13px;
}
button.neuronix-card {
  background-color: #3a3a3a;
  background-image: none;
  border: none;
  border-radius: 10px;
  box-shadow: none;
  outline: none;
  padding: 14px 18px;
  margin: 0;
  min-height: 56px;
}
button.neuronix-card:hover {
  background-color: #4a4a4a;
}
button.neuronix-card label.neuronix-row-title {
  color: #f5f5f5;
  font-size: 15px;
  font-weight: 600;
}
button.neuronix-card label.neuronix-row-desc {
  color: #a8a8a8;
  font-size: 12px;
}
button.neuronix-close {
  background-color: #3a3a3a;
  background-image: none;
  color: #f5f5f5;
  border: none;
  border-radius: 8px;
  padding: 10px 20px;
  font-size: 13px;
  min-width: 88px;
}
button.neuronix-close:hover {
  background-color: #4a4a4a;
}
"#;

/// Role picker matching Neuronix Settings (`neuronix_choice_dialog`) card buttons.
fn show_setup_role_dialog(
    parent: &impl IsA<gtk::Window>,
    script: PathBuf,
    start_folder: Option<PathBuf>,
) {
    let dialog = gtk::Window::builder()
        .title("Setup Sync")
        .transient_for(parent)
        .modal(true)
        .decorated(false)
        .resizable(false)
        .default_width(520)
        .default_height(340)
        .build();
    dialog.add_css_class("neuronix-choice");

    let provider = gtk::CssProvider::new();
    provider.load_from_data(NEURONIX_CHOICE_CSS);
    gtk::style_context_add_provider_for_display(
        &gtk::gdk::Display::default().expect("display"),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let outer = gtk::Box::new(gtk::Orientation::Vertical, 12);
    outer.add_css_class("neuronix-root");
    outer.set_margin_top(22);
    outer.set_margin_bottom(18);
    outer.set_margin_start(22);
    outer.set_margin_end(22);

    let title = gtk::Label::new(Some("Setup Sync"));
    title.add_css_class("neuronix-title");
    title.set_xalign(0.0);
    outer.append(&title);

    let subtitle = gtk::Label::new(Some(
        "Choose how this computer will use Sync — each option continues setup.",
    ));
    subtitle.add_css_class("neuronix-subtitle");
    subtitle.set_xalign(0.0);
    subtitle.set_wrap(true);
    outer.append(&subtitle);

    let list = gtk::Box::new(gtk::Orientation::Vertical, 8);
    list.set_margin_top(8);
    let server_btn =
        neuronix_card_button("Server", "Host the sync library on this machine");
    let client_btn =
        neuronix_card_button("Client", "Sync a folder to an existing server");
    list.append(&server_btn);
    list.append(&client_btn);
    outer.append(&list);

    let foot = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    foot.set_halign(gtk::Align::End);
    foot.set_margin_top(8);
    let close = gtk::Button::with_label("Close");
    close.add_css_class("neuronix-close");
    foot.append(&close);
    outer.append(&foot);

    dialog.set_child(Some(&outer));

    {
        let d = dialog.clone();
        close.connect_clicked(move |_| {
            d.close();
        });
    }
    {
        let d = dialog.clone();
        let parent = parent.clone().upcast::<gtk::Window>();
        let script = script.clone();
        server_btn.connect_clicked(move |_| {
            d.close();
            // Server storage defaults to /var/lib/gtk-sync — don't seed the
            // installer with the current gtk-files browse folder.
            spawn_install(Some(&parent), &script, None, Some("server"));
        });
    }
    {
        let d = dialog.clone();
        let parent = parent.clone().upcast::<gtk::Window>();
        client_btn.connect_clicked(move |_| {
            d.close();
            spawn_install(
                Some(&parent),
                &script,
                start_folder.as_deref(),
                Some("client"),
            );
        });
    }

    let key = gtk::EventControllerKey::new();
    {
        let d = dialog.clone();
        key.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape {
                d.close();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
    }
    dialog.add_controller(key);

    dialog.present();
}

fn neuronix_card_button(title: &str, desc: &str) -> gtk::Button {
    let btn = gtk::Button::new();
    btn.add_css_class("neuronix-card");
    btn.set_hexpand(true);
    btn.set_halign(gtk::Align::Fill);

    let inner = gtk::Box::new(gtk::Orientation::Vertical, 3);
    inner.set_halign(gtk::Align::Start);
    inner.set_hexpand(true);

    let t = gtk::Label::new(Some(title));
    t.add_css_class("neuronix-row-title");
    t.set_xalign(0.0);
    inner.append(&t);

    let d = gtk::Label::new(Some(desc));
    d.add_css_class("neuronix-row-desc");
    d.set_xalign(0.0);
    d.set_wrap(true);
    inner.append(&d);

    btn.set_child(Some(&inner));
    btn
}
