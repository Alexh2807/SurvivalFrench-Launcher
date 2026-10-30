//! Manifeste du serveur et installation : Java, Minecraft, Fabric, ressources, mods et shaders.
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};

use crate::base::{manifest_url, paths, with_state, STATE};
use crate::net::{download, get_json, parallel, url_host, urljoin, Fail, Job, Res};
use crate::task;

const VERSION_MANIFEST: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const JAVA_RUNTIMES: &str =
    "https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";
const ASSET_URL: &str = "https://resources.download.minecraft.net";

fn s(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}

fn read_json(p: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(p).ok()?).ok()
}

// --------------------------------------------------------------------------------------
// Manifeste
// --------------------------------------------------------------------------------------

pub fn fetch_manifest() -> Option<Value> {
    let url = manifest_url();
    match get_json(&url, 8) {
        Ok(m) if m.get("minecraft").is_some() => {
            let installed = is_installed(&m);
            with_state(|st| {
                st.manifest = Some(m.clone());
                st.manifest_error.clear();
                st.installed = Some(installed);
            });
            Some(m)
        }
        Ok(m) => {
            let why = m.get("_status").map(|c| format!("HTTP {c}")).unwrap_or_else(|| "manifeste invalide".into());
            with_state(|st| st.manifest_error = format!("Serveur injoignable ({url}) : {why}"));
            None
        }
        Err(e) => {
            with_state(|st| st.manifest_error = format!("Serveur injoignable ({url}) : {e}"));
            None
        }
    }
}

/// Toutes les 30 s tant que la fenêtre est ouverte (rien pendant le jeu).
pub fn manifest_loop() {
    loop {
        let (open, playing) = {
            let st = STATE.lock().unwrap();
            (st.window_open, st.game_running)
        };
        if open && !playing {
            if fetch_manifest().is_some() {
                crate::ping::refresh();
            }
        }
        std::thread::sleep(Duration::from_secs(30));
    }
}

/// Le jeu est-il déjà installé (sans rien relire) ? Sert à afficher « JOUER » ou « INSTALLER ».
pub fn is_installed(m: &Value) -> bool {
    let p = paths();
    let mc = s(&m["minecraft"]);
    if !p.versions.join(mc).join(format!("{mc}.jar")).is_file() || !p.runtime.is_dir() {
        return false;
    }
    let mods = p.game.join("mods");
    m["mods"].as_array().map(|list| {
        list.iter().all(|md| {
            let name = file_name(s(&md["file"]));
            std::fs::metadata(mods.join(&name)).is_ok_and(|meta| md["size"].as_u64().is_none_or(|z| z == meta.len()))
        })
    }).unwrap_or(true)
}

pub fn file_name(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

// --------------------------------------------------------------------------------------
// Règles Mojang
// --------------------------------------------------------------------------------------

fn machine() -> String {
    std::env::var("PROCESSOR_ARCHITECTURE").unwrap_or_else(|_| "amd64".into()).to_lowercase()
}

pub fn rules_allow(rules: &Value, features: &Value) -> bool {
    let Some(list) = rules.as_array().filter(|l| !l.is_empty()) else { return true };
    let mut allowed = false;
    let m = machine();
    for rule in list {
        let mut ok = true;
        if let Some(os) = rule.get("os").and_then(Value::as_object).filter(|o| !o.is_empty()) {
            if let Some(name) = os.get("name").and_then(Value::as_str) {
                if !name.is_empty() && name != "windows" {
                    ok = false;
                }
            }
            if os.get("arch").and_then(Value::as_str) == Some("x86") && matches!(m.as_str(), "amd64" | "x86_64" | "arm64") {
                ok = false;
            }
        }
        if let Some(f) = rule.get("features").and_then(Value::as_object) {
            for (k, wanted) in f {
                if features.get(k).and_then(Value::as_bool).unwrap_or(false) != wanted.as_bool().unwrap_or(false) {
                    ok = false;
                }
            }
        }
        if ok {
            allowed = rule["action"] == "allow";
        }
    }
    allowed
}

pub fn maven_path(name: &str) -> String {
    let parts: Vec<&str> = name.split(':').collect();
    let (group, artifact, version) = (parts[0], parts.get(1).copied().unwrap_or(""), parts.get(2).copied().unwrap_or(""));
    let classifier = parts.get(3).map(|c| format!("-{c}")).unwrap_or_default();
    format!("{}/{artifact}/{version}/{artifact}-{version}{classifier}.jar", group.replace('.', "/"))
}

// --------------------------------------------------------------------------------------
// Installation
// --------------------------------------------------------------------------------------

pub fn install_java(component: &str) -> Res<PathBuf> {
    let target = paths().runtime.join(component);
    let javaw = target.join("bin").join("javaw.exe");
    let marker = target.join(".installed");
    if javaw.is_file() && marker.is_file() {
        return Ok(javaw);
    }
    task!(step = "Java", detail = "Préparation de Java…");
    let runtimes = get_json(JAVA_RUNTIMES, 30)?;
    let key = if machine() == "arm64" { "windows-arm64" } else { "windows-x64" };
    let entry = runtimes[key][component].get(0).cloned()
        .ok_or_else(|| Fail::Other(format!("Aucun Java « {component} » disponible pour {key}.")))?;
    let files = get_json(s(&entry["manifest"]["url"]), 30)?;
    let mut jobs: Vec<Job> = Vec::new();
    if let Some(map) = files["files"].as_object() {
        for (rel, info) in map {
            let dest = target.join(rel);
            match s(&info["type"]) {
                "directory" => std::fs::create_dir_all(&dest)?,
                "file" => {
                    let raw = info["downloads"]["raw"].clone();
                    jobs.push(Box::new(move || {
                        download(s(&raw["url"]), &dest, raw["sha1"].as_str(), raw["size"].as_u64())
                    }));
                }
                _ => {}
            }
        }
    }
    parallel(jobs, "Java", 0.05, 0.2, 12)?;
    std::fs::write(&marker, "ok")?;
    Ok(javaw)
}

pub struct Prepared {
    pub vjson: Value,
    pub fabric: Value,
    pub javaw: PathBuf,
    pub classpath: Vec<PathBuf>,
    pub log_cfg: Option<PathBuf>,
    pub version: String,
    pub assets_index: String,
}

// Une version de Minecraft ou un profil Fabric (version du jeu + version du chargeur) ne changent jamais une fois
// publiés : déjà sur le disque, ils ne sont pas redemandés (près de 10 s gagnées à chaque lancement).

/// Profil Fabric : téléchargé au premier lancement de ce couple de versions, puis lu sur le disque.
fn fabric_profile(mc: &str, loader: &str) -> Res<Value> {
    let path = paths().versions.join(format!("fabric-{loader}-{mc}.json"));
    if let Some(f) = read_json(&path).filter(|f| f.get("mainClass").is_some()) {
        return Ok(f);
    }
    let url = format!("https://meta.fabricmc.net/v2/versions/loader/{mc}/{loader}/profile/json");
    let f = get_json(&url, 20)?;
    if f.get("mainClass").is_none() {
        return Err("Fabric indisponible pour cette version.".into());
    }
    let _ = std::fs::create_dir_all(&paths().versions);
    let _ = std::fs::write(&path, f.to_string());
    Ok(f)
}

/// Description de la version : téléchargée au premier lancement, puis lue sur le disque.
fn version_json(mc: &str) -> Res<Value> {
    let vjson_path = paths().versions.join(mc).join(format!("{mc}.json"));
    if let Some(v) = read_json(&vjson_path).filter(|v| v.get("mainClass").is_some()) {
        return Ok(v);
    }
    match get_json(VERSION_MANIFEST, 20) {
        Ok(vm) => {
            let entry = vm["versions"].as_array().and_then(|l| l.iter().find(|v| v["id"] == mc).cloned())
                .ok_or_else(|| Fail::Other(format!("Version {mc} introuvable chez Mojang.")))?;
            download(s(&entry["url"]), &vjson_path, entry["sha1"].as_str(), None)?;
        }
        Err(e) if !vjson_path.is_file() => return Err(e),
        Err(_) => {}
    }
    read_json(&vjson_path).ok_or_else(|| Fail::Other(format!("Description de la version {mc} illisible.")))
}

/// Installe ou vérifie tout ce qu'il faut pour lancer, et retourne de quoi construire la ligne de commande.
pub fn prepare(m: &Value) -> Res<Prepared> {
    let mc = s(&m["minecraft"]).to_string();
    let loader = s(&m["fabric_loader"]).to_string();
    task!(step = "Minecraft", progress = 0.02, detail = format!("Version {mc}…"));
    let p = paths();
    let (vjson, fabric) = std::thread::scope(|sc| {
        let f = sc.spawn(|| fabric_profile(&mc, &loader));
        let v = version_json(&mc);
        (v, f.join().unwrap_or_else(|_| Err("Fabric indisponible.".into())))
    });
    let (vjson, fabric) = (vjson?, fabric?);
    crate::base::trace("préparation : version et Fabric");
    let vdir = p.versions.join(&mc);

    let component = vjson["javaVersion"]["component"].as_str().unwrap_or("java-runtime-epsilon").to_string();
    let javaw = install_java(&component)?;
    crate::base::trace("préparation : Java");

    // Bibliothèques : Fabric d'abord (prioritaires), puis Minecraft.
    task!(step = "Bibliothèques", progress = 0.2, detail = "Vérification…");
    let mut classpath: Vec<PathBuf> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut jobs: Vec<Job> = Vec::new();
    for lib in fabric["libraries"].as_array().into_iter().flatten() {
        let name = s(&lib["name"]);
        seen.insert(name.split(':').take(2).collect::<Vec<_>>().join(":"));
        let rel = maven_path(name);
        let dest = p.libraries.join(&rel);
        let base = lib["url"].as_str().unwrap_or("https://maven.fabricmc.net/").to_string();
        let sha1 = lib["sha1"].as_str().map(str::to_string);
        let d = dest.clone();
        jobs.push(Box::new(move || download(&format!("{base}{rel}"), &d, sha1.as_deref(), None)));
        classpath.push(dest);
    }
    for lib in vjson["libraries"].as_array().into_iter().flatten() {
        if !rules_allow(&lib["rules"], &Value::Null) {
            continue;
        }
        let parts: Vec<&str> = s(&lib["name"]).split(':').collect();
        let mut key = parts.iter().take(2).copied().collect::<Vec<_>>().join(":");
        if let Some(c) = parts.get(3) {
            key = format!("{key}:{c}");
        }
        if !seen.insert(key) {
            continue;
        }
        let art = &lib["downloads"]["artifact"];
        if !art.is_object() {
            continue;
        }
        let dest = p.libraries.join(s(&art["path"]));
        let a = art.clone();
        let d = dest.clone();
        jobs.push(Box::new(move || download(s(&a["url"]), &d, a["sha1"].as_str(), a["size"].as_u64())));
        classpath.push(dest);
    }
    let client = vjson["downloads"]["client"].clone();
    let client_jar = vdir.join(format!("{mc}.jar"));
    {
        let d = client_jar.clone();
        jobs.push(Box::new(move || download(s(&client["url"]), &d, client["sha1"].as_str(), client["size"].as_u64())));
    }
    classpath.push(client_jar);
    let mut log_cfg = None;
    let logging = vjson["logging"]["client"].clone();
    if logging.is_object() {
        let path = p.versions.join("logging").join(s(&logging["file"]["id"]));
        log_cfg = Some(path.clone());
        jobs.push(Box::new(move || download(s(&logging["file"]["url"]), &path, logging["file"]["sha1"].as_str(), None)));
    }
    parallel(jobs, "Bibliothèques", 0.2, 0.4, 8)?;
    crate::base::trace("préparation : bibliothèques");

    // Assets (sons, langues, textures) : réutilisés s'ils existent déjà sur le PC.
    task!(step = "Ressources du jeu", progress = 0.4, detail = "Index…");
    let ai = vjson["assetIndex"].clone();
    let index_path = p.assets.join("indexes").join(format!("{}.json", s(&ai["id"])));
    download(s(&ai["url"]), &index_path, ai["sha1"].as_str(), None)?;
    let index = read_json(&index_path).ok_or("Index des ressources illisible.")?;
    let mut ajobs: Vec<Job> = Vec::new();
    let mut hashes = std::collections::HashSet::new();
    for obj in index["objects"].as_object().into_iter().flat_map(|o| o.values()) {
        let h = s(&obj["hash"]).to_string();
        let size = obj["size"].as_u64().unwrap_or(0);
        if h.len() < 2 || !hashes.insert(h.clone()) {
            continue;
        }
        let dest = p.assets.join("objects").join(&h[..2]).join(&h);
        if std::fs::metadata(&dest).is_ok_and(|m| m.len() == size) {
            continue;
        }
        ajobs.push(Box::new(move || fetch_asset(&h, size, &dest)));
    }
    parallel(ajobs, "Ressources", 0.4, 0.85, 16)?;
    crate::base::trace("préparation : ressources");

    sync_mods(m)?;
    crate::base::trace("préparation : mods");

    Ok(Prepared {
        version: mc,
        assets_index: s(&ai["id"]).to_string(),
        vjson,
        fabric,
        javaw,
        classpath,
        log_cfg,
    })
}

fn fetch_asset(h: &str, size: u64, dest: &Path) -> Res<()> {
    for base in &paths().asset_reuse {
        let src = base.join(&h[..2]).join(h);
        if std::fs::metadata(&src).is_ok_and(|m| m.is_file() && m.len() == size) {
            if let Some(dir) = dest.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if std::fs::copy(&src, dest).is_ok() && crate::net::sha1_of(dest).is_ok_and(|x| x == h) {
                return Ok(());
            }
        }
    }
    download(&format!("{ASSET_URL}/{}/{h}", &h[..2]), dest, Some(h), Some(size))
}

/// Le dossier mods reflète exactement le manifeste du serveur (mods ajoutés, mis à jour ou retirés).
pub fn sync_mods(m: &Value) -> Res<()> {
    task!(step = "Mods du serveur", progress = 0.85, detail = "Synchronisation…");
    let mods_dir = paths().game.join("mods");
    std::fs::create_dir_all(&mods_dir)?;
    let base = manifest_url();
    let mut wanted = std::collections::HashSet::new();
    let mut jobs: Vec<Job> = Vec::new();
    for md in m["mods"].as_array().into_iter().flatten() {
        let name = file_name(s(&md["file"]));
        wanted.insert(name.clone());
        let url = urljoin(&base, s(&md["url"]));
        let dest = mods_dir.join(&name);
        let sha1 = md["sha1"].as_str().map(str::to_string);
        let size = md["size"].as_u64();
        jobs.push(Box::new(move || download(&url, &dest, sha1.as_deref(), size)));
    }
    parallel(jobs, "Mods", 0.85, 0.97, 4)?;
    for entry in std::fs::read_dir(&mods_dir)?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.to_lowercase().ends_with(".jar") && !wanted.contains(&name) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    sync_shaderpacks(m)
}

// Qualité des shaders selon la carte graphique : profil du pack (Complementary : POTATO à ULTRA) et échelle de
// rendu de Vitrail (le monde est rendu plus petit puis agrandi, l'interface reste nette).
// Sur une puce intégrée, les shaders sont installés mais coupés (touche K) : le but est 100 images/s ou plus.
fn gpu_tier_settings(tier: &str) -> (bool, &'static str, u32) {
    match tier {
        "low" => (false, "POTATO", 67),
        "high" => (true, "MEDIUM", 100),
        _ => (true, "LOW", 85),
    }
}

// Réglages vidéo de départ selon la carte (appliqués une seule fois, le joueur peut les changer ensuite).
fn perf_options(tier: &str) -> Vec<(&'static str, &'static str)> {
    match tier {
        "low" => vec![("renderDistance", "8"), ("simulationDistance", "6"), ("biomeBlendRadius", "0"),
                      ("entityShadows", "false"), ("cloudRange", "64"), ("renderClouds", "\"fast\""), ("mipmapLevels", "2"),
                      ("maxAnisotropyBit", "1"), ("entityDistanceScaling", "0.75"), ("weatherRadius", "5"),
                      ("chunkSectionFadeInTime", "0.0"), ("enableVsync", "false"), ("maxFps", "260"),
                      ("graphicsPreset", "\"custom\"")],
        "high" => vec![("enableVsync", "false"), ("maxFps", "260")],
        _ => vec![("renderDistance", "10"), ("simulationDistance", "8"), ("biomeBlendRadius", "1"),
                  ("enableVsync", "false"), ("maxFps", "260")],
    }
}

const SHADER_MARKER: &str = "survivalfrench-shaders.done";
const PERF_MARKER: &str = "survivalfrench-perf.done";
const VULKAN_MARKER: &str = "survivalfrench-vulkan.done";

/// « rx », une espace facultative, puis au moins `min` chiffres dont le premier est dans `first` (rx ?[6-9]\d{3}).
fn contains_rx(n: &str, first: std::ops::RangeInclusive<char>, min: usize) -> bool {
    let b: Vec<char> = n.chars().collect();
    (0..b.len().saturating_sub(1)).any(|i| {
        if b[i] != 'r' || b[i + 1] != 'x' {
            return false;
        }
        let j = if b.get(i + 2) == Some(&' ') { i + 3 } else { i + 2 };
        let digits = b.get(j..).map_or(0, |rest| rest.iter().take_while(|c| c.is_ascii_digit()).count());
        digits >= min && b.get(j).is_some_and(|c| first.contains(c))
    })
}

fn word(n: &str, w: &str) -> bool {
    n.match_indices(w).any(|(i, _)| {
        let before = n[..i].chars().last().is_none_or(|c| !c.is_alphanumeric() && c != '_');
        let after = n[i + w.len()..].chars().next().is_none_or(|c| !c.is_alphanumeric() && c != '_');
        before && after
    })
}

/// Meilleure carte trouvée : « high » (RTX, RX 6000+, Arc), « mid » (autre carte dédiée), « low » (puce intégrée).
pub fn gpu_tier(names: &[String]) -> &'static str {
    let mut best = "";
    for name in names {
        let n = name.to_lowercase();
        if n.contains("rtx") || contains_rx(&n, '6'..='9', 4) || word(&n, "arc") {
            return "high";
        }
        let m_graphics = n.match_indices("m graphics").any(|(i, _)| {
            i >= 3 && n[..i].chars().rev().take(3).all(|c| c.is_ascii_digit())
        });
        let intel_hd = n.find("intel").is_some_and(|i| n[i..].contains("hd"));
        if n.contains("gtx") || contains_rx(&n, '0'..='9', 3) || n.contains("radeon pro") || n.contains("quadro") {
            best = "mid";
        } else if best.is_empty() && (n.contains("uhd") || n.contains("iris") || intel_hd || n.contains("radeon(tm)")
            || n.contains("radeon graphics") || n.contains("vega") || m_graphics) {
            best = "low";
        }
    }
    if best.is_empty() { "mid" } else { best }
}

/// Packs de shaders du manifeste (téléchargés dans shaderpacks/), et au premier lancement seulement :
/// le premier pack activé dans Vitrail, avec une qualité adaptée à la carte graphique.
fn sync_shaderpacks(m: &Value) -> Res<()> {
    let Some(packs) = m["shaderpacks"].as_array().filter(|l| !l.is_empty()) else { return Ok(()) };
    let game = &paths().game;
    let pack_dir = game.join("shaderpacks");
    std::fs::create_dir_all(&pack_dir)?;
    let base = manifest_url();
    for pack in packs {
        let name = file_name(s(&pack["file"]));
        download(&urljoin(&base, s(&pack["url"])), &pack_dir.join(&name), pack["sha1"].as_str(), pack["size"].as_u64())?;
    }
    let marker = game.join("config").join(SHADER_MARKER);
    if marker.exists() {
        return Ok(());
    }
    let first = file_name(s(&packs[0]["file"]));
    let (shaders, profile, scale) = gpu_tier_settings(crate::base::system().tier);
    let vitrail = game.join("vitrail").join("pack.txt");
    std::fs::create_dir_all(vitrail.parent().unwrap())?;
    let enabled = if shaders { "true" } else { "false" };
    std::fs::write(&vitrail, format!("pack={first}\nenabled={enabled}\nrenderscale={scale}\n"))?;
    let settings = pack_dir.join(format!("{first}.txt"));
    if !settings.exists() {
        std::fs::write(&settings, format!("profile={profile}\n"))?;
    }
    std::fs::create_dir_all(marker.parent().unwrap())?;
    std::fs::write(&marker, format!("shaders : {first} ({enabled}), profil {profile}, échelle {scale} %"))?;
    Ok(())
}

// Réglages imposés à chaque lancement : pas de tutoriel (« Bougez avec Z, Q, S et D »), pas d'écran
// d'accessibilité ni d'avertissement multijoueur au premier démarrage, narrateur coupé.
const FORCED_OPTIONS: [(&str, &str); 5] = [("tutorialStep", "none"), ("onboardAccessibility", "false"),
    ("skipMultiplayerWarning", "true"), ("joinedFirstServer", "true"), ("narrator", "0")];
// Réglages par défaut seulement (le joueur peut les changer ensuite).
const DEFAULT_OPTIONS: [(&str, &str); 3] = [("lang", "fr_fr"), ("resourcePacks", "[\"vanilla\"]"),
    ("preferredGraphicsBackend", "\"vulkan\"")];

/// options.txt : français par défaut, tutoriel et écrans d'accueil désactivés. Les touches suivent déjà la
/// position physique du clavier : en AZERTY, Minecraft affiche et utilise Z, Q, S, D.
pub fn first_run_options() {
    let game = &paths().game;
    let _ = std::fs::create_dir_all(game);
    let opts = game.join("options.txt");
    let text = std::fs::read(&opts).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    let mut values: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for line in text.lines() {
        if let Some((k, v)) = line.split_once(':') {
            if values.insert(k.to_string(), v.to_string()).is_none() {
                order.push(k.to_string());
            }
        }
    }
    let set = |k: &str, v: &str, values: &mut std::collections::HashMap<String, String>, order: &mut Vec<String>| {
        if values.insert(k.to_string(), v.to_string()).is_none() {
            order.push(k.to_string());
        }
    };
    for (k, v) in DEFAULT_OPTIONS {
        if !values.contains_key(k) {
            set(k, v, &mut values, &mut order);
        }
    }
    // Minecraft écrit « default » dès le premier lancement : une seule fois par installation, ce choix passe sur Vulkan.
    let vulkan = game.join("config").join(VULKAN_MARKER);
    if !vulkan.exists() {
        set("preferredGraphicsBackend", "\"vulkan\"", &mut values, &mut order);
        let _ = std::fs::create_dir_all(vulkan.parent().unwrap());
        let _ = std::fs::write(&vulkan, "graphisme : Vulkan par défaut");
    }
    let perf = game.join("config").join(PERF_MARKER);
    if !perf.exists() {
        let tier = crate::base::system().tier;
        for (k, v) in perf_options(tier) {
            set(k, v, &mut values, &mut order);
        }
        let _ = std::fs::create_dir_all(perf.parent().unwrap());
        let _ = std::fs::write(&perf, format!("réglages vidéo de départ : carte « {tier} »"));
    }
    for (k, v) in FORCED_OPTIONS {
        set(k, v, &mut values, &mut order);
    }
    let out: String = order.iter().map(|k| format!("{k}:{}\n", values[k])).collect();
    let _ = std::fs::write(&opts, out);
}

fn nbt_string(v: &str) -> Vec<u8> {
    let raw = v.as_bytes();
    let mut out = (raw.len() as u16).to_be_bytes().to_vec();
    out.extend_from_slice(raw);
    out
}

/// servers.dat (NBT non compressé) : le serveur en tête de la liste Multijoueur.
pub fn write_servers_dat(name: &str, address: &str) {
    let mut entry = vec![0x08];
    entry.extend(nbt_string("name"));
    entry.extend(nbt_string(name));
    entry.push(0x08);
    entry.extend(nbt_string("ip"));
    entry.extend(nbt_string(address));
    entry.push(0x01);
    entry.extend(nbt_string("acceptTextures"));
    entry.push(0x01);
    entry.push(0x00);
    let mut data = vec![0x0a];
    data.extend(nbt_string(""));
    data.push(0x09);
    data.extend(nbt_string("servers"));
    data.push(0x0a);
    data.extend(1i32.to_be_bytes());
    data.extend(entry);
    data.push(0x00);
    let _ = std::fs::create_dir_all(&paths().game);
    let _ = std::fs::write(paths().game.join("servers.dat"), data);
}

/// (hôte, port) du serveur Minecraft annoncé par le manifeste.
pub fn server_host_port(m: &Value) -> (String, u16) {
    let server = &m["server"];
    let mut host = server["address"].as_str().unwrap_or("").to_string();
    if host.is_empty() {
        host = url_host(&manifest_url()).unwrap_or_else(|| "localhost".into());
    }
    let port = server["port"].as_u64().unwrap_or(25565) as u16;
    if let Some((h, p)) = host.rsplit_once(':') {
        if let Ok(p) = p.parse() {
            return (h.to_string(), p);
        }
    }
    (host, port)
}

/// Adresse affichée dans la liste des serveurs (sans le port s'il est standard).
pub fn server_address(m: &Value) -> String {
    let server = &m["server"];
    let raw = server["address"].as_str().unwrap_or("");
    let host = if raw.is_empty() { url_host(&manifest_url()).unwrap_or_else(|| "localhost".into()) } else { raw.to_string() };
    let port = server["port"].as_u64().unwrap_or(25565);
    if host.contains(':') || port == 25565 { host } else { format!("{host}:{port}") }
}

// --------------------------------------------------------------------------------------
// Launcher Minecraft officiel (connexion Microsoft gérée par Mojang)
// --------------------------------------------------------------------------------------

const OFFICIAL_STORE_FAMILY: &str = "Microsoft.4297127D64EC6_8wekyb3d8bbwe";
const PROFILE_ID: &str = "survivalfrench";

/// Commande pour ouvrir le launcher officiel, ou None s'il n'est pas installé.
pub fn official_launcher() -> Option<Vec<String>> {
    for var in [r"%ProgramFiles(x86)%\Minecraft Launcher\MinecraftLauncher.exe",
                r"%ProgramFiles%\Minecraft Launcher\MinecraftLauncher.exe",
                r"%LocalAppData%\Programs\Minecraft Launcher\MinecraftLauncher.exe"] {
        let exe = expand_env(var);
        if Path::new(&exe).is_file() {
            return Some(vec![exe]);
        }
    }
    let pkg = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("Packages").join(OFFICIAL_STORE_FAMILY);
    if pkg.is_dir() {
        return Some(vec!["explorer.exe".into(), format!("shell:AppsFolder\\{OFFICIAL_STORE_FAMILY}!Minecraft")]);
    }
    None
}

fn expand_env(s: &str) -> String {
    let mut out = s.to_string();
    for var in ["ProgramFiles(x86)", "ProgramFiles", "LocalAppData"] {
        let key = format!("%{var}%");
        if out.contains(&key) {
            out = out.replace(&key, &std::env::var(var).unwrap_or_default());
        }
    }
    out
}

/// Crée/actualise le profil « SurvivalFrench RP » dans le launcher officiel et y synchronise les mods.
pub fn install_official(m: &Value) -> Res<()> {
    let mc = s(&m["minecraft"]);
    let loader = s(&m["fabric_loader"]);
    task!(step = "Fabric", progress = 0.05, detail = "Profil Fabric…");
    let fabric = fabric_profile(mc, loader)?;
    let id = s(&fabric["id"]).to_string();
    if id.is_empty() {
        return Err("Fabric indisponible pour cette version.".into());
    }
    let mdir = &paths().minecraft;
    let vdir = mdir.join("versions").join(&id);
    std::fs::create_dir_all(&vdir)?;
    std::fs::write(vdir.join(format!("{id}.json")), serde_json::to_string_pretty(&fabric).unwrap_or_default())?;

    sync_mods(m)?;
    first_run_options();
    let name = m["name"].as_str().unwrap_or("SurvivalFrench RP");
    write_servers_dat(name, &server_address(m));

    task!(step = "Profil", progress = 0.97, detail = "Ajout du profil au launcher Minecraft…");
    let profiles_path = mdir.join("launcher_profiles.json");
    let mut profiles = read_json(&profiles_path).filter(Value::is_object).unwrap_or_else(|| json!({}));
    let backup = mdir.join("launcher_profiles.json.bak-survivalfrench");
    if profiles_path.is_file() && !backup.exists() {
        let _ = std::fs::copy(&profiles_path, &backup);
    }
    if !profiles["profiles"].is_object() {
        profiles["profiles"] = json!({});
    }
    if !profiles["settings"].is_object() {
        profiles["settings"] = json!({});
    }
    if profiles.get("version").is_none() {
        profiles["version"] = json!(3);
    }
    let now = iso_now();
    let created = profiles["profiles"][PROFILE_ID]["created"].as_str().unwrap_or(&now).to_string();
    use base64::Engine;
    let icon = format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(crate::ui::LOGO));
    let ram = STATE.lock().unwrap().settings["ram_mb"].as_u64().unwrap_or(4096);
    profiles["profiles"][PROFILE_ID] = json!({
        "name": name, "type": "custom", "created": created, "lastUsed": now, "lastVersionId": id,
        "gameDir": paths().game.to_string_lossy(), "javaArgs": format!("-Xmx{ram}M -Xms512M -XX:+UseG1GC"), "icon": icon,
    });
    std::fs::create_dir_all(mdir)?;
    std::fs::write(&profiles_path, serde_json::to_string_pretty(&profiles).unwrap_or_default())?;
    Ok(())
}

fn iso_now() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // Jours depuis 1970 → date (algorithme de Howard Hinnant).
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if mo <= 2 { 1 } else { 0 };
    format!("{y:04}-{mo:02}-{d:02}T{:02}:{:02}:{:02}.000Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

pub fn play_official() {
    let r = (|| -> Res<()> {
        let cmd = official_launcher().ok_or_else(|| Fail::Login("Le launcher Minecraft officiel n'est pas installé.".into()))?;
        task!(state = "running", step = "Serveur", progress = 0.0, detail = "Lecture du manifeste…", error = "");
        let m = fetch_manifest().ok_or_else(|| Fail::Other(STATE.lock().unwrap().manifest_error.clone()))?;
        install_official(&m)?;
        std::process::Command::new(&cmd[0]).args(&cmd[1..]).spawn()?;
        let name = m["name"].as_str().unwrap_or("SurvivalFrench RP");
        task!(state = "official", step = "Prêt !", progress = 1.0,
              detail = format!("Dans le launcher Minecraft : choisis le profil « {name} », clique sur JOUER, puis Multijoueur → {name}."));
        Ok(())
    })();
    match r {
        Ok(()) => {}
        Err(Fail::Login(e)) => task!(state = "error", step = "Launcher Minecraft", error = e),
        Err(Fail::Other(e)) => task!(state = "error", step = "Installation", error = e),
    }
}
