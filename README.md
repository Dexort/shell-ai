# Shell IA

Un terminal indépendant, avec sa propre fenêtre, qui intègre une **IA locale** grâce à [Ollama](https://ollama.com). Tu tapes tes commandes comme dans un cmd classique, et tu poses une question à l'IA en la préfixant d'un `?`.

Tout tourne en local : aucune donnée n'est envoyée sur Internet.

> Projet écrit en Rust avec [`eframe` / `egui`](https://github.com/emilk/egui). Le résultat est un simple exécutable, sans installation.

<!-- Ajoute ici une capture d'écran : ![Aperçu](docs/apercu.png) -->

## Fonctionnalités

- Fenêtre autonome, sombre et semi-transparente, avec un prompt inspiré de Kali Linux.
- Exécution des commandes natives en arrière-plan, sans ouvrir de fenêtre cmd visible.
- Assistant IA local via Ollama, avec mémoire de la conversation.
- L'IA connaît ton système, ton dossier courant et ton nom d'utilisateur pour proposer des commandes adaptées.
- Historique des commandes avec les flèches haut et bas.
- Traduction automatique de quelques commandes Linux sous Windows (`ls`, `cat`, `rm`, `cp`, `mv`, `which`, `grep`).
- Affichage correct des accents dans la sortie des commandes Windows.

## Prérequis

- [Rust](https://rustup.rs) (toolchain stable). Sous Windows, utilise la toolchain **MSVC** avec les *Build Tools for Visual Studio* (charge de travail « Développement Desktop en C++ »).
- [Ollama](https://ollama.com) installé et lancé.
- Un modèle téléchargé, par exemple :

```bash
ollama pull llama3.2
```

## Compilation

```bash
git clone https://github.com/Dexort/shell-ia.git
cd shell-ia
cargo build --release
```

L'exécutable se trouve dans :

- Windows : `target\release\shell-ia.exe`
- Linux : `target/release/shell-ia`

Pour tester rapidement sans compiler en mode optimisé :

```bash
cargo run
```

> Ferme toujours la fenêtre du shell avant de recompiler, sinon Windows refuse de remplacer l'exécutable.

### Linux (Debian / Ubuntu)

Le code est prévu pour fonctionner aussi sous Linux (il lance `bash` à la place de `cmd`), mais cette version est moins testée. Installe d'abord les bibliothèques nécessaires à l'interface :

```bash
sudo apt install build-essential pkg-config libxcb-render0-dev libxcb-shape0-dev \
  libxcb-xfixes0-dev libxkbcommon-dev libssl-dev
```

## Utilisation

Assure-toi qu'Ollama tourne (sous Windows, l'icône est dans la barre des tâches), puis lance `shell-ia`.

| Ce que tu tapes | Ce qui se passe |
| --- | --- |
| `dir`, `ipconfig`, `git status`... | La commande est exécutée par le shell du système |
| `? comment voir les ports ouverts` | La question est envoyée à l'IA |
| `cd dossier`, `cd ..`, `cd ~/Downloads` | Changement de dossier |
| `pwd` | Affiche le dossier courant |
| `clear` ou `cls` | Vide l'écran |
| `exit` | Ferme l'application |
| Flèches haut / bas | Parcourent l'historique |

## Configuration

Les réglages se trouvent en haut de `src/main.rs` :

```rust
const OLLAMA_URL: &str = "http://localhost:11434/api/chat";
const MODEL: &str = "llama3.2";   // modèle utilisé (voir `ollama list`)
const AI_PREFIX: char = '?';      // caractère qui déclenche l'IA
```

La transparence de la fenêtre se règle avec la valeur alpha (0 à 255) de `Color32::from_rgba_unmultiplied(12, 14, 24, 215)` dans la fonction `update`.

## Dépannage

**« Ollama injoignable »** : vérifie que le serveur répond avec `curl.exe http://localhost:11434` (réponse attendue : `Ollama is running`) et que le modèle indiqué dans `MODEL` apparaît dans `ollama list`.

**`error calling dlltool 'dlltool.exe': program not found`** (Windows) : tu compiles avec la toolchain GNU. Passe sur MSVC :

```powershell
rustup default stable-x86_64-pc-windows-msvc
cargo clean
cargo build --release
```

**`failed to remove file ... shell-ia.exe`** : l'application est encore ouverte. Ferme-la, ou lance `taskkill /F /IM shell-ia.exe`, puis recompile.

## Limites actuelles

- Les programmes interactifs (`vim`, `python` en mode REPL, `ssh`) ne fonctionnent pas encore : il faudrait un vrai pseudo-terminal (crate `portable-pty`).
- L'IA propose des commandes mais ne les exécute pas toute seule.
- La réponse de l'IA s'affiche d'un bloc, sans affichage progressif.
- Une commande longue bloque l'entrée suivante jusqu'à sa fin.

## Idées pour la suite

- [ ] Exécuter les commandes proposées par l'IA, après confirmation
- [ ] Affichage progressif des réponses de l'IA (streaming)
- [ ] Support des programmes interactifs avec `portable-pty`
- [ ] Recherche web pour l'IA
- [ ] Choix du modèle et des couleurs depuis un fichier de configuration
- [ ] Autocomplétion des commandes et des chemins

## Structure du projet

```
shell-ia/
├── Cargo.toml
├── README.md
└── src/
    └── main.rs
```

## Licence

À définir. Ajoute un fichier `LICENSE` (par exemple MIT) avant de publier le dépôt.
