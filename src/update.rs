//! Mise à jour du launcher (GitHub Releases), en arrière-plan : la fenêtre s'ouvre tout de suite, la nouvelle
//! version remplace l'exe dès qu'elle est téléchargée et sert au prochain démarrage (ou tout de suite sur clic).
use std::path::PathBuf;

use serde_json::json;
use sha2::Digest;

use crate::base::{arg_flag, config, with_state, VERSION};
use crate::net::{download, http_json, Body};

fn version_tuple(v: &str) -> Vec<u32> {
    v.trim().trim_start_matches(['v', 'V']).split('.').filter_map(|p| p.parse().ok()).collect()
}

fn exe() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

/// Restes d'une mise à jour précédente.
pub fn cleanup() {
    let Some(exe) = exe() else { return };
    let (Some(dir), Some(stem)) = (exe.parent(), exe.file_stem().map(|s| s.to_string_lossy().into_owned())) else { return };
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.starts_with(&format!("{stem}.old-")) && n.ends_with(".exe") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

pub fn check() {
    let repo = config().update_repo.clone();
    let Some(exe) = exe() else { return };
    // Pas en développement (exe de cargo) ni quand on le demande.
    let dev = exe.components().any(|c| c.as_os_str() == "target");
    if repo.is_empty() || dev || arg_flag("--no-update") {
        return;
    }
    cleanup();
    let rel = match http_json(&format!("https://api.github.com/repos/{repo}/releases/latest"), Body::None,
                              &[("Accept", "application/vnd.github+json")], 8) {
        Ok(r) => r,
        Err(_) => return,
    };
    let tag = rel["tag_name"].as_str().unwrap_or("").to_string();
    if tag.is_empty() || version_tuple(&tag) <= version_tuple(VERSION) {
        return;
    }
    let Some(asset) = rel["assets"].as_array().and_then(|l| {
        l.iter().find(|a| a["name"].as_str().unwrap_or("").to_lowercase().ends_with(".exe")).cloned()
    }) else { return };
    let version = tag.trim_start_matches(['v', 'V']).to_string();
    with_state(|st| st.update = json!({"state": "downloading", "version": version}));
    let stem = exe.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let new = exe.with_file_name(format!("{stem}.new.exe"));
    let _ = std::fs::remove_file(&new);
    let ok = (|| -> Option<()> {
        download(asset["browser_download_url"].as_str()?, &new, None, asset["size"].as_u64()).ok()?;
        if let Some(d) = asset["digest"].as_str().and_then(|d| d.strip_prefix("sha256:")) {
            let got = sha2::Sha256::digest(std::fs::read(&new).ok()?);
            let hex: String = got.iter().map(|b| format!("{b:02x}")).collect();
            if !hex.eq_ignore_ascii_case(d) {
                return None;
            }
        }
        // Windows accepte de renommer un exe en cours d'exécution, pas de l'écraser.
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
        std::fs::rename(&exe, exe.with_file_name(format!("{stem}.old-{secs}.exe"))).ok()?;
        if std::fs::rename(&new, &exe).is_err() {
            return None;
        }
        Some(())
    })();
    let _ = std::fs::remove_file(&new);
    with_state(|st| {
        st.update = if ok.is_some() { json!({"state": "ready", "version": version}) } else { json!({"state": "none"}) };
    });
}

/// Relance le launcher sur la nouvelle version.
pub fn restart() {
    if let Some(exe) = exe() {
        let args: Vec<String> = std::env::args().skip(1).collect();
        if std::process::Command::new(exe).args(args).spawn().is_ok() {
            std::process::exit(0);
        }
    }
}
