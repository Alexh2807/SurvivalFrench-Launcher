//! HTTP (connexions gardées ouvertes entre les téléchargements), empreintes et téléchargements parallèles.
use std::collections::HashMap;
use std::fmt;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{LazyLock, Mutex, OnceLock};
use std::time::{Duration, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::base::{paths, USER_AGENT};

/// Erreur affichée au joueur. `Login` : problème de compte (affiché comme tel par l'interface).
#[derive(Debug)]
pub enum Fail {
    Login(String),
    Other(String),
}

impl fmt::Display for Fail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fail::Login(s) | Fail::Other(s) => f.write_str(s),
        }
    }
}

impl From<String> for Fail {
    fn from(s: String) -> Self {
        Fail::Other(s)
    }
}

impl From<&str> for Fail {
    fn from(s: &str) -> Self {
        Fail::Other(s.to_string())
    }
}

impl From<std::io::Error> for Fail {
    fn from(e: std::io::Error) -> Self {
        Fail::Other(e.to_string())
    }
}

impl From<ureq::Error> for Fail {
    fn from(e: ureq::Error) -> Self {
        Fail::Other(e.to_string())
    }
}

pub type Res<T> = Result<T, Fail>;

pub fn agent() -> &'static ureq::Agent {
    static A: OnceLock<ureq::Agent> = OnceLock::new();
    A.get_or_init(|| {
        use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
        // Certificats du magasin Windows (comme le navigateur) : fonctionne derrière les antivirus qui inspectent HTTPS.
        let tls = TlsConfig::builder().provider(TlsProvider::NativeTls).root_certs(RootCerts::PlatformVerifier).build();
        ureq::Agent::config_builder()
            .tls_config(tls)
            .user_agent(USER_AGENT)
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(10)))
            .max_idle_connections_per_host(16)
            .build()
            .into()
    })
}

pub fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub fn form(pairs: &[(&str, &str)]) -> String {
    pairs.iter().map(|(k, v)| format!("{}={}", urlencode(k), urlencode(v))).collect::<Vec<_>>().join("&")
}

pub enum Body<'a> {
    None,
    Json(&'a Value),
    Form(String),
}

/// Requête JSON. Une réponse d'erreur HTTP est renvoyée telle quelle avec `_status` ; une panne réseau est une erreur.
pub fn http_json(url: &str, body: Body, headers: &[(&str, &str)], timeout: u64) -> Res<Value> {
    let a = agent();
    let t = Some(Duration::from_secs(timeout));
    let mut resp = match body {
        Body::None => {
            let mut r = a.get(url).config().timeout_global(t).build().header("Accept", "application/json");
            for (k, v) in headers {
                r = r.header(*k, *v);
            }
            r.call()?
        }
        Body::Json(v) => {
            let mut r = a.post(url).config().timeout_global(t).build().header("Accept", "application/json")
                .header("Content-Type", "application/json");
            for (k, v) in headers {
                r = r.header(*k, *v);
            }
            r.send(v.to_string())?
        }
        Body::Form(f) => {
            let mut r = a.post(url).config().timeout_global(t).build().header("Accept", "application/json")
                .header("Content-Type", "application/x-www-form-urlencoded");
            for (k, v) in headers {
                r = r.header(*k, *v);
            }
            r.send(f)?
        }
    };
    let status = resp.status().as_u16();
    let raw = resp.body_mut().with_config().limit(64 * 1024 * 1024).read_to_vec()?;
    let mut v: Value = if raw.is_empty() {
        json!({})
    } else {
        match serde_json::from_slice(&raw) {
            Ok(v) => v,
            Err(_) if status >= 400 => json!({"raw": String::from_utf8_lossy(&raw[..raw.len().min(300)])}),
            Err(e) => return Err(Fail::Other(format!("réponse illisible de {url} : {e}"))),
        }
    };
    if status >= 400 {
        if !v.is_object() {
            v = json!({"raw": v});
        }
        v["_status"] = json!(status);
    }
    Ok(v)
}

pub fn get_json(url: &str, timeout: u64) -> Res<Value> {
    http_json(url, Body::None, &[], timeout)
}

/// Contenu brut (images, petits fichiers) ; None si absent ou injoignable.
pub fn get_bytes(url: &str, timeout: u64) -> Option<Vec<u8>> {
    let mut resp = agent().get(url).config().timeout_global(Some(Duration::from_secs(timeout))).build().call().ok()?;
    if resp.status().as_u16() != 200 {
        return None;
    }
    resp.body_mut().with_config().limit(16 * 1024 * 1024).read_to_vec().ok()
}

// --------------------------------------------------------------------------------------
// Empreintes : un fichier déjà vérifié (même taille, même date) n'est pas relu à chaque lancement.
// --------------------------------------------------------------------------------------

static VERIFIED: LazyLock<Mutex<HashMap<String, (u64, u64, String)>>> = LazyLock::new(|| {
    let mut map = HashMap::new();
    if let Ok(t) = std::fs::read_to_string(paths().cache.join("verified.json")) {
        if let Ok(Value::Object(o)) = serde_json::from_str::<Value>(&t) {
            for (k, v) in o {
                if let (Some(s), Some(m), Some(h)) = (v[0].as_u64(), v[1].as_u64(), v[2].as_str()) {
                    map.insert(k, (s, m, h.to_string()));
                }
            }
        }
    }
    Mutex::new(map)
});
static VERIFIED_DIRTY: AtomicBool = AtomicBool::new(false);

pub fn save_verified() {
    if !VERIFIED_DIRTY.swap(false, Ordering::SeqCst) {
        return;
    }
    let map = VERIFIED.lock().unwrap();
    let o: serde_json::Map<String, Value> = map.iter().map(|(k, (s, m, h))| (k.clone(), json!([s, m, h]))).collect();
    let _ = std::fs::create_dir_all(&paths().cache);
    let _ = std::fs::write(paths().cache.join("verified.json"), Value::Object(o).to_string());
}

fn stamp(meta: &std::fs::Metadata) -> u64 {
    meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_nanos() as u64).unwrap_or(0)
}

pub fn sha1_of(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = sha1_smol::Sha1::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.digest().to_string())
}

fn remember(path: &Path, sha1: &str) {
    if let Ok(meta) = std::fs::metadata(path) {
        VERIFIED.lock().unwrap().insert(path.to_string_lossy().into_owned(), (meta.len(), stamp(&meta), sha1.to_string()));
        VERIFIED_DIRTY.store(true, Ordering::SeqCst);
    }
}

pub fn valid(path: &Path, sha1: Option<&str>, size: Option<u64>) -> bool {
    let Ok(meta) = std::fs::metadata(path) else { return false };
    if !meta.is_file() || size.is_some_and(|s| s != meta.len()) {
        return false;
    }
    let Some(expected) = sha1.filter(|s| !s.is_empty()) else { return true };
    let key = path.to_string_lossy().into_owned();
    if let Some((s, m, h)) = VERIFIED.lock().unwrap().get(&key) {
        if *s == meta.len() && *m == stamp(&meta) && h.eq_ignore_ascii_case(expected) {
            return true;
        }
    }
    match sha1_of(path) {
        Ok(h) if h.eq_ignore_ascii_case(expected) => {
            remember(path, &h);
            true
        }
        _ => false,
    }
}

pub fn download(url: &str, path: &Path, sha1: Option<&str>, size: Option<u64>) -> Res<()> {
    if valid(path, sha1, size) {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = PathBuf::from(format!("{}.part", path.display()));
    let mut last = String::new();
    for attempt in 0..3u64 {
        match fetch_to(url, &tmp, sha1) {
            Ok(()) => {
                std::fs::rename(&tmp, path).or_else(|_| {
                    let _ = std::fs::remove_file(path);
                    std::fs::rename(&tmp, path)
                })?;
                if let Some(h) = sha1.filter(|s| !s.is_empty()) {
                    remember(path, h);
                }
                return Ok(());
            }
            Err(e) => {
                last = e.to_string();
                std::thread::sleep(Duration::from_secs(1 + attempt));
            }
        }
    }
    let _ = std::fs::remove_file(&tmp);
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    Err(Fail::Other(format!("Téléchargement impossible : {name} ({last})\n{url}")))
}

fn fetch_to(url: &str, tmp: &Path, sha1: Option<&str>) -> Res<()> {
    let mut resp = agent().get(url).config().timeout_global(Some(Duration::from_secs(300))).build().call()?;
    let status = resp.status().as_u16();
    if status != 200 {
        return Err(Fail::Other(format!("HTTP {status}")));
    }
    let mut reader = resp.body_mut().with_config().limit(u64::MAX).reader();
    let mut out = std::fs::File::create(tmp)?;
    let mut h = sha1_smol::Sha1::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        out.write_all(&buf[..n])?;
    }
    out.flush()?;
    drop(out);
    if let Some(expected) = sha1.filter(|s| !s.is_empty()) {
        if !h.digest().to_string().eq_ignore_ascii_case(expected) {
            return Err(Fail::Other("empreinte incorrecte".into()));
        }
    }
    Ok(())
}

pub type Job = Box<dyn FnOnce() -> Res<()> + Send>;

/// Exécute les téléchargements en parallèle en faisant avancer la barre de `start` à `end`.
pub fn parallel(jobs: Vec<Job>, label: &str, start: f64, end: f64, workers: usize) -> Res<()> {
    let total = jobs.len();
    if total == 0 {
        crate::task!(progress = end);
        return Ok(());
    }
    let queue: Mutex<Vec<Job>> = Mutex::new(jobs.into_iter().rev().collect());
    let done = AtomicUsize::new(0);
    let failed: Mutex<Option<Fail>> = Mutex::new(None);
    std::thread::scope(|scope| {
        for _ in 0..workers.min(total) {
            scope.spawn(|| loop {
                if failed.lock().unwrap().is_some() {
                    return;
                }
                let Some(job) = queue.lock().unwrap().pop() else { return };
                if let Err(e) = job() {
                    failed.lock().unwrap().get_or_insert(e);
                    return;
                }
                let n = done.fetch_add(1, Ordering::SeqCst) + 1;
                if n % 8 == 0 || n == total {
                    crate::task!(progress = start + (end - start) * n as f64 / total as f64,
                                 detail = format!("{label} : {n} / {total}"));
                }
            });
        }
    });
    save_verified();
    match failed.into_inner().unwrap() {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Résout une adresse relative par rapport à celle du manifeste (comme urllib.parse.urljoin).
pub fn urljoin(base: &str, rel: &str) -> String {
    if rel.contains("://") {
        return rel.to_string();
    }
    let scheme_end = base.find("://").map(|i| i + 3).unwrap_or(0);
    if rel.starts_with('/') {
        let host_end = base[scheme_end..].find('/').map(|i| i + scheme_end).unwrap_or(base.len());
        return format!("{}{}", &base[..host_end], rel);
    }
    let cut = base.split(['?', '#']).next().unwrap_or(base);
    match cut.rfind('/') {
        Some(i) if i >= scheme_end => format!("{}{}", &cut[..=i], rel),
        _ => format!("{cut}/{rel}"),
    }
}

pub fn url_host(url: &str) -> Option<String> {
    let rest = url.split("://").nth(1)?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit('@').next()?;
    if let Some(stripped) = authority.strip_prefix('[') {
        return stripped.split(']').next().map(str::to_string);
    }
    Some(authority.split(':').next()?.to_string()).filter(|h| !h.is_empty())
}
