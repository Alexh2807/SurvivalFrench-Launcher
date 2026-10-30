// Personnage en 3D (WebGL, sans bibliothèque) : pixels nets, lumière du couchant dans le dos, tête qui suit la souris,
// respiration et bras qui se balancent. En mode économie, il ne se redessine que quand on le touche.
"use strict";
const SkinView = (() => {
  let canvas, gl, prog, buffers = [], tex, loc = {};
  let eco = false, paused = false, ready = false, fallback2d = false;
  let slim = false, legacy = false, img = null;
  let yaw = -.42, vyaw = 0, dragging = false, lastX = 0, dragged = false;
  let headYaw = 0, headPitch = 0, lookYaw = .3, targetPitch = 0;
  let raf = 0, lastFrame = 0, keepUntil = 0, idleTimer = 0, active = true;
  const t0 = performance.now();

  // ------------------------------------------------------------------------------ skin par défaut
  // Un Parisien : veste bleu marine, écharpe rouge, chemise blanche, jean (dessin original).
  function defaultSkin() {
    const c = document.createElement("canvas");
    c.width = c.height = 64;
    const g = c.getContext("2d");
    const f = (x, y, w, h, col) => { g.fillStyle = col; g.fillRect(x, y, w, h); };
    const P = {
      skin: "#e2a77e", skinS: "#c88b64", hair: "#3b2618", hairH: "#54372a", white: "#f3f4f6", iris: "#3a6fd8", brow: "#2a1a10",
      mouth: "#a65f49", jacket: "#22397a", jacketS: "#1a2c5e", jacketH: "#2f4c97", shirt: "#eef1f6", scarf: "#d23b44", scarfH: "#ea5a62",
      jeans: "#2d4567", jeansS: "#233651", shoe: "#3a2a20", sole: "#1f1712", belt: "#17171f", gold: "#d8b45a",
    };
    // Tête
    f(8, 0, 8, 8, P.hair); f(16, 0, 8, 8, P.skinS);
    for (const x of [0, 16]) { f(x, 8, 8, 8, P.skin); f(x, 8, 8, 4, P.hair); f(x + (x ? 0 : 5), 12, 3, 1, P.hair); }
    f(2, 12, 2, 2, P.skinS); f(20, 12, 2, 2, P.skinS);
    f(24, 8, 8, 8, P.hair); f(24, 15, 8, 1, P.skinS);
    f(8, 8, 8, 8, P.skin); f(8, 8, 8, 2, P.hair); f(8, 10, 1, 2, P.hair); f(15, 10, 1, 2, P.hair); f(12, 10, 2, 1, P.hair);
    f(9, 11, 2, 1, P.brow); f(13, 11, 2, 1, P.brow);
    f(9, 12, 1, 1, P.white); f(10, 12, 1, 1, P.iris); f(13, 12, 1, 1, P.iris); f(14, 12, 1, 1, P.white);
    f(11, 13, 2, 1, P.skinS); f(11, 14, 2, 1, P.mouth); f(9, 15, 6, 1, P.skinS);
    // Corps
    f(20, 16, 8, 4, P.jacket); f(28, 16, 8, 4, P.jeans);
    f(16, 20, 24, 12, P.jacket);
    f(20, 20, 8, 2, P.scarf); f(20, 20, 8, 1, P.scarfH); f(32, 20, 8, 2, P.scarf); f(16, 20, 4, 2, P.scarf); f(28, 20, 4, 2, P.scarf);
    f(23, 22, 2, 8, P.shirt); f(22, 22, 1, 4, P.scarf); f(22, 26, 1, 1, P.scarfH);
    f(21, 22, 1, 8, P.jacketH); f(26, 22, 1, 8, P.jacketS);
    f(16, 30, 24, 1, P.belt); f(24, 30, 1, 1, P.gold); f(16, 31, 24, 1, P.jeans);
    f(17, 24, 1, 6, P.jacketS); f(29, 24, 1, 6, P.jacketS);
    // Bras (droit puis gauche, même dessin)
    for (const [u, v] of [[40, 16], [32, 48]]) {
      f(u + 4, v, 4, 4, P.jacket); f(u + 8, v, 4, 4, P.skin);
      f(u, v + 4, 16, 9, P.jacket); f(u, v + 13, 16, 1, P.jacketS); f(u, v + 14, 16, 2, P.skin);
      f(u + 4, v + 4, 1, 9, P.jacketH); f(u + 12, v + 4, 4, 9, P.jacketS);
    }
    // Jambes
    for (const [u, v] of [[0, 16], [16, 48]]) {
      f(u + 4, v, 4, 4, P.jeans); f(u + 8, v, 4, 4, P.sole);
      f(u, v + 4, 16, 10, P.jeans); f(u + 4, v + 9, 4, 1, P.jeansS); f(u, v + 14, 16, 1, P.shoe); f(u, v + 15, 16, 1, P.sole);
      f(u + 4, v + 4, 1, 10, P.jeansS);
    }
    // Seconde couche : mèche du front et volume des cheveux
    f(40, 8, 8, 1, P.hair); f(40, 9, 2, 1, P.hair); f(46, 9, 2, 1, P.hairH); f(44, 9, 1, 1, P.hair);
    f(40, 0, 8, 8, P.hairH); f(41, 1, 6, 6, "rgba(0,0,0,0)");
    g.clearRect(41, 1, 6, 6);
    for (const x of [32, 48]) f(x, 8, 8, 3, P.hair);
    f(56, 8, 8, 6, P.hair);
    // Col de la veste
    f(20, 36, 2, 2, P.jacketH); f(26, 36, 2, 2, P.jacketH);
    return c;
  }

  // ------------------------------------------------------------------------------- géométrie
  function box(u, v, w, h, d, inflate, texH) {
    const W = w / 2 + inflate, Hh = h / 2 + inflate, D = d / 2 + inflate;
    const x0 = -W, x1 = W, y0 = -Hh, y1 = Hh, z0 = -D, z1 = D;
    const e = .02;
    const uv = (a, b, c2, d2) => [(a + e) / 64, (b + e) / texH, (c2 - e) / 64, (d2 - e) / texH];
    const faces = [
      // [coins TL, TR, BR, BL], normale, rectangle de texture
      [[[x0, y1, z1], [x1, y1, z1], [x1, y0, z1], [x0, y0, z1]], [0, 0, 1], uv(u + d, v + d, u + d + w, v + d + h)],
      [[[x1, y1, z0], [x0, y1, z0], [x0, y0, z0], [x1, y0, z0]], [0, 0, -1], uv(u + 2 * d + w, v + d, u + 2 * d + 2 * w, v + d + h)],
      [[[x0, y1, z0], [x0, y1, z1], [x0, y0, z1], [x0, y0, z0]], [-1, 0, 0], uv(u, v + d, u + d, v + d + h)],
      [[[x1, y1, z1], [x1, y1, z0], [x1, y0, z0], [x1, y0, z1]], [1, 0, 0], uv(u + d + w, v + d, u + 2 * d + w, v + d + h)],
      [[[x0, y1, z0], [x1, y1, z0], [x1, y1, z1], [x0, y1, z1]], [0, 1, 0], uv(u + d, v, u + d + w, v + d)],
      [[[x0, y0, z1], [x1, y0, z1], [x1, y0, z0], [x0, y0, z0]], [0, -1, 0], uv(u + d + w, v, u + d + 2 * w, v + d)],
    ];
    const out = [];
    for (const [[a, b, c, dd], n, [u0, v0, u1, v1]] of faces) {
      const q = [[a, u0, v0], [b, u1, v0], [c, u1, v1], [a, u0, v0], [c, u1, v1], [dd, u0, v1]];
      for (const [p, tu, tv] of q) out.push(p[0], p[1], p[2], tu, tv, n[0], n[1], n[2]);
    }
    return out;
  }

  function mirror(data) {
    for (let i = 0; i < data.length; i += 8) { data[i] = -data[i]; data[i + 5] = -data[i + 5]; }
    return data;
  }

  // Parties : [nom, u, v, w, h, d, gonflement, pivot, décalage du centre, symétrie]
  function parts() {
    const aw = slim ? 3 : 4, ax = 4 + aw / 2, th = legacy ? 32 : 64;
    const list = [
      ["head", 0, 0, 8, 8, 8, 0, [0, 24, 0], [0, 4, 0]],
      ["body", 16, 16, 8, 12, 4, 0, [0, 18, 0], [0, 0, 0]],
      ["rarm", 40, 16, aw, 12, 4, 0, [-ax, 22, 0], [0, -4, 0]],
      ["rleg", 0, 16, 4, 12, 4, 0, [-1.98, 12, 0], [0, -6, 0]],
      ["hat", 32, 0, 8, 8, 8, .5, [0, 24, 0], [0, 4, 0], 0, "head"],
    ];
    if (legacy) {
      list.push(["larm", 40, 16, aw, 12, 4, 0, [ax, 22, 0], [0, -4, 0], 1], ["lleg", 0, 16, 4, 12, 4, 0, [1.98, 12, 0], [0, -6, 0], 1]);
    } else {
      list.push(["larm", 32, 48, aw, 12, 4, 0, [ax, 22, 0], [0, -4, 0]], ["lleg", 16, 48, 4, 12, 4, 0, [1.98, 12, 0], [0, -6, 0]],
        ["jacket", 16, 32, 8, 12, 4, .25, [0, 18, 0], [0, 0, 0], 0, "body"],
        ["rsleeve", 40, 32, aw, 12, 4, .25, [-ax, 22, 0], [0, -4, 0], 0, "rarm"],
        ["lsleeve", 48, 48, aw, 12, 4, .25, [ax, 22, 0], [0, -4, 0], 0, "larm"],
        ["rpants", 0, 32, 4, 12, 4, .25, [-1.98, 12, 0], [0, -6, 0], 0, "rleg"],
        ["lpants", 0, 48, 4, 12, 4, .25, [1.98, 12, 0], [0, -6, 0], 0, "lleg"]);
    }
    return list.map(([name, u, v, w, h, d, inf, pivot, off, mir, anim]) => {
      let data = box(u, v, w, h, d, inf, th);
      for (let i = 0; i < data.length; i += 8) { data[i] += off[0]; data[i + 1] += off[1]; data[i + 2] += off[2]; }
      if (mir) data = mirror(data);
      return { name, anim: anim || name, pivot, overlay: inf > 0, data: new Float32Array(data) };
    });
  }

  // ------------------------------------------------------------------------------- matrices
  const ident = () => [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
  function mul(a, b) {
    const o = new Array(16);
    for (let c = 0; c < 4; c++) for (let r = 0; r < 4; r++) {
      o[c * 4 + r] = a[r] * b[c * 4] + a[4 + r] * b[c * 4 + 1] + a[8 + r] * b[c * 4 + 2] + a[12 + r] * b[c * 4 + 3];
    }
    return o;
  }
  const tr = (x, y, z) => [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, x, y, z, 1];
  const rx = a => { const c = Math.cos(a), s = Math.sin(a); return [1, 0, 0, 0, 0, c, s, 0, 0, -s, c, 0, 0, 0, 0, 1]; };
  const ry = a => { const c = Math.cos(a), s = Math.sin(a); return [c, 0, -s, 0, 0, 1, 0, 0, s, 0, c, 0, 0, 0, 0, 1]; };
  const rz = a => { const c = Math.cos(a), s = Math.sin(a); return [c, s, 0, 0, -s, c, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]; };
  function persp(fov, asp, n, f) {
    const t = 1 / Math.tan(fov / 2);
    return [t / asp, 0, 0, 0, 0, t, 0, 0, 0, 0, (f + n) / (n - f), -1, 0, 0, 2 * f * n / (n - f), 0];
  }

  // ------------------------------------------------------------------------------- WebGL
  const VS = `attribute vec3 aPos; attribute vec2 aUv; attribute vec3 aNor;
    uniform mat4 uProj, uView, uModel; varying vec2 vUv; varying vec3 vN;
    void main() { vUv = aUv; vN = (uModel * vec4(aNor, 0.0)).xyz; gl_Position = uProj * uView * uModel * vec4(aPos, 1.0); }`;
  const FS = `precision mediump float; uniform sampler2D uTex; uniform float uOverlay; varying vec2 vUv; varying vec3 vN;
    void main() {
      vec4 c = texture2D(uTex, vUv);
      if (uOverlay > 0.5 && c.a < 0.5) discard;
      vec3 n = normalize(vN);
      float key = max(dot(n, normalize(vec3(-0.35, 0.55, 0.8))), 0.0);
      float rim = max(dot(n, normalize(vec3(0.75, 0.25, -0.6))), 0.0);
      float top = max(n.y, 0.0);
      vec3 light = vec3(0.42, 0.44, 0.56) + vec3(0.66, 0.64, 0.62) * key + vec3(1.0, 0.58, 0.38) * rim * 0.75 + vec3(0.1, 0.08, 0.14) * top;
      gl_FragColor = vec4(c.rgb * light, 1.0);
    }`;

  function initGL() {
    gl = canvas.getContext("webgl", { alpha: true, antialias: true, premultipliedAlpha: true, powerPreference: "low-power" });
    if (!gl) return false;
    const sh = (type, src) => { const s = gl.createShader(type); gl.shaderSource(s, src); gl.compileShader(s); return s; };
    prog = gl.createProgram();
    gl.attachShader(prog, sh(gl.VERTEX_SHADER, VS));
    gl.attachShader(prog, sh(gl.FRAGMENT_SHADER, FS));
    gl.linkProgram(prog);
    if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) return false;
    gl.useProgram(prog);
    for (const n of ["aPos", "aUv", "aNor"]) loc[n] = gl.getAttribLocation(prog, n);
    for (const n of ["uProj", "uView", "uModel", "uTex", "uOverlay"]) loc[n] = gl.getUniformLocation(prog, n);
    gl.enable(gl.DEPTH_TEST);
    gl.clearColor(0, 0, 0, 0);
    tex = gl.createTexture();
    canvas.addEventListener("webglcontextlost", e => { e.preventDefault(); ready = false; }, false);
    canvas.addEventListener("webglcontextrestored", () => { initGL() && upload(); }, false);
    return true;
  }

  function upload() {
    if (!gl || !img) return;
    buffers.forEach(b => gl.deleteBuffer(b.buf));
    buffers = parts().map(p => {
      const buf = gl.createBuffer();
      gl.bindBuffer(gl.ARRAY_BUFFER, buf);
      gl.bufferData(gl.ARRAY_BUFFER, p.data, gl.STATIC_DRAW);
      return { ...p, buf, count: p.data.length / 8 };
    });
    gl.bindTexture(gl.TEXTURE_2D, tex);
    gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, false);
    gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, false);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, img);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    ready = true;
  }

  function resize() {
    const dpr = Math.min(devicePixelRatio || 1, 2);
    const w = Math.round(canvas.clientWidth * dpr), h = Math.round(canvas.clientHeight * dpr);
    if (canvas.width !== w || canvas.height !== h) { canvas.width = w; canvas.height = h; }
  }

  function render(now) {
    resize();
    if (fallback2d) return render2d();
    if (!ready) return;
    const t = (now - t0) / 1000;
    // Poids du geste en cours (0 au repos) : la pose de repos est exactement celle de début et de fin de geste.
    const gp = gesture ? Math.min(1, Math.max(0, (now - gesture.start) / GESTURE_MS)) : 1;
    const w = gesture ? Math.sin(Math.PI * gp) ** 2 : 0;
    const look = gesture && gesture.kind === "look" ? Math.sin(gp * Math.PI * 2) * .42 * w : 0;
    gl.viewport(0, 0, canvas.width, canvas.height);
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
    const asp = canvas.width / canvas.height;
    gl.uniformMatrix4fv(loc.uProj, false, persp(.52, asp, 10, 200));
    gl.uniformMatrix4fv(loc.uView, false, tr(0, -16.4, -82));
    if (!dragging) {
      yaw += vyaw;
      vyaw *= .92;
      if (Math.abs(vyaw) < .0004) vyaw = 0;
    }
    // La tête vise la souris : direction voulue moins l'orientation du corps, sans se tordre le cou.
    const targetYaw = Math.max(-.75, Math.min(.75, lookYaw - yaw + look));
    headYaw += (targetYaw - headYaw) * .14;
    headPitch += (targetPitch - headPitch) * .14;
    const root = ry(yaw);
    const breathe = w * Math.sin(gp * Math.PI * 2 - Math.PI / 2) * .5 + w * .5;
    const swing = gesture && gesture.kind === "sway" ? Math.sin(gp * Math.PI * 3) * w : 0;
    const pose = {
      head: mul(rx(headPitch - breathe * .05), ry(headYaw)),
      body: ident(),
      rarm: mul(rz(-(.045 + .05 * breathe)), rx(.02 + .14 * swing)),
      larm: mul(rz(.045 + .05 * breathe), rx(-.02 - .14 * swing)),
      rleg: rx(.02), lleg: rx(-.02),
    };
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, tex);
    gl.uniform1i(loc.uTex, 0);
    const lift = breathe * .22;
    for (const pass of [false, true]) {
      gl.uniform1f(loc.uOverlay, pass ? 1 : 0);
      for (const b of buffers) {
        if (b.overlay !== pass) continue;
        const [px, py, pz] = b.pivot;
        const up = b.anim === "rleg" || b.anim === "lleg" ? 0 : lift;
        const model = mul(root, mul(tr(px, py + up, pz), pose[b.anim] || ident()));
        gl.uniformMatrix4fv(loc.uModel, false, model);
        gl.bindBuffer(gl.ARRAY_BUFFER, b.buf);
        gl.enableVertexAttribArray(loc.aPos); gl.vertexAttribPointer(loc.aPos, 3, gl.FLOAT, false, 32, 0);
        gl.enableVertexAttribArray(loc.aUv); gl.vertexAttribPointer(loc.aUv, 2, gl.FLOAT, false, 32, 12);
        gl.enableVertexAttribArray(loc.aNor); gl.vertexAttribPointer(loc.aNor, 3, gl.FLOAT, false, 32, 20);
        gl.drawArrays(gl.TRIANGLES, 0, b.count);
      }
    }
  }

  // Sans WebGL (très vieilles cartes) : vue de face à plat.
  function render2d() {
    const g = canvas.getContext("2d");
    if (!img) return;
    g.clearRect(0, 0, canvas.width, canvas.height);
    g.imageSmoothingEnabled = false;
    const k = Math.floor(Math.min(canvas.width / 18, canvas.height / 36)), ox = canvas.width / 2, oy = canvas.height - 2 * k;
    const s = img.width / 64, aw = slim ? 3 : 4;
    const part = (u, v, w, h, x, y) => g.drawImage(img, u * s, v * s, w * s, h * s, ox + x * k, oy - y * k, w * k, h * k);
    const layers = [[8, 8, 8, 8, -4, 32], [20, 20, 8, 12, -4, 24], [44, 20, aw, 12, -4 - aw, 24], [4, 20, 4, 12, -4, 12],
      legacy ? [44, 20, aw, 12, 4, 24] : [36, 52, aw, 12, 4, 24], legacy ? [4, 20, 4, 12, 0, 12] : [20, 52, 4, 12, 0, 12]];
    layers.forEach(l => part(...l));
    part(40, 8, 8, 8, -4, 32);
    if (!legacy) [[20, 36, 8, 12, -4, 24], [44, 36, aw, 12, -4 - aw, 24], [52, 52, aw, 12, 4, 24], [4, 36, 4, 12, -4, 12], [4, 52, 4, 12, 0, 12]]
      .forEach(l => part(...l));
  }

  // ------------------------------------------------------------------------------- boucle
  // Chaque image coûte au moteur graphique : on n'en dessine que quand quelque chose bouge. Au repos, un petit
  // geste toutes les 8 à 12 s (regarder autour de soi, respirer, se balancer) ; rien entre deux gestes.
  const GESTURE_MS = 1700;
  let gesture = null, nextGesture = 0;
  function loop(now) {
    raf = 0;
    if (paused) return;
    const alive = !eco && active;
    if (gesture && now - gesture.start > GESTURE_MS) gesture = null;
    if (!gesture && alive && now >= nextGesture) {
      if (nextGesture) gesture = { start: now, kind: ["look", "breathe", "sway", "look"][Math.floor(Math.random() * 4)] };
      nextGesture = now + 8000 + Math.random() * 4000;
    }
    const interacting = dragging || vyaw !== 0 || now < keepUntil;
    if (interacting || gesture || now - lastFrame > 1000) {
      // 30 images/s pour suivre la souris (20 en mode économie) ; chaque image pendant qu'on le fait tourner.
      if (dragging || now - lastFrame >= (eco ? 50 : 30)) { lastFrame = now; render(now); }
    }
    if (interacting || gesture) raf = requestAnimationFrame(loop);
    else if (alive) { clearTimeout(idleTimer); idleTimer = setTimeout(() => { if (!raf) raf = requestAnimationFrame(loop); }, nextGesture - now); }
  }
  function kick(ms = 0) {
    keepUntil = Math.max(keepUntil, performance.now() + ms);
    clearTimeout(idleTimer);
    if (!raf && !paused) raf = requestAnimationFrame(loop);
  }

  function analyse(image, model) {
    legacy = image.height * 2 === image.width;
    if (model === "slim") slim = true;
    else if (model === "wide" || model === "default" || legacy) slim = false;
    else {
      // Bras fins : la 4e colonne du bras droit est transparente.
      const c = document.createElement("canvas");
      c.width = image.width; c.height = image.height;
      const g = c.getContext("2d");
      g.drawImage(image, 0, 0);
      const s = image.width / 64;
      slim = g.getImageData(Math.floor(54 * s), Math.floor(20 * s), 1, 1).data[3] === 0;
    }
  }

  let faceUrl = "";
  function makeFace() {
    const c = document.createElement("canvas");
    c.width = c.height = 8;
    const g = c.getContext("2d");
    g.imageSmoothingEnabled = false;
    const s = img.width / 64;
    g.drawImage(img, 8 * s, 8 * s, 8 * s, 8 * s, 0, 0, 8, 8);
    g.drawImage(img, 40 * s, 8 * s, 8 * s, 8 * s, 0, 0, 8, 8);
    faceUrl = c.toDataURL();
  }

  return {
    init(el, isEco) {
      canvas = el;
      eco = isEco;
      if (!initGL()) fallback2d = true;
      img = defaultSkin();
      analyse(img, "wide");
      makeFace();
      if (!fallback2d) upload();
      canvas.addEventListener("pointerdown", e => {
        dragging = true; dragged = true; lastX = e.clientX; vyaw = 0;
        canvas.setPointerCapture(e.pointerId); canvas.classList.add("dragging"); kick();
        const hint = document.getElementById("dragHint"); if (hint) hint.style.opacity = 0;
      });
      canvas.addEventListener("pointermove", e => {
        if (!dragging) return;
        const dx = e.clientX - lastX; lastX = e.clientX;
        yaw += dx * .011; vyaw = dx * .011;
      });
      const up = () => { if (dragging) { dragging = false; canvas.classList.remove("dragging"); kick(1500); } };
      canvas.addEventListener("pointerup", up);
      canvas.addEventListener("pointercancel", up);
      // La tête suit la souris partout dans la fenêtre.
      addEventListener("mousemove", e => {
        const r = canvas.getBoundingClientRect();
        const dx = e.clientX - (r.left + r.width / 2), dy = e.clientY - (r.top + r.height * .22);
        lookYaw = Math.atan2(dx, 420) * 1.25;
        targetPitch = Math.max(-.45, Math.min(.5, Math.atan2(dy, 520)));
        if (eco) kick(400);
      }, { passive: true });
      document.addEventListener("visibilitychange", () => { paused = document.hidden; if (!paused) kick(300); });
      kick(200);
    },
    /** Charge un skin (adresse) ou le personnage par défaut (null). */
    load(url, model, done) {
      const apply = image => {
        img = image;
        analyse(image, model);
        makeFace();
        if (!fallback2d) upload();
        kick(300);
        done && done(faceUrl);
      };
      if (!url) return apply(defaultSkin());
      const i = new Image();
      i.onload = () => (i.width === 64 * Math.round(i.width / 64) && i.width >= 64 ? apply(i) : apply(defaultSkin()));
      i.onerror = () => apply(defaultSkin());
      i.src = url;
    },
    face: () => faceUrl,
    setEco(v) { eco = v; kick(300); },
    setActive(v) { active = v; if (v) kick(0); },
  };
})();
