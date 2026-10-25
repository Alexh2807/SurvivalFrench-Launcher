# SurvivalFrench RP — Launcher

## Jouer

1. Télécharge **[SurvivalFrench-Launcher.exe](https://github.com/Alexh2807/SurvivalFrench-Launcher/releases/latest/download/SurvivalFrench-Launcher.exe)**.
2. Ouvre-le, choisis ton pseudo et clique sur **Jouer**.

C'est tout : le launcher installe Java, Minecraft, Fabric, les ressources et les mods du serveur, puis te connecte directement. Rien d'autre à installer.

Windows peut afficher « Windows a protégé votre ordinateur » (l'exe n'est pas signé) : clique sur **Informations complémentaires** puis **Exécuter quand même**.

## Mises à jour

- **Mods et pack** : synchronisés avec le serveur à chaque lancement.
- **Launcher** : au démarrage, il vérifie la dernière release de ce dépôt, se remplace et se relance tout seul.

## Publier une nouvelle version (staff)

1. Augmenter `VERSION` dans `launcher.py`.
2. `powershell -ExecutionPolicy Bypass -File launcher\publish.ps1 "Ce qui change"`

Le script compile l'exe, pousse le code et crée la release `vX.Y.Z` avec l'exe.

## Réglages (`launcher.json`)

| Clé | Rôle |
|---|---|
| `manifest_url` | Manifeste publié par le plugin du serveur |
| `allow_offline` | Connexion par simple pseudo (serveur en `online-mode=false`) |
| `client_id` | Application Azure pour la connexion Microsoft |
| `update_repo` | Dépôt GitHub des releases du launcher |

Un `launcher.json` posé à côté de l'exe remplace les valeurs intégrées.
