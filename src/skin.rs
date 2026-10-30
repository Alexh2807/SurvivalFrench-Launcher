//! Skin affiché dans le launcher : personnage RP enregistré par le mod (dernière partie), sinon skin du compte
//! Microsoft (Mojang), sinon personnage par défaut dessiné par l'interface. Mis en cache sur le disque.
use serde_json::Value;

use crate::base::{paths, with_state, Skin, STATE};
use crate::net::get_json;

/// Fichiers écrits par le mod SurvivalFrench à chaque partie (personnage RP composé) : prioritaires.
fn rp_skin() -> Option<Skin> {
    let dir = paths().game.join("survivalfrench");
    let png = std::fs::read(dir.join("launcher-skin.png")).ok()?;
    let meta: Value = std::fs::read(dir.join("launcher-skin.json")).ok()
        .and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
    Some(Skin { png, model: meta["model"].as_str().unwrap_or("").to_string(), source: "rp".into() })
}

fn mojang_skin(uuid: &str) -> Option<Skin> {
    let cache = paths().cache.join("skins");
    let file = cache.join(format!("{uuid}.png"));
    let model_file = cache.join(format!("{uuid}.model"));
    let fresh = std::fs::metadata(&file).ok().and_then(|m| m.modified().ok())
        .and_then(|t| t.elapsed().ok()).is_some_and(|age| age.as_secs() < 3600);
    if !fresh {
        if let Some((png, model)) = fetch_mojang(uuid) {
            let _ = std::fs::create_dir_all(&cache);
            let _ = std::fs::write(&file, &png);
            let _ = std::fs::write(&model_file, &model);
            return Some(Skin { png, model, source: "mojang".into() });
        }
    }
    let png = std::fs::read(&file).ok()?;
    let model = std::fs::read_to_string(&model_file).unwrap_or_default();
    Some(Skin { png, model, source: "mojang".into() })
}

fn fetch_mojang(uuid: &str) -> Option<(Vec<u8>, String)> {
    use base64::Engine;
    let p = get_json(&format!("https://sessionserver.mojang.com/session/minecraft/profile/{uuid}"), 8).ok()?;
    let prop = p["properties"].as_array()?.iter().find(|x| x["name"] == "textures")?;
    let raw = base64::engine::general_purpose::STANDARD.decode(prop["value"].as_str()?).ok()?;
    let tex: Value = serde_json::from_slice(&raw).ok()?;
    let skin = &tex["textures"]["SKIN"];
    let url = skin["url"].as_str()?.replace("http://", "https://");
    let png = crate::net::get_bytes(&url, 10)?;
    Some((png, skin["metadata"]["model"].as_str().unwrap_or("").to_string()))
}

/// Recharge le skin du compte courant en arrière-plan.
pub fn refresh() {
    std::thread::spawn(|| {
        let acc = STATE.lock().unwrap().account.clone();
        let key = acc.as_ref().map(|a| a["uuid"].as_str().unwrap_or("").to_string()).unwrap_or_default();
        let skin = match &acc {
            None => None,
            Some(a) => rp_skin().or_else(|| if a["type"] == "msa" { mojang_skin(&key) } else { None }),
        };
        with_state(|st| {
            let same = match (&st.skin, &skin) {
                (Some(a), Some(b)) => a.png == b.png && a.model == b.model,
                (None, None) => true,
                _ => false,
            };
            if !same || st.skin_for != key {
                st.skin = skin;
                st.skin_v += 1;
                st.skin_for = key;
            }
        });
    });
}
