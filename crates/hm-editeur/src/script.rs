//! Mode script : rejouer des gestes dans la vraie fenêtre et en prendre des captures. C'est ainsi que l'interface
//! est essayée sans personne devant l'écran : `hammer-master.exe --script gestes.txt`.
//!
//! Une commande par ligne (`#` : commentaire). Une position s'écrit `x y` en points de la fenêtre, ou `v<n>:fx,fy`
//! en fraction de la vue n (0 = 3D en haut à gauche, 1 en haut à droite, 2 en bas à gauche, 3 en bas à droite),
//! `h<n>:cx,cy` pour la poignée de côté (cx, cy) dans la vue n (`h2:0,1` : celle du haut), `w<n>:x,y,z` pour un point
//! du monde vu dans la vue 2D n, `d:x,y` dans le panneau de droite depuis son bord gauche :
//!
//! ```text
//! ouvrir <carte.vmf>            attendre <images>   pause <ms>   capture <fichier.png>   capture3d <fichier.png>
//! souris <pos>                  clic <pos> [droit|milieu] [ctrl] [maj] [alt]   double <pos>
//! glisser <pos> <pos> [droit|milieu] [ctrl] [maj] [alt]     molette <pos> <crans>
//! touche <combo>   (ctrl+z, maj+b, entree, echap, f5, suppr…)   tenir <touche> <images>
//! texte <chaîne>                commande <json du noyau>   cadrer   enregistrer <fichier.vmf>   planter   couper   quitter
//! camera <x y z> <x y z>      (la vue 3D en ce point, tournée vers le second ; les vues 2D centrées dessus)
//! ```
use std::collections::VecDeque;

use egui::{Event, Key, Modifiers, PointerButton, Pos2};

/// Ce qu'une image du script injecte.
#[derive(Default, Clone)]
pub struct Image {
    pub evenements: Vec<Event>,
    pub modificateurs: Modifiers,
}

pub struct Script {
    lignes: VecDeque<String>,
    pub images: VecDeque<Image>,
    pub attente: u32,
    /// Attente en temps réel (`pause <ms>`).
    pub jusqua: Option<std::time::Instant>,
    pub capture: Option<String>,
    /// Images restantes avant d'abandonner une capture qui n'arrive pas.
    pub capture_delai: u32,
    pub journal: Vec<String>,
}

/// Ce que l'application doit faire elle-même (fichier, capture, commande du noyau).
pub enum Action {
    Ouvrir(String),
    Capture(String),
    /// La vue 3D seule (`capture3d`).
    Capture3d(String),
    Commande(String),
    Cadrer,
    Enregistrer(String),
    /// Image de fond de la vue de dessus (pour vérifier sans boîte de dialogue).
    Fond(String),
    /// Erreur interne provoquée exprès (pour essayer la récupération).
    Planter,
    /// Compiler : `compiler [geometrie|rapide|normale|finale] [nom]`, sans lancer le jeu.
    Compiler(String),
    /// Arrêt brutal, sans rien ranger (comme une coupure de courant).
    Couper,
    /// Caméra 3D : position, point visé.
    Camera([f32; 6]),
    Quitter,
}

pub fn touche(nom: &str) -> Option<(Key, Modifiers)> {
    let mut m = Modifiers::NONE;
    let mut k = None;
    for p in nom.split('+') {
        match p.trim().to_lowercase().as_str() {
            "ctrl" => {
                m.ctrl = true;
                m.command = true;
            }
            "maj" | "shift" => m.shift = true,
            "alt" => m.alt = true,
            "entree" | "enter" => k = Some(Key::Enter),
            "echap" | "escape" => k = Some(Key::Escape),
            "suppr" | "delete" => k = Some(Key::Delete),
            "espace" | "space" => k = Some(Key::Space),
            "tab" => k = Some(Key::Tab),
            "haut" => k = Some(Key::ArrowUp),
            "bas" => k = Some(Key::ArrowDown),
            "gauche" => k = Some(Key::ArrowLeft),
            "droite" => k = Some(Key::ArrowRight),
            "[" => k = Some(Key::OpenBracket),
            "]" => k = Some(Key::CloseBracket),
            autre => k = Key::from_name(&autre.to_uppercase()).or_else(|| Key::from_name(autre)),
        }
    }
    k.map(|k| (k, m))
}

impl Script {
    pub fn lire(texte: &str) -> Script {
        let lignes = texte.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty() && !l.starts_with('#')).collect();
        Script { lignes, images: VecDeque::new(), attente: 0, jusqua: None, capture: None, capture_delai: 0, journal: Vec::new() }
    }

    pub fn fini(&self) -> bool {
        self.lignes.is_empty() && self.images.is_empty() && self.capture.is_none() && self.attente == 0 && self.jusqua.is_none()
    }

    /// Avance d'une ligne si rien n'est en cours. `pos` traduit `v<n>:fx,fy` en points.
    pub fn suivante(&mut self, pos: &dyn Fn(&str) -> Option<Pos2>) -> Option<Action> {
        if !self.images.is_empty() || self.capture.is_some() {
            return None;
        }
        if self.attente > 0 {
            self.attente -= 1;
            return None;
        }
        if let Some(t) = self.jusqua {
            if std::time::Instant::now() < t {
                return None;
            }
            self.jusqua = None;
        }
        let l = self.lignes.pop_front()?;
        self.journal.push(l.clone());
        let mut mots = l.split_whitespace();
        let cmd = mots.next().unwrap_or("");
        let reste: Vec<&str> = mots.collect();
        // une position : deux nombres, ou `v<n>:fx,fy`
        let mut i = 0;
        let mut position = |reste: &[&str]| -> Option<Pos2> {
            let m = reste.get(i)?;
            if m.contains(':') && matches!(m.chars().next(), Some('v' | 'h' | 'w' | 'd')) {
                i += 1;
                return pos(m);
            }
            let (x, y) = (m.parse().ok()?, reste.get(i + 1)?.parse().ok()?);
            i += 2;
            Some(Pos2::new(x, y))
        };
        let mods = |reste: &[&str]| {
            let mut m = Modifiers::NONE;
            for w in reste {
                match *w {
                    "ctrl" => {
                        m.ctrl = true;
                        m.command = true;
                    }
                    "maj" => m.shift = true,
                    "alt" => m.alt = true,
                    _ => {}
                }
            }
            m
        };
        let bouton = |reste: &[&str]| if reste.contains(&"droit") { PointerButton::Secondary } else if reste.contains(&"milieu") { PointerButton::Middle } else { PointerButton::Primary };
        match cmd {
            "ouvrir" => return Some(Action::Ouvrir(reste.join(" "))),
            "capture" => return Some(Action::Capture(reste.join(" "))),
            "capture3d" => return Some(Action::Capture3d(reste.join(" "))),
            "commande" => return Some(Action::Commande(reste.join(" "))),
            "quitter" => return Some(Action::Quitter),
            "cadrer" => return Some(Action::Cadrer),
            "planter" => return Some(Action::Planter),
            "compiler" => return Some(Action::Compiler(reste.join(" "))),
            "couper" => return Some(Action::Couper),
            "camera" => {
                let v: Vec<f32> = reste.iter().filter_map(|x| x.parse().ok()).collect();
                if let Ok(p) = <[f32; 6]>::try_from(v.as_slice()) {
                    return Some(Action::Camera(p));
                }
                self.journal.push(crate::langue::t("camera : six nombres attendus").into());
            }
            "enregistrer" => return Some(Action::Enregistrer(reste.join(" "))),
            "fond" => return Some(Action::Fond(reste.join(" "))),
            "attendre" => self.attente = reste.first().and_then(|v| v.parse().ok()).unwrap_or(10),
            "pause" => self.jusqua = Some(std::time::Instant::now() + std::time::Duration::from_millis(reste.first().and_then(|v| v.parse().ok()).unwrap_or(500))),
            "souris" => {
                if let Some(p) = position(&reste) {
                    self.images.push_back(Image { evenements: vec![Event::PointerMoved(p)], ..Default::default() });
                }
            }
            "clic" => {
                if let Some(p) = position(&reste) {
                    let (b, m) = (bouton(&reste), mods(&reste));
                    self.images.push_back(Image { evenements: vec![Event::PointerMoved(p)], modificateurs: m, ..Default::default() });
                    self.images.push_back(Image { evenements: vec![Event::PointerButton { pos: p, button: b, pressed: true, modifiers: m }], modificateurs: m, ..Default::default() });
                    self.images.push_back(Image { evenements: vec![Event::PointerButton { pos: p, button: b, pressed: false, modifiers: m }], modificateurs: m, ..Default::default() });
                    self.images.push_back(Image::default());
                }
            }
            "double" => {
                if let Some(p) = position(&reste) {
                    self.images.push_back(Image { evenements: vec![Event::PointerMoved(p)], ..Default::default() });
                    // les deux clics dans la même image : egui ne compte un double clic que si le second suit le premier
                    // de moins de 0,3 s, et une image peut être longue (la sélection refait la géométrie)
                    let bouton = |pressed| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
                    self.images.push_back(Image { evenements: vec![bouton(true), bouton(false), bouton(true), bouton(false)], ..Default::default() });
                    self.images.push_back(Image::default());
                }
            }
            "glisser" => {
                if let (Some(a), Some(b)) = (position(&reste), position(&reste)) {
                    let (bt, m) = (bouton(&reste), mods(&reste));
                    self.images.push_back(Image { evenements: vec![Event::PointerMoved(a)], modificateurs: m, ..Default::default() });
                    self.images.push_back(Image { evenements: vec![Event::PointerButton { pos: a, button: bt, pressed: true, modifiers: m }], modificateurs: m, ..Default::default() });
                    let n = 12;
                    for k in 1..=n {
                        let t = k as f32 / n as f32;
                        self.images.push_back(Image { evenements: vec![Event::PointerMoved(a + (b - a) * t)], modificateurs: m, ..Default::default() });
                    }
                    self.images.push_back(Image { evenements: vec![Event::PointerButton { pos: b, button: bt, pressed: false, modifiers: m }], modificateurs: m, ..Default::default() });
                    self.images.push_back(Image::default());
                }
            }
            "molette" => {
                if let Some(p) = position(&reste) {
                    let crans: f32 = reste.get(i).and_then(|v| v.parse().ok()).unwrap_or(1.0);
                    self.images.push_back(Image { evenements: vec![Event::PointerMoved(p)], ..Default::default() });
                    self.images.push_back(Image { evenements: vec![Event::MouseWheel { unit: egui::MouseWheelUnit::Line, delta: egui::vec2(0.0, crans), modifiers: Modifiers::NONE, phase: egui::TouchPhase::Move, source: egui::MouseWheelSource::Wheel }], ..Default::default() });
                }
            }
            "touche" => {
                if let Some((k, m)) = reste.first().and_then(|n| touche(n)) {
                    self.images.push_back(Image { evenements: vec![Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: m }], modificateurs: m });
                    self.images.push_back(Image { evenements: vec![Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers: m }], modificateurs: m, ..Default::default() });
                    self.images.push_back(Image::default());
                }
            }
            "tenir" => {
                if let Some((k, m)) = reste.first().and_then(|n| touche(n)) {
                    let n: usize = reste.get(1).and_then(|v| v.parse().ok()).unwrap_or(30);
                    self.images.push_back(Image { evenements: vec![Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: m }], modificateurs: m });
                    for _ in 0..n {
                        self.images.push_back(Image { evenements: vec![], modificateurs: m });
                    }
                    self.images.push_back(Image { evenements: vec![Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers: m }], modificateurs: m, ..Default::default() });
                }
            }
            "texte" => self.images.push_back(Image { evenements: vec![Event::Text(reste.join(" "))], ..Default::default() }),
            autre => self.journal.push(tr!("commande de script inconnue : {autre}", "unknown script command: {autre}")),
        }
        None
    }
}
