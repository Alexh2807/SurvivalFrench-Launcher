"""SurvivalFrench — Launcher.

Connexion Microsoft (compte Minecraft officiel), installation de Minecraft + Fabric + mods du serveur
(synchronisés avec le manifeste publié par le plugin), puis lancement direct sur le serveur.
L'interface est servie en local (127.0.0.1) et s'ouvre dans une fenêtre d'application.
Aucune dépendance externe : bibliothèque standard Python uniquement.
"""
from __future__ import annotations

import concurrent.futures
import ctypes
import ctypes.wintypes
import hashlib
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import threading
import time
import traceback
import urllib.error
import urllib.parse
import urllib.request
import uuid
import webbrowser
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

APP_NAME = "SurvivalFrench Launcher"
VERSION = "1.2.1"


def _instance() -> int:
    for i, arg in enumerate(sys.argv):
        if arg == "--instance" and i + 1 < len(sys.argv) and sys.argv[i + 1].isdigit():
            return max(1, min(9, int(sys.argv[i + 1])))
    return 1


INSTANCE = _instance()
UI_PORT = 47833 + (INSTANCE - 1)
USER_AGENT = f"SurvivalFrench-Launcher/{VERSION}"

VERSION_MANIFEST = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json"
JAVA_RUNTIMES = "https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json"
FABRIC_PROFILE = "https://meta.fabricmc.net/v2/versions/loader/{mc}/{loader}/profile/json"
ASSET_URL = "https://resources.download.minecraft.net/{prefix}/{hash}"

MS_DEVICE = "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode"
MS_TOKEN = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token"
XBL_AUTH = "https://user.auth.xboxlive.com/user/authenticate"
XSTS_AUTH = "https://xsts.auth.xboxlive.com/xsts/authorize"
MC_LOGIN = "https://api.minecraftservices.com/authentication/login_with_xbox"
MC_PROFILE = "https://api.minecraftservices.com/minecraft/profile"
MS_SCOPE = "XboxLive.signin offline_access"


# --------------------------------------------------------------------------------------
# Emplacements et réglages
# --------------------------------------------------------------------------------------

def app_dir() -> Path:
    return Path(sys.executable).parent if getattr(sys, "frozen", False) else Path(__file__).resolve().parent


def resource(name: str) -> Path:
    return Path(getattr(sys, "_MEIPASS", Path(__file__).resolve().parent)) / name


DATA = Path(os.environ.get("SFRP_DATA_DIR") or Path(os.environ.get("APPDATA", str(Path.home()))) / ".survivalfrench")
GAME = DATA / ("game" if INSTANCE == 1 else f"game-{INSTANCE}")
LIBRARIES = DATA / "libraries"
ASSETS = DATA / "assets"
VERSIONS = DATA / "versions"
RUNTIME = DATA / "runtime"
NATIVES = DATA / "natives"
SETTINGS_FILE = DATA / ("settings.json" if INSTANCE == 1 else f"settings-{INSTANCE}.json")
ACCOUNT_FILE = DATA / ("account.dat" if INSTANCE == 1 else f"account-{INSTANCE}.dat")

# Launcher Minecraft officiel : il gère lui-même la connexion Microsoft.
MINECRAFT_DIR = Path(os.environ.get("SFRP_MINECRAFT_DIR") or Path(os.environ.get("APPDATA", str(Path.home()))) / ".minecraft")
OFFICIAL_STORE_FAMILY = "Microsoft.4297127D64EC6_8wekyb3d8bbwe"
OFFICIAL_DOWNLOAD = "https://www.minecraft.net/fr-fr/download"
PROFILE_ID = "survivalfrench"

# Assets déjà présents sur la machine (lecture seule) : évite de retélécharger ~600 Mo.
ASSET_REUSE = [
    Path(os.environ.get("APPDATA", "")) / ".minecraft" / "assets" / "objects",
    Path.home() / ".gradle" / "caches" / "fabric-loom" / "assets" / "objects",
]


def load_config() -> dict:
    """launcher.json : réglages du serveur (embarqué dans l'exe, surchargeable à côté de l'exe)."""
    cfg = {"manifest_url": "http://localhost:8163/launcher/manifest.json", "client_id": "", "allow_offline": False,
           "update_repo": ""}
    extra = os.environ.get("SFRP_LAUNCHER_CONFIG")
    for path in (resource("launcher.json"), app_dir() / "launcher.json", Path(extra) if extra else None):
        if path is None:
            continue
        try:
            cfg.update(json.loads(path.read_text(encoding="utf-8")))
        except (OSError, ValueError):
            pass
    return cfg


CONFIG = load_config()


def load_settings() -> dict:
    s = {"ram_mb": 4096, "manifest_url": ""}
    try:
        s.update(json.loads(SETTINGS_FILE.read_text(encoding="utf-8")))
    except (OSError, ValueError):
        pass
    return s


def save_settings(s: dict) -> None:
    DATA.mkdir(parents=True, exist_ok=True)
    SETTINGS_FILE.write_text(json.dumps(s, indent=2), encoding="utf-8")


def manifest_url() -> str:
    return (STATE["settings"].get("manifest_url") or CONFIG["manifest_url"]).strip()


# --------------------------------------------------------------------------------------
# Chiffrement du compte (DPAPI Windows : seul cet utilisateur Windows peut le relire)
# --------------------------------------------------------------------------------------

class _Blob(ctypes.Structure):
    _fields_ = [("cbData", ctypes.wintypes.DWORD), ("pbData", ctypes.POINTER(ctypes.c_char))]


def _dpapi(data: bytes, protect: bool) -> bytes:
    if os.name != "nt":
        return data
    crypt = ctypes.windll.crypt32
    buf = ctypes.create_string_buffer(data, len(data))
    blob_in = _Blob(len(data), buf)
    blob_out = _Blob()
    fn = crypt.CryptProtectData if protect else crypt.CryptUnprotectData
    if not fn(ctypes.byref(blob_in), None, None, None, None, 0, ctypes.byref(blob_out)):
        raise OSError("DPAPI a échoué")
    try:
        return ctypes.string_at(blob_out.pbData, blob_out.cbData)
    finally:
        ctypes.windll.kernel32.LocalFree(blob_out.pbData)


def load_account() -> dict | None:
    try:
        return json.loads(_dpapi(ACCOUNT_FILE.read_bytes(), False).decode("utf-8"))
    except (OSError, ValueError):
        return None


def save_account(acc: dict | None) -> None:
    DATA.mkdir(parents=True, exist_ok=True)
    if acc is None:
        ACCOUNT_FILE.unlink(missing_ok=True)
    else:
        ACCOUNT_FILE.write_bytes(_dpapi(json.dumps(acc).encode("utf-8"), True))


# --------------------------------------------------------------------------------------
# État partagé avec l'interface
# --------------------------------------------------------------------------------------

LOCK = threading.RLock()
STATE: dict = {
    "account": None,
    "login": None,
    "task": {"state": "idle", "step": "", "progress": 0.0, "detail": "", "error": ""},
    "settings": load_settings(),
    "manifest": None,
    "manifest_error": "",
}
LAST_PING = time.time()
GAME_PROCESS: subprocess.Popen | None = None


def task(**kw) -> None:
    with LOCK:
        STATE["task"].update(kw)


def public_account(acc: dict | None) -> dict | None:
    if not acc:
        return None
    return {"name": acc.get("name"), "uuid": acc.get("uuid"), "offline": acc.get("type") == "offline"}


# --------------------------------------------------------------------------------------
# HTTP
# --------------------------------------------------------------------------------------

def http_json(url: str, data: dict | None = None, form: dict | None = None, headers: dict | None = None,
              method: str | None = None, timeout: int = 30) -> dict:
    h = {"User-Agent": USER_AGENT, "Accept": "application/json"}
    body = None
    if form is not None:
        body = urllib.parse.urlencode(form).encode()
        h["Content-Type"] = "application/x-www-form-urlencoded"
    elif data is not None:
        body = json.dumps(data).encode()
        h["Content-Type"] = "application/json"
    h.update(headers or {})
    req = urllib.request.Request(url, data=body, headers=h, method=method)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            raw = r.read()
            return json.loads(raw) if raw else {}
    except urllib.error.HTTPError as e:
        raw = e.read()
        try:
            payload = json.loads(raw)
        except ValueError:
            payload = {"raw": raw.decode("utf-8", "replace")[:300]}
        payload["_status"] = e.code
        return payload


def sha1_of(path: Path) -> str:
    h = hashlib.sha1()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def valid(path: Path, sha1: str | None, size: int | None = None) -> bool:
    if not path.is_file():
        return False
    if size is not None and path.stat().st_size != size:
        return False
    return sha1 is None or sha1_of(path) == sha1


def download(url: str, path: Path, sha1: str | None = None, size: int | None = None, retries: int = 3) -> None:
    if valid(path, sha1, size):
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(path.name + ".part")
    last = None
    for attempt in range(retries):
        try:
            req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
            with urllib.request.urlopen(req, timeout=60) as r, open(tmp, "wb") as out:
                shutil.copyfileobj(r, out, 1 << 20)
            if sha1 and sha1_of(tmp) != sha1:
                raise OSError(f"empreinte incorrecte pour {path.name}")
            os.replace(tmp, path)
            return
        except (OSError, urllib.error.URLError) as e:
            last = e
            time.sleep(1 + attempt)
    tmp.unlink(missing_ok=True)
    raise OSError(f"Téléchargement impossible : {url} ({last})")


# --------------------------------------------------------------------------------------
# Mise à jour du launcher (GitHub Releases)
# --------------------------------------------------------------------------------------

def _version_tuple(v: str) -> tuple[int, ...]:
    return tuple(int(p) for p in v.strip().lstrip("vV").split(".") if p.isdigit())


def self_update() -> bool:
    """Au démarrage : si une version plus récente est publiée sur GitHub, remplace l'exe et le relance."""
    repo = str(CONFIG.get("update_repo") or "").strip()
    exe = Path(sys.executable)
    if not repo or not getattr(sys, "frozen", False) or "--no-update" in sys.argv:
        return False
    for old in exe.parent.glob(exe.stem + ".old-*.exe"):  # restes d'une mise à jour précédente
        try:
            old.unlink()
        except OSError:
            pass
    try:
        rel = http_json(f"https://api.github.com/repos/{repo}/releases/latest",
                        headers={"Accept": "application/vnd.github+json"}, timeout=5)
        tag = str(rel.get("tag_name") or "")
        if not tag or _version_tuple(tag) <= _version_tuple(VERSION):
            return False
        asset = next((a for a in rel.get("assets", []) if str(a.get("name", "")).lower().endswith(".exe")), None)
        if not asset:
            return False
        new = exe.with_name(exe.stem + ".new.exe")
        new.unlink(missing_ok=True)
        download(asset["browser_download_url"], new, size=asset.get("size"))
        digest = str(asset.get("digest") or "")
        if digest.startswith("sha256:") and hashlib.sha256(new.read_bytes()).hexdigest() != digest[7:]:
            new.unlink(missing_ok=True)
            return False
        # Windows accepte de renommer un exe en cours d'exécution, pas de l'écraser.
        os.replace(exe, exe.with_name(f"{exe.stem}.old-{int(time.time())}.exe"))
        os.replace(new, exe)
    except Exception:  # noqa: BLE001 — pas de réseau, dossier en lecture seule… : on garde la version actuelle
        traceback.print_exc()
        return False
    # PYINSTALLER_RESET_ENVIRONMENT : le nouvel exe se décompresse dans son propre dossier temporaire.
    subprocess.Popen([str(exe), *sys.argv[1:]], env=dict(os.environ, PYINSTALLER_RESET_ENVIRONMENT="1"),
                     close_fds=True)
    return True


def parallel(jobs: list, label: str, start: float, end: float, workers: int = 12) -> None:
    """Exécute des téléchargements en parallèle en faisant avancer la barre de start à end."""
    total = len(jobs)
    if total == 0:
        task(progress=end)
        return
    done = 0
    with concurrent.futures.ThreadPoolExecutor(workers) as pool:
        futures = [pool.submit(j) for j in jobs]
        for f in concurrent.futures.as_completed(futures):
            f.result()
            done += 1
            if done % 10 == 0 or done == total:
                task(progress=start + (end - start) * done / total, detail=f"{label} : {done} / {total}")


# --------------------------------------------------------------------------------------
# Connexion Microsoft → Xbox Live → Minecraft
# --------------------------------------------------------------------------------------

class LoginError(Exception):
    pass


def ms_device_login() -> None:
    """Connexion par code : l'utilisateur valide sur microsoft.com/link (aucun mot de passe ne passe par le launcher)."""
    client_id = CONFIG.get("client_id", "").strip()
    try:
        if not client_id:
            raise LoginError("Connexion Microsoft non configurée (client_id manquant dans launcher.json).")
        r = http_json(MS_DEVICE, form={"client_id": client_id, "scope": MS_SCOPE})
        if "device_code" not in r:
            raise LoginError(r.get("error_description", "Microsoft refuse la demande de connexion."))
        with LOCK:
            STATE["login"] = {"user_code": r["user_code"], "url": r.get("verification_uri", "https://www.microsoft.com/link"),
                              "expires": time.time() + int(r.get("expires_in", 900)), "error": ""}
        interval = int(r.get("interval", 5))
        while True:
            time.sleep(interval)
            with LOCK:
                if STATE["login"] is None:
                    return  # annulé
            t = http_json(MS_TOKEN, form={"grant_type": "urn:ietf:params:oauth:grant-type:device_code",
                                          "client_id": client_id, "device_code": r["device_code"]})
            err = t.get("error")
            if err == "authorization_pending":
                continue
            if err == "slow_down":
                interval += 5
                continue
            if err:
                raise LoginError({"expired_token": "Le code a expiré, recommence.",
                                  "authorization_declined": "Connexion refusée."}.get(err, t.get("error_description", err)))
            acc = minecraft_from_ms(t["access_token"], t.get("refresh_token", ""))
            save_account(acc)
            with LOCK:
                STATE["account"] = acc
                STATE["login"] = None
            return
    except LoginError as e:
        with LOCK:
            STATE["login"] = {"error": str(e)}
    except Exception as e:  # noqa: BLE001 — erreur réseau ou inattendue, affichée à l'utilisateur
        with LOCK:
            STATE["login"] = {"error": f"Erreur de connexion : {e}"}


def minecraft_from_ms(ms_token: str, refresh_token: str) -> dict:
    xbl = http_json(XBL_AUTH, data={"Properties": {"AuthMethod": "RPS", "SiteName": "user.auth.xboxlive.com",
                                                   "RpsTicket": "d=" + ms_token},
                                    "RelyingParty": "http://auth.xboxlive.com", "TokenType": "JWT"})
    if "Token" not in xbl:
        raise LoginError("Xbox Live refuse la connexion.")
    uhs = xbl["DisplayClaims"]["xui"][0]["uhs"]
    xsts = http_json(XSTS_AUTH, data={"Properties": {"SandboxId": "RETAIL", "UserTokens": [xbl["Token"]]},
                                      "RelyingParty": "rp://api.minecraftservices.com/", "TokenType": "JWT"})
    if "Token" not in xsts:
        code = xsts.get("XErr")
        raise LoginError({2148916233: "Ce compte Microsoft n'a pas de profil Xbox : crée-le sur xbox.com puis réessaie.",
                          2148916235: "Xbox Live n'est pas disponible dans ton pays.",
                          2148916238: "Compte mineur : il doit être ajouté à une famille Microsoft par un adulte."}
                         .get(code, f"Xbox Live refuse l'accès (code {code})."))
    xid = xsts["DisplayClaims"]["xui"][0].get("xid", "0")
    mc = http_json(MC_LOGIN, data={"identityToken": f"XBL3.0 x={uhs};{xsts['Token']}"})
    if "access_token" not in mc:
        if mc.get("_status") == 403:
            raise LoginError("Cette application n'est pas encore autorisée par Mojang (validation de l'app Azure en attente).")
        raise LoginError("Minecraft refuse la connexion.")
    profile = http_json(MC_PROFILE, headers={"Authorization": "Bearer " + mc["access_token"]})
    if "id" not in profile:
        raise LoginError("Ce compte ne possède pas Minecraft Java Edition.")
    return {"type": "msa", "name": profile["name"], "uuid": profile["id"], "xuid": xid,
            "mc_token": mc["access_token"], "mc_expires": time.time() + int(mc.get("expires_in", 86400)),
            "refresh_token": refresh_token}


def ensure_fresh(acc: dict) -> dict:
    """Renouvelle le jeton Minecraft s'il expire bientôt (grâce au jeton de rafraîchissement Microsoft)."""
    if acc.get("type") != "msa" or acc.get("mc_expires", 0) - 300 > time.time():
        return acc
    task(step="Connexion", detail="Renouvellement de la session Microsoft…")
    t = http_json(MS_TOKEN, form={"grant_type": "refresh_token", "client_id": CONFIG.get("client_id", ""),
                                  "refresh_token": acc.get("refresh_token", ""), "scope": MS_SCOPE})
    if "access_token" not in t:
        raise LoginError("Session expirée : reconnecte-toi avec Microsoft.")
    fresh = minecraft_from_ms(t["access_token"], t.get("refresh_token", acc.get("refresh_token", "")))
    save_account(fresh)
    with LOCK:
        STATE["account"] = fresh
    return fresh


def offline_account(name: str) -> dict:
    digest = bytearray(hashlib.md5(("OfflinePlayer:" + name).encode("utf-8")).digest())
    digest[6] = (digest[6] & 0x0F) | 0x30
    digest[8] = (digest[8] & 0x3F) | 0x80
    return {"type": "offline", "name": name, "uuid": uuid.UUID(bytes=bytes(digest)).hex, "xuid": "0", "mc_token": "0"}


# --------------------------------------------------------------------------------------
# Manifeste du serveur
# --------------------------------------------------------------------------------------

def fetch_manifest() -> dict | None:
    try:
        m = http_json(manifest_url(), timeout=8)
        if "minecraft" not in m:
            raise ValueError("manifeste invalide")
        with LOCK:
            STATE["manifest"] = m
            STATE["manifest_error"] = ""
        return m
    except Exception as e:  # noqa: BLE001
        with LOCK:
            STATE["manifest_error"] = f"Serveur injoignable ({manifest_url()}) : {e}"
        return None


def manifest_loop() -> None:
    while True:
        fetch_manifest()
        time.sleep(30)


# --------------------------------------------------------------------------------------
# Installation
# --------------------------------------------------------------------------------------

def rules_allow(rules: list | None, features: dict | None = None) -> bool:
    if not rules:
        return True
    allowed = False
    machine = platform.machine().lower()
    for rule in rules:
        ok = True
        os_rule = rule.get("os")
        if os_rule:
            if os_rule.get("name") and os_rule["name"] != "windows":
                ok = False
            if os_rule.get("arch") == "x86" and machine in ("amd64", "x86_64", "arm64"):
                ok = False
        for feature, wanted in (rule.get("features") or {}).items():
            if bool((features or {}).get(feature)) != wanted:
                ok = False
        if ok:
            allowed = rule.get("action") == "allow"
    return allowed


def maven_path(name: str) -> str:
    parts = name.split(":")
    group, artifact, version = parts[0], parts[1], parts[2]
    classifier = "-" + parts[3] if len(parts) > 3 else ""
    return f"{group.replace('.', '/')}/{artifact}/{version}/{artifact}-{version}{classifier}.jar"


def install_java(component: str) -> Path:
    target = RUNTIME / component
    javaw = target / "bin" / "javaw.exe"
    marker = target / ".installed"
    if javaw.is_file() and marker.is_file():
        return javaw
    task(step="Java", detail="Préparation de Java…")
    runtimes = http_json(JAVA_RUNTIMES)
    key = "windows-arm64" if platform.machine().lower() == "arm64" else "windows-x64"
    entries = runtimes.get(key, {}).get(component) or []
    if not entries:
        raise OSError(f"Aucun Java « {component} » disponible pour {key}.")
    files = http_json(entries[0]["manifest"]["url"])["files"]
    jobs = []
    for rel, info in files.items():
        dest = target / rel
        if info["type"] == "directory":
            dest.mkdir(parents=True, exist_ok=True)
        elif info["type"] == "file":
            raw = info["downloads"]["raw"]
            jobs.append(lambda u=raw["url"], d=dest, s=raw["sha1"], z=raw["size"]: download(u, d, s, z))
    parallel(jobs, "Java", 0.05, 0.2)
    marker.write_text("ok")
    return javaw


def prepare(m: dict) -> dict:
    """Installe ou vérifie tout ce qu'il faut pour lancer, et retourne de quoi construire la ligne de commande."""
    mc_version, loader = m["minecraft"], m["fabric_loader"]
    task(step="Minecraft", progress=0.02, detail=f"Version {mc_version}…")
    vm = http_json(VERSION_MANIFEST)
    entry = next((v for v in vm["versions"] if v["id"] == mc_version), None)
    if not entry:
        raise OSError(f"Version {mc_version} introuvable chez Mojang.")
    vdir = VERSIONS / mc_version
    vjson_path = vdir / f"{mc_version}.json"
    download(entry["url"], vjson_path, entry.get("sha1"))
    vjson = json.loads(vjson_path.read_text(encoding="utf-8"))
    fabric_path = VERSIONS / f"fabric-{loader}-{mc_version}.json"
    fabric = http_json(FABRIC_PROFILE.format(mc=mc_version, loader=loader))
    if "mainClass" in fabric:
        fabric_path.parent.mkdir(parents=True, exist_ok=True)
        fabric_path.write_text(json.dumps(fabric), encoding="utf-8")
    elif fabric_path.is_file():
        fabric = json.loads(fabric_path.read_text(encoding="utf-8"))
    else:
        raise OSError("Fabric indisponible pour cette version.")

    javaw = install_java(vjson.get("javaVersion", {}).get("component", "java-runtime-epsilon"))

    # Bibliothèques : Fabric d'abord (prioritaires), puis Minecraft.
    task(step="Bibliothèques", progress=0.2, detail="Vérification…")
    classpath: list[Path] = []
    seen: set[str] = set()
    jobs = []
    for lib in fabric.get("libraries", []):
        key = ":".join(lib["name"].split(":")[:2])
        seen.add(key)
        rel = maven_path(lib["name"])
        dest = LIBRARIES / rel
        jobs.append(lambda u=lib.get("url", "https://maven.fabricmc.net/") + rel, d=dest, s=lib.get("sha1"): download(u, d, s))
        classpath.append(dest)
    for lib in vjson["libraries"]:
        if not rules_allow(lib.get("rules")):
            continue
        parts = lib["name"].split(":")
        key = ":".join(parts[:2]) + (":" + parts[3] if len(parts) > 3 else "")
        if key in seen:
            continue
        seen.add(key)
        art = lib.get("downloads", {}).get("artifact")
        if not art:
            continue
        dest = LIBRARIES / art["path"]
        jobs.append(lambda a=art, d=dest: download(a["url"], d, a["sha1"], a["size"]))
        classpath.append(dest)
    client = vjson["downloads"]["client"]
    client_jar = vdir / f"{mc_version}.jar"
    jobs.append(lambda: download(client["url"], client_jar, client["sha1"], client["size"]))
    classpath.append(client_jar)
    log_cfg = None
    logging = vjson.get("logging", {}).get("client")
    if logging:
        log_cfg = VERSIONS / "logging" / logging["file"]["id"]
        jobs.append(lambda: download(logging["file"]["url"], log_cfg, logging["file"]["sha1"]))
    parallel(jobs, "Bibliothèques", 0.2, 0.4, workers=8)

    # Assets (sons, langues, textures) : réutilisés s'ils existent déjà sur le PC.
    task(step="Ressources du jeu", progress=0.4, detail="Index…")
    ai = vjson["assetIndex"]
    index_path = ASSETS / "indexes" / f"{ai['id']}.json"
    download(ai["url"], index_path, ai["sha1"])
    objects = json.loads(index_path.read_text(encoding="utf-8"))["objects"]
    ajobs = []
    for obj in {o["hash"]: o for o in objects.values()}.values():
        h, size = obj["hash"], obj["size"]
        dest = ASSETS / "objects" / h[:2] / h
        if dest.is_file() and dest.stat().st_size == size:
            continue
        ajobs.append(lambda h=h, size=size, dest=dest: fetch_asset(h, size, dest))
    parallel(ajobs, "Ressources", 0.4, 0.85, workers=16)

    # Mods du serveur
    sync_mods(m)

    return {"vjson": vjson, "fabric": fabric, "javaw": javaw, "classpath": classpath, "log_cfg": log_cfg,
            "version": mc_version, "assets_index": ai["id"]}


def fetch_asset(h: str, size: int, dest: Path) -> None:
    for base in ASSET_REUSE:
        src = base / h[:2] / h
        try:
            if src.is_file() and src.stat().st_size == size:
                dest.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(src, dest)
                if sha1_of(dest) == h:
                    return
        except OSError:
            pass
    download(ASSET_URL.format(prefix=h[:2], hash=h), dest, h, size)


def sync_mods(m: dict) -> None:
    """Le dossier mods reflète exactement le manifeste du serveur (mods ajoutés, mis à jour ou retirés)."""
    task(step="Mods du serveur", progress=0.85, detail="Synchronisation…")
    mods_dir = GAME / "mods"
    mods_dir.mkdir(parents=True, exist_ok=True)
    wanted = set()
    jobs = []
    base = manifest_url()
    for mod in m.get("mods", []):
        name = Path(mod["file"]).name
        wanted.add(name)
        url = urllib.parse.urljoin(base, mod["url"])
        jobs.append(lambda u=url, d=mods_dir / name, s=mod.get("sha1"), z=mod.get("size"): download(u, d, s, z))
    parallel(jobs, "Mods", 0.85, 0.97, workers=4)
    for f in mods_dir.glob("*.jar"):
        if f.name not in wanted:
            f.unlink()
    sync_shaderpacks(m)


# Qualité des shaders selon la carte graphique : profil du pack (Complementary : POTATO à ULTRA) et échelle de
# rendu de Vitrail (le monde est rendu plus petit puis agrandi, l'interface reste nette).
# Sur une puce intégrée, les shaders sont installés mais coupés (touche K) : le but est 100 images/s ou plus.
GPU_TIERS = {
    "low": {"shaders": False, "profile": "POTATO", "renderscale": 67},
    "mid": {"shaders": True, "profile": "LOW", "renderscale": 85},
    "high": {"shaders": True, "profile": "MEDIUM", "renderscale": 100},
}
# Réglages vidéo de départ selon la carte (appliqués une seule fois, le joueur peut les changer ensuite).
PERF_OPTIONS = {
    "low": {"renderDistance": "8", "simulationDistance": "6", "biomeBlendRadius": "0", "entityShadows": "false",
            "cloudRange": "64", "renderClouds": '"fast"', "mipmapLevels": "2", "maxAnisotropyBit": "0",
            "entityDistanceScaling": "0.75", "weatherRadius": "5", "chunkSectionFadeInTime": "0.0",
            "enableVsync": "false", "maxFps": "260", "graphicsPreset": '"custom"'},
    "mid": {"renderDistance": "10", "simulationDistance": "8", "biomeBlendRadius": "1", "enableVsync": "false",
            "maxFps": "260"},
    "high": {"enableVsync": "false", "maxFps": "260"},
}
SHADER_MARKER = "survivalfrench-shaders.done"
PERF_MARKER = "survivalfrench-perf.done"
_GPU_TIER: str | None = None


def detected_tier() -> str:
    global _GPU_TIER
    if _GPU_TIER is None:
        _GPU_TIER = gpu_tier(gpu_names())
    return _GPU_TIER


def gpu_names() -> list[str]:
    """Cartes graphiques vues par Windows (vide ailleurs ou en cas d'échec)."""
    if os.name != "nt":
        return []
    try:
        out = subprocess.run(["powershell", "-NoProfile", "-Command",
                              "(Get-CimInstance Win32_VideoController).Name"],
                             capture_output=True, text=True, timeout=15,
                             creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0)).stdout
    except (OSError, subprocess.SubprocessError):
        return []
    return [line.strip() for line in out.splitlines() if line.strip()]


def gpu_tier(names: list[str]) -> str:
    """Meilleure carte trouvée : « high » (RTX, RX 6000+, Arc), « mid » (autre carte dédiée), « low » (puce intégrée)."""
    best = ""
    for name in names:
        n = name.lower()
        if re.search(r"rtx|rx ?[6-9]\d{3}|\barc\b", n):
            return "high"
        if re.search(r"gtx|rx ?\d{3,4}|radeon pro|quadro", n):
            best = "mid"
        elif not best and re.search(r"uhd|iris|intel.*hd|radeon\(tm\)|radeon graphics|vega|\d{3}m graphics", n):
            best = "low"
    return best or "mid"


def sync_shaderpacks(m: dict) -> None:
    """Packs de shaders du manifeste (téléchargés dans shaderpacks/), et au premier lancement seulement :
    le premier pack activé dans Vitrail, avec une qualité adaptée à la carte graphique."""
    packs = m.get("shaderpacks", [])
    if not packs:
        return
    pack_dir = GAME / "shaderpacks"
    pack_dir.mkdir(parents=True, exist_ok=True)
    base = manifest_url()
    for pack in packs:
        name = Path(pack["file"]).name
        download(urllib.parse.urljoin(base, pack["url"]), pack_dir / name, pack.get("sha1"), pack.get("size"))
    marker = GAME / "config" / SHADER_MARKER
    if marker.exists():
        return
    first = Path(packs[0]["file"]).name
    tier = GPU_TIERS[detected_tier()]
    vitrail = GAME / "vitrail" / "pack.txt"
    vitrail.parent.mkdir(parents=True, exist_ok=True)
    enabled = "true" if tier["shaders"] else "false"
    vitrail.write_text(f"pack={first}\nenabled={enabled}\nrenderscale={tier['renderscale']}\n", encoding="utf-8")
    settings = pack_dir / f"{first}.txt"
    if not settings.exists():
        settings.write_text(f"profile={tier['profile']}\n", encoding="utf-8")
    marker.parent.mkdir(parents=True, exist_ok=True)
    marker.write_text(f"shaders : {first} ({enabled}), profil {tier['profile']}, échelle {tier['renderscale']} %",
                      encoding="utf-8")


# --------------------------------------------------------------------------------------
# Launcher Minecraft officiel (connexion Microsoft gérée par Mojang)
# --------------------------------------------------------------------------------------

def official_launcher() -> list[str] | None:
    """Commande pour ouvrir le launcher officiel, ou None s'il n'est pas installé."""
    for exe in (os.path.expandvars(r"%ProgramFiles(x86)%\Minecraft Launcher\MinecraftLauncher.exe"),
                os.path.expandvars(r"%ProgramFiles%\Minecraft Launcher\MinecraftLauncher.exe"),
                os.path.expandvars(r"%LocalAppData%\Programs\Minecraft Launcher\MinecraftLauncher.exe")):
        if os.path.isfile(exe):
            return [exe]
    if (Path(os.environ.get("LOCALAPPDATA", "")) / "Packages" / OFFICIAL_STORE_FAMILY).is_dir():
        return ["explorer.exe", f"shell:AppsFolder\\{OFFICIAL_STORE_FAMILY}!Minecraft"]
    return None


def _nbt_string(value: str) -> bytes:
    raw = value.encode("utf-8")
    return len(raw).to_bytes(2, "big") + raw


def write_servers_dat(name: str, address: str) -> None:
    """servers.dat (NBT non compressé) : le serveur en tête de la liste Multijoueur."""
    entry = (b"\x08" + _nbt_string("name") + _nbt_string(name)
             + b"\x08" + _nbt_string("ip") + _nbt_string(address)
             + b"\x01" + _nbt_string("acceptTextures") + b"\x01"
             + b"\x00")
    data = (b"\x0a" + _nbt_string("")
            + b"\x09" + _nbt_string("servers") + b"\x0a" + (1).to_bytes(4, "big") + entry
            + b"\x00")
    GAME.mkdir(parents=True, exist_ok=True)
    (GAME / "servers.dat").write_bytes(data)


def server_address(m: dict) -> str:
    server = m.get("server", {})
    host = server.get("address") or urllib.parse.urlparse(manifest_url()).hostname or "localhost"
    port = server.get("port", 25565)
    return host if ":" in host else (host if port == 25565 else f"{host}:{port}")


def install_official(m: dict) -> None:
    """Crée/actualise le profil « SurvivalFrench RP » dans le launcher officiel et y synchronise les mods."""
    mc_version, loader = m["minecraft"], m["fabric_loader"]
    task(step="Fabric", progress=0.05, detail="Profil Fabric…")
    fabric = http_json(FABRIC_PROFILE.format(mc=mc_version, loader=loader))
    if "id" not in fabric:
        raise OSError("Fabric indisponible pour cette version.")
    vdir = MINECRAFT_DIR / "versions" / fabric["id"]
    vdir.mkdir(parents=True, exist_ok=True)
    (vdir / f"{fabric['id']}.json").write_text(json.dumps(fabric, indent=2), encoding="utf-8")

    sync_mods(m)
    first_run_options()
    write_servers_dat(m.get("name", "SurvivalFrench RP"), server_address(m))

    task(step="Profil", progress=0.97, detail="Ajout du profil au launcher Minecraft…")
    profiles_path = MINECRAFT_DIR / "launcher_profiles.json"
    try:
        profiles = json.loads(profiles_path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        profiles = {}
    backup = MINECRAFT_DIR / "launcher_profiles.json.bak-survivalfrench"
    if profiles_path.is_file() and not backup.exists():
        shutil.copyfile(profiles_path, backup)
    profiles.setdefault("profiles", {})
    profiles.setdefault("settings", {})
    profiles.setdefault("version", 3)
    now = time.strftime("%Y-%m-%dT%H:%M:%S.000Z", time.gmtime())
    previous = profiles["profiles"].get(PROFILE_ID, {})
    icon = ""
    if resource("logo.png").is_file():
        icon = "data:image/png;base64," + __import__("base64").b64encode(resource("logo.png").read_bytes()).decode()
    profiles["profiles"][PROFILE_ID] = {
        "name": m.get("name", "SurvivalFrench RP"),
        "type": "custom",
        "created": previous.get("created", now),
        "lastUsed": now,
        "lastVersionId": fabric["id"],
        "gameDir": str(GAME),
        "javaArgs": f"-Xmx{int(STATE['settings'].get('ram_mb', 4096))}M -Xms512M -XX:+UseG1GC",
        "icon": icon or "Grass",
    }
    MINECRAFT_DIR.mkdir(parents=True, exist_ok=True)
    profiles_path.write_text(json.dumps(profiles, indent=2), encoding="utf-8")


def play_official() -> None:
    try:
        cmd = official_launcher()
        if cmd is None:
            raise LoginError("Le launcher Minecraft officiel n'est pas installé.")
        task(state="running", step="Serveur", progress=0.0, detail="Lecture du manifeste…", error="")
        m = fetch_manifest()
        if not m:
            raise OSError(STATE["manifest_error"])
        install_official(m)
        subprocess.Popen(cmd)
        task(state="official", step="Prêt !", progress=1.0,
             detail=f"Dans le launcher Minecraft : choisis le profil « {m.get('name', 'SurvivalFrench RP')} », clique sur JOUER, "
                    f"puis Multijoueur → {m.get('name', 'SurvivalFrench RP')}.")
    except LoginError as e:
        task(state="error", step="Launcher Minecraft", error=str(e))
    except Exception as e:  # noqa: BLE001
        traceback.print_exc()
        task(state="error", step="Installation", error=str(e))


# Réglages imposés à chaque lancement : pas de tutoriel (« Bougez avec Z, Q, S et D »), pas d'écran
# d'accessibilité ni d'avertissement multijoueur au premier démarrage, narrateur coupé.
FORCED_OPTIONS = {
    "tutorialStep": "none",
    "onboardAccessibility": "false",
    "skipMultiplayerWarning": "true",
    "joinedFirstServer": "true",
    "narrator": "0",
}
# Réglages par défaut seulement (le joueur peut les changer ensuite).
DEFAULT_OPTIONS = {
    "lang": "fr_fr",
    "resourcePacks": '["vanilla"]',
    "preferredGraphicsBackend": '"vulkan"',
}
# Minecraft écrit « default » dans options.txt dès le premier lancement : une seule fois par installation, ce
# choix automatique passe sur Vulkan. Un joueur qui revient ensuite sur OpenGL ou Défaut garde son choix.
VULKAN_MARKER = "survivalfrench-vulkan.done"


def first_run_options() -> None:
    """options.txt : français par défaut, tutoriel et écrans d'accueil désactivés. Les touches suivent déjà la
    position physique du clavier : en AZERTY, Minecraft affiche et utilise Z, Q, S, D."""
    GAME.mkdir(parents=True, exist_ok=True)
    opts = GAME / "options.txt"
    lines = opts.read_text(encoding="utf-8", errors="replace").splitlines() if opts.exists() else []
    values, order = {}, []
    for line in lines:
        key, sep, val = line.partition(":")
        if sep:
            values[key] = val
            order.append(key)
    for k, v in DEFAULT_OPTIONS.items():
        if k not in values:
            values[k] = v
            order.append(k)
    marker = GAME / "config" / VULKAN_MARKER
    if not marker.exists():
        values["preferredGraphicsBackend"] = '"vulkan"'
        try:
            marker.parent.mkdir(parents=True, exist_ok=True)
            marker.write_text("graphisme : Vulkan par défaut", encoding="utf-8")
        except OSError:
            pass
    perf = GAME / "config" / PERF_MARKER
    if not perf.exists():
        tier = detected_tier()
        for k, v in PERF_OPTIONS[tier].items():
            if k not in values:
                order.append(k)
            values[k] = v
        try:
            perf.parent.mkdir(parents=True, exist_ok=True)
            perf.write_text(f"réglages vidéo de départ : carte « {tier} »", encoding="utf-8")
        except OSError:
            pass
    for k, v in FORCED_OPTIONS.items():
        if k not in values:
            order.append(k)
        values[k] = v
    opts.write_text("".join(f"{k}:{values[k]}\n" for k in order), encoding="utf-8")


def build_command(p: dict, acc: dict, m: dict) -> list[str]:
    server = m.get("server", {})
    host = server.get("address") or urllib.parse.urlparse(manifest_url()).hostname or "localhost"
    port = server.get("port", 25565)
    address = host if ":" in host else f"{host}:{port}"
    natives = NATIVES / p["version"]
    for sub in ("java", "jna", "lwjgl", "netty"):
        (natives / sub).mkdir(parents=True, exist_ok=True)
    values = {
        "natives_directory": str(natives),
        "launcher_name": "SurvivalFrench",
        "launcher_version": VERSION,
        "classpath": os.pathsep.join(str(c) for c in p["classpath"]),
        "classpath_separator": os.pathsep,
        "library_directory": str(LIBRARIES),
        "version_name": p["version"],
        "auth_player_name": acc["name"],
        "game_directory": str(GAME),
        "assets_root": str(ASSETS),
        "assets_index_name": p["assets_index"],
        "auth_uuid": acc["uuid"],
        "auth_access_token": acc.get("mc_token", "0"),
        "clientid": CONFIG.get("client_id", "") or "0",
        "auth_xuid": acc.get("xuid", "0"),
        "user_type": "msa" if acc.get("type") == "msa" else "legacy",
        "version_type": "release",
        "quickPlayMultiplayer": address,
    }

    def expand(items, features=None):
        out = []
        for a in items:
            if isinstance(a, str):
                out.append(a)
            elif rules_allow(a.get("rules"), features):
                v = a["value"]
                out.extend(v if isinstance(v, list) else [v])
        return [subst(x) for x in out]

    def subst(s: str) -> str:
        for k, v in values.items():
            s = s.replace("${" + k + "}", v)
        return s

    ram = int(STATE["settings"].get("ram_mb", 4096))
    jvm = [f"-Xmx{ram}M", "-Xms512M", "-XX:+UseG1GC"]
    jvm += expand(p["vjson"]["arguments"]["jvm"]) + expand(p["fabric"].get("arguments", {}).get("jvm", []))
    if p["log_cfg"]:
        jvm.append(p["vjson"]["logging"]["client"]["argument"].replace("${path}", str(p["log_cfg"])))
    game = expand(p["vjson"]["arguments"]["game"], {"is_quick_play_multiplayer": True})
    game += expand(p["fabric"].get("arguments", {}).get("game", []))
    return [str(p["javaw"])] + jvm + [p["fabric"]["mainClass"]] + game


def play() -> None:
    global GAME_PROCESS
    try:
        with LOCK:
            acc = STATE["account"]
        if not acc:
            raise LoginError("Connecte-toi d'abord.")
        task(state="running", step="Serveur", progress=0.0, detail="Lecture du manifeste…", error="")
        m = fetch_manifest()
        if not m:
            raise OSError(STATE["manifest_error"])
        acc = ensure_fresh(acc)
        p = prepare(m)
        first_run_options()
        write_servers_dat(m.get("name", "SurvivalFrench RP"), server_address(m))
        task(step="Lancement", progress=0.98, detail="Démarrage de Minecraft…")
        cmd = build_command(p, acc, m)
        log = open(GAME / "launcher-game.log", "w", encoding="utf-8", errors="replace")
        GAME_PROCESS = subprocess.Popen(cmd, cwd=str(GAME), stdout=log, stderr=subprocess.STDOUT,
                                        creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        task(state="playing", step="En jeu", progress=1.0, detail="Bon jeu sur " + m.get("name", "le serveur") + " !")
        code = GAME_PROCESS.wait()
        log.close()
        GAME_PROCESS = None
        if code != 0:
            tail = (GAME / "launcher-game.log").read_text(encoding="utf-8", errors="replace").splitlines()[-12:]
            task(state="error", step="Le jeu s'est arrêté", error=f"Code {code}.\n" + "\n".join(tail))
        else:
            task(state="idle", step="", progress=0.0, detail="")
    except LoginError as e:
        task(state="error", step="Connexion", error=str(e))
    except Exception as e:  # noqa: BLE001
        traceback.print_exc()
        task(state="error", step="Installation", error=str(e))


# --------------------------------------------------------------------------------------
# Interface
# --------------------------------------------------------------------------------------

class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args) -> None:
        pass

    def _send(self, code: int, body: bytes, ctype: str) -> None:
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)

    def _json(self, obj, code: int = 200) -> None:
        self._send(code, json.dumps(obj).encode("utf-8"), "application/json; charset=utf-8")

    def do_GET(self) -> None:  # noqa: N802
        global LAST_PING
        if self.path in ("/", "/index.html"):
            self._send(200, resource("index.html").read_bytes(), "text/html; charset=utf-8")
        elif self.path == "/logo.png" and resource("logo.png").is_file():
            self._send(200, resource("logo.png").read_bytes(), "image/png")
        elif self.path == "/api/state":
            LAST_PING = time.time()
            with LOCK:
                login = dict(STATE["login"]) if STATE["login"] else None
                self._json({
                    "account": public_account(STATE["account"]),
                    "login": login,
                    "task": STATE["task"],
                    "settings": STATE["settings"],
                    "manifest": STATE["manifest"],
                    "manifest_error": STATE["manifest_error"],
                    "manifest_url": manifest_url(),
                    "allow_offline": bool(CONFIG.get("allow_offline")),
                    "ms_configured": bool(CONFIG.get("client_id", "").strip()),
                    "official": official_launcher() is not None,
                    "version": VERSION,
                    "instance": INSTANCE,
                })
        else:
            self._send(404, b"", "text/plain")

    def do_POST(self) -> None:  # noqa: N802
        # Seule notre propre page peut piloter le launcher.
        origin = self.headers.get("Origin", "")
        if origin and origin != f"http://127.0.0.1:{UI_PORT}":
            self._json({"error": "origine refusée"}, 403)
            return
        length = int(self.headers.get("Content-Length") or 0)
        try:
            body = json.loads(self.rfile.read(length) or b"{}")
        except ValueError:
            body = {}
        path = self.path
        if path == "/api/login":
            with LOCK:
                STATE["login"] = {"user_code": "", "url": "", "error": ""}
            threading.Thread(target=ms_device_login, daemon=True).start()
            self._json({"ok": True})
        elif path == "/api/login/cancel":
            with LOCK:
                STATE["login"] = None
            self._json({"ok": True})
        elif path == "/api/offline":
            name = str(body.get("name", "")).strip()
            if not CONFIG.get("allow_offline"):
                self._json({"error": "Mode hors-ligne désactivé."}, 403)
            elif not (3 <= len(name) <= 16) or not all(c.isalnum() or c == "_" for c in name):
                self._json({"error": "Pseudo invalide (3 à 16 lettres, chiffres ou _)."}, 400)
            else:
                acc = offline_account(name)
                save_account(acc)
                with LOCK:
                    STATE["account"] = acc
                self._json({"ok": True})
        elif path == "/api/logout":
            save_account(None)
            with LOCK:
                STATE["account"] = None
            self._json({"ok": True})
        elif path == "/api/play":
            with LOCK:
                busy = STATE["task"]["state"] in ("running", "playing")
            if busy:
                self._json({"error": "Déjà en cours."}, 409)
            else:
                threading.Thread(target=play, daemon=True).start()
                self._json({"ok": True})
        elif path == "/api/play_official":
            with LOCK:
                busy = STATE["task"]["state"] in ("running", "playing")
            if busy:
                self._json({"error": "Déjà en cours."}, 409)
            else:
                threading.Thread(target=play_official, daemon=True).start()
                self._json({"ok": True})
        elif path == "/api/settings":
            with LOCK:
                s = STATE["settings"]
                if "ram_mb" in body:
                    s["ram_mb"] = max(2048, min(16384, int(body["ram_mb"])))
                if "manifest_url" in body:
                    s["manifest_url"] = str(body["manifest_url"]).strip()
                save_settings(s)
            threading.Thread(target=fetch_manifest, daemon=True).start()
            self._json({"ok": True})
        elif path == "/api/folder":
            GAME.mkdir(parents=True, exist_ok=True)
            os.startfile(str(GAME))  # noqa: S606 — dossier du jeu, choisi par nous
            self._json({"ok": True})
        elif path == "/api/open":
            url = str(body.get("url", ""))
            if url.startswith(("https://www.microsoft.com/", "https://microsoft.com/", "https://login.live.com/", OFFICIAL_DOWNLOAD)):
                webbrowser.open(url)
            self._json({"ok": True})
        elif path == "/api/dismiss":
            task(state="idle", error="", step="", progress=0.0, detail="")
            self._json({"ok": True})
        elif path == "/api/quit":
            self._json({"ok": True})
            threading.Thread(target=lambda: (time.sleep(0.3), os._exit(0)), daemon=True).start()
        else:
            self._send(404, b"", "text/plain")


def open_window(url: str) -> None:
    profile = DATA / f"ui-profile-{INSTANCE}"
    for exe in (os.path.expandvars(r"%ProgramFiles(x86)%\Microsoft\Edge\Application\msedge.exe"),
                os.path.expandvars(r"%ProgramFiles%\Microsoft\Edge\Application\msedge.exe"),
                os.path.expandvars(r"%ProgramFiles%\Google\Chrome\Application\chrome.exe"),
                os.path.expandvars(r"%LocalAppData%\Google\Chrome\Application\chrome.exe")):
        if os.path.isfile(exe):
            subprocess.Popen([exe, f"--app={url}", "--window-size=1180,720", f"--user-data-dir={profile}",
                              "--no-first-run", "--disable-features=Translate"])
            return
    webbrowser.open(url)


def watchdog() -> None:
    """Ferme le launcher quand sa fenêtre est fermée (plus de nouvelles de l'interface) et que rien ne tourne."""
    while True:
        time.sleep(5)
        with LOCK:
            busy = STATE["task"]["state"] == "running"
        if time.time() - LAST_PING > 20 and not busy and GAME_PROCESS is None:
            os._exit(0)


def main() -> None:
    if self_update():
        return
    url = f"http://127.0.0.1:{UI_PORT}/"
    try:
        server = ThreadingHTTPServer(("127.0.0.1", UI_PORT), Handler)
    except OSError:
        open_window(url)  # déjà ouvert : on remet la fenêtre au premier plan
        return
    DATA.mkdir(parents=True, exist_ok=True)
    acc = load_account()
    if acc and acc.get("type") == "offline" and not CONFIG.get("allow_offline"):
        acc = None
    STATE["account"] = acc
    threading.Thread(target=manifest_loop, daemon=True).start()
    threading.Thread(target=watchdog, daemon=True).start()
    if "--no-window" not in sys.argv:
        open_window(url)
    server.serve_forever()


if __name__ == "__main__":
    main()
