//! Client that talks to gtk-neurond and dispatches capability invokes on the GTK thread.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

use gtk4 as gtk;
use gtk::glib;
use gtk::glib::{ControlFlow, IOCondition};
use serde_json::Value;

use crate::protocol::{
    CapabilityConfirmParams, CapabilityErrorParams, CapabilityInvokeParams, CapabilityResultParams,
    CapabilitySpec, ChatCancelParams, ChatSendParams, ChatStartParams, CredentialsSetParams,
    HelloParams, Message, ProviderId, ProviderStatus, PROTOCOL_VERSION,
};
use crate::socket::{connect_or_spawn, read_message, write_message};

pub type CapabilityHandler = Rc<dyn Fn(&str, &Value) -> Result<Value, String>>;

struct PendingConfirm {
    name: String,
    args: Value,
}

pub struct NeuronClient {
    tx: Sender<Message>,
    #[allow(dead_code)]
    app_id: String,
    #[allow(dead_code)]
    capabilities: Vec<CapabilitySpec>,
    handler: CapabilityHandler,
    providers: RefCell<Vec<ProviderStatus>>,
    on_delta: RefCell<Option<Rc<dyn Fn(&str, &str)>>>,
    on_done: RefCell<Option<Rc<dyn Fn(&str, &str)>>>,
    on_error: RefCell<Option<Rc<dyn Fn(&str, &str)>>>,
    on_providers: RefCell<Option<Rc<dyn Fn(&[ProviderStatus])>>>,
    on_propose: RefCell<Option<Rc<dyn Fn(String, String, Value, bool)>>>,
    pending: RefCell<HashMap<String, PendingConfirm>>,
}

impl NeuronClient {
    pub fn start(
        app_id: &str,
        capabilities: Vec<CapabilitySpec>,
        handler: CapabilityHandler,
    ) -> Result<Rc<Self>, String> {
        let stream = connect_or_spawn()?;
        let (to_daemon_tx, to_daemon_rx) = mpsc::channel::<Message>();
        let (from_daemon_tx, from_daemon_rx) = mpsc::channel::<Message>();
        // Wake the GTK main loop as soon as a daemon frame arrives (avoids
        // "reply only appears after the next keystroke").
        let (wake_reader, wake_writer) =
            UnixStream::pair().map_err(|e| format!("wake pipe: {e}"))?;
        wake_reader
            .set_nonblocking(true)
            .map_err(|e| format!("wake pipe nb: {e}"))?;
        wake_writer
            .set_nonblocking(true)
            .map_err(|e| format!("wake pipe nb: {e}"))?;

        let stream_writer = stream.try_clone().map_err(|e| e.to_string())?;
        let stream_reader = stream;

        thread::spawn(move || writer_loop(stream_writer, to_daemon_rx));
        thread::spawn(move || reader_loop(stream_reader, from_daemon_tx, wake_writer));

        let client = Rc::new(Self {
            tx: to_daemon_tx,
            app_id: app_id.to_string(),
            capabilities: capabilities.clone(),
            handler,
            providers: RefCell::new(Vec::new()),
            on_delta: RefCell::new(None),
            on_done: RefCell::new(None),
            on_error: RefCell::new(None),
            on_providers: RefCell::new(None),
            on_propose: RefCell::new(None),
            pending: RefCell::new(HashMap::new()),
        });

        client.send(Message::Hello(HelloParams {
            app_id: app_id.to_string(),
            protocol: PROTOCOL_VERSION,
            capabilities,
        }))?;
        client.send(Message::ProvidersList)?;

        attach_incoming(client.clone(), from_daemon_rx, wake_reader);
        Ok(client)
    }

    pub fn send(&self, msg: Message) -> Result<(), String> {
        self.tx
            .send(msg)
            .map_err(|_| "daemon connection closed".to_string())
    }

    pub fn set_on_delta(&self, f: impl Fn(&str, &str) + 'static) {
        *self.on_delta.borrow_mut() = Some(Rc::new(f));
    }

    pub fn set_on_done(&self, f: impl Fn(&str, &str) + 'static) {
        *self.on_done.borrow_mut() = Some(Rc::new(f));
    }

    pub fn set_on_error(&self, f: impl Fn(&str, &str) + 'static) {
        *self.on_error.borrow_mut() = Some(Rc::new(f));
    }

    pub fn set_on_providers(&self, f: impl Fn(&[ProviderStatus]) + 'static) {
        *self.on_providers.borrow_mut() = Some(Rc::new(f));
    }

    pub fn set_on_propose(&self, f: impl Fn(String, String, Value, bool) + 'static) {
        *self.on_propose.borrow_mut() = Some(Rc::new(f));
    }

    pub fn providers(&self) -> Vec<ProviderStatus> {
        self.providers.borrow().clone()
    }

    pub fn chat_start(&self, session_id: &str, provider: ProviderId) -> Result<(), String> {
        self.send(Message::ChatStart(ChatStartParams {
            session_id: session_id.to_string(),
            provider,
        }))
    }

    pub fn chat_send(&self, session_id: &str, text: &str) -> Result<(), String> {
        self.send(Message::ChatSend(ChatSendParams {
            session_id: session_id.to_string(),
            text: text.to_string(),
        }))
    }

    pub fn chat_cancel(&self, session_id: &str) -> Result<(), String> {
        self.send(Message::ChatCancel(ChatCancelParams {
            session_id: session_id.to_string(),
        }))
    }

    pub fn set_credential(&self, provider: ProviderId, api_key: &str) -> Result<(), String> {
        self.send(Message::CredentialsSet(CredentialsSetParams {
            provider,
            api_key: api_key.to_string(),
        }))
    }

    pub fn confirm_capability(&self, id: &str, allow: bool) -> Result<(), String> {
        // If local pending and allow, invoke handler then reply to daemon.
        if allow {
            if let Some(p) = self.pending.borrow_mut().remove(id) {
                match (self.handler)(&p.name, &p.args) {
                    Ok(result) => {
                        self.send(Message::CapabilityResult(CapabilityResultParams {
                            id: id.to_string(),
                            result,
                        }))?;
                    }
                    Err(message) => {
                        self.send(Message::CapabilityError(CapabilityErrorParams {
                            id: id.to_string(),
                            message,
                        }))?;
                    }
                }
                return Ok(());
            }
        } else {
            self.pending.borrow_mut().remove(id);
        }
        self.send(Message::CapabilityConfirm(CapabilityConfirmParams {
            id: id.to_string(),
            allow,
        }))
    }

    fn handle_message(self: &Rc<Self>, msg: Message) {
        match msg {
            Message::HelloOk(_) => {}
            Message::ProvidersStatus(p) => {
                *self.providers.borrow_mut() = p.providers.clone();
                if let Some(cb) = self.on_providers.borrow().as_ref() {
                    cb(&p.providers);
                }
            }
            Message::ChatDelta(d) => {
                if let Some(cb) = self.on_delta.borrow().as_ref() {
                    cb(&d.session_id, &d.text);
                }
            }
            Message::ChatDone(d) => {
                if let Some(cb) = self.on_done.borrow().as_ref() {
                    cb(&d.session_id, &d.text);
                }
            }
            Message::ChatError(e) => {
                if let Some(cb) = self.on_error.borrow().as_ref() {
                    cb(&e.session_id, &e.message);
                }
            }
            Message::CapabilityPropose(p) => {
                self.pending.borrow_mut().insert(
                    p.id.clone(),
                    PendingConfirm {
                        name: p.name.clone(),
                        args: p.args.clone(),
                    },
                );
                if let Some(cb) = self.on_propose.borrow().as_ref() {
                    cb(p.id, p.name, p.args, p.requires_confirm);
                }
            }
            Message::CapabilityInvoke(p) => {
                self.dispatch_invoke(p);
            }
            Message::Error(e) => {
                if let Some(cb) = self.on_error.borrow().as_ref() {
                    cb("", &e.message);
                }
            }
            _ => {}
        }
    }

    fn dispatch_invoke(self: &Rc<Self>, p: CapabilityInvokeParams) {
        match (self.handler)(&p.name, &p.args) {
            Ok(result) => {
                let _ = self.send(Message::CapabilityResult(CapabilityResultParams {
                    id: p.id,
                    result,
                }));
            }
            Err(message) => {
                let _ = self.send(Message::CapabilityError(CapabilityErrorParams {
                    id: p.id,
                    message,
                }));
            }
        }
    }
}

fn writer_loop(mut stream: UnixStream, rx: Receiver<Message>) {
    while let Ok(msg) = rx.recv() {
        if write_message(&mut stream, &msg).is_err() {
            break;
        }
    }
}

fn reader_loop(mut stream: UnixStream, tx: Sender<Message>, mut wake: UnixStream) {
    let _ = stream.set_read_timeout(None);
    loop {
        match read_message(&mut stream) {
            Ok(msg) => {
                if tx.send(msg).is_err() {
                    break;
                }
                let _ = wake.write_all(&[1u8]);
            }
            Err(_) => break,
        }
    }
}

fn attach_incoming(client: Rc<NeuronClient>, rx: Receiver<Message>, mut wake: UnixStream) {
    let fd = wake.as_raw_fd();
    glib::unix_fd_add_local(fd, IOCondition::IN, move |_, _cond| {
        let mut buf = [0u8; 64];
        while wake.read(&mut buf).is_ok() {}
        loop {
            match rx.try_recv() {
                Ok(msg) => client.handle_message(msg),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return ControlFlow::Break,
            }
        }
        ControlFlow::Continue
    });
}

/// Embedded steering-wheel symbolic SVG (Adwaita fg `#2e3436` for theme recolor).
const DRIVE_SVG: &str = include_str!("../data/symbolic/actions/neuron-drive-symbolic.svg");

pub const DRIVE_ICON_NAME: &str = "neuron-drive-symbolic";

/// Install the drive icon into the user hicolor theme and register search paths.
/// Returns the on-disk SVG path when available.
pub fn ensure_drive_icon() -> Option<std::path::PathBuf> {
    let dest = install_drive_icon_file()?;

    let Some(display) = gtk::gdk::Display::default() else {
        return Some(dest);
    };
    let theme = gtk::IconTheme::for_display(&display);

    if let Some(icons) = dirs::data_local_dir().map(|d| d.join("icons")) {
        theme.add_search_path(icons.join("hicolor"));
        let actions = icons.join("hicolor/scalable/actions");
        if actions.is_dir() {
            theme.add_search_path(actions);
        }
    }

    // Dev tree (cargo) — also search Adwaita-style layout.
    let data = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data");
    if data.is_dir() {
        theme.add_search_path(&data);
        let actions = data.join("symbolic/actions");
        if actions.is_dir() {
            theme.add_search_path(actions);
        }
    }

    Some(dest)
}

fn install_drive_icon_file() -> Option<std::path::PathBuf> {
    let dir = dirs::data_local_dir()?.join("icons/hicolor/scalable/actions");
    std::fs::create_dir_all(&dir).ok()?;
    let dest = dir.join(format!("{DRIVE_ICON_NAME}.svg"));
    let needs_write = match std::fs::read_to_string(&dest) {
        Ok(existing) => existing != DRIVE_SVG,
        Err(_) => true,
    };
    if needs_write {
        std::fs::write(&dest, DRIVE_SVG).ok()?;
    }
    // Ensure a minimal hicolor index so some desktops pick up scalable/actions.
    let index = dirs::data_local_dir()?.join("icons/hicolor/index.theme");
    if !index.is_file() {
        let _ = std::fs::write(
            &index,
            "[Icon Theme]\nName=Hicolor\nComment=Fallback icon theme\nDirectories=scalable/actions\n\n[scalable/actions]\nSize=16\nType=Scalable\nMinSize=1\nMaxSize=256\nContext=Actions\n",
        );
    }
    Some(dest)
}

/// Symbolic drive image that follows light/dark chrome fg color.
pub fn drive_symbolic_image() -> gtk::Image {
    let installed = ensure_drive_icon();
    let image = gtk::Image::new();
    image.set_pixel_size(gtk_theme::SYMBOLIC_PIXEL_SIZE);
    image.set_icon_size(gtk::IconSize::Normal);

    // Prefer the installed SVG file — reliable vs IconTheme name lookup for
    // custom icons that are not part of Adwaita.
    if let Some(path) = installed {
        let file = gtk::gio::File::for_path(path);
        let paintable =
            gtk::IconPaintable::for_file(&file, gtk_theme::SYMBOLIC_PIXEL_SIZE, 1);
        image.set_paintable(Some(&paintable));
        return image;
    }

    if let Some(display) = gtk::gdk::Display::default() {
        let theme = gtk::IconTheme::for_display(&display);
        let paintable = theme.lookup_icon(
            DRIVE_ICON_NAME,
            &[],
            gtk_theme::SYMBOLIC_PIXEL_SIZE,
            1,
            gtk::TextDirection::Ltr,
            gtk::IconLookupFlags::FORCE_SYMBOLIC,
        );
        image.set_paintable(Some(&paintable));
        return image;
    }

    image.set_icon_name(Some("image-missing"));
    image
}
