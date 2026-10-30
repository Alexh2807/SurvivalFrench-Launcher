# SurvivalFrench RP — Launcher

## Jouer

1. Télécharge **[SurvivalFrench-Launcher.exe](https://github.com/Alexh2807/SurvivalFrench-Launcher/releases/latest/download/SurvivalFrench-Launcher.exe)**.
2. Ouvre-le, choisis ton pseudo et clique sur **Jouer**.

C'est tout : le launcher installe Java, Minecraft, Fabric, les ressources et les mods du serveur, puis te connecte directement. Rien d'autre à installer.

Windows peut afficher « Windows a protégé votre ordinateur » (l'exe n'est pas signé) : clique sur **Informations complémentaires** puis **Exécuter quand même**.

Le launcher utilise le composant **Microsoft Edge WebView2**, déjà présent sur Windows 10 et 11 à jour. S'il manque, le launcher propose de l'installer.

## Léger, même sur un petit PC

- Un seul exe de moins de 2 Mo, ouvert en une fraction de seconde.
- Aucune animation continue : quand tu ne regardes pas le launcher, il ne consomme rien.
- Mode **Économie** automatique sur les machines modestes (ou au choix dans les Paramètres).
- **Pendant que tu joues**, le launcher se cache et libère sa mémoire (quelques Mo seulement). Il revient tout seul si le jeu plante, avec le journal de l'erreur.
- La mémoire donnée à Minecraft est réglée selon ta RAM (modifiable dans les Paramètres).

## Mises à jour

- **Mods et pack** : synchronisés avec le serveur à chaque lancement.
- **Launcher** : au démarrage, il vérifie la dernière release de ce dépôt et la télécharge en arrière-plan ; elle sert au lancement suivant (ou tout de suite avec « Redémarrer »).

## Publier une nouvelle version (staff)

1. Augmenter `version` dans `Cargo.toml` et les numéros de version de `res/app.rc`.
2. `powershell -ExecutionPolicy Bypass -File launcher\publish.ps1 "Ce qui change"`

Le script compile l'exe, pousse le code et crée la release `vX.Y.Z` avec l'exe. Compiler demande [Rust](https://rustup.rs) et les outils C++ de Visual Studio ; `launcher\build_exe.ps1` compile seul.

## Réglages (`launcher.json`)

| Clé | Rôle |
|---|---|
| `manifest_url` | Manifeste publié par le plugin du serveur |
| `allow_offline` | Connexion par simple pseudo (serveur en `online-mode=false`) |
| `client_id` | Application Azure pour la connexion Microsoft |
| `update_repo` | Dépôt GitHub des releases du launcher |

Un `launcher.json` posé à côté de l'exe remplace les valeurs intégrées.

## Organisation

| Dossier | Contenu |
|---|---|
| `src/` | Programme (Rust) : fenêtre, connexion Microsoft, installation, lancement, statut du serveur, mise à jour |
| `ui/` | Interface : décor en pixel art (`scene.js`), personnage 3D (`skin.js`), application (`app.js`) |
| `launcher.py`, `index.html` | Ancien launcher Python (v1), gardé en secours : `build_exe_python.ps1` |
