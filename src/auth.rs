//! Connexion Microsoft → Xbox Live → Minecraft (par code : aucun mot de passe ne passe par le launcher).
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::base::{config, save_account, with_state, STATE};
use crate::net::{form, http_json, Body, Fail, Res};

const MS_DEVICE: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";
const MS_TOKEN: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const XBL_AUTH: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_AUTH: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MC_LOGIN: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";
const MC_PROFILE: &str = "https://api.minecraftservices.com/minecraft/profile";
const MS_SCOPE: &str = "XboxLive.signin offline_access";

pub fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}

pub fn ms_device_login() {
    let result = (|| -> Res<()> {
        let client_id = config().client_id.clone();
        if client_id.is_empty() {
            return Err(Fail::Login("Connexion Microsoft non configurée (client_id manquant dans launcher.json).".into()));
        }
        let r = http_json(MS_DEVICE, Body::Form(form(&[("client_id", &client_id), ("scope", MS_SCOPE)])), &[], 30)?;
        if r.get("device_code").is_none() {
            let msg = r["error_description"].as_str().unwrap_or("Microsoft refuse la demande de connexion.");
            return Err(Fail::Login(msg.to_string()));
        }
        let url = r["verification_uri"].as_str().unwrap_or("https://www.microsoft.com/link").to_string();
        with_state(|st| {
            st.login = Some(json!({"user_code": r["user_code"], "url": url,
                                   "expires": now() + r["expires_in"].as_f64().unwrap_or(900.0), "error": ""}));
        });
        let mut interval = r["interval"].as_u64().unwrap_or(5);
        loop {
            std::thread::sleep(Duration::from_secs(interval));
            if STATE.lock().unwrap().login.is_none() {
                return Ok(()); // annulé
            }
            let t = http_json(MS_TOKEN, Body::Form(form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("client_id", &client_id),
                ("device_code", s(&r["device_code"])),
            ])), &[], 30)?;
            match t["error"].as_str() {
                Some("authorization_pending") => continue,
                Some("slow_down") => {
                    interval += 5;
                    continue;
                }
                Some("expired_token") => return Err(Fail::Login("Le code a expiré, recommence.".into())),
                Some("authorization_declined") => return Err(Fail::Login("Connexion refusée.".into())),
                Some(err) => return Err(Fail::Login(t["error_description"].as_str().unwrap_or(err).to_string())),
                None => {}
            }
            let acc = minecraft_from_ms(s(&t["access_token"]), s(&t["refresh_token"]))?;
            save_account(Some(&acc));
            with_state(|st| {
                st.account = Some(acc);
                st.login = None;
            });
            crate::skin::refresh();
            return Ok(());
        }
    })();
    if let Err(e) = result {
        let msg = match e {
            Fail::Login(m) => m,
            Fail::Other(m) => format!("Erreur de connexion : {m}"),
        };
        with_state(|st| st.login = Some(json!({"error": msg})));
    }
}

pub fn minecraft_from_ms(ms_token: &str, refresh_token: &str) -> Res<Value> {
    let xbl = http_json(XBL_AUTH, Body::Json(&json!({
        "Properties": {"AuthMethod": "RPS", "SiteName": "user.auth.xboxlive.com", "RpsTicket": format!("d={ms_token}")},
        "RelyingParty": "http://auth.xboxlive.com", "TokenType": "JWT"})), &[], 30)?;
    if xbl.get("Token").is_none() {
        return Err(Fail::Login("Xbox Live refuse la connexion.".into()));
    }
    let uhs = s(&xbl["DisplayClaims"]["xui"][0]["uhs"]).to_string();
    let xsts = http_json(XSTS_AUTH, Body::Json(&json!({
        "Properties": {"SandboxId": "RETAIL", "UserTokens": [xbl["Token"]]},
        "RelyingParty": "rp://api.minecraftservices.com/", "TokenType": "JWT"})), &[], 30)?;
    if xsts.get("Token").is_none() {
        let code = xsts["XErr"].as_u64().unwrap_or(0);
        return Err(Fail::Login(match code {
            2148916233 => "Ce compte Microsoft n'a pas de profil Xbox : crée-le sur xbox.com puis réessaie.".into(),
            2148916235 => "Xbox Live n'est pas disponible dans ton pays.".into(),
            2148916238 => "Compte mineur : il doit être ajouté à une famille Microsoft par un adulte.".into(),
            _ => format!("Xbox Live refuse l'accès (code {code})."),
        }));
    }
    let xid = xsts["DisplayClaims"]["xui"][0]["xid"].as_str().unwrap_or("0").to_string();
    let mc = http_json(MC_LOGIN, Body::Json(&json!({"identityToken": format!("XBL3.0 x={uhs};{}", s(&xsts["Token"]))})), &[], 30)?;
    if mc.get("access_token").is_none() {
        if mc["_status"] == 403 {
            return Err(Fail::Login("Cette application n'est pas encore autorisée par Mojang (validation de l'app Azure en attente).".into()));
        }
        return Err(Fail::Login("Minecraft refuse la connexion.".into()));
    }
    let token = s(&mc["access_token"]).to_string();
    let profile = http_json(MC_PROFILE, Body::None, &[("Authorization", &format!("Bearer {token}"))], 30)?;
    if profile.get("id").is_none() {
        return Err(Fail::Login("Ce compte ne possède pas Minecraft Java Edition.".into()));
    }
    Ok(json!({"type": "msa", "name": profile["name"], "uuid": profile["id"], "xuid": xid, "mc_token": token,
              "mc_expires": now() + mc["expires_in"].as_f64().unwrap_or(86400.0), "refresh_token": refresh_token}))
}

/// Renouvelle le jeton Minecraft s'il expire bientôt (grâce au jeton de rafraîchissement Microsoft).
pub fn ensure_fresh(acc: Value) -> Res<Value> {
    if acc["type"] != "msa" || acc["mc_expires"].as_f64().unwrap_or(0.0) - 300.0 > now() {
        return Ok(acc);
    }
    crate::task!(step = "Connexion", detail = "Renouvellement de la session Microsoft…");
    let t = http_json(MS_TOKEN, Body::Form(form(&[
        ("grant_type", "refresh_token"),
        ("client_id", &config().client_id),
        ("refresh_token", s(&acc["refresh_token"])),
        ("scope", MS_SCOPE),
    ])), &[], 30)?;
    if t.get("access_token").is_none() {
        return Err(Fail::Login("Session expirée : reconnecte-toi avec Microsoft.".into()));
    }
    let refresh = t["refresh_token"].as_str().unwrap_or(s(&acc["refresh_token"])).to_string();
    let fresh = minecraft_from_ms(s(&t["access_token"]), &refresh)?;
    save_account(Some(&fresh));
    with_state(|st| st.account = Some(fresh.clone()));
    Ok(fresh)
}

/// Compte hors ligne : même UUID que Minecraft (« OfflinePlayer:<pseudo> », version 3).
pub fn offline_account(name: &str) -> Value {
    let mut d = md5::compute(format!("OfflinePlayer:{name}").as_bytes()).0;
    d[6] = (d[6] & 0x0F) | 0x30;
    d[8] = (d[8] & 0x3F) | 0x80;
    let hex: String = d.iter().map(|b| format!("{b:02x}")).collect();
    json!({"type": "offline", "name": name, "uuid": hex, "xuid": "0", "mc_token": "0"})
}

pub fn valid_pseudo(name: &str) -> bool {
    (3..=16).contains(&name.chars().count()) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}
