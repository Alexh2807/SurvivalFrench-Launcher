//! Emplacements, réglages, compte et état partagé avec l'interface.
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, OnceLock};

use serde_json::{json, Value};

use crate::sys;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const USER_AGENT: &str = concat!("SurvivalFrench-Launcher/", env!("CARGO_PKG_VERSION"));

/// Repères de temps (SFRP_TRACE=1) : démarrage et préparation du jeu, dans startup-trace.txt du dossier de données.
pub fn trace(label: &str) {
    static START: LazyLock<std::time::Instant> = LazyLock::new(std::time::Instant::now);
    static LINES: Mutex<String> = Mutex::new(String::new());
    if std::env::var_os("SFRP_TRACE").is_none() {
        return;
    }
    let mut l = LINES.lock().unwrap();
    l.push_str(&format!("{:>6} ms  {label}
", START.elapsed().as_millis()));
    let _ = std::fs::write(paths().data.join("startup-trace.txt"), l.as_str());
}

pub fn arg_flag(name: &str) -> bool {
    std::env::args().any(|a| a == name)
}

pub fn arg_value(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

pub fn instance() -> u32 {
    static I: OnceLock<u32> = OnceLock::new();
    *I.get_or_init(|| arg_value("--instance").and_then(|v| v.parse().ok()).map(|n: u32| n.clamp(1, 9)).unwrap_or(1))
}

fn suffixed(base: &str, ext: &str) -> String {
    if instance() == 1 { format!("{base}{ext}") } else { format!("{base}-{}{ext}", instance()) }
}

pub struct Paths {
    pub data: PathBuf,
    pub game: PathBuf,
    pub libraries: PathBuf,
    pub assets: PathBuf,
    pub versions: PathBuf,
    pub runtime: PathBuf,
    pub natives: PathBuf,
    pub cache: PathBuf,
    pub settings: PathBuf,
    pub account: PathBuf,
    pub minecraft: PathBuf,
    pub asset_reuse: Vec<PathBuf>,
}

fn appdata() -> PathBuf {
    std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(home)
}

fn home() -> PathBuf {
    std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

pub fn paths() -> &'static Paths {
    static P: OnceLock<Paths> = OnceLock::new();
    P.get_or_init(|| {
        let data = std::env::var_os("SFRP_DATA_DIR").map(PathBuf::from).unwrap_or_else(|| appdata().join(".survivalfrench"));
        let minecraft = std::env::var_os("SFRP_MINECRAFT_DIR").map(PathBuf::from).unwrap_or_else(|| appdata().join(".minecraft"));
        Paths {
            game: data.join(suffixed("game", "")),
            libraries: data.join("libraries"),
            assets: data.join("assets"),
            versions: data.join("versions"),
            runtime: data.join("runtime"),
            natives: data.join("natives"),
            cache: data.join("cache"),
            settings: data.join(suffixed("settings", ".json")),
            account: data.join(suffixed("account", ".dat")),
            // Assets déjà présents sur la machine (lecture seule) : évite de retélécharger ~600 Mo.
            asset_reuse: vec![
                appdata().join(".minecraft").join("assets").join("objects"),
                home().join(".gradle").join("caches").join("fabric-loom").join("assets").join("objects"),
            ],
            minecraft,
            data,
        }
    })
}

pub fn app_dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_else(|| PathBuf::from("."))
}

// --------------------------------------------------------------------------------------
// launcher.json : réglages du serveur (intégré à l'exe, surchargeable à côté de l'exe)
// --------------------------------------------------------------------------------------

pub struct Config {
    pub manifest_url: String,
    pub client_id: String,
    pub allow_offline: bool,
    pub update_repo: String,
}

pub fn config() -> &'static Config {
    static C: OnceLock<Config> = OnceLock::new();
    C.get_or_init(|| {
        let mut cfg = json!({"manifest_url": "http://localhost:8163/launcher/manifest.json", "client_id": "",
                             "allow_offline": false, "update_repo": ""});
        let mut sources = vec![include_str!("../launcher.json").to_string()];
        for p in [Some(app_dir().join("launcher.json")), std::env::var_os("SFRP_LAUNCHER_CONFIG").map(PathBuf::from)]
            .into_iter().flatten() {
            if let Ok(t) = std::fs::read_to_string(p) {
                sources.push(t);
            }
        }
        for s in sources {
            if let Ok(Value::Object(o)) = serde_json::from_str::<Value>(s.trim_start_matches('\u{feff}')) {
                for (k, v) in o {
                    cfg[k] = v;
                }
            }
        }
        Config {
            manifest_url: cfg["manifest_url"].as_str().unwrap_or("").trim().to_string(),
            client_id: cfg["client_id"].as_str().unwrap_or("").trim().to_string(),
            allow_offline: cfg["allow_offline"].as_bool().unwrap_or(false),
            update_repo: cfg["update_repo"].as_str().unwrap_or("").trim().to_string(),
        }
    })
}

// --------------------------------------------------------------------------------------
// Machine
// --------------------------------------------------------------------------------------

pub struct System {
    pub ram_mb: u64,
    pub cpus: usize,
    pub gpus: Vec<String>,
    pub tier: &'static str,
}

pub fn system() -> &'static System {
    static S: OnceLock<System> = OnceLock::new();
    S.get_or_init(|| {
        let gpus = sys::gpu_names();
        System {
            ram_mb: sys::total_ram_mb(),
            cpus: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4),
            tier: crate::install::gpu_tier(&gpus),
            gpus,
        }
    })
}

/// Mémoire conseillée pour le jeu : la moitié de la RAM au plus, de 2 à 6 Go.
pub fn recommended_ram_mb() -> u64 {
    let total = system().ram_mb;
    let r = if total <= 4600 { 2048 } else if total <= 8800 { 3072 } else if total <= 17000 { 4096 } else { 6144 };
    r.min((total / 2 / 512 * 512).max(2048))
}

pub fn max_ram_mb() -> u64 {
    (system().ram_mb.saturating_sub(2048) / 512 * 512).clamp(2048, 16384)
}

/// Mode économie : petites machines (peu de RAM ou de cœurs, puce graphique intégrée) → interface sans animation continue.
pub fn auto_eco() -> bool {
    let s = system();
    s.ram_mb <= 6200 || s.cpus <= 2 || (s.tier == "low" && s.cpus <= 4)
}

// --------------------------------------------------------------------------------------
// Réglages du joueur
// --------------------------------------------------------------------------------------

pub fn load_settings() -> Value {
    let mut s = json!({"ram_mb": 0, "manifest_url": "", "after_launch": "hide", "perf": "auto"});
    if let Ok(t) = std::fs::read_to_string(&paths().settings) {
        if let Ok(Value::Object(o)) = serde_json::from_str::<Value>(&t) {
            for (k, v) in o {
                s[k] = v;
            }
        }
    }
    if s["ram_mb"].as_u64().unwrap_or(0) == 0 {
        s["ram_mb"] = json!(recommended_ram_mb());
    }
    s
}

pub fn save_settings(s: &Value) {
    let _ = std::fs::create_dir_all(&paths().data);
    let _ = std::fs::write(&paths().settings, serde_json::to_string_pretty(s).unwrap_or_default());
}

pub fn manifest_url() -> String {
    let st = STATE.lock().unwrap();
    let custom = st.settings["manifest_url"].as_str().unwrap_or("").trim().to_string();
    if custom.is_empty() { config().manifest_url.clone() } else { custom }
}

// --------------------------------------------------------------------------------------
// Compte (chiffré par DPAPI : seul cet utilisateur Windows peut le relire)
// --------------------------------------------------------------------------------------

pub fn load_account() -> Option<Value> {
    let raw = std::fs::read(&paths().account).ok()?;
    let plain = sys::dpapi(&raw, false)?;
    serde_json::from_slice::<Value>(&plain).ok().filter(Value::is_object)
}

pub fn save_account(acc: Option<&Value>) {
    let _ = std::fs::create_dir_all(&paths().data);
    match acc {
        None => {
            let _ = std::fs::remove_file(&paths().account);
        }
        Some(a) => {
            if let Some(enc) = sys::dpapi(a.to_string().as_bytes(), true) {
                let _ = std::fs::write(&paths().account, enc);
            }
        }
    }
}

// --------------------------------------------------------------------------------------
// État partagé
// --------------------------------------------------------------------------------------

pub struct Skin {
    pub png: Vec<u8>,
    pub model: String,
    pub source: String,
}

pub struct State {
    pub account: Option<Value>,
    pub login: Option<Value>,
    pub task: Value,
    pub settings: Value,
    pub manifest: Option<Value>,
    pub manifest_error: String,
    pub ping: Option<Value>,
    pub favicon: Option<Vec<u8>>,
    pub favicon_v: u64,
    pub skin: Option<Skin>,
    pub skin_v: u64,
    pub skin_for: String,
    pub update: Value,
    pub installed: Option<bool>,
    pub official: bool,
    pub game_running: bool,
    pub window_open: bool,
}

pub static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| {
    Mutex::new(State {
        account: None,
        login: None,
        task: json!({"state": "idle", "step": "", "progress": 0.0, "detail": "", "error": ""}),
        settings: Value::Null,
        manifest: None,
        manifest_error: String::new(),
        ping: None,
        favicon: None,
        favicon_v: 0,
        skin: None,
        skin_v: 0,
        skin_for: String::new(),
        update: json!({"state": "none"}),
        installed: None,
        official: false,
        game_running: false,
        window_open: true,
    })
});

static NOTIFY: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

pub fn set_notifier(f: impl Fn() + Send + Sync + 'static) {
    let _ = NOTIFY.set(Box::new(f));
}

/// Prévient la fenêtre que l'état a changé (elle regroupe les envois).
pub fn notify() {
    if let Some(f) = NOTIFY.get() {
        f();
    }
}

pub fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let r = f(&mut STATE.lock().unwrap());
    notify();
    r
}

pub fn task_update(kv: &[(&str, Value)]) {
    with_state(|s| {
        for (k, v) in kv {
            s.task[*k] = v.clone();
        }
    });
}

#[macro_export]
macro_rules! task {
    ($($k:ident = $v:expr),* $(,)?) => {
        $crate::base::task_update(&[$((stringify!($k), serde_json::json!($v))),*])
    };
}

pub fn task_state() -> String {
    STATE.lock().unwrap().task["state"].as_str().unwrap_or("idle").to_string()
}

fn public_account(acc: &Option<Value>) -> Value {
    match acc {
        None => Value::Null,
        Some(a) => json!({"name": a["name"], "uuid": a["uuid"], "offline": a["type"] == "offline"}),
    }
}

/// Ce que voit l'interface (jamais les jetons du compte).
pub fn public_state() -> Value {
    let url = manifest_url();
    let s = STATE.lock().unwrap();
    let cfg = config();
    let sysinfo = system();
    let mut settings = s.settings.clone();
    if let Value::Object(ref mut o) = settings {
        o.insert("ram_max".into(), json!(max_ram_mb()));
        o.insert("ram_recommended".into(), json!(recommended_ram_mb()));
    }
    let skin = match &s.skin {
        Some(k) => json!({"v": s.skin_v, "model": k.model, "source": k.source}),
        None => json!({"v": s.skin_v, "model": "", "source": "default"}),
    };
    json!({
        "version": VERSION,
        "instance": instance(),
        "account": public_account(&s.account),
        "login": s.login,
        "task": s.task,
        "settings": settings,
        "manifest": s.manifest,
        "manifest_error": s.manifest_error,
        "manifest_url": url,
        "ping": s.ping,
        "favicon_v": if s.favicon.is_some() { s.favicon_v } else { 0 },
        "skin": skin,
        "update": s.update,
        "installed": s.installed,
        "allow_offline": cfg.allow_offline,
        "ms_configured": !cfg.client_id.is_empty(),
        "official": s.official,
        "system": {"ram_mb": sysinfo.ram_mb, "cpus": sysinfo.cpus, "gpu": sysinfo.gpus.first().cloned().unwrap_or_default(),
                   "tier": sysinfo.tier, "eco": auto_eco()},
    })
}
