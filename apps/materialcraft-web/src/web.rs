//! The eframe web runner: canvas lookup, backend selection, the loading screen's startup steps,
//! browser shortcuts the app takes over, and startup errors shown in the page.

use eframe::App as _;
use materialcraft_ui_egui::MaterialCraftApp;
use wasm_bindgen::JsCast as _;
use wasm_bindgen::closure::Closure;

const CANVAS_ID: &str = "materialcraft_canvas";
const LOADING_ID: &str = "materialcraft_loading";
const STATUS_ID: &str = "materialcraft_status";
const PROGRESS_ID: &str = "materialcraft_progress";

pub fn start() {
    eframe::WebLogger::init(log::LevelFilter::Info).ok();
    capture_app_shortcuts();
    // Framed by OneOS (the frame page declared the ONE parent origin): the parent opens the file in it and stores what it saves.
    // Otherwise every save and export reaches the person as a browser download.
    let bridge = crate::host::Bridge::new();
    if let Some(bridge) = &bridge {
        bridge.install();
        materialcraft_ui_egui::web_files::host::install(crate::host::deliver);
    }
    wasm_bindgen_futures::spawn_local(async move {
        let Some(document) = web_sys::window().and_then(|w| w.document()) else {
            log::error!("no document");
            return;
        };
        let result = match document.get_element_by_id(CANVAS_ID).and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok()) {
            Some(canvas) => run(canvas, bridge.clone()).await,
            None => Err(format!("the page has no <canvas id=\"{CANVAS_ID}\">")),
        };
        if let Some(el) = document.get_element_by_id(LOADING_ID) {
            match result {
                Ok(()) => el.remove(),
                Err(e) => {
                    log::error!("Material failed to start: {e}");
                    if let Some(bridge) = &bridge {
                        bridge.post_failed(&e);
                    }
                    el.set_text_content(Some(&format!(
                        "Material failed to start: {e}. It needs a browser with WebGPU (current Chrome, Edge or Safari) or at least WebGL2."
                    )));
                }
            }
        }
    });
}

async fn run(canvas: web_sys::HtmlCanvasElement, bridge: Option<crate::host::Bridge>) -> Result<(), String> {
    // The WebAssembly has downloaded and compiled when this runs.
    step(0.5, "Preparing a new package…").await;
    let session = materialcraft_engine::new_session();
    step(0.8, "Starting the renderer…").await;
    let mut options = eframe::WebOptions::default();
    if query().contains("webgl")
        && let eframe::egui_wgpu::WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup
    {
        create.instance_descriptor.backends = eframe::wgpu::Backends::GL;
    }
    eframe::WebRunner::new()
        .start(
            canvas,
            options,
            Box::new(move |cc| {
                let mut app = MaterialCraftApp::new(cc, session);
                // One thread cooks in the browser: keep previews small.
                app.ui.prefs.preview_size = app.ui.prefs.preview_size.clamp(128, 256);
                let Some(bridge) = bridge else { return Ok(Box::new(app)) };
                // Framed: no welcome screen over the file the host is about to open.
                app.ui.windows.home = false;
                let hosted = Hosted::new(bridge, cc.egui_ctx.clone());
                Ok(Box::new(Shell { app, hosted }))
            }),
        )
        .await
        .map_err(|e| e.as_string().unwrap_or_else(|| format!("{e:?}")))
}

/// Show a startup step on the loading screen and let the browser paint it before the next
/// (blocking) step runs.
async fn step(progress: f32, status: &str) {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
    if let Some(el) = document.get_element_by_id(STATUS_ID) {
        el.set_text_content(Some(status));
    }
    if let Some(el) = document.get_element_by_id(PROGRESS_ID) {
        let _ = el.set_attribute("value", &progress.to_string());
    }
    // A timer longer than a frame lets the browser paint the step before work resumes. Not
    // `requestAnimationFrame`: it never fires in a background tab, which would stall startup.
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        if let Some(w) = web_sys::window() {
            let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 20);
        }
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}

/// The app's shortcuts (Ctrl+S, Ctrl+O, Ctrl+D, F5…) would also trigger the browser's own
/// actions; take them first. Copy, cut and paste stay with the browser, which turns them into
/// the clipboard events egui reads. Shortcuts the browser reserves (Ctrl+N, Ctrl+T, Ctrl+W)
/// never reach the page.
fn capture_app_shortcuts() {
    let Some(window) = web_sys::window() else { return };
    let on_key = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(|e: web_sys::KeyboardEvent| {
        let ctrl = e.ctrl_key() || e.meta_key();
        let k = e.key().to_ascii_lowercase();
        let taken = (ctrl && ["s", "o", "d", "a", "z", "y", "e"].contains(&k.as_str())) || k == "f5" || (k == " " && !ctrl);
        if taken {
            e.prevent_default();
        }
    });
    let options = web_sys::AddEventListenerOptions::new();
    options.set_capture(true);
    if let Err(e) = window.add_event_listener_with_callback_and_add_event_listener_options("keydown", on_key.as_ref().unchecked_ref(), &options) {
        log::warn!("couldn't take over the browser's shortcuts: {e:?}");
    }
    // The listener lives as long as the page.
    on_key.forget();
}

fn query() -> String {
    web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default()
}

/// What OneOS framing adds around the app: the bridge, the dirty state last reported, the assistant's runs, the files and notices waiting
/// for the next frame, and the progressive open's hand-over (keyed by how many files were opened).
struct Hosted {
    bridge: crate::host::Bridge,
    reported: crate::host::DirtyReport,
    runs: crate::host::Runs,
    opens: crate::host::Opens,
    notices: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
    handoff: crate::handoff::Handoff<u64>,
    serial: u64,
}

impl Hosted {
    fn new(bridge: crate::host::Bridge, ctx: eframe::egui::Context) -> Self {
        let runs = crate::host::Runs::default();
        let opens = crate::host::Opens::default();
        let notices = std::rc::Rc::default();
        bridge.listen(opens.clone(), runs.clone(), std::rc::Rc::clone(&notices), ctx);
        bridge.post_ready();
        Self { bridge, reported: crate::host::DirtyReport::default(), runs, opens, notices, handoff: crate::handoff::Handoff::new(), serial: 0 }
    }

    /// The file the host sent: a Substance package opens as the package; anything Material would not write back unchanged is refused by name.
    fn open_files(&mut self, app: &mut MaterialCraftApp) {
        for (name, bytes) in std::mem::take(&mut *self.opens.borrow_mut()) {
            match materialcraft_engine::host_open::open_host_file(&mut app.session, &name, &bytes) {
                Ok(()) => {
                    materialcraft_ui_egui::web_files::host::set_opened(Some(name.clone()));
                    app.ui.frame_all = true;
                    app.toast(format!("Opened {name}"), false);
                }
                Err(refusal) => {
                    materialcraft_ui_egui::web_files::host::set_opened(None);
                    self.bridge.post_failed(&refusal.to_string());
                    app.toast(refusal.to_string(), true);
                }
            }
            self.serial += 1;
        }
    }

    /// The assistant's runs, each ONE undo step (or none): answered to the host on the session it was asked in.
    fn run_commands(&self, app: &mut MaterialCraftApp) {
        for run in std::mem::take(&mut *self.runs.borrow_mut()) {
            let outcome = materialcraft_engine::host_commands::run_command_batch(&mut app.session, &run.params);
            self.bridge.post_result(run.id, outcome);
        }
        for notice in std::mem::take(&mut *self.notices.borrow_mut()) {
            app.toast(notice, true);
        }
    }
}

/// The app with OneOS's host around it.
struct Shell {
    app: MaterialCraftApp,
    hosted: Hosted,
}

impl eframe::App for Shell {
    fn auto_save_interval(&self) -> std::time::Duration {
        self.app.auto_save_interval()
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.app.save(storage);
    }

    fn raw_input_hook(&mut self, ctx: &eframe::egui::Context, raw: &mut eframe::egui::RawInput) {
        self.app.raw_input_hook(ctx, raw);
    }

    fn logic(&mut self, ctx: &eframe::egui::Context, frame: &mut eframe::Frame) {
        let h = &mut self.hosted;
        h.handoff.before(&h.bridge, h.serial);
        h.open_files(&mut self.app);
        h.run_commands(&mut self.app);
        self.app.logic(ctx, frame);
        h.bridge.report_dirty(self.app.session.modified, &h.reported);
        // A material graph has no scroll position a first view could hand over: the editor opens framed on the whole graph.
        if h.handoff.after(&h.bridge, h.serial, |_| {}) {
            ctx.request_repaint();
        }
    }

    fn ui(&mut self, ui: &mut eframe::egui::Ui, frame: &mut eframe::Frame) {
        self.app.ui(ui, frame);
    }
}
