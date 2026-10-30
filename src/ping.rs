//! Statut du serveur Minecraft (« Server List Ping ») : latence, joueurs connectés, message du jour, icône.
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::base::{with_state, STATE};

fn varint(out: &mut Vec<u8>, mut v: i32) {
    loop {
        let mut b = (v & 0x7f) as u8;
        v = ((v as u32) >> 7) as i32;
        if v != 0 {
            b |= 0x80;
        }
        out.push(b);
        if v == 0 {
            break;
        }
    }
}

fn read_varint(r: &mut impl Read) -> std::io::Result<i32> {
    let mut v = 0i32;
    for i in 0..5 {
        let mut b = [0u8];
        r.read_exact(&mut b)?;
        v |= ((b[0] & 0x7f) as i32) << (7 * i);
        if b[0] & 0x80 == 0 {
            return Ok(v);
        }
    }
    Err(std::io::Error::other("varint trop long"))
}

fn packet(id: i32, body: &[u8]) -> Vec<u8> {
    let mut inner = Vec::new();
    varint(&mut inner, id);
    inner.extend_from_slice(body);
    let mut out = Vec::new();
    varint(&mut out, inner.len() as i32);
    out.extend(inner);
    out
}

/// Texte brut d'un composant de chat (message du jour).
fn plain(v: &Value) -> String {
    match v {
        Value::String(s) => strip_codes(s),
        Value::Array(list) => list.iter().map(plain).collect(),
        Value::Object(o) => {
            let mut s = o.get("text").map(plain).unwrap_or_default();
            if let Some(Value::Array(extra)) = o.get("extra") {
                s.extend(extra.iter().map(plain));
            }
            s
        }
        _ => String::new(),
    }
}

fn strip_codes(s: &str) -> String {
    let mut out = String::new();
    let mut skip = false;
    for c in s.chars() {
        if skip {
            skip = false;
        } else if c == '§' {
            skip = true;
        } else {
            out.push(c);
        }
    }
    out
}

pub fn status(host: &str, port: u16) -> std::io::Result<(Value, u64)> {
    let addr = (host, port).to_socket_addrs()?.next().ok_or_else(|| std::io::Error::other("adresse introuvable"))?;
    let t = Duration::from_secs(4);
    let mut sock = TcpStream::connect_timeout(&addr, t)?;
    sock.set_read_timeout(Some(t))?;
    sock.set_write_timeout(Some(t))?;
    sock.set_nodelay(true)?;
    let mut hs = Vec::new();
    varint(&mut hs, 767);
    varint(&mut hs, host.len() as i32);
    hs.extend_from_slice(host.as_bytes());
    hs.extend_from_slice(&port.to_be_bytes());
    varint(&mut hs, 1);
    let mut first = packet(0, &hs);
    first.extend(packet(0, &[]));
    sock.write_all(&first)?;
    let _len = read_varint(&mut sock)?;
    let _id = read_varint(&mut sock)?;
    let n = read_varint(&mut sock)?.clamp(0, 1 << 21) as usize;
    let mut buf = vec![0u8; n];
    sock.read_exact(&mut buf)?;
    let start = Instant::now();
    sock.write_all(&packet(1, &12345i64.to_be_bytes()))?;
    let _ = read_varint(&mut sock)?;
    let ms = start.elapsed().as_millis() as u64;
    let v: Value = serde_json::from_slice(&buf).map_err(std::io::Error::other)?;
    Ok((v, ms))
}

static BUSY: AtomicBool = AtomicBool::new(false);

/// Interroge le serveur en arrière-plan (une requête à la fois).
pub fn refresh() {
    if BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(|| {
        let m = STATE.lock().unwrap().manifest.clone();
        if let Some(m) = m {
            let (host, port) = crate::install::server_host_port(&m);
            let address = crate::install::server_address(&m);
            let result = status(&host, port);
            with_state(|st| match result {
                Ok((v, ms)) => {
                    let players: Vec<Value> = v["players"]["sample"].as_array().into_iter().flatten()
                        .filter_map(|p| p["name"].as_str()).filter(|n| !n.is_empty() && !n.contains('§'))
                        .take(12).map(|n| json!(n)).collect();
                    if let Some(icon) = v["favicon"].as_str().and_then(|f| f.strip_prefix("data:image/png;base64,")) {
                        use base64::Engine;
                        let clean: String = icon.chars().filter(|c| !c.is_whitespace()).collect();
                        if let Ok(png) = base64::engine::general_purpose::STANDARD.decode(clean) {
                            if st.favicon.as_ref() != Some(&png) {
                                st.favicon = Some(png);
                                st.favicon_v += 1;
                            }
                        }
                    }
                    st.ping = Some(json!({"ok": true, "ms": ms, "online": v["players"]["online"], "max": v["players"]["max"],
                                          "players": players, "motd": plain(&v["description"]),
                                          "version": v["version"]["name"], "address": address}));
                }
                Err(e) => st.ping = Some(json!({"ok": false, "error": e.to_string()})),
            });
        }
        BUSY.store(false, Ordering::SeqCst);
    });
}

/// Toutes les 15 s tant que la fenêtre est ouverte (le manifeste en relance une aussi à chaque lecture).
pub fn ping_loop() {
    loop {
        std::thread::sleep(Duration::from_secs(15));
        let (open, playing, has) = {
            let st = STATE.lock().unwrap();
            (st.window_open, st.game_running, st.manifest.is_some())
        };
        if open && !playing && has {
            refresh();
        }
    }
}
