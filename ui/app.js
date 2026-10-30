// Interface du launcher : reçoit l'état du programme (poussé à chaque changement, sans interrogation périodique)
// et lui envoie les commandes. Hors du launcher (simple navigateur), un état de démonstration la fait vivre.
"use strict";
const $ = id => document.getElementById(id);
const esc = s => String(s ?? "").replace(/[&<>"]/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));
const icon = (name, cls = "") => `<svg class="${cls}"><use href="#i-${name}"/></svg>`;
const send = (cmd, data = {}) => window.ipc ? window.ipc.postMessage(JSON.stringify({ cmd, ...data })) : Demo.cmd(cmd, data);
const go = n => (n / 1024).toLocaleString("fr-FR", { maximumFractionDigits: 1 });

let S = null;
const keys = {};
/** N'exécute `fn` que si `value` a changé depuis la dernière fois (évite de reconstruire le DOM pour rien). */
function changed(name, value) {
  const k = JSON.stringify(value);
  if (keys[name] === k) return false;
  keys[name] = k;
  return true;
}

// ------------------------------------------------------------------------------------- serveur
function hue(name) {
  let h = 0;
  for (const c of name) h = (h * 31 + c.charCodeAt(0)) % 360;
  return h;
}

function renderServer() {
  const m = S.manifest, p = S.ping;
  if (!changed("server", [m && m.name, m && m.minecraft, m && m.server, p, S.favicon_v, S.manifest_error, !!m])) return;
  const name = (m && m.name) || "SurvivalFrench RP";
  $("title").textContent = name.replace(/\s*RP$/i, "");
  const fav = S.favicon_v ? `favicon.png?v=${S.favicon_v}` : "logo.png";
  const addr = (p && p.address) || (m ? "" : "");
  let status, bottom, motd = "";
  if (!m && !S.manifest_error) {
    status = `<span class="badge">${'<span class="dot"></span>'}Connexion…</span>`;
    bottom = `<span class="skeleton"></span><span class="skeleton" style="width:80px;margin-left:auto"></span>`;
  } else if (!m || (p && !p.ok)) {
    status = `<span class="badge off"><span class="dot"></span>Hors ligne</span>`;
    bottom = `<span class="players"><span class="count">${m ? "Le serveur de jeu ne répond pas." : "Serveur injoignable."}</span></span>
      <button class="retry" id="retry" style="margin-left:auto">${icon("refresh")}Réessayer</button>`;
  } else {
    const online = p ? p.online : m.server.online, max = p ? p.max : m.server.max;
    const q = !p ? 0 : p.ms < 60 ? 4 : p.ms < 120 ? 3 : p.ms < 250 ? 2 : 1;
    status = `<span class="badge on"><span class="dot"></span>En ligne</span>
      ${p ? `<span class="latency"><span class="bars q${q}"><i></i><i></i><i></i><i></i></span>${p.ms} ms</span>` : ""}`;
    const names = (p && p.players) || [];
    const faces = names.slice(0, 5).map(n => `<span class="face" title="${esc(n)}" style="background:hsl(${hue(n)} 45% 42%)">${esc(n[0])}</span>`).join("")
      + (online > 5 && names.length ? `<span class="face more">+${online - 5}</span>` : "");
    bottom = `<span class="players">${faces ? `<span class="faces">${faces}</span>` : ""}
        <span class="count"><b>${online}</b> / ${max} joueur${max > 1 ? "s" : ""}</span></span>
      <span class="server-meta">Minecraft ${esc(m.minecraft)} · Fabric</span>`;
    if (p && p.motd && p.motd.trim()) motd = `<div class="motd">${esc(p.motd.replace(/\s+/g, " ").trim())}</div>`;
  }
  $("server").innerHTML = `<div class="server-top">
      <img class="favicon" src="${fav}" alt="" onerror="this.onerror=null;this.src='logo.png'">
      <div class="server-id"><div class="server-name">${esc(name)}</div>
        ${addr ? `<button class="server-addr" id="addr" title="Copier l'adresse">${esc(addr)}${icon("copy")}</button>`
               : `<div class="server-addr" style="pointer-events:none">${m ? "Serveur roleplay français" : esc(S.manifest_url || "")}</div>`}</div>
      <div class="status">${status}</div></div>
    <div class="server-bottom">${bottom}</div>${motd}`;
  if ($("retry")) $("retry").onclick = () => { send("refresh"); toast("Nouvelle tentative…"); };
  if ($("addr")) $("addr").onclick = () => copy(addr, "Adresse copiée");
}

// -------------------------------------------------------------------------------------- actualités
let news = [];
function renderNews() {
  const list = (S.manifest && S.manifest.news) || [];
  if (!changed("news", list)) return;
  news = list;
  $("newsCount").textContent = list.length;
  $("news").innerHTML = list.length ? list.map((n, i) => `<button class="news-card" data-i="${i}">
      <div class="date">${esc(n.date)}</div><div class="title">${esc(n.title)}</div><div class="text">${esc(n.text)}</div></button>`).join("")
    : `<div class="news-empty">${S.manifest ? "Aucune actualité pour le moment." : "Les actualités arrivent avec le serveur."}</div>`;
  fadeNews();
  $("news").querySelectorAll(".news-card").forEach(c => c.onclick = () => {
    const n = news[+c.dataset.i];
    modal(`<div class="date">${esc(n.date)}</div><h3>${esc(n.title)}</h3><p>${esc(n.text)}</p>
      <div class="acts"><button class="btn primary" data-close>Fermer</button></div>`);
  });
}

/** Fondu à droite quand les actualités dépassent (on peut les faire défiler), retiré une fois au bout. */
function fadeNews() {
  const row = $("news");
  row.classList.toggle("more", row.scrollWidth - row.scrollLeft - row.clientWidth > 4);
}

// -------------------------------------------------------------------------------------- personnage
let skinKey = "";
function renderSkin() {
  const a = S.account, k = JSON.stringify([a && a.uuid, S.skin.v, S.skin.source]);
  if (k === skinKey) return;
  skinKey = k;
  const url = a && S.skin.source !== "default" && window.ipc ? `skin.png?v=${S.skin.v}` : null;
  SkinView.load(url, S.skin.model, face => {
    const f = document.querySelector(".me-face");
    if (f) f.src = face;
  });
}

function renderNametag() {
  const a = S.account;
  if (!changed("tag", [a, S.skin.source])) return;
  const kind = !a ? "" : S.skin.source === "rp" ? `<span class="kind rp">Personnage RP</span>`
    : a.offline ? `<span class="kind">Pseudo</span>` : `<span class="kind ms">Microsoft</span>`;
  $("nametag").innerHTML = a ? `<span class="who">${esc(a.name)}</span>${kind}` : `<span class="who" style="color:var(--ink-2);font-weight:400">Ton personnage</span>`;
}

// -------------------------------------------------------------------------------------- dock
function loginHtml() {
  const l = S.login;
  if (l && l.user_code) {
    return `<div class="login code-box"><h3>Connexion Microsoft</h3>
      <p>Va sur <b>${esc(l.url.replace(/^https?:\/\//, ""))}</b> et saisis ce code :</p>
      <div class="code">${esc(l.user_code)}</div>
      <div class="play-row"><button class="btn wide" id="copyCode">${icon("copy")}Copier</button>
      <button class="btn primary wide" id="openMs">${icon("external")}Ouvrir la page</button></div>
      <div class="wait"><span class="spinner"></span>En attente de validation… <button class="link" id="cancelLogin">Annuler</button></div></div>`;
  }
  if (l && !l.error) return `<div class="login code-box"><h3>Connexion Microsoft</h3><div class="wait" style="margin:18px 0"><span class="spinner"></span>Préparation…</div></div>`;
  let html = `<div class="login"><h3>Bienvenue !</h3>`;
  if (S.allow_offline) {
    html += `<p>Choisis le pseudo de ton personnage pour commencer.</p>
      <div class="play-row"><div class="field" style="flex:1">${icon("user")}<input id="pseudo" maxlength="16" placeholder="Ton pseudo" spellcheck="false" autocomplete="off"></div>
      <button class="btn primary" id="pseudoOk" style="height:46px">Continuer</button></div>
      <div class="hint" style="margin-top:8px">3 à 16 caractères : lettres, chiffres ou _.</div>`;
  } else {
    html += `<p>Connecte-toi avec le compte qui possède Minecraft Java.</p><div class="stack">`;
    if (S.ms_configured || !S.official) html += `<button class="btn ms wide" id="msLogin"><span class="mslogo"><i></i><i></i><i></i><i></i></span>Se connecter avec Microsoft</button>`;
    if (l && l.error) html += `<div class="alert" style="margin:2px 0 0">${icon("alert")}<div class="body">${esc(l.error)}</div></div>`;
    if (!S.ms_configured) {
      html += S.official
        ? `<button class="btn wide" id="officialBtn">${icon("rocket")}Jouer via le launcher Minecraft</button>
           <div class="hint">Ajoute le profil « SurvivalFrench RP » (mods inclus) à ton launcher Minecraft officiel, qui gère la connexion.</div>`
        : `<button class="btn wide" id="getOfficial">${icon("download")}Installer le launcher Minecraft officiel</button>`;
    }
    html += `</div>`;
  }
  return html + `</div>`;
}

function alertHtml(t) {
  if (t.state === "error") {
    const lines = (t.error || "").split("\n");
    const head = esc(lines[0]), rest = lines.slice(1).join("\n");
    return `<div class="alert">${icon("alert")}<div class="body"><div class="t">${esc(t.step || "Erreur")}</div>${head}
      ${rest.trim() ? `<div class="log">${esc(rest)}</div>` : ""}
      <div class="acts">${rest.trim() ? `<button class="btn small" id="copyLog">${icon("copy")}Copier</button>` : ""}
      <button class="btn small" id="openFolder">${icon("folder")}Dossier</button></div></div>
      <button class="x" id="dismiss" title="Fermer">${icon("close")}</button></div>`;
  }
  if (t.state === "official") {
    return `<div class="alert info">${icon("info")}<div class="body"><div class="t">${esc(t.step)}</div>${esc(t.detail)}</div>
      <button class="x" id="dismiss" title="Fermer">${icon("close")}</button></div>`;
  }
  return "";
}

function renderDock() {
  const a = S.account, t = S.task, m = S.manifest;
  const busy = t.state === "running", playing = t.state === "playing";
  const pct = Math.round((t.progress || 0) * 100);
  // Structure reconstruite seulement quand le contexte change ; la progression est mise à jour en place.
  const ctx = [!!a, a && a.name, a && a.offline, a ? null : S.login, S.allow_offline, S.ms_configured, S.official, t.state,
    t.state === "error" || t.state === "official" ? [t.step, t.error, t.detail] : null, !!m, S.installed, S.settings.ram_mb,
    m && m.minecraft, m && m.fabric_loader];
  if (changed("dock", ctx)) {
    let html = alertHtml(t);
    if (!a) {
      html += loginHtml();
    } else {
      html += `<div class="dock-head"><div class="me"><img class="me-face" src="${SkinView.face()}" alt="">
          <div class="me-text"><div class="me-name">${esc(a.name)}</div><div class="me-sub">${a.offline ? "Pseudo du serveur" : "Compte Microsoft"}</div></div></div>
          <button class="link" id="logout">Changer</button></div>`;
      if (busy) {
        html += `<div class="progress"><div class="row"><span class="step" id="pStep"></span><span class="pct" id="pPct"></span></div>
          <div class="detail" id="pDetail"></div></div>`;
      } else {
        const ready = S.installed;
        html += `<div class="ready-line">${playing ? `<span class="ok">Minecraft est lancé</span>`
          : !m ? `<span>En attente du serveur</span>`
          : ready ? `<span class="ok">Prêt à jouer</span>` : `<span class="todo">Installation au premier lancement</span>`}
          ${m ? `<span class="sep"></span><span>Minecraft ${esc(m.minecraft)}</span><span class="sep"></span><span>${go(S.settings.ram_mb)} Go</span>` : ""}</div>`;
      }
      const label = playing ? `<span class="playing-dot"></span>EN JEU` : busy ? `<span id="pBtn">INSTALLATION…</span>`
        : `${icon("play")}${S.installed === false ? "INSTALLER" : "JOUER"}`;
      html += `<div class="play-row"><button class="play ${playing ? "playing" : ""} ${busy ? "busy" : ""}" id="play" ${busy || playing || !m ? "disabled" : ""}>
          ${busy ? `<span class="fill" id="pFill"></span>` : ""}<span class="lbl">${label}</span></button>
          <button class="icon-btn" id="dockSettings" title="Paramètres">${icon("gear")}</button></div>`;
    }
    $("dock").innerHTML = html;
    // Le journal d'erreur s'ouvre sur ses dernières lignes, les plus parlantes.
    const log = $("dock").querySelector(".alert .log");
    if (log) log.scrollTop = log.scrollHeight;
    bindDock();
  }
  if (busy && a) {
    $("pStep").textContent = t.step || "Préparation";
    $("pPct").textContent = pct + " %";
    $("pDetail").textContent = t.detail || "";
    $("pFill").style.width = pct + "%";
    $("pBtn").textContent = pct > 0 ? `INSTALLATION · ${pct} %` : "PRÉPARATION…";
  }
}

function bindDock() {
  const on = (id, fn) => { const el = $(id); if (el) el.onclick = fn; };
  on("play", () => { $("play").disabled = true; send("play"); });
  on("dockSettings", openSettings);
  on("logout", () => send("logout"));
  on("dismiss", () => send("dismiss"));
  on("copyLog", () => copy(S.task.error, "Journal copié"));
  on("openFolder", () => send("folder"));
  on("msLogin", () => send("login"));
  on("cancelLogin", () => send("login_cancel"));
  on("openMs", () => send("open", { url: S.login.url }));
  on("copyCode", () => copy(S.login.user_code, "Code copié"));
  on("officialBtn", () => send("play_official"));
  on("getOfficial", () => send("open", { url: "https://www.minecraft.net/fr-fr/download" }));
  const pseudo = $("pseudo");
  if (pseudo) {
    const ok = () => {
      const v = pseudo.value.trim();
      if (!/^[A-Za-z0-9_]{3,16}$/.test(v)) { toast("Pseudo invalide : 3 à 16 lettres, chiffres ou _."); pseudo.focus(); return; }
      send("offline", { name: v });
    };
    on("pseudoOk", ok);
    pseudo.onkeydown = e => { if (e.key === "Enter") ok(); };
    setTimeout(() => pseudo.focus(), 50);
  }
}

// -------------------------------------------------------------------------------------- titre, mise à jour
function renderChrome() {
  const u = S.update || {};
  if (changed("update", u)) {
    const b = $("update");
    b.classList.toggle("hidden", u.state !== "ready" && u.state !== "downloading");
    b.classList.toggle("wait", u.state === "downloading");
    b.innerHTML = u.state === "ready" ? `${icon("refresh")}Version ${esc(u.version)} prête · Redémarrer`
      : `<span class="spinner" style="width:12px;height:12px"></span>Mise à jour ${esc(u.version || "")}…`;
  }
  document.title = "SurvivalFrench" + (S.instance > 1 ? ` — compte ${S.instance}` : "");
}

// -------------------------------------------------------------------------------------- mode économie
let eco = null;
function applyEco() {
  const pref = S.settings.perf || "auto";
  const v = pref === "eco" || (pref === "auto" && S.system && S.system.eco);
  if (v === eco) return;
  eco = v;
  document.body.classList.toggle("eco", v);
  Scene.setEco(v);
  SkinView.setEco(v);
}

function render() {
  applyEco();
  renderChrome();
  renderServer();
  renderNews();
  renderSkin();
  renderNametag();
  renderDock();
}

// -------------------------------------------------------------------------------------- paramètres
let draft = {};
function openSettings() {
  const s = S.settings, sys = S.system || {};
  draft = { ram_mb: s.ram_mb, perf: s.perf || "auto", after_launch: s.after_launch || "hide" };
  const ram = $("ram");
  ram.min = 2048; ram.max = s.ram_max || 8192; ram.step = 512; ram.value = s.ram_mb;
  $("ramMin").textContent = "2 Go"; $("ramMax").textContent = go(ram.max) + " Go";
  $("ramRec").innerHTML = `Conseillé : <b>${go(s.ram_recommended || 4096)} Go</b>`;
  updateRam();
  $("manifest").value = s.manifest_url || "";
  $("manifest").placeholder = S.manifest_url || "";
  syncChoices();
  const gpu = sys.gpu || "Inconnue";
  const tier = { low: "puce intégrée", mid: "carte dédiée", high: "carte performante" }[sys.tier] || "";
  $("machine").innerHTML = `<span class="k">Carte graphique</span><span class="v" title="${esc(gpu)}">${esc(gpu)}</span>
    <span class="k">Catégorie</span><span class="v">${esc(tier)}</span>
    <span class="k">Mémoire vive</span><span class="v">${go(sys.ram_mb || 0)} Go · ${sys.cpus || "?"} cœurs</span>
    <span class="k">Launcher</span><span class="v">Version ${esc(S.version)}${S.instance > 1 ? ` · compte ${S.instance}` : ""}</span>`;
  $("drawer").classList.add("open");
  $("veil").classList.add("open");
}
function closeSettings() { $("drawer").classList.remove("open"); $("veil").classList.remove("open"); }
function updateRam() {
  const r = $("ram"), p = (r.value - r.min) / (r.max - r.min) * 100;
  r.style.setProperty("--p", p + "%");
  $("ramV").textContent = go(+r.value);
  draft.ram_mb = +r.value;
}
function syncChoices() {
  $("perf").querySelectorAll("button").forEach(b => b.classList.toggle("on", b.dataset.v === draft.perf));
  document.querySelectorAll(".choice").forEach(c => c.classList.toggle("on", c.dataset.after === draft.after_launch));
  const sysEco = S.system && S.system.eco;
  $("perfDesc").textContent = {
    auto: `Choisi selon ta machine : ${sysEco ? "économie (PC modeste détecté)" : "animations complètes"}.`,
    eco: "Aucune animation continue : le launcher n'utilise presque pas la carte graphique.",
    full: "Décor animé, personnage vivant, effets de lumière.",
  }[draft.perf];
}

// -------------------------------------------------------------------------------------- outils
function modal(html) {
  $("dialog").innerHTML = html;
  $("modal").classList.add("open");
  $("dialog").querySelectorAll("[data-close]").forEach(b => b.onclick = closeModal);
}
function closeModal() { $("modal").classList.remove("open"); }

let toastTimer = 0;
function toast(msg) {
  const t = $("toast");
  t.textContent = msg;
  t.classList.add("show");
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => t.classList.remove("show"), 2600);
}
function copy(text, msg) {
  (navigator.clipboard ? navigator.clipboard.writeText(text) : Promise.reject()).then(() => toast(msg), () => toast("Copie impossible"));
}

// -------------------------------------------------------------------------------------- liens avec le programme
window.sf = {
  state(s) {
    S = s;
    // Repères de démarrage (diagnostic) : arrivée du manifeste et du statut du serveur.
    if (s.manifest && !sf.times.manifest) sf.times.manifest = Math.round(performance.now());
    if (s.ping && !sf.times.ping) sf.times.ping = Math.round(performance.now());
    render();
  },
  times: {},
  toast,
  win(w) {
    document.body.classList.toggle("maximized", !!w.max);
    $("maxIcon").setAttribute("href", w.max ? "#i-restore" : "#i-max");
  },
  confirmQuit() {
    modal(`<h3>Installation en cours</h3><p>Si tu fermes maintenant, l'installation reprendra au prochain lancement.</p>
      <div class="acts"><button class="btn" data-close>Continuer</button><button class="btn primary" id="quitNow">Quitter</button></div>`);
    $("quitNow").onclick = () => send("quit");
  },
};

function bindStatic() {
  const tb = $("titlebar");
  tb.addEventListener("mousedown", e => { if (e.button === 0 && !e.target.closest("button")) send("drag"); });
  tb.addEventListener("dblclick", e => { if (!e.target.closest("button")) send("max"); });
  $("tbMin").onclick = () => send("min");
  $("tbMax").onclick = () => send("max");
  $("tbClose").onclick = () => send("close");
  $("tbSettings").onclick = openSettings;
  $("update").onclick = () => send("update_restart");
  document.querySelectorAll(".edge").forEach(e => e.addEventListener("mousedown", ev => {
    if (ev.button === 0) { ev.preventDefault(); send("resize", { dir: e.dataset.dir }); }
  }));
  $("news").addEventListener("scroll", fadeNews, { passive: true });
  addEventListener("resize", fadeNews);
  // Molette verticale = défilement horizontal des actualités.
  $("news").addEventListener("wheel", e => { if (e.deltaY) { $("news").scrollLeft += e.deltaY; e.preventDefault(); } }, { passive: false });
  $("veil").onclick = closeSettings;
  $("drawerClose").onclick = closeSettings;
  $("modal").addEventListener("mousedown", e => { if (e.target === $("modal")) closeModal(); });
  $("ram").oninput = updateRam;
  $("perf").querySelectorAll("button").forEach(b => b.onclick = () => { draft.perf = b.dataset.v; syncChoices(); });
  document.querySelectorAll(".choice").forEach(c => c.onclick = () => { draft.after_launch = c.dataset.after; syncChoices(); });
  $("folderBtn").onclick = () => send("folder");
  $("saveSettings").onclick = () => {
    send("settings", { ram_mb: draft.ram_mb, perf: draft.perf, after_launch: draft.after_launch, manifest_url: $("manifest").value });
    closeSettings();
    toast("Paramètres enregistrés");
  };
  document.addEventListener("keydown", e => {
    if (e.key === "Escape") { closeSettings(); closeModal(); }
    if ((e.key === "F5" || (e.ctrlKey && e.key.toLowerCase() === "r"))) e.preventDefault();
  });
  document.addEventListener("contextmenu", e => { if (!e.target.closest("input")) e.preventDefault(); });
  document.addEventListener("dragstart", e => e.preventDefault());
}

// -------------------------------------------------------------------------------------- activité
// Décor et personnage ne vivent que si quelqu'un regarde : en pause quand la fenêtre n'est plus au premier plan,
// cachée, ou après 25 s sans mouvement de souris. Le launcher ne consomme alors presque plus rien.
function watchActivity() {
  const IDLE = 25000;
  let last = performance.now(), active = true;
  const set = v => {
    if (v === active) return;
    active = v;
    Scene.setActive(v);
    SkinView.setActive(v);
  };
  const poke = () => { last = performance.now(); if (!document.hidden) set(true); };
  addEventListener("mousemove", poke, { passive: true });
  addEventListener("mousedown", poke, { passive: true });
  addEventListener("keydown", poke, { passive: true });
  addEventListener("focus", poke);
  addEventListener("blur", () => set(false));
  document.addEventListener("visibilitychange", () => (document.hidden ? set(false) : poke()));
  setInterval(() => { if (active && performance.now() - last > IDLE) set(false); }, 3000);
}

// -------------------------------------------------------------------------------------- démonstration
// Ouvert dans un navigateur ordinaire : état fictif (ajouter #connexion, #installation, #erreur, #jeu, #horsligne).
const Demo = {
  state: null,
  cmd(cmd, d) {
    const s = this.state;
    if (cmd === "offline") s.account = { name: d.name, uuid: "0", offline: true };
    if (cmd === "logout") s.account = null;
    if (cmd === "dismiss") s.task = { state: "idle", step: "", progress: 0, detail: "", error: "" };
    if (cmd === "settings") Object.assign(s.settings, d);
    if (cmd === "play") {
      let p = 0;
      const steps = ["Java", "Bibliothèques", "Ressources du jeu", "Mods du serveur", "Lancement"];
      const id = setInterval(() => {
        p += .013;
        if (p >= 1) { clearInterval(id); s.task = { state: "playing", step: "En jeu", progress: 1, detail: "Bon jeu !", error: "" }; s.installed = true; }
        else s.task = { state: "running", step: steps[Math.min(4, Math.floor(p * 5))], progress: p, detail: `Ressources : ${Math.round(p * 3400)} / 3400`, error: "" };
        sf.state(s);
      }, 60);
    }
    if (cmd === "login") s.login = { user_code: "F7K2-QX9M", url: "https://www.microsoft.com/link", error: "" };
    if (cmd === "login_cancel") s.login = null;
    sf.state(s);
  },
  boot() {
    const h = location.hash.slice(1);
    const s = this.state = {
      version: "2.0.0", instance: 1, account: h === "connexion" ? null : { name: "Aurore", uuid: "0", offline: true }, login: null,
      task: { state: "idle", step: "", progress: 0, detail: "", error: "" },
      settings: { ram_mb: 3072, manifest_url: "", after_launch: "hide", perf: "full", ram_max: 14336, ram_recommended: 4096 },
      manifest: h === "horsligne" ? null : { name: "SurvivalFrench RP", minecraft: "26.3", fabric_loader: "0.20.1", server: { online: 7, max: 50 },
        news: [
          { title: "Bienvenue sur SurvivalFrench RP", date: "22/10/2026", text: "Crée ton personnage, trouve un métier à France Travail et installe-toi en ville. Le chat vocal de proximité est actif : touche Caps Lock pour parler, V pour les réglages." },
          { title: "Nouveautés", date: "22/10/2026", text: "Contrats avec séquestre, boutiques, services publics (police, SAMU, pompiers, justice), saisons et météo, missions du jour et réputation." },
          { title: "Routes en pente", date: "29/10/2026", text: "Les routes épousent enfin le relief : pentes douces, virages et véhicules qui suivent le terrain." },
        ] },
      manifest_error: h === "horsligne" ? "Serveur injoignable" : "", manifest_url: "http://93.1.78.21:50163/launcher/manifest.json",
      ping: h === "horsligne" ? null : { ok: true, ms: 38, online: 7, max: 50, players: ["Aurore", "Bastien", "Chloe", "Dylan", "Elias", "Farah", "Gaspard"],
        motd: "SurvivalFrench RP — la vie réelle, version Minecraft", version: "26.3", address: "93.1.78.21:50565" },
      favicon_v: 0, skin: { v: 0, model: "", source: "default" }, update: { state: "none" }, installed: h !== "premier",
      allow_offline: true, ms_configured: false, official: true,
      system: { ram_mb: 16234, cpus: 8, gpu: "NVIDIA GeForce RTX 4060 Laptop GPU", tier: "high", eco: false },
    };
    if (h === "installation") s.task = { state: "running", step: "Ressources du jeu", progress: .46, detail: "Ressources : 1 572 / 3 400", error: "" };
    if (h === "erreur") s.task = { state: "error", step: "Le jeu s'est arrêté", progress: 0, detail: "",
      error: "Code 1.\n[12:04:31] [Render thread/ERROR]: Unable to initialise OpenGL\n[12:04:31] [main/INFO]: Stopping!" };
    if (h === "jeu") s.task = { state: "playing", step: "En jeu", progress: 1, detail: "", error: "" };
    if (h === "eco") s.settings.perf = "eco";
    sf.state(s);
  },
};

// -------------------------------------------------------------------------------------- démarrage
(function start() {
  const boot = window.__SF_BOOT;
  const isEco = boot ? (boot.settings.perf === "eco" || (boot.settings.perf !== "full" && boot.system.eco)) : location.hash === "#eco";
  document.body.classList.toggle("eco", isEco);
  Scene.init(isEco);
  SkinView.init($("skin"), isEco);
  bindStatic();
  if (boot) { sf.state(boot); send("sync"); } else Demo.boot();
  setTimeout(() => { const h = $("dragHint"); if (h) h.style.opacity = 0; }, 6000);
  watchActivity();
  // La fenêtre ne s'affiche qu'une fois la page peinte (polices comprises) : pas de flash blanc ni de texte qui saute.
  const shown = () => requestAnimationFrame(() => requestAnimationFrame(() => send("ready")));
  Promise.race([document.fonts.ready, new Promise(r => setTimeout(r, 350))]).then(shown);
})();
