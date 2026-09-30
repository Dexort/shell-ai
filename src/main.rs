// Pas de console noire derrière la fenêtre en mode release (Windows)
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui::{self, Color32, FontId, Key, RichText, ScrollArea, TextEdit};
use std::{
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc::{channel, Receiver, Sender},
    thread,
    time::Duration,
};
//test

// ---------- Configuration ----------
const OLLAMA_URL: &str = "http://localhost:11434/api/chat";
const MODEL: &str = "llama3.2"; // change ici le modèle (ollama list)
const AI_PREFIX: char = '?'; // "? ta question" => envoyé à l'IA

// ---------- Types ----------
#[derive(Clone, Copy)]
enum Kind {
    Input,
    Output,
    Error,
    Ai,
    Info,
}

struct Line {
    kind: Kind,
    text: String,
}

enum Msg {
    Line(Line),
    AiReply(String),
    Done,
}

struct ShellApp {
    lines: Vec<Line>,
    input: String,
    cwd: PathBuf,
    history: Vec<String>,
    hist_pos: Option<usize>,
    chat: Vec<serde_json::Value>, // mémoire de la conversation avec l'IA
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    busy: bool,
}

// ---------- Utilitaires ----------
fn home_dir() -> PathBuf {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

fn user_name() -> String {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "user".into())
}

/// Nettoie les "." et ".." sans passer par canonicalize (qui ajoute \\?\ sous Windows)
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(windows)]
fn shell_command(line: &str) -> Command {
    use std::os::windows::process::CommandExt;
    let mut c = Command::new("cmd");
    c.raw_arg(format!("/C {}", line));
    c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW : aucune fenêtre cmd visible
    c
}

#[cfg(not(windows))]
fn shell_command(line: &str) -> Command {
    let mut c = Command::new("bash");
    c.args(["-c", line]);
    c
}

/// Décode la sortie d'une commande (Windows : page de code OEM, ex. CP850 en France)
#[cfg(windows)]
fn decode_output(bytes: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    use windows_sys::Win32::Globalization::{MultiByteToWideChar, CP_OEMCP};
    unsafe {
        let len = MultiByteToWideChar(
            CP_OEMCP,
            0,
            bytes.as_ptr(),
            bytes.len() as i32,
            std::ptr::null_mut(),
            0,
        );
        if len <= 0 {
            return String::from_utf8_lossy(bytes).to_string();
        }
        let mut buf = vec![0u16; len as usize];
        MultiByteToWideChar(
            CP_OEMCP,
            0,
            bytes.as_ptr(),
            bytes.len() as i32,
            buf.as_mut_ptr(),
            len,
        );
        String::from_utf16_lossy(&buf)
    }
}

#[cfg(not(windows))]
fn decode_output(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).to_string()
}

/// Sous Windows : traduit quelques commandes Linux courantes vers leur équivalent cmd
#[cfg(windows)]
fn translate_unix(line: &str) -> String {
    let (first, rest) = match line.split_once(' ') {
        Some((f, r)) => (f, r.trim()),
        None => (line, ""),
    };
    let mapped = match first {
        "ls" | "ll" => "dir",
        "cat" => "type",
        "rm" => "del",
        "cp" => "copy",
        "mv" => "move",
        "which" => "where",
        "grep" => "findstr",
        _ => return line.to_string(),
    };
    // "ls -la" => "dir" : on retire les options de style Linux (-l, -a...)
    let args: Vec<&str> = if first == "ls" || first == "ll" {
        rest.split_whitespace().filter(|a| !a.starts_with('-')).collect()
    } else {
        rest.split_whitespace().collect()
    };
    format!("{} {}", mapped, args.join(" ")).trim().to_string()
}

#[cfg(not(windows))]
fn translate_unix(line: &str) -> String {
    line.to_string()
}

fn system_prompt(cwd: &Path) -> String {
    let os = if cfg!(windows) {
        "Windows (cmd)"
    } else {
        "Linux (bash)"
    };
    format!(
        "Tu es l'assistant IA intégré à un shell {os}. Réponds en français, de façon concise. \
         Quand tu proposes une commande, mets-la seule dans un bloc de code. \
         Contexte réel : utilisateur = {user}, dossier personnel = {home}, dossier courant = {cwd}. \
         Utilise toujours ces vrais chemins et n'invente jamais de chemin factice \
         comme NomUtilisateur ou <user>. Propose uniquement des commandes qui existent \
         dans ce shell (sous Windows : dir, type, del, copy, move, where, etc., pas ls ni cat).",
        user = user_name(),
        home = home_dir().display(),
        cwd = cwd.display()
    )
}

fn ask_ollama(chat: &[serde_json::Value], cwd: &Path) -> Result<String, String> {
    let mut messages = vec![serde_json::json!({"role": "system", "content": system_prompt(cwd)})];
    messages.extend_from_slice(chat);

    let body = serde_json::json!({
        "model": MODEL,
        "messages": messages,
        "stream": false
    });

    let resp: serde_json::Value = ureq::post(OLLAMA_URL)
        .timeout(Duration::from_secs(300))
        .send_json(body)
        .map_err(|e| format!("Ollama injoignable ({e}). Vérifie qu'il tourne et que le modèle '{MODEL}' est installé."))?
        .into_json()
        .map_err(|e| e.to_string())?;

    Ok(resp["message"]["content"]
        .as_str()
        .unwrap_or("(réponse vide)")
        .to_string())
}

// ---------- Application ----------
impl ShellApp {
    fn new() -> Self {
        let (tx, rx) = channel();
        let mut app = Self {
            lines: vec![],
            input: String::new(),
            cwd: home_dir(),
            history: vec![],
            hist_pos: None,
            chat: vec![],
            tx,
            rx,
            busy: false,
        };
        app.push(Kind::Info, "Shell IA v0.1 — commandes normales : exécutées directement");
        app.push(
            Kind::Info,
            &format!("Préfixe '{AI_PREFIX}' pour parler à l'IA (modèle : {MODEL}). 'clear' pour vider, 'exit' pour quitter."),
        );
        app
    }

    fn push(&mut self, kind: Kind, text: &str) {
        self.lines.push(Line {
            kind,
            text: text.to_string(),
        });
    }

    fn prompt(&self) -> String {
        format!("┌──({}@ia)-[{}]", user_name(), self.cwd.display())
    }

    fn change_dir(&mut self, arg: &str) {
        let mut arg = arg.trim();
        if let Some(r) = arg.strip_prefix("/d ").or_else(|| arg.strip_prefix("/D ")) {
            arg = r.trim();
        }
        let arg = arg.trim_matches('"');
        let (base, arg) = match arg.strip_prefix("~/").or_else(|| arg.strip_prefix("~\\")) {
            Some(rest) => (home_dir(), rest),
            None => (self.cwd.clone(), arg),
        };
        let target = if arg.is_empty() || arg == "~" {
            home_dir()
        } else {
            base.join(arg)
        };
        let target = normalize(&target);
        if target.is_dir() {
            self.cwd = target;
        } else {
            self.push(
                Kind::Error,
                &format!("Répertoire introuvable : {}", target.display()),
            );
        }
    }

    fn submit(&mut self, ctx: &egui::Context) {
        let line = self.input.trim().to_string();
        self.input.clear();
        self.hist_pos = None;
        if line.is_empty() {
            return;
        }
        self.history.push(line.clone());
        let echo = format!("{}\n└─$ {}", self.prompt(), line);
        self.push(Kind::Input, &echo);

        // Commandes internes
        if line == "clear" || line == "cls" {
            self.lines.clear();
            return;
        }
        if line == "exit" {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if line == "pwd" {
            let p = self.cwd.display().to_string();
            self.push(Kind::Output, &p);
            return;
        }
        if line == "cd" || line.starts_with("cd ") {
            self.change_dir(&line[2..]);
            return;
        }

        self.busy = true;
        let tx = self.tx.clone();
        let ctx = ctx.clone();

        if let Some(question) = line.strip_prefix(AI_PREFIX) {
            // ---- Appel à l'IA (Ollama) ----
            self.chat
                .push(serde_json::json!({"role": "user", "content": question.trim()}));
            let chat = self.chat.clone();
            let cwd_ai = self.cwd.clone();
            thread::spawn(move || {
                match ask_ollama(&chat, &cwd_ai) {
                    Ok(text) => {
                        let _ = tx.send(Msg::AiReply(text));
                    }
                    Err(e) => {
                        let _ = tx.send(Msg::Line(Line {
                            kind: Kind::Error,
                            text: e,
                        }));
                    }
                }
                let _ = tx.send(Msg::Done);
                ctx.request_repaint();
            });
        } else {
            // ---- Commande native ----
            let cwd = self.cwd.clone();
            let cmd_line = translate_unix(&line);
            thread::spawn(move || {
                let mut cmd = shell_command(&cmd_line);
                cmd.current_dir(&cwd)
                    .stdin(Stdio::null());
                match cmd.output() {
                    Ok(out) => {
                        let so = decode_output(&out.stdout).trim_end().to_string();
                        let se = decode_output(&out.stderr).trim_end().to_string();
                        if !so.is_empty() {
                            let _ = tx.send(Msg::Line(Line { kind: Kind::Output, text: so }));
                        }
                        if !se.is_empty() {
                            let _ = tx.send(Msg::Line(Line { kind: Kind::Error, text: se }));
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Msg::Line(Line {
                            kind: Kind::Error,
                            text: format!("Impossible de lancer la commande : {e}"),
                        }));
                    }
                }
                let _ = tx.send(Msg::Done);
                ctx.request_repaint();
            });
        }
    }
}

impl eframe::App for ShellApp {
    // Fond de la fenêtre entièrement transparent : c'est le Frame des panneaux qui donne la teinte
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Messages venant des threads
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Line(l) => self.lines.push(l),
                Msg::AiReply(text) => {
                    self.chat
                        .push(serde_json::json!({"role": "assistant", "content": text}));
                    self.push(Kind::Ai, &text);
                }
                Msg::Done => self.busy = false,
            }
        }

        // Transparence : baisse le dernier chiffre (alpha, 0-255) pour rendre plus transparent
        let bg = Color32::from_rgba_unmultiplied(12, 14, 24, 215);
        let frame = egui::Frame::none().fill(bg).inner_margin(10.0);

        let mut submit = false;

        // Zone de saisie (en bas)
        egui::TopBottomPanel::bottom("input")
            .frame(frame.clone())
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(self.prompt())
                        .font(FontId::monospace(15.0))
                        .color(Color32::from_rgb(90, 160, 255)),
                );
                ui.horizontal(|ui| {
                    let symbol = if self.busy { "└─… " } else { "└─$ " };
                    ui.label(
                        RichText::new(symbol)
                            .font(FontId::monospace(15.0))
                            .color(Color32::from_rgb(90, 160, 255)),
                    );
                    let resp = ui.add(
                        TextEdit::singleline(&mut self.input)
                            .font(FontId::monospace(15.0))
                            .frame(false)
                            .desired_width(f32::INFINITY)
                            .lock_focus(true),
                    );

                    if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        submit = true;
                    }

                    // Historique avec les flèches haut / bas
                    if resp.has_focus() {
                        if ui.input(|i| i.key_pressed(Key::ArrowUp)) && !self.history.is_empty() {
                            let pos = match self.hist_pos {
                                None => self.history.len() - 1,
                                Some(p) => p.saturating_sub(1),
                            };
                            self.hist_pos = Some(pos);
                            self.input = self.history[pos].clone();
                        }
                        if ui.input(|i| i.key_pressed(Key::ArrowDown)) {
                            match self.hist_pos {
                                Some(p) if p + 1 < self.history.len() => {
                                    self.hist_pos = Some(p + 1);
                                    self.input = self.history[p + 1].clone();
                                }
                                Some(_) => {
                                    self.hist_pos = None;
                                    self.input.clear();
                                }
                                None => {}
                            }
                        }
                    }
                    resp.request_focus(); // le curseur reste toujours dans le champ
                });
            });

        // Zone d'affichage
        egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
            ScrollArea::vertical()
                .stick_to_bottom(true)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for l in &self.lines {
                        let color = match l.kind {
                            Kind::Input => Color32::from_rgb(120, 220, 120),
                            Kind::Output => Color32::from_rgb(215, 215, 215),
                            Kind::Error => Color32::from_rgb(255, 110, 110),
                            Kind::Ai => Color32::from_rgb(120, 210, 230),
                            Kind::Info => Color32::from_rgb(140, 140, 160),
                        };
                        ui.label(
                            RichText::new(&l.text)
                                .font(FontId::monospace(15.0))
                                .color(color),
                        );
                    }
                });
        });

        if submit {
            self.submit(ctx);
        }
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Shell IA")
            .with_inner_size([950.0, 600.0])
            .with_min_inner_size([500.0, 300.0])
            .with_transparent(true),
        ..Default::default()
    };
    eframe::run_native(
        "Shell IA",
        options,
        Box::new(|cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(ShellApp::new()))
        }),
    )
}
