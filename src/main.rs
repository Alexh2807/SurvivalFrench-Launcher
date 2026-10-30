//! SurvivalFrench — Launcher.
//!
//! Connexion Microsoft (compte Minecraft officiel) ou pseudo, installation de Minecraft + Fabric + mods du serveur
//! (synchronisés avec le manifeste publié par le plugin), puis lancement direct sur le serveur.
//! Fenêtre native sans bordure avec une interface WebView2 (le moteur d'Edge, déjà présent sur Windows) :
//! un seul petit exe, rien à extraire au démarrage, et la page est libérée pendant que le jeu tourne.
#![windows_subsystem = "windows"]

mod auth;
mod base;
mod game;
mod install;
mod net;
mod ping;
mod skin;
mod sys;
mod ui;
mod update;

use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tao::dpi::{LogicalPosition, LogicalSize};
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::platform::windows::{IconExtWindows, WindowBuilderExtWindows, WindowExtWindows};
use tao::window::{Icon, ProgressBarState, ProgressState, ResizeDirection, Window, WindowBuilder};
use wry::http::{header::CONTENT_TYPE, Request, Response};
use wry::{MemoryUsageLevel, WebContext, WebView, WebViewBuilder, WebViewBuilderExtWindows, WebViewExtWindows};

use crate::base::{arg_flag, config, paths, with_state, STATE};

#[derive(Debug)]
pub enum AppEvent {
    /// L'état a changé : à envoyer à la page (regroupé).
    Push,
    /// Une autre copie du launcher a été ouverte : on se montre.
    Show,
    /// Commande de fenêtre venue de la page.
    Ui(Value),
    /// Script à exécuter dans la page.
    Js(String),
    GameEnded,
}

static PROXY: OnceLock<Mutex<EventLoopProxy<AppEvent>>> = OnceLock::new();
static PUSH_PENDING: AtomicBool = AtomicBool::new(false);

pub fn app_event(e: AppEvent) {
    if let Some(p) = PROXY.get() {
        let _ = p.lock().unwrap().send_event(e);
    }
}

fn toast(msg: &str) {
    app_event(AppEvent::Js(format!("window.sf&&sf.toast({})", json!(msg))));
}

fn title() -> String {
    if base::instance() == 1 { "SurvivalFrench Launcher".into() } else { format!("SurvivalFrench Launcher — compte {}", base::instance()) }
}

// --------------------------------------------------------------------------------------
// Page : fichiers, images dynamiques et commandes
// --------------------------------------------------------------------------------------

fn protocol(req: Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    let path = req.uri().path().trim_start_matches('/').to_string();
    let dynamic = |bytes: Option<Vec<u8>>| match bytes {
        Some(b) => Response::builder().header(CONTENT_TYPE, "image/png").header("Cache-Control", "no-store")
            .body(Cow::Owned(b)).unwrap(),
        None => Response::builder().status(404).body(Cow::Borrowed(&b""[..])).unwrap(),
    };
    match path.as_str() {
        "skin.png" => dynamic(STATE.lock().unwrap().skin.as_ref().map(|s| s.png.clone())),
        "favicon.png" => dynamic(STATE.lock().unwrap().favicon.clone()),
        _ => match ui::file(&path) {
            Some((body, mime)) => Response::builder().header(CONTENT_TYPE, mime).header("Cache-Control", "no-cache")
                .body(body).unwrap(),
            None => Response::builder().status(404).body(Cow::Borrowed(&b""[..])).unwrap(),
        },
    }
}

fn start_task(official: bool) {
    // L'état passe à « en cours » ici même, sous le verrou : un double clic ne lance pas deux jeux.
    let start = with_state(|st| {
        let busy = matches!(st.task["state"].as_str(), Some("running" | "playing")) || st.game_running;
        if !busy {
            st.task = json!({"state": "running", "step": "Serveur", "progress": 0.0, "detail": "Préparation…", "error": ""});
        }
        !busy
    });
    if !start {
        toast("Le jeu est déjà lancé ou en cours d'installation.");
    } else if official {
        std::thread::spawn(install::play_official);
    } else {
        std::thread::spawn(game::play);
    }
}

fn ipc(body: &str) {
    let Ok(msg) = serde_json::from_str::<Value>(body) else { return };
    let cmd = msg["cmd"].as_str().unwrap_or("").to_string();
    match cmd.as_str() {
        "ready" | "sync" | "drag" | "resize" | "min" | "max" | "close" | "quit" => app_event(AppEvent::Ui(msg)),
        "login" => {
            with_state(|st| st.login = Some(json!({"user_code": "", "url": "", "error": ""})));
            std::thread::spawn(auth::ms_device_login);
        }
        "login_cancel" => with_state(|st| st.login = None),
        "offline" => {
            let name = msg["name"].as_str().unwrap_or("").trim().to_string();
            if !config().allow_offline {
                toast("Mode hors-ligne désactivé.");
            } else if !auth::valid_pseudo(&name) {
                toast("Pseudo invalide : 3 à 16 lettres, chiffres ou _.");
            } else {
                let acc = auth::offline_account(&name);
                base::save_account(Some(&acc));
                with_state(|st| st.account = Some(acc));
                skin::refresh();
            }
        }
        "logout" => {
            base::save_account(None);
            with_state(|st| st.account = None);
            skin::refresh();
        }
        "play" => start_task(false),
        "play_official" => start_task(true),
        "settings" => {
            with_state(|st| {
                let s = &mut st.settings;
                if let Some(r) = msg["ram_mb"].as_u64() {
                    s["ram_mb"] = json!(r.clamp(2048, base::max_ram_mb()));
                }
                for key in ["manifest_url", "after_launch", "perf"] {
                    if let Some(v) = msg[key].as_str() {
                        s[key] = json!(v.trim());
                    }
                }
                base::save_settings(s);
            });
            std::thread::spawn(|| {
                if install::fetch_manifest().is_some() {
                    ping::refresh();
                }
            });
        }
        "refresh" => {
            std::thread::spawn(|| {
                if install::fetch_manifest().is_some() {
                    ping::refresh();
                }
            });
        }
        "folder" => {
            let _ = std::fs::create_dir_all(&paths().game);
            sys::shell_open(&paths().game.to_string_lossy());
        }
        "open" => {
            let url = msg["url"].as_str().unwrap_or("");
            let allowed = ["https://www.microsoft.com/", "https://microsoft.com/", "https://login.live.com/",
                           "https://www.minecraft.net/fr-fr/download", "https://go.microsoft.com/"];
            if allowed.iter().any(|p| url.starts_with(p)) {
                sys::shell_open(url);
            }
        }
        "dismiss" => base::task_update(&[("state", json!("idle")), ("error", json!("")), ("step", json!("")),
                                         ("progress", json!(0.0)), ("detail", json!(""))]),
        "update_restart" => {
            if !STATE.lock().unwrap().game_running && base::task_state() != "running" {
                update::restart();
            }
        }
        _ => {}
    }
}

fn make_webview(window: &Window, ctx: &mut WebContext) -> wry::Result<WebView> {
    let boot = base::public_state();
    WebViewBuilder::new_with_web_context(ctx)
        .with_url("https://sf.localhost/index.html")
        .with_custom_protocol("sf".into(), |_id, req| protocol(req))
        .with_ipc_handler(|req: Request<String>| ipc(req.body()))
        .with_initialization_script(format!("window.__SF_BOOT={boot};"))
        .with_background_color((7, 10, 17, 255))
        .with_devtools(cfg!(debug_assertions) || arg_flag("--devtools"))
        .with_https_scheme(true)
        .with_browser_accelerator_keys(false)
        .with_default_context_menus(false)
        // Rien en arrière-plan : pas de traduction, de mises à jour de composants ni de réseau inutile.
        // (--in-process-gpu économise ~150 Mo mais rend chaque image 3 à 5 fois plus coûteuse en processeur : écarté.)
        .with_additional_browser_args("--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,Translate,\
            msEdgeTranslate,AutofillServerCommunication,msEdgeShoppingUI --disable-background-networking \
            --disable-component-update --no-first-run")
        .build(window)
}

// --------------------------------------------------------------------------------------
// Fenêtre
// --------------------------------------------------------------------------------------

fn round_corners(window: &Window) {
    use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
    // DWMWA_WINDOW_CORNER_PREFERENCE = 33, DWMWCP_ROUND = 2 (Windows 11 ; ignoré ailleurs).
    let pref: u32 = 2;
    unsafe {
        DwmSetWindowAttribute(window.hwnd() as _, 33, &pref as *const u32 as _, 4);
    }
}

fn resize_dir(d: &str) -> Option<ResizeDirection> {
    Some(match d {
        "n" => ResizeDirection::North,
        "s" => ResizeDirection::South,
        "e" => ResizeDirection::East,
        "w" => ResizeDirection::West,
        "ne" => ResizeDirection::NorthEast,
        "nw" => ResizeDirection::NorthWest,
        "se" => ResizeDirection::SouthEast,
        "sw" => ResizeDirection::SouthWest,
        _ => return None,
    })
}

fn main() {
    base::trace("début");
    let instance = base::instance();
    if !sys::single_instance(instance, || app_event(AppEvent::Show)) {
        return;
    }
    base::trace("instance unique");
    let _ = std::fs::create_dir_all(&paths().data);
    {
        let mut st = STATE.lock().unwrap();
        st.settings = base::load_settings();
        st.account = base::load_account().filter(|a| a["type"] != "offline" || config().allow_offline);
        st.official = install::official_launcher().is_some();
    }

    base::trace("réglages et compte");
    let event_loop = EventLoopBuilder::<AppEvent>::with_user_event().build();
    base::trace("boucle d'événements");
    let _ = PROXY.set(Mutex::new(event_loop.create_proxy()));
    base::set_notifier(|| {
        if !PUSH_PENDING.swap(true, Ordering::SeqCst) {
            app_event(AppEvent::Push);
        }
    });

    // Taille adaptée à l'écran (portables 1366×768 avec mise à l'échelle 125 % compris).
    let (mut w, mut h, mut pos) = (1200.0, 720.0, None);
    if let Some(mon) = event_loop.primary_monitor() {
        let sf = mon.scale_factor();
        let size = mon.size().to_logical::<f64>(sf);
        let origin = mon.position().to_logical::<f64>(sf);
        w = f64::min(w, size.width * 0.94);
        h = f64::min(h, size.height * 0.88);
        pos = Some(LogicalPosition::new(origin.x + (size.width - w) / 2.0, origin.y + (size.height - h) / 2.0 - 16.0));
    }
    let mut builder = WindowBuilder::new()
        .with_title(title())
        .with_inner_size(LogicalSize::new(w, h))
        .with_min_inner_size(LogicalSize::new(f64::min(900.0, w), f64::min(560.0, h)))
        .with_decorations(false)
        .with_undecorated_shadow(true)
        // Affichée tout de suite, peinte en bleu nuit par Windows : le joueur voit le launcher s'ouvrir pendant que la
        // page se charge (une seconde environ sur un PC lent), sans flash blanc.
        .with_background_color((7, 10, 17, 255))
        .with_visible(true)
        .with_window_icon(Icon::from_resource(1, None).ok());
    if let Some(p) = pos {
        builder = builder.with_position(p);
    }
    let window = match builder.build(&event_loop) {
        Ok(w) => w,
        Err(e) => {
            sys::message("SurvivalFrench", &format!("Impossible d'ouvrir la fenêtre : {e}"), true);
            return;
        }
    };
    round_corners(&window);
    base::trace("fenêtre affichée");

    let mut ctx = WebContext::new(Some(paths().data.join("webview")));
    let mut webview = match make_webview(&window, &mut ctx) {
        Ok(wv) => Some(wv),
        Err(e) => {
            if sys::ask("SurvivalFrench", &format!("Le composant Microsoft Edge WebView2 est nécessaire pour afficher le launcher.\n\n\
                Veux-tu ouvrir la page de téléchargement de Microsoft ?\n\n(Détail : {e})")) {
                sys::shell_open("https://go.microsoft.com/fwlink/p/?LinkId=2124703");
            }
            return;
        }
    };

    base::trace("webview créée");
    std::thread::spawn(install::manifest_loop);
    std::thread::spawn(ping::ping_loop);
    std::thread::spawn(update::check);
    skin::refresh();

    let started = Instant::now();
    let mut shown = true;
    let mut taskbar: Option<u64> = None;
    let mut last_push = Instant::now() - Duration::from_secs(1);
    let mut push_due: Option<Instant> = None;
    let mut hidden_for_game = false;
    let mut auto_hidden = false;
    let mut minimized = false;
    const PUSH_EVERY: Duration = Duration::from_millis(66);

    event_loop.run(move |event, _target, control_flow| {
        let push = |webview: &Option<WebView>, window: &Window, taskbar: &mut Option<u64>| {
            PUSH_PENDING.store(false, Ordering::SeqCst);
            if let Some(wv) = webview {
                let _ = wv.evaluate_script(&format!("window.sf&&sf.state({})", base::public_state()));
            }
            // Progression de l'installation sur le bouton de la barre des tâches.
            let progress = {
                let st = STATE.lock().unwrap();
                (st.task["state"] == "running").then(|| (st.task["progress"].as_f64().unwrap_or(0.0) * 100.0) as u64)
            };
            if progress != *taskbar {
                *taskbar = progress;
                window.set_progress_bar(ProgressBarState {
                    state: Some(if progress.is_some() { ProgressState::Normal } else { ProgressState::None }),
                    progress,
                    desktop_filename: None,
                });
            }
        };
        let show = |window: &Window, shown: &mut bool| {
            if !*shown {
                *shown = true;
                window.set_visible(true);
                window.set_focus();
            }
        };

        match event {
            Event::NewEvents(StartCause::Init) => {}
            Event::NewEvents(StartCause::ResumeTimeReached { .. }) => {
                if push_due.is_some_and(|t| Instant::now() >= t) {
                    push_due = None;
                    last_push = Instant::now();
                    push(&webview, &window, &mut taskbar);
                }
            }
            Event::UserEvent(AppEvent::Push) => {
                // Au plus ~15 envois par seconde pendant les téléchargements.
                if last_push.elapsed() >= PUSH_EVERY {
                    last_push = Instant::now();
                    push(&webview, &window, &mut taskbar);
                } else if push_due.is_none() {
                    push_due = Some(last_push + PUSH_EVERY);
                }
            }
            Event::UserEvent(AppEvent::Js(js)) => {
                if let Some(wv) = &webview {
                    let _ = wv.evaluate_script(&js);
                }
            }
            Event::UserEvent(AppEvent::Show) => {
                if webview.is_none() {
                    webview = make_webview(&window, &mut ctx).ok();
                }
                hidden_for_game = false;
                STATE.lock().unwrap().window_open = true;
                window.set_minimized(false);
                window.set_visible(true);
                shown = true;
                window.set_focus();
            }
            Event::UserEvent(AppEvent::GameEnded) => {
                auto_hidden = false;
                if hidden_for_game {
                    // Le jeu a planté : on revient montrer l'erreur ; sinon le launcher s'arrête avec le jeu.
                    if base::task_state() == "error" {
                        hidden_for_game = false;
                        webview = make_webview(&window, &mut ctx).ok();
                        STATE.lock().unwrap().window_open = true;
                        window.set_visible(true);
                        shown = true;
                        window.set_focus();
                    } else {
                        *control_flow = ControlFlow::Exit;
                        return;
                    }
                }
            }
            Event::UserEvent(AppEvent::Ui(msg)) => match msg["cmd"].as_str().unwrap_or("") {
                // La page vient de se charger : l'état envoyé avant qu'elle soit prête a pu se perdre.
                "sync" => {
                    last_push = Instant::now();
                    push(&webview, &window, &mut taskbar);
                }
                "ready" => {
                    base::trace("page prête");
                    last_push = Instant::now();
                    push(&webview, &window, &mut taskbar);
                    show(&window, &mut shown);
                }
                "drag" => {
                    let _ = window.drag_window();
                }
                "resize" => {
                    if let Some(d) = resize_dir(msg["dir"].as_str().unwrap_or("")) {
                        let _ = window.drag_resize_window(d);
                    }
                }
                "min" => window.set_minimized(true),
                "max" => window.set_maximized(!window.is_maximized()),
                "close" => {
                    let (running, playing) = (base::task_state() == "running", STATE.lock().unwrap().game_running);
                    if running {
                        if let Some(wv) = &webview {
                            let _ = wv.evaluate_script("window.sf&&sf.confirmQuit()");
                        }
                    } else if playing {
                        hidden_for_game = true;
                        window.set_visible(false);
                        webview = None;
                        shown = false;
                        STATE.lock().unwrap().window_open = false;
                    } else {
                        *control_flow = ControlFlow::Exit;
                        return;
                    }
                }
                "quit" => {
                    *control_flow = ControlFlow::Exit;
                    return;
                }
                _ => {}
            },
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                app_event(AppEvent::Ui(json!({"cmd": "close"})));
            }
            Event::WindowEvent { event: WindowEvent::Resized(_), .. } => {
                let now_min = window.is_minimized();
                if now_min != minimized {
                    minimized = now_min;
                    // Réduite : la page arrête de se dessiner et rend sa mémoire.
                    if let Some(wv) = &webview {
                        let _ = wv.set_visible(!now_min);
                        let _ = wv.set_memory_usage_level(if now_min { MemoryUsageLevel::Low } else { MemoryUsageLevel::Normal });
                    }
                    STATE.lock().unwrap().window_open = !now_min && !hidden_for_game;
                    if !now_min {
                        std::thread::spawn(|| {
                            if install::fetch_manifest().is_some() {
                                ping::refresh();
                            }
                        });
                    }
                }
                if let Some(wv) = &webview {
                    let _ = wv.evaluate_script(&format!("window.sf&&sf.win({{max:{}}})", window.is_maximized()));
                }
            }
            _ => {}
        }

        // Jeu lancé : la fenêtre se cache et la page est libérée (quelques Mo au lieu de ~100). Une seule fois par
        // partie : si le joueur rouvre le launcher pendant le jeu, il reste affiché.
        let (running, after) = {
            let st = STATE.lock().unwrap();
            (st.game_running, st.settings["after_launch"].as_str().unwrap_or("hide") == "hide")
        };
        if running && after && !auto_hidden && webview.is_some() {
            auto_hidden = true;
            hidden_for_game = true;
            window.set_visible(false);
            webview = None;
            shown = false;
            STATE.lock().unwrap().window_open = false;
        }

        // Filet de sécurité : la fenêtre apparaît même si la page tarde à se signaler.
        if !shown && !hidden_for_game && started.elapsed() > Duration::from_millis(1500) {
            show(&window, &mut shown);
        }
        *control_flow = match (push_due, shown || hidden_for_game) {
            (Some(t), _) => ControlFlow::WaitUntil(t),
            (None, false) => ControlFlow::WaitUntil(started + Duration::from_millis(1600)),
            (None, true) => ControlFlow::Wait,
        };
    });
}
