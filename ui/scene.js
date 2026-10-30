// Décor : Paris au crépuscule en pixel art, dessiné une seule fois (aucune image à charger), agrandi sans flou.
// Ciel en dégradé, soleil couchant, Montmartre, tour Eiffel scintillante, immeubles haussmanniens, Seine et quais.
"use strict";
const Scene = (() => {
  const PX = 3;            // pixels d'écran par pixel du décor
  const M = 5;             // marge des calques (parallaxe) en pixels du décor
  let W = 0, H = 0, hz = 0, quay = 0, eco = false, parallax = null;
  const $ = id => document.getElementById(id);

  function rng(seed) {
    let s = seed >>> 0;
    return () => ((s = (s * 1664525 + 1013904223) >>> 0) / 4294967296);
  }

  function canvas(id, w, h) {
    const c = $(id);
    c.width = w; c.height = h;
    c.style.width = w * PX + "px"; c.style.height = h * PX + "px";
    // Dessinés une seule fois : en mémoire ordinaire plutôt que sur la carte graphique.
    const g = c.getContext("2d", { willReadFrequently: true });
    g.imageSmoothingEnabled = false;
    g.clearRect(0, 0, w, h);
    return g;
  }

  const rect = (g, x, y, w, h, c) => { g.fillStyle = c; g.fillRect(Math.round(x), Math.round(y), Math.round(w), Math.round(h)); };

  function disc(g, cx, cy, r, colorAt) {
    for (let y = -r; y <= r; y++) {
      const half = Math.floor(Math.sqrt(r * r - y * y) + .35);
      g.fillStyle = colorAt((y + r) / (2 * r));
      g.fillRect(Math.round(cx - half), Math.round(cy + y), half * 2 + 1, 1);
    }
  }

  function lerpColor(a, b, t) {
    const pa = a.match(/\w\w/g).map(h => parseInt(h, 16)), pb = b.match(/\w\w/g).map(h => parseInt(h, 16));
    return "rgb(" + pa.map((v, i) => Math.round(v + (pb[i] - v) * t)).join(",") + ")";
  }

  // ---------------------------------------------------------------------------------------------- ciel
  function sky(r, sunX, sunY, sunR) {
    const g = canvas("sky", W, H);
    for (let i = 0; i < W * hz / 900; i++) {
      const x = Math.floor(r() * W), y = Math.floor(Math.pow(r(), 1.7) * hz * .62);
      const a = (1 - y / (hz * .62)) * (.35 + r() * .65);
      g.fillStyle = `rgba(${r() < .3 ? "200,215,255" : "255,255,255"},${a.toFixed(2)})`;
      g.fillRect(x, y, 1, 1);
      if (r() < .05 && y < hz * .4) {
        g.fillStyle = `rgba(255,255,255,${(a * .45).toFixed(2)})`;
        g.fillRect(x - 1, y, 1, 1); g.fillRect(x + 1, y, 1, 1); g.fillRect(x, y - 1, 1, 1); g.fillRect(x, y + 1, 1, 1);
      }
    }
    disc(g, sunX, sunY, sunR, t => t < .5 ? lerpColor("fff1bd", "ffc477", t * 2) : lerpColor("ffc477", "ff7f5c", (t - .5) * 2));
    // Bandes de chaleur au bas du soleil
    for (let k = 0; k < 3; k++) {
      const y = Math.round(sunY + sunR * (.35 + k * .22));
      g.clearRect(0, y, W, 1 + (k > 1 ? 1 : 0));
    }
  }

  function clouds(r) {
    const cw = W * 2, ch = Math.round(hz * .75);
    const g = canvas("clouds", cw, ch);
    const n = Math.max(4, Math.round(W / 70));
    for (let i = 0; i < n; i++) {
      const x0 = Math.floor(i * W / n + r() * 30), y0 = Math.floor(hz * (.22 + r() * .38)), w = 26 + Math.floor(r() * 46);
      const blobs = [];
      for (let b = 0; b < 4; b++) blobs.push([x0 + r() * w, y0 - r() * 4, 4 + r() * 6]);
      for (const shift of [0, W]) {
        for (let x = x0 - 8; x < x0 + w + 12; x++) {
          let top = Infinity;
          for (const [bx, by, br] of blobs) {
            const dx = (x - bx) / (br * 1.9);
            if (Math.abs(dx) < 1) top = Math.min(top, Math.round(by - br * Math.sqrt(1 - dx * dx)));
          }
          if (top === Infinity) continue;
          const bottom = y0 + 2;
          if (bottom - top < 1) continue;
          rect(g, (x + shift) % cw, top, 1, bottom - top, "rgba(52,34,80,.82)");
          rect(g, (x + shift) % cw, top, 1, 1, "rgba(92,58,112,.7)");
          rect(g, (x + shift) % cw, bottom - 1, 1, 1, "rgba(196,98,112,.75)");
          rect(g, (x + shift) % cw, bottom, 1, 1, "rgba(240,136,110,.35)");
        }
      }
    }
  }

  // ------------------------------------------------------------------------------------- tour Eiffel
  function eiffel(g, cx, base, T, color, lights) {
    const hwAt = t => Math.max(.5, .205 * T * Math.pow(t, 2.25));
    for (let row = 0; row <= T; row++) {
      const t = row / T, y = base - T + row, hw = hwAt(t);
      if (t < .035) { rect(g, cx, y, 1, 1, color); continue; }
      for (let dx = -Math.ceil(hw); dx <= Math.ceil(hw); dx++) {
        const ax = Math.abs(dx);
        if (ax > hw) continue;
        // Arche entre les pieds
        if (t > .86) {
          const leg = Math.max(2, hw * .36), inner = hw - leg;
          const archT = .865 + .135 * Math.pow(ax / Math.max(1, inner), 2);
          if (ax < inner && t > archT) continue;
        }
        // Treillis : ajours en diagonale à l'intérieur
        const edge = ax > hw - 1.2;
        if (!edge && t > .2 && ((dx + row) % 4 === 0 || (dx - row) % 4 === 0) && hw > 2.5) continue;
        rect(g, cx + dx, y, 1, 1, color);
      }
    }
    // Plateformes
    for (const [t, extra, rows] of [[.08, 1, 1], [.615, 2, 2], [.805, 3, 2]]) {
      const hw = Math.round(hwAt(t)) + extra, y = Math.round(base - T + t * T);
      rect(g, cx - hw, y, hw * 2 + 1, rows, color);
      if (lights) for (let x = -hw; x <= hw; x += 2) lights.push([cx + x, y]);
    }
    return { top: base - T, hwAt };
  }

  // ----------------------------------------------------------------------------------- arrière-plan
  function far(r, towerX, towerH) {
    const g = canvas("far", W + 2 * M, H + 2 * M);
    g.translate(M, M);
    const c = "#3b2652", c2 = "#45305e";
    // Montmartre et le Sacré-Cœur
    const mx = Math.round(W * .1), mh = Math.round(hz * .2);
    for (let x = -M; x < W * .3; x++) {
      const d = (x - mx) / (W * .2);
      const h = Math.max(0, mh * (1 - d * d));
      if (h > 0) rect(g, x, hz - h, 1, h, c);
    }
    const sy = hz - mh;
    rect(g, mx - 9, sy - 6, 19, 6, c); disc(g, mx, sy - 8, 5, () => c); rect(g, mx, sy - 17, 1, 4, c);
    disc(g, mx - 7, sy - 6, 2, () => c); disc(g, mx + 7, sy - 6, 2, () => c);
    rect(g, mx + 12, sy - 15, 3, 11, c); disc(g, mx + 13, sy - 16, 2, () => c);
    // Toits lointains
    for (let x = -M; x < W + M;) {
      const w = 5 + Math.floor(r() * 10), h = 6 + Math.floor(r() * 14);
      rect(g, x, hz - h, w, h, c);
      rect(g, x, hz - h, w, 1, c2);
      if (r() < .12) { disc(g, x + w / 2, hz - h - 2, 3, () => c); rect(g, x + w / 2, hz - h - 7, 1, 3, c); }
      x += w + (r() < .2 ? 1 : 0);
    }
    // Dôme des Invalides (doré)
    const ix = Math.round(W * .44), iy = hz - 18;
    rect(g, ix - 8, iy, 17, 18, c); disc(g, ix, iy - 2, 6, t => t < .5 ? "#8a6a4a" : c);
    rect(g, ix, iy - 12, 1, 4, "#a88152"); rect(g, ix - 2, iy - 7, 1, 1, "#c79a5c"); rect(g, ix + 2, iy - 6, 1, 1, "#b88b55");
    // La Défense, au loin à droite
    const dx0 = Math.round(W * .86);
    rect(g, dx0 - 16, hz - 13, 13, 13, "#35294e"); g.clearRect(dx0 - 13, hz - 10, 7, 10);
    for (let x = dx0; x < W + M;) {
      const w = 4 + Math.floor(r() * 5), h = Math.round(hz * (.18 + r() * .22));
      rect(g, x, hz - h, w, h, "#33294d");
      for (let wy = hz - h + 2; wy < hz - 2; wy += 3) for (let wx = x + 1; wx < x + w - 1; wx += 2)
        if (r() < .16) rect(g, wx, wy, 1, 1, "rgba(255,214,140,.55)");
      x += w + 1 + Math.floor(r() * 3);
    }
    // La tour, un ton plus proche
    const lights = [];
    const tw = eiffel(g, towerX, hz, towerH, "#271a3d", lights);
    for (const [x, y] of lights) if (r() < .6) rect(g, x, y, 1, 1, "rgba(255,196,110,.75)");
    return { lights, tw };
  }

  // ------------------------------------------------------------------------------- immeubles et Seine
  const WARM = ["#ffd27a", "#ffbf66", "#ffe3a3", "#ff9f5a", "#ffd27a"];

  function mid(r) {
    const g = canvas("mid", W + 2 * M, H + 2 * M);
    g.translate(M, M);
    const wall = "#211634", roof = "#2b2546", win = "#2c2142", line = "#33264c";
    const lit = [];
    const scale = H / 240;
    for (let x = -M; x < W + M;) {
      const w = 15 + Math.floor(r() * 16);
      const floors = 4 + Math.floor(r() * 2);
      const groundH = 5, floorH = 4;
      const bodyH = Math.round((groundH + floors * floorH) * Math.max(.8, scale));
      const roofH = 6;
      const top = hz - bodyH;
      rect(g, x, top, w, bodyH, wall);
      // Toit mansardé
      for (let k = 0; k < roofH; k++) {
        const inset = k < 2 ? 2 : 1;
        rect(g, x + inset, top - roofH + k, w - inset * 2, 1, roof);
      }
      for (let dx = 3; dx < w - 3; dx += 5) {
        rect(g, x + dx, top - 4, 2, 2, r() < .35 ? WARM[Math.floor(r() * WARM.length)] : win);
      }
      // Cheminées
      for (let k = 0; k < 1 + Math.floor(r() * 3); k++) rect(g, x + 2 + Math.floor(r() * (w - 5)), top - roofH - 2, 2, 3, roof);
      // Balcons filants (2e et 5e étages)
      rect(g, x, top + 4, w, 1, line);
      if (floors >= 5) rect(g, x, top + 4 + floorH * 3, w, 1, line);
      // Fenêtres
      const fh = Math.max(4, Math.round(floorH * Math.max(.8, scale)));
      for (let f = 0; f < floors; f++) {
        const wy = top + 1 + f * fh;
        for (let dx = 2; dx < w - 2; dx += 4) {
          const on = r() < .3;
          const col = on ? WARM[Math.floor(r() * WARM.length)] : win;
          rect(g, x + dx, wy, 2, 2, col);
          if (on) lit.push([x + dx, wy, col]);
        }
      }
      // Rez-de-chaussée : vitrines
      for (let dx = 1; dx < w - 3; dx += 6) {
        const on = r() < .55;
        rect(g, x + dx, hz - 4, 4, 3, on ? "#ffcf7a" : win);
        if (on) lit.push([x + dx + 1, hz - 4, "#ffcf7a"], [x + dx + 3, hz - 4, "#ffcf7a"]);
      }
      x += w + (r() < .25 ? 3 + Math.floor(r() * 4) : 0);
    }
    // Arbres du quai d'en face
    for (let x = -M + Math.floor(r() * 8); x < W + M; x += 9 + Math.floor(r() * 9)) {
      const rr = 3 + Math.floor(r() * 3);
      disc(g, x, hz - rr + 1, rr, t => t < .3 ? "#21162f" : "#170f24");
      rect(g, x, hz - 1, 1, 2, "#140c1f");
    }
    rect(g, -M, hz, W + 2 * M, 1, "#1a1229");
    return lit;
  }

  function near(r, sunX, sunR, lit, towerX, stageX) {
    const g = canvas("near", W + 2 * M, H + 2 * M);
    g.translate(M, M);
    // Seine : reflet du ciel en haut, sombre en bas
    const depth = quay - hz;
    for (let y = hz + 1; y < quay; y++) {
      const t = (y - hz) / depth;
      rect(g, -M, y, W + 2 * M, 1, lerpColor("3a2550", "120d22", Math.pow(t, .7)));
    }
    // Reflet du soleil
    for (let y = hz + 1; y < quay; y++) {
      const t = (y - hz) / depth;
      const half = sunR * (1.15 - t * .7) * (.55 + r() * .45);
      if ((y + Math.floor(r() * 2)) % 2) continue;
      rect(g, sunX - half + (r() - .5) * 3, y, half * 2, 1, `rgba(255,${Math.round(190 - t * 70)},110,${(.75 - t * .5).toFixed(2)})`);
    }
    // Reflets des fenêtres et de la tour
    for (const [x, y, col] of lit) {
      if (y < hz - 22 || r() < .35) continue;
      const len = Math.round((hz - y) * .9 + 2);
      for (let k = 1; k < len; k += 2) {
        const yy = hz + k + 1;
        if (yy >= quay) break;
        g.globalAlpha = .45 * (1 - k / len);
        rect(g, x + Math.round((r() - .5) * 2), yy, 2, 1, col);
      }
    }
    g.globalAlpha = 1;
    // Bateau-mouche illuminé
    const bx = Math.round(W * .3), by = hz + Math.round(depth * .45);
    rect(g, bx, by, 30, 2, "#0f0a1a"); rect(g, bx + 2, by + 2, 26, 1, "#0b0814");
    rect(g, bx + 5, by - 3, 20, 3, "#1a1228");
    for (let k = 0; k < 9; k++) rect(g, bx + 6 + k * 2, by - 2, 1, 1, WARM[k % WARM.length]);
    rect(g, bx + 1, by - 1, 1, 1, "#ff6b6b"); rect(g, bx + 29, by - 1, 1, 1, "#9fe6b8");
    for (let k = 1; k < 7; k += 2) { g.globalAlpha = .3 - k * .03; rect(g, bx + 6, by + 3 + k, 18, 1, "#ffbf66"); }
    g.globalAlpha = 1;
    // Quai : parapet en pierre puis pavés
    rect(g, -M, quay, W + 2 * M, H + M - quay, "#100b1b");
    rect(g, -M, quay, W + 2 * M, 4, "#1d1530");
    rect(g, -M, quay, W + 2 * M, 1, "#3a2c52");
    for (let x = -M + 3; x < W + M; x += 9) rect(g, x, quay + 1, 1, 3, "#150f24");
    for (let y = quay + 6; y < H; y += 3) for (let x = -M + (y % 2) * 2; x < W + M; x += 5) if (r() < .5) rect(g, x, y, 2, 1, "#16101f");
    // Réverbères (pas devant le personnage)
    const glow = [];
    for (let x = Math.round(W * .06); x < W; x += Math.round(W * .2)) {
      if (Math.abs(x - stageX) < 34) continue;
      rect(g, x, quay - 20, 1, 20, "#0c0915");
      rect(g, x - 1, quay - 22, 3, 3, "#ffe0a0"); rect(g, x - 1, quay - 23, 3, 1, "#0c0915");
      glow.push([x, quay - 21]);
    }
    for (const [x, y] of glow) {
      const grad = g.createRadialGradient(x + .5, y + .5, 0, x + .5, y + .5, 16);
      grad.addColorStop(0, "rgba(255,214,150,.35)"); grad.addColorStop(1, "rgba(255,214,150,0)");
      g.fillStyle = grad; g.fillRect(x - 16, y - 16, 33, 33);
      const pool = g.createRadialGradient(x + .5, quay + 2, 0, x + .5, quay + 2, 14);
      pool.addColorStop(0, "rgba(255,200,130,.22)"); pool.addColorStop(1, "rgba(255,200,130,0)");
      g.fillStyle = pool; g.fillRect(x - 14, quay - 2, 29, 10);
    }
  }

  // ------------------------------------------------------------------------- étincelles et faisceau
  // Points lumineux posés une fois ; l'horloge d'ambiance les allume et les éteint d'un coup, comme du pixel art.
  let stars = [], sparks = [], beacon = null;
  function sparkles(r, towerX, tw, towerH) {
    const box = $("sparkles");
    box.innerHTML = "";
    stars = []; sparks = []; beacon = null;
    if (eco) return;
    const at = v => (v + M) * PX;   // le conteneur est dans le calque lointain, qui déborde de M pixels
    const dot = (x, y, cls) => {
      const d = document.createElement("i");
      d.className = cls;
      d.style.left = x + "px"; d.style.top = y + "px";
      box.appendChild(d);
      return d;
    };
    for (let i = 0; i < 18; i++) {
      const t = .15 + r() * .8, hw = tw.hwAt(t);
      sparks.push(dot(at(towerX + (r() < .5 ? -1 : 1) * hw * (.4 + r() * .6)), at(hz - towerH + t * towerH), "spark"));
    }
    for (let i = 0; i < 10; i++) stars.push(dot(at(Math.floor(r() * W)), at(Math.floor(r() * hz * .4)), "star"));
    beacon = dot(at(towerX) - 1, at(hz - towerH) - 3, "beacon");
  }

  // Horloge d'ambiance : 2 changements par seconde au plus (étoiles, nuages, phare, scintillement de la tour),
  // arrêtée dès que la fenêtre n'est plus au premier plan ou que personne ne s'en sert.
  let active = true, tick = 0, ambient = 0, cloudX = 0, burstUntil = 0, nextBurst = 0, nextShoot = 0;
  function step() {
    ambient = 0;
    if (!active || eco) return;
    tick++;
    const now = performance.now();
    const pick = list => list[Math.floor(Math.random() * list.length)];
    if (stars.length) pick(stars).classList.toggle("on");
    if (beacon && tick % 2 === 0) beacon.classList.toggle("on");
    if (!nextBurst) nextBurst = now + 4000;
    // La tour scintille par moments, quelques secondes.
    if (now > nextBurst) { burstUntil = now + 5000; nextBurst = now + 30000 + Math.random() * 20000; }
    if (now < burstUntil) sparks.forEach(s => s.classList.toggle("on", Math.random() < .35));
    else if (tick % 2 === 0) sparks.forEach(s => s.classList.remove("on"));
    // Les nuages avancent d'un pixel du décor toutes les deux secondes.
    if (tick % 4 === 0) {
      cloudX = (cloudX + 1) % W;
      $("clouds").style.transform = `translate3d(${-cloudX * PX}px,0,0)`;
    }
    if (!nextShoot) nextShoot = now + 9000;
    if (now > nextShoot) {
      nextShoot = now + 25000 + Math.random() * 30000;
      const s = document.querySelector(".shooting");
      s.classList.remove("go"); void s.offsetWidth; s.classList.add("go");
    }
    ambient = setTimeout(step, 500);
  }
  function setActive(v) {
    active = v;
    clearTimeout(ambient); ambient = 0;
    if (active && !eco) ambient = setTimeout(step, 500);
  }

  function draw() {
    const vw = innerWidth, vh = innerHeight;
    W = Math.ceil(vw / PX); H = Math.ceil(vh / PX);
    const bottomH = parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--bottom-h")) || 196;
    quay = H - Math.round(bottomH / PX) - 2;
    hz = Math.min(Math.round(H * .62), quay - 14);
    const r = rng(1789);
    const stage = $("stage").getBoundingClientRect();
    const stageX = Math.round((stage.left + stage.width / 2) / PX);
    const sunX = Math.round(Math.min(W - 30, stageX - 38)), sunR = Math.max(14, Math.round(H * .075)), sunY = hz - Math.round(H * .15);
    const towerX = Math.round(Math.min(W * .6, stageX - 88)), towerH = Math.round(Math.min(hz * .8, H * .5));
    const hzPct = (hz / H * 100).toFixed(2);
    const scene = $("scene");
    scene.style.background = `linear-gradient(180deg, #050816 0%, #0a1030 ${hzPct * .3}%, #1b1847 ${hzPct * .52}%, ` +
      `#3d2259 ${hzPct * .68}%, #7a3462 ${hzPct * .8}%, #c4566a ${hzPct * .9}%, #ee8460 ${hzPct * .965}%, #ffb26b ${hzPct}%, #2a1a3a ${hzPct}%)`;
    scene.style.setProperty("--sunx", (sunX / W * 100).toFixed(2) + "%");
    scene.style.setProperty("--suny", (sunY / H * 100).toFixed(2) + "%");
    sky(r, sunX, sunY, sunR);
    clouds(r);
    const { tw } = far(r, towerX, towerH);
    const lit = mid(r);
    near(r, sunX, sunR, lit, towerX, stageX);
    for (const id of ["far", "mid", "near"]) { const c = $(id); c.style.left = "0px"; c.style.top = "0px"; }
    sparkles(r, towerX, tw, towerH);
    $("scene").querySelectorAll(".layer").forEach(l => { l.style.inset = `${-M * PX}px`; });
  }

  function onMove(e) {
    if (eco || !active || parallax) return;
    parallax = requestAnimationFrame(() => {
      parallax = null;
      const nx = e.clientX / innerWidth - .5, ny = e.clientY / innerHeight - .5;
      document.querySelectorAll(".scene .layer").forEach(l => {
        const d = +l.dataset.depth;
        l.style.transform = `translate3d(${Math.round(-nx * d)}px,${Math.round(-ny * d * .5)}px,0)`;
      });
    });
  }

  let timer = 0;
  return {
    init(isEco) {
      eco = isEco;
      draw();
      setActive(true);
      addEventListener("resize", () => { clearTimeout(timer); timer = setTimeout(draw, 120); });
      addEventListener("mousemove", onMove, { passive: true });
    },
    setEco(v) {
      if (v === eco) return;
      eco = v;
      if (eco) document.querySelectorAll(".scene .layer").forEach(l => { l.style.transform = ""; });
      draw();
      setActive(active);
    },
    setActive,
  };
})();
