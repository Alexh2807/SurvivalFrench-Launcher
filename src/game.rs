//! Ligne de commande de Minecraft, lancement et surveillance du jeu.
use std::io::{Read, Seek, SeekFrom};
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::base::{config, paths, with_state, VERSION, STATE};
use crate::install::{fetch_manifest, first_run_options, prepare, rules_allow, server_host_port, server_address,
                     write_servers_dat, Prepared};
use crate::net::{Fail, Res};
use crate::task;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn s(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}

pub fn build_command(p: &Prepared, acc: &Value, m: &Value) -> Vec<String> {
    let (host, port) = server_host_port(m);
    let address = format!("{host}:{port}");
    let natives = paths().natives.join(&p.version);
    for sub in ["java", "jna", "lwjgl", "netty"] {
        let _ = std::fs::create_dir_all(natives.join(sub));
    }
    let cp = p.classpath.iter().map(|c| c.to_string_lossy().into_owned()).collect::<Vec<_>>().join(";");
    let client_id = if config().client_id.is_empty() { "0".to_string() } else { config().client_id.clone() };
    let values: Vec<(&str, String)> = vec![
        ("natives_directory", natives.to_string_lossy().into_owned()),
        ("launcher_name", "SurvivalFrench".into()),
        ("launcher_version", VERSION.into()),
        ("classpath", cp),
        ("classpath_separator", ";".into()),
        ("library_directory", paths().libraries.to_string_lossy().into_owned()),
        ("version_name", p.version.clone()),
        ("auth_player_name", s(&acc["name"]).into()),
        ("game_directory", paths().game.to_string_lossy().into_owned()),
        ("assets_root", paths().assets.to_string_lossy().into_owned()),
        ("assets_index_name", p.assets_index.clone()),
        ("auth_uuid", s(&acc["uuid"]).into()),
        ("auth_access_token", acc["mc_token"].as_str().unwrap_or("0").into()),
        ("clientid", client_id),
        ("auth_xuid", acc["xuid"].as_str().unwrap_or("0").into()),
        ("user_type", if acc["type"] == "msa" { "msa" } else { "legacy" }.into()),
        ("version_type", "release".into()),
        ("quickPlayMultiplayer", address),
    ];
    let subst = |x: &str| {
        let mut out = x.to_string();
        for (k, v) in &values {
            out = out.replace(&format!("${{{k}}}"), v);
        }
        out
    };
    let expand = |items: &Value, features: &Value| -> Vec<String> {
        let mut out = Vec::new();
        for a in items.as_array().into_iter().flatten() {
            if let Some(x) = a.as_str() {
                out.push(subst(x));
            } else if rules_allow(&a["rules"], features) {
                match &a["value"] {
                    Value::Array(list) => out.extend(list.iter().map(|x| subst(s(x)))),
                    v => out.push(subst(s(v))),
                }
            }
        }
        out
    };
    let ram = STATE.lock().unwrap().settings["ram_mb"].as_u64().unwrap_or(4096);
    // Réglages du ramasse-miettes du launcher officiel : moins d'à-coups sur les petites machines.
    let mut jvm: Vec<String> = vec![format!("-Xmx{ram}M"), "-Xms512M".into(), "-XX:+UnlockExperimentalVMOptions".into(),
        "-XX:+UseG1GC".into(), "-XX:G1NewSizePercent=20".into(), "-XX:G1ReservePercent=20".into(),
        "-XX:MaxGCPauseMillis=50".into(), "-XX:G1HeapRegionSize=32M".into()];
    jvm.extend(expand(&p.vjson["arguments"]["jvm"], &Value::Null));
    jvm.extend(expand(&p.fabric["arguments"]["jvm"], &Value::Null));
    if let Some(cfg) = &p.log_cfg {
        jvm.push(s(&p.vjson["logging"]["client"]["argument"]).replace("${path}", &cfg.to_string_lossy()));
    }
    let mut game = expand(&p.vjson["arguments"]["game"], &json!({"is_quick_play_multiplayer": true}));
    game.extend(expand(&p.fabric["arguments"]["game"], &Value::Null));
    let mut cmd = vec![p.javaw.to_string_lossy().into_owned()];
    cmd.extend(jvm);
    cmd.push(s(&p.fabric["mainClass"]).to_string());
    cmd.extend(game);
    cmd
}

/// Attend la fin du jeu. Si Minecraft a annoncé son arrêt (« Stopping! » dans son journal) mais que Java reste en
/// mémoire (un fil encore actif l'empêche de se fermer), on le termine au bout de 20 s : sinon le launcher restait
/// « EN JEU » avec un jeu invisible.
fn watch_game(child: &mut Child, log: &Path) -> i32 {
    let mut stopping_at: Option<Instant> = None;
    let mut offset = 0u64;
    let mut tail: Vec<u8> = Vec::new();
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            return status.code().unwrap_or(1);
        }
        std::thread::sleep(Duration::from_secs(1));
        match stopping_at {
            None => {
                if let Ok(mut f) = std::fs::File::open(log) {
                    if f.seek(SeekFrom::Start(offset)).is_ok() {
                        let mut chunk = Vec::new();
                        if f.read_to_end(&mut chunk).is_ok() {
                            offset += chunk.len() as u64;
                            let mut hay = tail.clone();
                            hay.extend_from_slice(&chunk);
                            if hay.windows(9).any(|w| w == b"Stopping!") {
                                stopping_at = Some(Instant::now());
                            }
                            if !chunk.is_empty() {
                                tail = chunk[chunk.len().saturating_sub(16)..].to_vec();
                            }
                        }
                    }
                }
            }
            Some(t) if t.elapsed() > Duration::from_secs(20) => {
                let _ = child.kill();
                let _ = child.wait();
                return 0; // arrêt demandé par le joueur : pas une erreur
            }
            _ => {}
        }
    }
}

fn between<'a>(s: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let i = s.find(start)? + start.len();
    s[i..].find(end).map(|j| &s[i..i + j])
}

/// Dernières lignes lisibles du journal. Lancé par un launcher, Minecraft écrit en XML (log4j) : on n'en garde que
/// le niveau et le message, avec les dernières erreurs même si d'autres lignes les suivent.
fn log_tail(text: &str, n: usize) -> String {
    if !text.contains("<log4j:Event") {
        let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        return lines[lines.len().saturating_sub(n)..].join("\n");
    }
    let events: Vec<(bool, String)> = text.split("<log4j:Event").skip(1).map(|ev| {
        let level = between(ev, "level=\"", "\"").unwrap_or("");
        let thread = between(ev, "thread=\"", "\"").unwrap_or("");
        let msg = between(ev, "<log4j:Message><![CDATA[", "]]>").unwrap_or("").trim();
        let mut line = format!("[{thread}/{level}] {msg}");
        if let Some(t) = between(ev, "<log4j:Throwable><![CDATA[", "]]>") {
            for l in t.lines().take(3) {
                line.push_str("\n    ");
                line.push_str(l.trim());
            }
        }
        (matches!(level, "ERROR" | "FATAL"), line)
    }).collect();
    let start = events.len().saturating_sub(n);
    let mut out: Vec<&str> = events[..start].iter().filter(|(e, _)| *e).rev().take(3)
        .map(|(_, l)| l.as_str()).collect::<Vec<_>>().into_iter().rev().collect();
    if !out.is_empty() {
        out.push("…");
    }
    out.extend(events[start..].iter().map(|(_, l)| l.as_str()));
    out.join("\n")
}

pub fn play() {
    let r = (|| -> Res<()> {
        let acc = STATE.lock().unwrap().account.clone().ok_or_else(|| Fail::Login("Connecte-toi d'abord.".into()))?;
        task!(state = "running", step = "Serveur", progress = 0.0, detail = "Lecture du manifeste…", error = "");
        crate::base::trace("JOUER");
        let m = fetch_manifest().ok_or_else(|| Fail::Other(STATE.lock().unwrap().manifest_error.clone()))?;
        crate::base::trace("préparation : manifeste");
        let acc = crate::auth::ensure_fresh(acc)?;
        let p = prepare(&m)?;
        with_state(|st| st.installed = Some(true));
        first_run_options();
        let name = m["name"].as_str().unwrap_or("SurvivalFrench RP").to_string();
        write_servers_dat(&name, &server_address(&m));
        task!(step = "Lancement", progress = 0.98, detail = "Démarrage de Minecraft…");
        let cmd = build_command(&p, &acc, &m);
        let log_path = paths().game.join("launcher-game.log");
        let log = std::fs::File::create(&log_path)?;
        let mut child = Command::new(&cmd[0]).args(&cmd[1..]).current_dir(&paths().game)
            .stdin(Stdio::null()).stdout(log.try_clone()?).stderr(log)
            .creation_flags(CREATE_NO_WINDOW).spawn()
            .map_err(|e| Fail::Other(format!("Java ne démarre pas : {e}")))?;
        crate::base::trace("Minecraft démarré");
        with_state(|st| {
            st.game_running = true;
            st.task = json!({"state": "playing", "step": "En jeu", "progress": 1.0,
                             "detail": format!("Bon jeu sur {name} !"), "error": ""});
        });
        let code = watch_game(&mut child, &log_path);
        with_state(|st| st.game_running = false);
        if code != 0 {
            let text = std::fs::read(&log_path).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
            task!(state = "error", step = "Le jeu s'est arrêté", error = format!("Code {code}.\n{}", log_tail(&text, 10)));
        } else {
            task!(state = "idle", step = "", progress = 0.0, detail = "");
        }
        Ok(())
    })();
    match r {
        Ok(()) => {}
        Err(Fail::Login(e)) => task!(state = "error", step = "Connexion", error = e),
        Err(Fail::Other(e)) => task!(state = "error", step = "Installation", error = e),
    }
    crate::app_event(crate::AppEvent::GameEnded);
}
