//! Compiler une map (vbsp, vvis, vrad) en tâche de fond, lire le journal, retrouver une fuite, copier le `.bsp` dans
//! le dossier des maps du jeu, lancer le jeu sur la map.
//!
//! Les outils et dossiers sont ceux de la configuration de jeu de Hammer++ s'il est installé (`BSP`, `Vis`, `Light`,
//! `GameExe`, `GameDir`, `BSPDir`), sinon ceux de Garry's Mod trouvé par Steam.
pub mod comparer;

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outils {
    pub vbsp: PathBuf,
    pub vvis: PathBuf,
    pub vrad: PathBuf,
    /// L'exécutable du jeu (gmod.exe).
    pub jeu: PathBuf,
    /// Le dossier du jeu passé à `-game` (celui du gameinfo.txt).
    pub dossier_jeu: PathBuf,
    /// Où le `.bsp` est copié pour que le jeu le trouve.
    pub dossier_maps: PathBuf,
    /// D'où viennent ces réglages.
    pub source: String,
    /// La texture des murs qui ferment un cordon actif (`CordonTexture` de la configuration).
    pub texture_cordon: String,
}

/// Les exécutables choisis dans Hammer Master pour chaque étape (`vbsp = …`, `vvis = …`, `vrad = …`, une ligne
/// chacun) : ils l'emportent sur la configuration de Hammer++ (par exemple VVIS++ à la place de vvis).
pub fn fichier_choix() -> PathBuf {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("HammerMaster").join("outils.txt")
}

impl Outils {
    /// Les outils trouvés (Garry's Mod, configuration de Hammer++), puis les choix faits dans Hammer Master.
    pub fn detecter() -> Option<Outils> {
        let mut o = Outils::detecter_sans_choix()?;
        if let Ok(t) = std::fs::read_to_string(fichier_choix()) {
            o.appliquer_choix(&t);
        }
        Some(o)
    }

    /// Applique les lignes `étape = chemin` ; un chemin qui n'existe plus est ignoré.
    pub fn appliquer_choix(&mut self, texte: &str) {
        for l in texte.lines() {
            let Some((cle, val)) = l.split_once('=') else { continue };
            let p = PathBuf::from(val.trim());
            if !p.is_file() {
                continue;
            }
            match cle.trim() {
                "vbsp" => self.vbsp = p,
                "vvis" => self.vvis = p,
                "vrad" => self.vrad = p,
                _ => continue,
            }
            self.source = "choisis dans Hammer Master".into();
        }
    }

    /// Les lignes du fichier de choix : les étapes dont l'exécutable diffère de `defaut`.
    pub fn texte_choix(&self, defaut: &Outils) -> String {
        [("vbsp", &self.vbsp, &defaut.vbsp), ("vvis", &self.vvis, &defaut.vvis), ("vrad", &self.vrad, &defaut.vrad)]
            .iter()
            .filter(|(_, a, b)| a != b)
            .map(|(k, a, _)| format!("{k} = {}\n", a.display()))
            .collect()
    }

    /// Sans les choix faits dans Hammer Master : ce qui sert de « défaut ».
    pub fn detecter_sans_choix() -> Option<Outils> {
        let gmod = hm_assets::jeu::garrysmod()?;
        let bin = hm_assets::jeu::bin_gmod()?;
        let mut o = Outils {
            vbsp: bin.join("vbsp.exe"),
            vvis: bin.join("vvis.exe"),
            vrad: bin.join("vrad.exe"),
            jeu: gmod.join("gmod.exe"),
            dossier_jeu: gmod.join("garrysmod"),
            dossier_maps: gmod.join("garrysmod/maps"),
            source: format!("Garry's Mod ({})", gmod.display()),
            texture_cordon: "tools/toolsskybox".into(),
        };
        // la configuration de Hammer++, si elle existe, l'emporte : ce sont les réglages de l'utilisateur
        let cfg = gmod.join("bin/win64/hammerplusplus/hammerplusplus_gameconfig.txt");
        if let Ok(src) = std::fs::read(&cfg) {
            let mut n = 0;
            for l in String::from_utf8_lossy(&src).lines() {
                let m: Vec<&str> = l.split('"').filter(|x| !x.trim().is_empty()).collect();
                if m.len() < 2 {
                    continue;
                }
                if m[0] == "CordonTexture" {
                    o.texture_cordon = m[1].replace('\\', "/");
                    continue;
                }
                let p = PathBuf::from(m[1]);
                let champ = match m[0] {
                    "BSP" => &mut o.vbsp,
                    "Vis" => &mut o.vvis,
                    "Light" => &mut o.vrad,
                    "GameExe" => &mut o.jeu,
                    "GameDir" => &mut o.dossier_jeu,
                    "BSPDir" => &mut o.dossier_maps,
                    _ => continue,
                };
                if p.exists() {
                    *champ = p;
                    n += 1;
                }
            }
            if n > 0 {
                o.source = format!("configuration de Hammer++ ({})", cfg.display());
            }
        }
        Some(o)
    }

    /// La ligne qui lance le jeu : celle du « Run Map » de Hammer++ (`hammerplusplus_sequences_default.cfg` :
    /// `$game_exe -dev -console -allowdebug -hijack -game $gamedir +map $file`), sans la map (voir `lancer_jeu`),
    /// avec `-condebug` (la console écrite dans `console.log`, pour voir le menu arriver).
    pub fn commande_jeu(&self) -> (PathBuf, Vec<String>) {
        let mut args: Vec<String> = ["-dev", "-console", "-allowdebug", "-condebug", "-game"].map(String::from).into();
        args.push(self.dossier_jeu.display().to_string());
        (self.jeu.clone(), args)
    }

    /// Lance le jeu sur la map, sans attendre ; chaque étape est dite sur le canal rendu. GMod ignore parfois le `+map`
    /// d'une ligne de lancement et reste au menu (22, 24, 25/10 et 14/11/2026 ; notes du skill gmod-addon-maker) :
    /// la map lui est envoyée par `-hijack +map` (« prend le contrôle d'une instance du jeu déjà ouverte », wiki de
    /// Valve), au jeu déjà ouvert tout de suite, sinon au jeu lancé dès que son menu est là.
    pub fn lancer_jeu(&self, nom: &str) -> Receiver<String> {
        let (tx, rx) = mpsc::channel();
        let (o, nom) = (self.clone(), nom.to_string());
        std::thread::spawn(move || {
            let dit = o.lancer_jeu_attendre(&nom, &tx);
            let _ = tx.send(dit);
        });
        rx
    }

    fn lancer_jeu_attendre(&self, nom: &str, tx: &Sender<String>) -> String {
        let dossier = self.jeu.parent().unwrap_or(Path::new(".")).to_path_buf();
        let envoyer = || Command::new(&self.jeu).current_dir(&dossier).args(["-hijack", "+map", nom]).spawn().map(|_| ());
        if self.jeu_ouvert() {
            return match envoyer() {
                Ok(()) => format!("map {nom} envoyée au jeu ouvert (-hijack +map {nom})"),
                Err(e) => format!("envoi de la map au jeu impossible : {e}"),
            };
        }
        let journal = self.dossier_jeu.join("console.log");
        let debut = std::fs::metadata(&journal).map_or(0, |m| m.len());
        let (exe, args) = self.commande_jeu();
        if let Err(e) = Command::new(&exe).current_dir(&dossier).args(&args).spawn() {
            return format!("lancement du jeu impossible : {e}");
        }
        let _ = tx.send(format!("jeu lancé : {} {} ; la map lui sera envoyée dès son menu affiché", exe.display(), args.join(" ")));
        for _ in 0..300 {
            std::thread::sleep(std::time::Duration::from_secs(1));
            let Ok(mut f) = std::fs::File::open(&journal) else { continue };
            let mut texte = Vec::new();
            use std::io::{Read, Seek, SeekFrom};
            if f.seek(SeekFrom::Start(debut)).is_err() || f.read_to_end(&mut texte).is_err() {
                continue;
            }
            if menu_affiche(&String::from_utf8_lossy(&texte)) {
                // comme les notes du skill : un temps après le montage des addons
                std::thread::sleep(std::time::Duration::from_secs(3));
                return match envoyer() {
                    Ok(()) => format!("menu du jeu affiché : map {nom} envoyée (-hijack +map {nom})"),
                    Err(e) => format!("envoi de la map au jeu impossible : {e}"),
                };
            }
        }
        format!("menu du jeu pas vu en 5 minutes dans {} : map non envoyée", journal.display())
    }

    /// Le jeu a une fenêtre ouverte (les aides Chromium du même exécutable n'ont pas de titre : « N/A »).
    fn jeu_ouvert(&self) -> bool {
        let Some(exe) = self.jeu.file_name() else { return false };
        let mut cmd = Command::new("tasklist");
        cmd.args(["/V", "/NH", "/FO", "CSV", "/FI"]).arg(format!("IMAGENAME eq {}", exe.to_string_lossy()));
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let Ok(s) = cmd.output() else { return false };
        String::from_utf8_lossy(&s.stdout).lines().any(|l| l.starts_with('"') && !l.trim_end().ends_with("\"N/A\""))
    }
}

/// Le menu du jeu est là : les addons du Workshop sont montés (« Mounted 41 of 41 workshop addons! » ; le premier
/// « Mounted 0 of 41 » vient avant le montage).
pub fn menu_affiche(console: &str) -> bool {
    console.lines().any(|l| {
        let mots: Vec<&str> = l.split_whitespace().collect();
        matches!(mots.as_slice(), ["Mounted", a, "of", b, "workshop", "addons!"] if a == b)
    })
}

/// Ce qu'une compilation refait, d'après ce qui a changé depuis la précédente (même nom, même mode, réussie).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Portee {
    /// Rien n'a changé : le BSP précédent est repris tel quel.
    Rien,
    /// Seulement des entités ponctuelles ordinaires : `vbsp -onlyents` réécrit leur lump, visibilité et lumière gardées.
    Entites,
    /// Des lumières aussi : `vbsp -onlyents` puis vrad, la visibilité gardée.
    Lumieres,
    /// Le reste : vbsp, vvis, vrad.
    Tout,
}

/// Classes que vbsp verse dans ses propres lumps ou qui changent l'arbre : `-onlyents` ne les met pas à jour (vu le
/// 14/11/2026 : un `prop_static` déplacé puis `-onlyents` ou `-onlyprops`, le BSP ne change pas).
const VERSEES_PAR_VBSP: [&str; 11] = [
    "prop_static", "prop_detail", "info_overlay", "info_overlay_transition", "env_cubemap", "sky_camera", "info_no_dynamic_shadow",
    "func_instance", "infodecal", "func_viscluster", "info_overlay_accessor",
];

/// Ce qu'il faut refaire pour passer du VMF compilé `avant` à `apres` (les objets gardent leur numéro d'une version à
/// l'autre dans l'éditeur). Les réglages du monde, un solide, une entité à solides ou une classe versée par vbsp :
/// tout ; une lumière (`light*` sauf `light_dynamic`, `info_lighting`) : les lumières ; une autre entité : les
/// entités. Le bloc `editor` (couleur, visgroups) ne compte pas. Dans le doute, tout.
pub fn portee(avant: &[u8], apres: &[u8]) -> Portee {
    let (Ok(a), Ok(b)) = (hm_vmf::Vmf::parse(avant.to_vec()), hm_vmf::Vmf::parse(apres.to_vec())) else { return Portee::Tout };
    if a.world().map(signature) != b.world().map(signature) {
        return Portee::Tout;
    }
    let par_id = |v: &hm_vmf::Vmf| -> Option<std::collections::HashMap<u64, String>> { v.entities().map(|e| e.id().map(|i| (i, signature(e)))).collect() };
    let (Some(ea), Some(eb)) = (par_id(&a), par_id(&b)) else { return Portee::Tout };
    let noeud = |v: &hm_vmf::Vmf, id: u64| v.entities().find(|e| e.id() == Some(id)).cloned();
    let mut p = Portee::Rien;
    for id in ea.keys().chain(eb.keys()).copied().collect::<std::collections::BTreeSet<u64>>() {
        if ea.get(&id) == eb.get(&id) {
            continue;
        }
        for n in [noeud(&a, id), noeud(&b, id)].into_iter().flatten() {
            let classe = n.classname().to_ascii_lowercase();
            if n.solids().next().is_some() || VERSEES_PAR_VBSP.contains(&classe.as_str()) {
                return Portee::Tout;
            }
            let lumiere = (classe.starts_with("light") && classe != "light_dynamic") || classe == "info_lighting";
            p = p.max(if lumiere { Portee::Lumieres } else { Portee::Entites });
        }
    }
    p
}

/// Un objet du VMF en texte, sans ses blocs `editor` ni `mapversion` : deux versions égales ont la même signature.
fn signature(n: &hm_vmf::Node) -> String {
    let mut s = format!("{}{{", n.name);
    for p in n.pairs.iter().filter(|p| !p.key.eq_ignore_ascii_case("mapversion")) {
        s += &format!("{}={};", p.key, p.value);
    }
    for c in n.children.iter().filter(|c| !c.name.eq_ignore_ascii_case("editor")) {
        s += &signature(c);
    }
    s + "}"
}

/// Le VMF de la compilation précédente de `nom` dans `dossier`, si elle a réussi dans ce mode et que son BSP est là.
pub fn precedente(dossier: &Path, nom: &str, mode: Mode) -> Option<Vec<u8>> {
    let marque = std::fs::read_to_string(dossier.join(format!("{nom}.compile"))).ok()?;
    (marque.trim() == mode.nom() && dossier.join(format!("{nom}.bsp")).is_file()).then(|| std::fs::read(dossier.join(format!("{nom}.vmf"))).ok()).flatten()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// vbsp seul : géométrie et fuite, en quelques secondes (sans visibilité ni lumière).
    Geometrie,
    /// vvis et vrad en `-fast`.
    Rapide,
    Normal,
    /// vrad en `-final`.
    Final,
}

impl Mode {
    /// `geometrie`, `rapide`, `normale` (ou `normal`), `finale` (ou `final`), sans tenir compte des accents ni de la casse.
    pub fn lire(t: &str) -> Option<Mode> {
        match t.trim().to_lowercase().replace('é', "e").as_str() {
            "geometrie" => Some(Mode::Geometrie),
            "rapide" => Some(Mode::Rapide),
            "normale" | "normal" => Some(Mode::Normal),
            "finale" | "final" => Some(Mode::Final),
            _ => None,
        }
    }

    pub fn nom(self) -> &'static str {
        match self {
            Mode::Geometrie => "géométrie seule (vbsp)",
            Mode::Rapide => "rapide",
            Mode::Normal => "normale",
            Mode::Final => "finale",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    /// Début d'une étape (`vbsp`, `vvis`, `vrad`, `copie`).
    Etape(String),
    Ligne(String),
    /// La map fuit : les points du chemin (fichier `.lin`).
    Fuite(Vec<[f32; 3]>),
    Fin { reussi: bool, bsp: Option<PathBuf>, secondes: f32, erreur: Option<String> },
}

/// Une compilation en cours.
pub struct Compilation {
    pub rx: Receiver<Message>,
    arret: Arc<AtomicBool>,
    enfant: Arc<Mutex<Option<Child>>>,
}

impl Compilation {
    /// Arrête l'outil en cours.
    pub fn arreter(&self) {
        self.arret.store(true, Ordering::SeqCst);
        if let Some(c) = self.enfant.lock().unwrap().as_mut() {
            let _ = c.kill();
        }
    }
}

/// Le dossier où une map est compilée : `.hammer-master/compile/` à côté du VMF, sinon un dossier temporaire.
pub fn dossier_compile(vmf: Option<&Path>) -> PathBuf {
    match vmf.and_then(|v| v.parent()) {
        Some(d) => d.join(".hammer-master").join("compile"),
        None => std::env::temp_dir().join("HammerMaster").join("compile"),
    }
}

/// Lit le fichier de fuite de vbsp (`.lin` : une ligne `x y z` par point).
pub fn lire_fuite(lin: &Path) -> Option<Vec<[f32; 3]>> {
    let t = std::fs::read_to_string(lin).ok()?;
    let pts: Vec<[f32; 3]> = t
        .lines()
        .filter_map(|l| {
            let v: Vec<f32> = l.split_whitespace().filter_map(|x| x.parse().ok()).collect();
            (v.len() >= 3).then(|| [v[0], v[1], v[2]])
        })
        .collect();
    (!pts.is_empty()).then_some(pts)
}

/// Lit le fichier de portails de vbsp (`.prt` : `PRT1`, clusters, portails, puis une ligne
/// `<n> <cluster> <cluster> (x y z ) …` par portail) : les polygones des portails.
pub fn lire_portails(prt: &Path) -> Option<Vec<Vec<[f32; 3]>>> {
    let t = std::fs::read_to_string(prt).ok()?;
    let mut l = t.lines();
    if l.next()?.trim() != "PRT1" {
        return None;
    }
    let polys: Vec<Vec<[f32; 3]>> = l
        .filter(|x| x.contains('('))
        .map(|x| {
            x.split('(').skip(1).filter_map(|p| {
                let v: Vec<f32> = p.trim_end_matches([')', ' ']).split_whitespace().filter_map(|n| n.parse().ok()).collect();
                (v.len() >= 3).then(|| [v[0], v[1], v[2]])
            }).collect()
        })
        .collect();
    Some(polys)
}

/// Une ligne de journal qui mérite l'attention (erreur ou avertissement de l'outil).
pub fn grave(l: &str) -> bool {
    let b = l.to_ascii_lowercase();
    b.contains("error") || b.contains("leaked") || b.contains("warning") || b.contains("couldn't") || b.contains("can't") || b.contains("missing") || b.contains("bad ") || b.contains("too many") || b.contains("exceeded")
}

fn executer(exe: &Path, args: &[String], dossier: &Path, tx: &Sender<Message>, arret: &AtomicBool, enfant: &Mutex<Option<Child>>) -> Result<bool, String> {
    let mut cmd = Command::new(exe);
    cmd.args(args).current_dir(dossier).stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut c = cmd.spawn().map_err(|e| format!("{} : {e}", exe.display()))?;
    let sortie = c.stdout.take().unwrap();
    let erreurs = c.stderr.take().unwrap();
    *enfant.lock().unwrap() = Some(c);
    let tx2 = tx.clone();
    let lecteur_err = std::thread::spawn(move || {
        for l in BufReader::new(erreurs).lines().map_while(Result::ok) {
            let _ = tx2.send(Message::Ligne(l));
        }
    });
    // vbsp écrit sans fin de ligne pendant les étapes : on coupe aussi sur les retours chariot
    let mut lecteur = BufReader::new(sortie);
    let mut tampon = Vec::new();
    loop {
        tampon.clear();
        match lecteur.read_until(b'\n', &mut tampon) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                for morceau in String::from_utf8_lossy(&tampon).split('\r') {
                    let m = morceau.trim_end();
                    if !m.is_empty() {
                        let _ = tx.send(Message::Ligne(m.to_string()));
                    }
                }
            }
        }
    }
    let _ = lecteur_err.join();
    let statut = enfant.lock().unwrap().take().map(|mut c| c.wait());
    if arret.load(Ordering::SeqCst) {
        return Err("arrêtée".into());
    }
    match statut {
        Some(Ok(s)) => Ok(s.success()),
        Some(Err(e)) => Err(e.to_string()),
        None => Err("outil perdu".into()),
    }
}

/// Lance la compilation de `vmf` (le contenu de la map, écrit sous `nom.vmf` dans `dossier`).
pub fn lancer(vmf: Vec<u8>, nom: &str, dossier: &Path, outils: &Outils, mode: Mode, copier: bool) -> Compilation {
    lancer_avec(vmf, nom, dossier, outils, mode, copier, Vec::new(), Portee::Tout)
}

/// Embarque des fichiers dans le pakfile d'un `.bsp` par le bspzip du jeu (`-addorupdatelist` : une ligne pour le
/// chemin dans le jeu, une pour le fichier sur le disque). `fichiers` : (chemin dans le jeu, contenu).
pub fn empaqueter(bsp: &Path, fichiers: &[(String, Vec<u8>)], outils: &Outils) -> Result<(), String> {
    let exe = outils.vbsp.with_file_name("bspzip.exe");
    let dossier = bsp.with_extension("embarque");
    let _ = std::fs::remove_dir_all(&dossier);
    let mut liste = String::new();
    for (rel, octets) in fichiers {
        let f = dossier.join(rel);
        std::fs::create_dir_all(f.parent().unwrap()).and_then(|_| std::fs::write(&f, octets)).map_err(|e| format!("{} : {e}", f.display()))?;
        liste += &format!("{rel}\n{}\n", f.display());
    }
    let l = dossier.join("liste.txt");
    std::fs::write(&l, liste).map_err(|e| e.to_string())?;
    let neuf = bsp.with_extension("embarque.bsp");
    let st = Command::new(&exe).arg("-addorupdatelist").arg(bsp).arg(&l).arg(&neuf).current_dir(bsp.parent().unwrap_or(Path::new("."))).stdout(Stdio::null()).stderr(Stdio::null()).status().map_err(|e| format!("{} : {e}", exe.display()))?;
    let _ = std::fs::remove_dir_all(&dossier);
    if !st.success() || !neuf.is_file() {
        return Err(format!("bspzip a échoué ({st})"));
    }
    std::fs::rename(&neuf, bsp).map_err(|e| e.to_string())
}

/// Comme `lancer`, puis embarque `embarquer` (chemin dans le jeu, contenu) dans le `.bsp`, avant de le copier.
/// `portee` : ce qu'il faut refaire (`portee()` contre `precedente()`) ; autre chose que `Tout` reprend le BSP précédent.
pub fn lancer_avec(vmf: Vec<u8>, nom: &str, dossier: &Path, outils: &Outils, mode: Mode, copier: bool, embarquer: Vec<(String, Vec<u8>)>, portee: Portee) -> Compilation {
    let (tx, rx) = mpsc::channel();
    let arret = Arc::new(AtomicBool::new(false));
    let enfant: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));
    let (a, e) = (arret.clone(), enfant.clone());
    let (nom, dossier, outils) = (nom.to_string(), dossier.to_path_buf(), outils.clone());
    std::thread::spawn(move || {
        let t0 = Instant::now();
        let fin = |reussi: bool, bsp: Option<PathBuf>, erreur: Option<String>| Message::Fin { reussi, bsp, secondes: t0.elapsed().as_secs_f32(), erreur };
        if let Err(err) = std::fs::create_dir_all(&dossier) {
            let _ = tx.send(fin(false, None, Some(format!("{} : {err}", dossier.display()))));
            return;
        }
        let marque = dossier.join(format!("{nom}.compile"));
        let _ = std::fs::remove_file(&marque);
        let exts: &[&str] = if portee == Portee::Tout { &["bsp", "prt", "lin", "log"] } else { &["lin", "log"] };
        for ext in exts {
            let _ = std::fs::remove_file(dossier.join(format!("{nom}.{ext}")));
        }
        if let Err(err) = std::fs::write(dossier.join(format!("{nom}.vmf")), &vmf) {
            let _ = tx.send(fin(false, None, Some(err.to_string())));
            return;
        }
        let jeu = outils.dossier_jeu.display().to_string();
        let mut etapes: Vec<(&str, &PathBuf, Vec<String>)> = vec![("vbsp", &outils.vbsp, vec!["-game".into(), jeu.clone(), nom.clone()])];
        let _ = tx.send(Message::Ligne(match portee {
            Portee::Tout => "compilation complète".to_string(),
            Portee::Rien => "rien n'a changé depuis la compilation précédente : son BSP est repris".to_string(),
            Portee::Entites => "seulement des entités ont changé : vbsp -onlyents sur le BSP précédent, visibilité et lumière gardées".to_string(),
            Portee::Lumieres => "des lumières ont changé : vbsp -onlyents puis vrad sur le BSP précédent, visibilité gardée".to_string(),
        }));
        let (vis, rad): (Vec<&str>, Vec<&str>) = match mode {
            Mode::Geometrie => (vec![], vec![]),
            Mode::Rapide => (vec!["-fast"], vec!["-fast"]),
            Mode::Normal => (vec![], vec![]),
            Mode::Final => (vec![], vec!["-final"]),
        };
        if portee != Portee::Tout {
            etapes.clear();
            if portee != Portee::Rien {
                etapes.push(("vbsp", &outils.vbsp, vec!["-onlyents".into(), "-game".into(), jeu.clone(), nom.clone()]));
            }
        }
        if mode != Mode::Geometrie && portee == Portee::Lumieres {
            let mut r = vec!["-game".to_string(), jeu.clone()];
            r.extend(rad.iter().map(|s| s.to_string()));
            r.push(nom.clone());
            etapes.push(("vrad", &outils.vrad, r));
        }
        if mode != Mode::Geometrie && portee == Portee::Tout {
            let mut v = vec!["-game".to_string(), jeu.clone()];
            v.extend(vis.iter().map(|s| s.to_string()));
            v.push(nom.clone());
            etapes.push(("vvis", &outils.vvis, v));
            let mut r = vec!["-game".to_string(), jeu.clone()];
            r.extend(rad.iter().map(|s| s.to_string()));
            r.push(nom.clone());
            etapes.push(("vrad", &outils.vrad, r));
        }
        for (etape, exe, args) in &etapes {
            let _ = tx.send(Message::Etape(etape.to_string()));
            let _ = tx.send(Message::Ligne(format!("> {} {}", exe.display(), args.join(" "))));
            match executer(exe, args, &dossier, &tx, &a, &e) {
                Ok(ok) => {
                    if *etape == "vbsp" {
                        if let Some(p) = lire_fuite(&dossier.join(format!("{nom}.lin"))) {
                            let _ = tx.send(Message::Fuite(p));
                            let _ = tx.send(fin(false, None, Some("la map fuit : elle n'est pas fermée (chemin de la fuite affiché dans les vues)".into())));
                            return;
                        }
                    }
                    if !ok || !dossier.join(format!("{nom}.bsp")).is_file() {
                        let _ = tx.send(fin(false, None, Some(format!("{etape} a échoué : lire le journal"))));
                        return;
                    }
                }
                Err(err) => {
                    let _ = tx.send(fin(false, None, Some(format!("{etape} : {err}"))));
                    return;
                }
            }
        }
        let bsp = dossier.join(format!("{nom}.bsp"));
        if !embarquer.is_empty() {
            let _ = tx.send(Message::Etape("bspzip".into()));
            match empaqueter(&bsp, &embarquer, &outils) {
                Ok(()) => {
                    let _ = tx.send(Message::Ligne(format!("{} fichier(s) embarqué(s) dans le .bsp", embarquer.len())));
                }
                Err(err) => {
                    let _ = tx.send(fin(false, Some(bsp), Some(format!("empaquetage : {err}"))));
                    return;
                }
            }
        }
        if copier {
            let _ = tx.send(Message::Etape("copie".into()));
            let dest = outils.dossier_maps.join(format!("{nom}.bsp"));
            match std::fs::create_dir_all(&outils.dossier_maps).and_then(|_| std::fs::copy(&bsp, &dest)) {
                Ok(_) => {
                    let _ = tx.send(Message::Ligne(format!("copiée dans {}", dest.display())));
                }
                Err(err) => {
                    let _ = tx.send(fin(false, Some(bsp), Some(format!("copie vers {} : {err}", dest.display()))));
                    return;
                }
            }
        }
        // ce BSP pourra être repris par une compilation partielle dans le même mode
        let _ = std::fs::write(&marque, mode.nom());
        let _ = tx.send(fin(true, Some(bsp), None));
    });
    Compilation { rx, arret, enfant }
}

/// Attend la fin d'une compilation et rend tous ses messages (pour les tests et la ligne de commande).
pub fn attendre(c: &Compilation) -> Vec<Message> {
    let mut out = Vec::new();
    while let Ok(m) = c.rx.recv() {
        let fin = matches!(m, Message::Fin { .. });
        out.push(m);
        if fin {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use hm_doc::{Commande, Session};

    /// Une pièce fermée de 512 × 512 × 256, une lumière, un point d'apparition. `trou` : un mur en moins.
    fn piece(trou: bool) -> Vec<u8> {
        let mut s = Session::nouvelle();
        let m = "DEV/DEV_MEASUREGENERIC01".to_string();
        let murs: Vec<([f64; 3], [f64; 3])> = vec![
            ([-256.0, -256.0, -16.0], [256.0, 256.0, 0.0]),
            ([-256.0, -256.0, 256.0], [256.0, 256.0, 272.0]),
            ([-272.0, -256.0, 0.0], [-256.0, 256.0, 256.0]),
            ([256.0, -256.0, 0.0], [272.0, 256.0, 256.0]),
            ([-272.0, -272.0, 0.0], [272.0, -256.0, 256.0]),
            ([-272.0, 256.0, 0.0], [272.0, 272.0, 256.0]),
        ];
        let n = if trou { murs.len() - 1 } else { murs.len() };
        for (lo, hi) in murs.into_iter().take(n) {
            s.executer(Commande::CreerBloc { min: lo, max: hi, materiau: m.clone(), classe: None }).unwrap();
        }
        s.executer(Commande::CreerEntite { classe: "info_player_start".into(), origine: [0.0, 0.0, 1.0], cles: vec![] }).unwrap();
        s.executer(Commande::CreerEntite { classe: "light".into(), origine: [0.0, 0.0, 200.0], cles: vec![("_light".into(), "255 255 255 200".into())] }).unwrap();
        s.doc().ecrire()
    }

    #[test]
    fn compiler_une_piece_et_trouver_une_fuite() {
        let Some(o) = Outils::detecter_sans_choix() else { return };
        let d = std::env::temp_dir().join(format!("hm_compile_essai_{}", std::process::id()));
        // fermée : vbsp, vvis, vrad en rapide, un .bsp
        let c = lancer(piece(false), "hm_essai_piece", &d, &o, Mode::Rapide, false);
        let msgs = attendre(&c);
        let fin = msgs.last().unwrap().clone();
        let journal: Vec<String> = msgs.iter().filter_map(|m| if let Message::Ligne(l) = m { Some(l.clone()) } else { None }).collect();
        assert!(matches!(fin, Message::Fin { reussi: true, .. }), "{fin:?}\n{}", journal.iter().rev().take(30).cloned().collect::<Vec<_>>().join("\n"));
        assert!(d.join("hm_essai_piece.bsp").is_file());
        assert!(msgs.iter().any(|m| *m == Message::Etape("vrad".into())));
        // percée : la fuite est trouvée par vbsp, avec son chemin
        let c = lancer(piece(true), "hm_essai_fuite", &d, &o, Mode::Rapide, false);
        let msgs = attendre(&c);
        assert!(msgs.iter().any(|m| matches!(m, Message::Fuite(p) if p.len() >= 2)), "pas de fuite trouvée");
        assert!(matches!(msgs.last().unwrap(), Message::Fin { reussi: false, .. }));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn vbsp_accepte_les_brosses_des_outils() {
        // une pièce fermée, et dedans ce que font les outils de brosses : vbsp ne doit signaler aucune brosse
        let Some(o) = Outils::detecter_sans_choix() else { return };
        let d = std::env::temp_dir().join(format!("hm_compile_brosses_{}", std::process::id()));
        let mut s = Session::nouvelle();
        let m = "DEV/DEV_MEASUREGENERIC01".to_string();
        let b = s.executer(Commande::CreerBloc { min: [-1024.0, -1024.0, -16.0], max: [1024.0, 1024.0, 528.0], materiau: m.clone(), classe: None }).unwrap().crees;
        s.executer(Commande::Evider { objets: b, epaisseur: 16.0 }).unwrap();
        let prim = |s: &mut Session, f: hm_doc::Forme, x: f64, y: f64| s.executer(Commande::Primitive { forme: f, min: [x, y, 0.0], max: [x + 256.0, y + 256.0, 192.0], materiau: "DEV/DEV_MEASUREGENERIC01".into(), classe: None }).unwrap().crees;
        prim(&mut s, hm_doc::Forme::Cylindre { cotes: 12 }, -900.0, -900.0);
        prim(&mut s, hm_doc::Forme::Pointe { cotes: 6 }, -500.0, -900.0);
        prim(&mut s, hm_doc::Forme::Sphere { cotes: 12 }, -100.0, -900.0);
        prim(&mut s, hm_doc::Forme::Coin, 300.0, -900.0);
        prim(&mut s, hm_doc::Forme::Arche(hm_doc::brosses::Arche { largeur_mur: 32.0, cotes: 10, arc: 270.0, depart: 45.0, hauteur_ajoutee: 8.0 }), -900.0, -400.0);
        prim(&mut s, hm_doc::Forme::Escalier { marches: 8, sens: 1 }, -500.0, -400.0);
        let c = prim(&mut s, hm_doc::Forme::Cylindre { cotes: 16 }, -100.0, -400.0);
        s.executer(Commande::Evider { objets: c, epaisseur: 16.0 }).unwrap();
        let blk = s.executer(Commande::CreerBloc { min: [300.0, -400.0, 0.0], max: [556.0, -144.0, 192.0], materiau: m, classe: None }).unwrap().crees;
        s.executer(Commande::Decouper { objets: blk, plan: [[300.0, -400.0, 0.0], [556.0, -144.0, 0.0], [556.0, -144.0, 192.0]], garder: hm_doc::Garder::LesDeux, materiau: "TOOLS/TOOLSNODRAW".into() }).unwrap();
        s.executer(Commande::CreerEntite { classe: "info_player_start".into(), origine: [0.0, 400.0, 1.0], cles: vec![] }).unwrap();
        let c = lancer(s.doc().pour_compilation(&o.texture_cordon), "hm_essai_brosses", &d, &o, Mode::Geometrie, false);
        let msgs = attendre(&c);
        let journal: Vec<String> = msgs.iter().filter_map(|m| if let Message::Ligne(l) = m { Some(l.clone()) } else { None }).collect();
        let mauvaises: Vec<&String> = journal.iter().filter(|l| {
            let l = l.to_ascii_lowercase();
            (l.contains("brush") || l.contains("plane") || l.contains("side")) && (l.contains("error") || l.contains("invalid") || l.contains("bad") || l.contains("warning") || l.contains("mixed"))
        }).collect();
        assert!(mauvaises.is_empty(), "{mauvaises:?}");
        assert!(!msgs.iter().any(|m| matches!(m, Message::Fuite(_))), "fuite");
        assert!(matches!(msgs.last().unwrap(), Message::Fin { reussi: true, .. }), "{}", journal.iter().rev().take(30).cloned().collect::<Vec<_>>().join("\n"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn displacement_compile_et_mesure_dans_le_bsp() {
        // une pièce fermée ; dedans un sol de 16 dont le dessus devient un displacement de puissance 3, sculpté en
        // colline ; vbsp ; les hauteurs des 81 sommets lues dans le BSP (bloc 33 : vecteur, distance, alpha)
        let Some(o) = Outils::detecter_sans_choix() else { return };
        let d = std::env::temp_dir().join(format!("hm_compile_disp_{}", std::process::id()));
        let mut s = Session::nouvelle();
        let m = "DEV/DEV_MEASUREGENERIC01".to_string();
        let b = s.executer(Commande::CreerBloc { min: [-640.0, -640.0, -16.0], max: [640.0, 640.0, 528.0], materiau: m.clone(), classe: None }).unwrap().crees;
        s.executer(Commande::Evider { objets: b, epaisseur: 16.0 }).unwrap();
        let sol = match s.executer(Commande::CreerBloc { min: [-256.0, -256.0, 0.0], max: [256.0, 256.0, 16.0], materiau: "NATURE/BLENDGRASSGRAVEL001A".into(), classe: None }).unwrap().crees[0] {
            hm_doc::Ref::Solide(i) => i,
            _ => unreachable!(),
        };
        let dessus = s.doc().solide(sol).unwrap().faces.iter().find(|f| f.normale().z > 0.99).unwrap().id;
        let rf = hm_doc::RefFace { solide: sol, face: dessus };
        s.executer(Commande::CreerDisp { faces: vec![rf], puissance: 3 }).unwrap();
        s.executer(Commande::PeindreDisp { faces: None, centre: [0.0, 0.0, 16.0], rayon: 200.0, pinceau: hm_doc::disp::Pinceau::Relever { hauteur: 96.0 } }).unwrap();
        s.executer(Commande::CreerEntite { classe: "info_player_start".into(), origine: [0.0, 400.0, 1.0], cles: vec![] }).unwrap();
        // les hauteurs voulues (au-dessus du dessus du sol), triées
        let f = s.doc().solide(sol).unwrap().faces.iter().find(|f| f.id == dessus).unwrap();
        let dd = hm_doc::disp::Disp::lire(f.disp.as_ref().unwrap()).unwrap();
        let mut voulu: Vec<f64> = dd.normales.iter().zip(&dd.distances).map(|(n, l)| n.z * l).collect();
        voulu.sort_by(f64::total_cmp);
        let c = lancer(s.doc().pour_compilation(&o.texture_cordon), "hm_essai_disp", &d, &o, Mode::Geometrie, false);
        let msgs = attendre(&c);
        assert!(matches!(msgs.last().unwrap(), Message::Fin { reussi: true, .. }), "compilation ratée");
        assert!(!msgs.iter().any(|m| matches!(m, Message::Fuite(_))), "fuite");
        let bsp = std::fs::read(d.join("hm_essai_disp.bsp")).unwrap();
        let bloc = |i: usize| {
            let o = 8 + i * 16;
            (u32::from_le_bytes(bsp[o..o + 4].try_into().unwrap()) as usize, u32::from_le_bytes(bsp[o + 4..o + 8].try_into().unwrap()) as usize)
        };
        assert_eq!(bloc(26).1 / 176, 1, "un dispinfo");
        let (pos, long) = bloc(33);
        assert_eq!(long / 20, 81, "81 sommets");
        let mut lu: Vec<f64> = (0..81)
            .map(|k| {
                let e = &bsp[pos + k * 20..pos + k * 20 + 20];
                let f = |o: usize| f32::from_le_bytes(e[o..o + 4].try_into().unwrap()) as f64;
                f(8) * f(12)
            })
            .collect();
        lu.sort_by(f64::total_cmp);
        let ecart = voulu.iter().zip(&lu).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
        assert!(ecart < 0.01, "écart maximal {ecart} ; voulu {voulu:?} ; lu {lu:?}");
        assert!(lu.last().unwrap() > &90.0, "le sommet de la colline");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Une salle fermée de six blocs : intérieur `[x0 - demi, x0 + demi] × [-demi, demi] × [0, haut]`.
    fn salle(s: &mut Session, x0: f64, demi: f64, haut: f64, plafond: &str) {
        let m = "DEV/DEV_MEASUREGENERIC01".to_string();
        let (a, b) = (x0 - demi, x0 + demi);
        let murs: Vec<([f64; 3], [f64; 3], &str)> = vec![
            ([a - 16.0, -demi - 16.0, -16.0], [b + 16.0, demi + 16.0, 0.0], &m),
            ([a - 16.0, -demi - 16.0, haut], [b + 16.0, demi + 16.0, haut + 16.0], plafond),
            ([a - 16.0, -demi, 0.0], [a, demi, haut], &m),
            ([b, -demi, 0.0], [b + 16.0, demi, haut], &m),
            ([a - 16.0, -demi - 16.0, 0.0], [b + 16.0, -demi, haut], &m),
            ([a - 16.0, demi, 0.0], [b + 16.0, demi + 16.0, haut], &m),
        ];
        for (lo, hi, mat) in murs {
            s.executer(Commande::CreerBloc { min: lo, max: hi, materiau: mat.to_string(), classe: None }).unwrap();
        }
    }

    #[test]
    fn compiler_une_zone() {
        // deux salles éloignées, le point d'apparition dans la seconde : la zone de la première se compile seule
        // (point d'apparition ajouté à la copie), sans fuite, avec ses 6 brosses et les 6 murs du cordon
        let Some(o) = Outils::detecter_sans_choix() else { return };
        let d = std::env::temp_dir().join(format!("hm_compile_zone_{}", std::process::id()));
        let mut s = Session::nouvelle();
        let m = "DEV/DEV_MEASUREGENERIC01";
        salle(&mut s, 0.0, 256.0, 256.0, m);
        salle(&mut s, 5000.0, 256.0, 256.0, m);
        s.executer(Commande::CreerEntite { classe: "info_player_start".into(), origine: [5000.0, 0.0, 1.0], cles: vec![] }).unwrap();
        let vmf = s.doc().pour_compilation_zone(&o.texture_cordon, None, &d, [-300.0, -300.0, -32.0], [300.0, 300.0, 300.0]);
        let c = lancer(vmf, "hm_essai_zone", &d, &o, Mode::Geometrie, false);
        let msgs = attendre(&c);
        assert!(!msgs.iter().any(|m| matches!(m, Message::Fuite(_))), "fuite");
        assert!(matches!(msgs.last().unwrap(), Message::Fin { reussi: true, .. }), "{:?}", msgs.last());
        let bsp = std::fs::read(d.join("hm_essai_zone.bsp")).unwrap();
        let o18 = 8 + 18 * 16;
        let brosses = u32::from_le_bytes(bsp[o18 + 4..o18 + 8].try_into().unwrap()) / 12;
        assert_eq!(brosses, 12, "6 brosses de la salle + 6 murs du cordon");
        // le cordon de la map n'a pas bougé
        assert!(s.doc().cordons().boites_actives().is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn contenu_embarque_relu_dans_le_bsp() {
        // une pièce compilée, deux fichiers embarqués par bspzip : relus dans le pakfile du BSP, octet pour octet
        let Some(o) = Outils::detecter_sans_choix() else { return };
        let d = std::env::temp_dir().join(format!("hm_compile_pak_{}", std::process::id()));
        let fichiers = vec![
            ("materials/hm_essai/rouge.vmt".to_string(), b"\"LightmappedGeneric\"\n{\n\t\"$basetexture\" \"hm_essai/rouge\"\n}\n".to_vec()),
            ("materials/hm_essai/rouge.vtf".to_string(), (0..=255u8).cycle().take(5000).collect()),
        ];
        let c = lancer_avec(piece(false), "hm_essai_pak", &d, &o, Mode::Geometrie, false, fichiers.clone(), Portee::Tout);
        let msgs = attendre(&c);
        assert!(matches!(msgs.last().unwrap(), Message::Fin { reussi: true, .. }), "{:?}", msgs.last());
        let mut vfs = hm_assets::Vfs::new();
        vfs.add_bsp_pak(d.join("hm_essai_pak.bsp")).unwrap();
        for (rel, octets) in &fichiers {
            assert_eq!(vfs.read(rel).map(|f| f.bytes).as_ref(), Some(octets), "{rel}");
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn portails_lus_apres_vbsp() {
        // une pièce coupée en deux par un mur percé d'une porte : vbsp écrit des portails, dont un dans la porte
        let Some(o) = Outils::detecter_sans_choix() else { return };
        let d = std::env::temp_dir().join(format!("hm_compile_portails_{}", std::process::id()));
        let mut s = Session::nouvelle();
        salle(&mut s, 0.0, 256.0, 256.0, "DEV/DEV_MEASUREGENERIC01");
        let m = "DEV/DEV_MEASUREGENERIC01".to_string();
        s.executer(Commande::CreerBloc { min: [-8.0, -256.0, 0.0], max: [8.0, -32.0, 256.0], materiau: m.clone(), classe: None }).unwrap();
        s.executer(Commande::CreerBloc { min: [-8.0, 32.0, 0.0], max: [8.0, 256.0, 256.0], materiau: m.clone(), classe: None }).unwrap();
        s.executer(Commande::CreerBloc { min: [-8.0, -32.0, 128.0], max: [8.0, 32.0, 256.0], materiau: m, classe: None }).unwrap();
        s.executer(Commande::CreerEntite { classe: "info_player_start".into(), origine: [-100.0, 0.0, 1.0], cles: vec![] }).unwrap();
        let c = lancer(s.doc().ecrire(), "hm_essai_portails", &d, &o, Mode::Geometrie, false);
        assert!(matches!(attendre(&c).last().unwrap(), Message::Fin { reussi: true, .. }));
        let prt = d.join("hm_essai_portails.prt");
        let annonce: usize = std::fs::read_to_string(&prt).unwrap().lines().nth(2).unwrap().trim().parse().unwrap();
        let p = lire_portails(&prt).expect("portails");
        assert_eq!(p.len(), annonce);
        assert!(p.iter().all(|x| x.len() >= 3));
        // un portail dans la porte : x entre -8 et 8, y entre -32 et 32, sous le linteau
        assert!(p.iter().any(|x| x.iter().all(|q| q[0].abs() <= 8.0 && q[1].abs() <= 32.0 && (0.0..=128.0).contains(&q[2]))), "{p:?}");
        assert!(lire_portails(&d.join("absent.prt")).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn eclairage_compare_a_vrad() {
        // Six salles fermées, une sorte de lumière dans chacune ; vrad sans rebond ni suréchantillonnage ; chaque luxel
        // du sol lu dans le BSP est comparé à la formule de l'aperçu (`hm_doc::lumiere`), loin des murs.
        let Some(o) = Outils::detecter_sans_choix() else { return };
        let d = std::env::temp_dir().join(format!("hm_compile_lumiere_{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let mut s = Session::nouvelle();
        let m = "DEV/DEV_MEASUREGENERIC01";
        let lum = |s: &mut Session, classe: &str, o: [f64; 3], cles: &[(&str, &str)]| {
            s.executer(Commande::CreerEntite { classe: classe.into(), origine: o, cles: cles.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect() }).unwrap();
        };
        // A : quadratique (les valeurs du FGD)
        salle(&mut s, 0.0, 256.0, 256.0, m);
        lum(&mut s, "light", [0.0, 0.0, 200.0], &[("_light", "255 255 255 200"), ("_constant_attn", "0"), ("_linear_attn", "0"), ("_quadratic_attn", "1")]);
        // B : sans clé d'atténuation (vrad prend alors une constante)
        salle(&mut s, 1024.0, 256.0, 256.0, m);
        lum(&mut s, "light", [1024.0, 0.0, 128.0], &[("_light", "255 200 120 150")]);
        // C : projecteur vers le bas
        salle(&mut s, 2048.0, 256.0, 256.0, m);
        lum(&mut s, "light_spot", [2048.0, 0.0, 240.0], &[("_light", "255 255 255 400"), ("pitch", "-90"), ("angles", "0 0 0"), ("_inner_cone", "20"), ("_cone", "40"), ("_exponent", "1"), ("_constant_attn", "0"), ("_linear_attn", "0"), ("_quadratic_attn", "1")]);
        // D : distances 50 % et 0 %
        salle(&mut s, 3072.0, 256.0, 256.0, m);
        lum(&mut s, "light", [3072.0, 0.0, 64.0], &[("_light", "255 255 255 300"), ("_fifty_percent_distance", "128"), ("_zero_percent_distance", "400")]);
        // F : linéaire
        salle(&mut s, 4096.0, 256.0, 256.0, m);
        lum(&mut s, "light", [4096.0, 0.0, 200.0], &[("_light", "120 255 120 200"), ("_constant_attn", "0"), ("_linear_attn", "1"), ("_quadratic_attn", "0")]);
        // E : à ciel ouvert, large et basse (le ciel se voit presque en entier depuis le milieu)
        salle(&mut s, 7000.0, 1024.0, 64.0, "TOOLS/TOOLSSKYBOX");
        lum(&mut s, "light_environment", [7000.0, 0.0, 32.0], &[("angles", "0 30 0"), ("pitch", "-60"), ("_light", "255 240 200 200"), ("_ambient", "120 140 255 60")]);
        // G : une plaque dont le dessus est un displacement, entre la lumière et le sol (vbsp retire la brosse) :
        // fait-elle de l'ombre ?
        salle(&mut s, 10000.0, 256.0, 256.0, m);
        lum(&mut s, "light", [10000.0, 0.0, 200.0], &[("_light", "255 255 255 200"), ("_constant_attn", "0"), ("_linear_attn", "0"), ("_quadratic_attn", "1")]);
        let plaque = match s.executer(Commande::CreerBloc { min: [10000.0 - 96.0, -96.0, 96.0], max: [10000.0 + 96.0, 96.0, 112.0], materiau: m.into(), classe: None }).unwrap().crees[0] {
            hm_doc::Ref::Solide(i) => i,
            _ => unreachable!(),
        };
        let dessus = s.doc().solide(plaque).unwrap().faces.iter().find(|f| f.normale().z > 0.99).unwrap().id;
        s.executer(Commande::CreerDisp { faces: vec![hm_doc::RefFace { solide: plaque, face: dessus }], puissance: 2 }).unwrap();
        for x in [0.0, 1024.0, 2048.0, 3072.0, 4096.0, 7000.0, 10000.0] {
            lum(&mut s, "info_player_start", [x + 200.0, 200.0, 1.0], &[]);
        }
        let (lumieres, soleil) = s.doc().lumieres();
        assert_eq!(lumieres.len(), 6);
        let soleil = soleil.expect("le soleil");
        std::fs::write(d.join("hm_essai_lumiere.vmf"), s.doc().ecrire()).unwrap();
        let jeu = o.dossier_jeu.display().to_string();
        for (exe, args) in [(&o.vbsp, vec!["-game", &jeu, "hm_essai_lumiere"]), (&o.vvis, vec!["-fast", "-game", &jeu, "hm_essai_lumiere"]), (&o.vrad, vec!["-bounce", "0", "-noextra", "-game", &jeu, "hm_essai_lumiere"])] {
            let st = std::process::Command::new(exe).args(&args).current_dir(&d).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().unwrap();
            assert!(st.success(), "{} a échoué", exe.display());
        }
        let bsp = std::fs::read(d.join("hm_essai_lumiere.bsp")).unwrap();
        let bloc = |i: usize| {
            let o = 8 + i * 16;
            let (a, l) = (u32::from_le_bytes(bsp[o..o + 4].try_into().unwrap()) as usize, u32::from_le_bytes(bsp[o + 4..o + 8].try_into().unwrap()) as usize);
            &bsp[a..a + l]
        };
        let (plans, faces, texinfo, lumiere) = (bloc(1), bloc(7), bloc(6), bloc(8));
        let f32_ = |b: &[u8], o: usize| f32::from_le_bytes(b[o..o + 4].try_into().unwrap()) as f64;
        let i32_ = |b: &[u8], o: usize| i32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        // par salle : (mesuré, attendu) de chaque luxel du sol
        let salles = [(0.0, 256.0), (1024.0, 256.0), (2048.0, 256.0), (3072.0, 256.0), (4096.0, 256.0), (7000.0, 1024.0)];
        let mut mesures: Vec<Vec<(glam::DVec3, glam::DVec3, glam::DVec3)>> = vec![Vec::new(); salles.len()];
        // sous la plaque de G : (mesuré, attendu sans ombre)
        let mut sous_plaque: Vec<(f64, f64)> = Vec::new();
        for f in faces.chunks_exact(56) {
            let plan = u16::from_le_bytes([f[0], f[1]]) as usize;
            let cote = f[2];
            let ti = i16::from_le_bytes([f[10], f[11]]);
            let ofs = i32_(f, 20);
            if ofs < 0 || ti < 0 || f[16] != 0 {
                continue;
            }
            let p = &plans[plan * 20..plan * 20 + 20];
            let (mut nz, mut dist) = (f32_(p, 8), f32_(p, 12));
            if cote != 0 {
                nz = -nz;
                dist = -dist;
            }
            if nz < 0.99 || dist.abs() > 0.01 {
                continue;
            }
            let t = &texinfo[ti as usize * 72..ti as usize * 72 + 72];
            let (sv, tv) = ([f32_(t, 32), f32_(t, 36), f32_(t, 44)], [f32_(t, 48), f32_(t, 52), f32_(t, 60)]);
            let (mins, taille) = ((i32_(f, 28), i32_(f, 32)), (i32_(f, 36), i32_(f, 40)));
            for j in 0..=taille.1 {
                for i in 0..=taille.0 {
                    let k = ofs as usize + 4 * ((j * (taille.0 + 1) + i) as usize);
                    let c = &lumiere[k..k + 4];
                    let e = 2f64.powi(c[3] as i8 as i32);
                    let mesure = glam::DVec3::new(c[0] as f64, c[1] as f64, c[2] as f64) * e / 255.0;
                    // le point du sol (z = 0) de coordonnées de lightmap (s, t)
                    let (s_, t_) = ((mins.0 + i) as f64 - sv[2], (mins.1 + j) as f64 - tv[2]);
                    let det = sv[0] * tv[1] - sv[1] * tv[0];
                    let (x, y) = ((s_ * tv[1] - t_ * sv[1]) / det, (sv[0] * t_ - tv[0] * s_) / det);
                    let pt = glam::DVec3::new(x, y, 0.0);
                    if (x - 10000.0).abs() < 32.0 && y.abs() < 32.0 {
                        sous_plaque.push((mesure.x, lumieres[5].eclairement(pt, glam::DVec3::Z).x));
                    }
                    let Some(n) =salles.iter().position(|(x0, demi)| (x - x0).abs() < demi - 48.0 && y.abs() < demi - 48.0) else { continue };
                    let attendu = if n == 5 {
                        soleil.eclairement(glam::DVec3::Z) + soleil.ambiance
                    } else {
                        lumieres.iter().filter(|l| (l.position.x - salles[n].0).abs() < 300.0).map(|l| l.eclairement(pt, glam::DVec3::Z)).sum()
                    };
                    mesures[n].push((pt, mesure, attendu));
                }
            }
        }
        let noms = ["quadratique", "constante", "projecteur", "50 % / 0 %", "linéaire", "soleil + ciel"];
        let mut rapport = String::new();
        let mut pire = 0.0f64;
        for (n, v) in mesures.iter().enumerate() {
            assert!(v.len() > 50, "{} : {} luxels seulement", noms[n], v.len());
            // écart relatif à la plus forte valeur attendue de la salle (les zones sombres ne gonflent pas l'écart)
            let haut = v.iter().map(|(_, _, a)| a.max_element()).fold(0.0, f64::max).max(1e-3);
            let ecarts: Vec<f64> = v.iter().map(|(_, m, a)| (*m - *a).abs().max_element() / haut).collect();
            let moyen = ecarts.iter().sum::<f64>() / ecarts.len() as f64;
            let max = ecarts.iter().cloned().fold(0.0, f64::max);
            let rapport_moyen = v.iter().filter(|(_, _, a)| a.max_element() > 0.05 * haut).map(|(_, m, a)| m.element_sum() / a.element_sum().max(1e-9)).sum::<f64>() / v.iter().filter(|(_, _, a)| a.max_element() > 0.05 * haut).count().max(1) as f64;
            let (p, mm, aa) = v.iter().max_by(|a, b| a.2.max_element().total_cmp(&b.2.max_element())).unwrap();
            rapport += &format!("{:14} {:5} luxels  écart moyen {:5.1} %  max {:5.1} %  mesuré/attendu {:.3}  (au plus fort {:?} : mesuré {:.3?} attendu {:.3?})\n", noms[n], v.len(), moyen * 100.0, max * 100.0, rapport_moyen, p.truncate(), mm.to_array(), aa.to_array());
            pire = pire.max(moyen);
        }
        let (m_, a_) = sous_plaque.iter().fold((0.0, 0.0), |(a, b), (m, e)| (a + m, b + e));
        let noir = sous_plaque.iter().filter(|(m, _)| *m < 0.005).count();
        rapport += &format!("sous un displacement : {} luxels, {noir} dans le noir, moyenne {:.3} pour {:.3} sans ombre\n", sous_plaque.len(), m_ / sous_plaque.len().max(1) as f64, a_ / sous_plaque.len().max(1) as f64);
        println!("{rapport}");
        std::fs::write(std::env::temp_dir().join("hm_eclairage_vrad.txt"), &rapport).unwrap();
        assert!(pire < 0.03, "l'aperçu s'écarte de vrad :\n{rapport}");
        // un displacement fait de l'ombre dans vrad (sa brosse, elle, est retirée par vbsp) : l'aperçu l'ombre aussi
        assert!(noir > 0, "aucun luxel dans l'ombre du displacement :\n{rapport}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn instance_compilee_par_vbsp() {
        // une pièce fermée et un func_instance qui pointe vers un VMF voisin (deux blocs) : vbsp doit les fondre dans
        // la map (même nombre de brosses qu'en les mettant à la main)
        let Some(o) = Outils::detecter_sans_choix() else { return };
        let d = std::env::temp_dir().join(format!("hm_compile_instance_{}", std::process::id()));
        std::fs::create_dir_all(d.join("instances")).unwrap();
        let mut inst = Session::nouvelle();
        let m = "DEV/DEV_MEASUREGENERIC01".to_string();
        inst.executer(Commande::CreerBloc { min: [-32.0, -32.0, 0.0], max: [32.0, 32.0, 64.0], materiau: m.clone(), classe: None }).unwrap();
        inst.executer(Commande::CreerBloc { min: [-16.0, -16.0, 64.0], max: [16.0, 16.0, 96.0], materiau: m.clone(), classe: None }).unwrap();
        std::fs::write(d.join("instances/caisse.vmf"), inst.doc().ecrire()).unwrap();
        let piece = |avec: bool| {
            let mut s = Session::nouvelle();
            let b = s.executer(Commande::CreerBloc { min: [-512.0, -512.0, -16.0], max: [512.0, 512.0, 272.0], materiau: m.clone(), classe: None }).unwrap().crees;
            s.executer(Commande::Evider { objets: b, epaisseur: 16.0 }).unwrap();
            s.executer(Commande::CreerEntite { classe: "info_player_start".into(), origine: [0.0, 300.0, 1.0], cles: vec![] }).unwrap();
            if avec {
                s.executer(Commande::CreerEntite { classe: "func_instance".into(), origine: [128.0, 0.0, 0.0], cles: vec![("file".into(), "instances/caisse.vmf".into()), ("angles".into(), "0 45 0".into())] }).unwrap();
            }
            // comme la fenêtre : la map est dans `d`, la compilation dans son sous-dossier .hammer-master/compile, le
            // chemin relatif de l'instance est refait depuis là
            s.doc().pour_compilation_dans(&o.texture_cordon, Some(&d.join("carte.vmf")), &dossier_compile(Some(&d.join("carte.vmf"))))
        };
        let brosses = |nom: &str, vmf: Vec<u8>| -> u64 {
            let dc = dossier_compile(Some(&d.join("carte.vmf")));
            let c = lancer(vmf, nom, &dc, &o, Mode::Geometrie, false);
            let msgs = attendre(&c);
            assert!(matches!(msgs.last().unwrap(), Message::Fin { reussi: true, .. }), "{nom} : compilation ratée");
            let bsp = std::fs::read(dc.join(format!("{nom}.bsp"))).unwrap();
            let o = 8 + 18 * 16;
            u32::from_le_bytes(bsp[o + 4..o + 8].try_into().unwrap()) as u64 / 12
        };
        let sans = brosses("hm_essai_sans_instance", piece(false));
        let avec = brosses("hm_essai_instance", piece(true));
        assert_eq!(avec, sans + 2, "les deux blocs de l'instance dans le BSP ({sans} sans, {avec} avec)");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn compiler_une_map_cordonnee() {
        // un sol seul et un point d'apparition : fuite sans cordon ; avec un cordon autour, les six murs de
        // `CordonTexture` ferment la map et un bloc lointain, hors de la boîte, n'est pas compilé
        let Some(o) = Outils::detecter_sans_choix() else { return };
        let d = std::env::temp_dir().join(format!("hm_compile_cordon_{}", std::process::id()));
        let mut s = Session::nouvelle();
        let m = "DEV/DEV_MEASUREGENERIC01".to_string();
        s.executer(Commande::CreerBloc { min: [-256.0, -256.0, -16.0], max: [256.0, 256.0, 0.0], materiau: m.clone(), classe: None }).unwrap();
        s.executer(Commande::CreerBloc { min: [4000.0, 4000.0, 0.0], max: [4064.0, 4064.0, 64.0], materiau: m, classe: None }).unwrap();
        s.executer(Commande::CreerEntite { classe: "info_player_start".into(), origine: [0.0, 0.0, 1.0], cles: vec![] }).unwrap();
        let c = lancer(s.doc().pour_compilation(&o.texture_cordon), "hm_essai_sans_cordon", &d, &o, Mode::Geometrie, false);
        assert!(attendre(&c).iter().any(|m| matches!(m, Message::Fuite(_))), "sans cordon, la map devrait fuir");

        s.executer(Commande::Cordon { min: [-300.0, -300.0, -32.0], max: [300.0, 300.0, 256.0], actif: true }).unwrap();
        let c = lancer(s.doc().pour_compilation(&o.texture_cordon), "hm_essai_cordon", &d, &o, Mode::Geometrie, false);
        let msgs = attendre(&c);
        let journal: Vec<String> = msgs.iter().filter_map(|m| if let Message::Ligne(l) = m { Some(l.clone()) } else { None }).collect();
        assert!(!msgs.iter().any(|m| matches!(m, Message::Fuite(_))), "fuite malgré le cordon");
        assert!(matches!(msgs.last().unwrap(), Message::Fin { reussi: true, .. }), "{}", journal.iter().rev().take(30).cloned().collect::<Vec<_>>().join("\n"));
        assert!(d.join("hm_essai_cordon.bsp").is_file());

        // un second cordon autour du bloc lointain : ses six murs à lui, toujours sans fuite
        let mut liste = s.doc().cordons().liste;
        liste.push(hm_doc::Cordon { nom: "loin".into(), actif: true, boites: vec![[[3900.0, 3900.0, -64.0], [4200.0, 4200.0, 200.0]]] });
        s.executer(Commande::Cordons { actif: true, liste }).unwrap();
        let c = lancer(s.doc().pour_compilation(&o.texture_cordon), "hm_essai_deux_cordons", &d, &o, Mode::Geometrie, false);
        let msgs = attendre(&c);
        let journal: Vec<String> = msgs.iter().filter_map(|m| if let Message::Ligne(l) = m { Some(l.clone()) } else { None }).collect();
        assert!(!msgs.iter().any(|m| matches!(m, Message::Fuite(_))), "fuite avec deux cordons");
        assert!(matches!(msgs.last().unwrap(), Message::Fin { reussi: true, .. }), "{}", journal.iter().rev().take(30).cloned().collect::<Vec<_>>().join("
"));
        assert!(d.join("hm_essai_deux_cordons.bsp").is_file());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn choix_des_executables() {
        let d = std::env::temp_dir().join(format!("hm_choix_{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let vvispp = d.join("vvis++.exe");
        std::fs::write(&vvispp, b"").unwrap();
        let defaut = Outils { vbsp: "vbsp.exe".into(), vvis: "vvis.exe".into(), vrad: "vrad.exe".into(), ..Default::default() };
        let mut o = defaut.clone();
        // un chemin qui n'existe pas est ignoré, une clé inconnue aussi
        o.appliquer_choix(&format!("vvis = {}\nvrad = C:/n/existe/pas.exe\nautre = x\n", vvispp.display()));
        assert_eq!((o.vbsp.clone(), o.vvis.clone(), o.vrad.clone()), (PathBuf::from("vbsp.exe"), vvispp.clone(), PathBuf::from("vrad.exe")));
        // aller-retour : seul ce qui diffère du défaut est écrit
        let t = o.texte_choix(&defaut);
        assert_eq!(t, format!("vvis = {}\n", vvispp.display()));
        let mut relu = defaut.clone();
        relu.appliquer_choix(&t);
        assert_eq!(relu.vvis, vvispp);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// La pièce fermée, un baril en prop_static et la session (pour la modifier en gardant les numéros).
    fn piece_session() -> Session {
        let mut s = Session::nouvelle();
        for (lo, hi) in [
            ([-256.0, -256.0, -16.0], [256.0, 256.0, 0.0]),
            ([-256.0, -256.0, 256.0], [256.0, 256.0, 272.0]),
            ([-272.0, -256.0, 0.0], [-256.0, 256.0, 256.0]),
            ([256.0, -256.0, 0.0], [272.0, 256.0, 256.0]),
            ([-272.0, -272.0, 0.0], [272.0, -256.0, 256.0]),
            ([-272.0, 256.0, 0.0], [272.0, 272.0, 256.0]),
        ] {
            s.executer(Commande::CreerBloc { min: lo, max: hi, materiau: "DEV/DEV_MEASUREGENERIC01".into(), classe: None }).unwrap();
        }
        for (classe, o, cles) in [
            ("info_player_start", [0.0, 0.0, 1.0], vec![]),
            ("light", [0.0, 0.0, 200.0], vec![("_light".to_string(), "255 255 255 200".to_string())]),
            ("prop_static", [128.0, 128.0, 0.0], vec![("model".to_string(), "models/props_c17/oildrum001.mdl".to_string())]),
        ] {
            s.executer(Commande::CreerEntite { classe: classe.into(), origine: o, cles }).unwrap();
        }
        s
    }

    fn entite(s: &Session, classe: &str) -> hm_doc::Ref {
        hm_doc::Ref::Entite(s.doc().entites().find(|e| e.classe() == classe).unwrap().id)
    }

    #[test]
    fn portee_selon_ce_qui_a_change() {
        let base = piece_session();
        let avant = base.doc().ecrire();
        let apres = |f: &dyn Fn(&mut Session)| {
            let mut s = Session::depuis_octets(&avant, None).unwrap();
            f(&mut s);
            portee(&avant, &s.doc().ecrire())
        };
        let bouger = |classe: &'static str| move |s: &mut Session| {
            let e = entite(s, classe);
            s.executer(Commande::Deplacer { objets: vec![e], delta: [32.0, 0.0, 0.0], verrou_texture: true }).unwrap();
        };
        assert_eq!(apres(&|_| {}), Portee::Rien);
        assert_eq!(apres(&bouger("info_player_start")), Portee::Entites);
        assert_eq!(apres(&bouger("light")), Portee::Lumieres);
        assert_eq!(apres(&bouger("prop_static")), Portee::Tout);
        assert_eq!(apres(&|s| { s.executer(Commande::CreerBloc { min: [0.0, 0.0, 0.0], max: [32.0, 32.0, 32.0], materiau: "DEV/DEV_MEASUREGENERIC01".into(), classe: None }).unwrap(); }), Portee::Tout);
        assert_eq!(apres(&|s| { let w = s.doc().monde().id; s.executer(Commande::Cles { entite: w, cles: vec![("skyname".into(), Some("painted".into()))] }).unwrap(); }), Portee::Tout);
        assert_eq!(apres(&|s| { s.executer(Commande::CreerEntite { classe: "func_button".into(), origine: [0.0; 3], cles: vec![] }).unwrap(); bouger("info_player_start")(s); }), Portee::Entites);
        // la couleur d'un objet dans l'éditeur ne change rien
        let teinte = String::from_utf8(avant.clone()).unwrap().replacen("\"color\"", "\"color\" \"1 2 3\"\n\t\t\t\"x_color_ancienne\"", 1);
        assert_eq!(portee(&avant, teinte.as_bytes()), Portee::Rien);
    }

    /// Le BSP reprise par une compilation partielle : visibilité et lumière gardées, entités à jour.
    #[test]
    fn compilation_partielle_avec_les_outils_du_jeu() {
        let Some(o) = Outils::detecter_sans_choix() else { return };
        let d = std::env::temp_dir().join(format!("hm_partiel_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let lump = |n: usize| {
            let b = std::fs::read(d.join("hm_partiel.bsp")).unwrap();
            let (off, lon) = (i32::from_le_bytes(b[8 + n * 16..12 + n * 16].try_into().unwrap()) as usize, i32::from_le_bytes(b[12 + n * 16..16 + n * 16].try_into().unwrap()) as usize);
            b[off..off + lon].to_vec()
        };
        let mut s = piece_session();
        let fin = |c: &Compilation| attendre(c).into_iter().any(|m| matches!(m, Message::Fin { reussi: true, .. }));
        assert!(fin(&lancer(s.doc().ecrire(), "hm_partiel", &d, &o, Mode::Rapide, false)), "compilation complète");
        let (ents0, vis0, lum0) = (lump(0), lump(4), lump(8));
        // le point d'apparition déplacé : vbsp -onlyents seul
        let e = entite(&s, "info_player_start");
        s.executer(Commande::Deplacer { objets: vec![e], delta: [64.0, 0.0, 0.0], verrou_texture: true }).unwrap();
        let avant = precedente(&d, "hm_partiel", Mode::Rapide).expect("la compilation précédente est reprise");
        assert!(precedente(&d, "hm_partiel", Mode::Normal).is_none(), "pas dans un autre mode");
        let vmf = s.doc().ecrire();
        assert_eq!(portee(&avant, &vmf), Portee::Entites);
        assert!(fin(&lancer_avec(vmf, "hm_partiel", &d, &o, Mode::Rapide, false, Vec::new(), Portee::Entites)), "-onlyents");
        let (ents1, vis1, lum1) = (lump(0), lump(4), lump(8));
        assert!(ents1 != ents0 && vis1 == vis0 && lum1 == lum0, "entités à jour, visibilité et lumière gardées");
        // la lumière déplacée : -onlyents puis vrad
        let e = entite(&s, "light");
        s.executer(Commande::Deplacer { objets: vec![e], delta: [-128.0, 0.0, 0.0], verrou_texture: true }).unwrap();
        let vmf = s.doc().ecrire();
        assert_eq!(portee(&precedente(&d, "hm_partiel", Mode::Rapide).unwrap(), &vmf), Portee::Lumieres);
        assert!(fin(&lancer_avec(vmf, "hm_partiel", &d, &o, Mode::Rapide, false, Vec::new(), Portee::Lumieres)), "-onlyents et vrad");
        assert!(lump(0) != ents1 && lump(4) == vis0 && lump(8) != lum1, "lumière refaite, visibilité gardée");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn commande_du_jeu() {
        let o = Outils { jeu: "gmod.exe".into(), dossier_jeu: "C:/jeu/garrysmod".into(), ..Default::default() };
        let (exe, args) = o.commande_jeu();
        assert_eq!(exe, PathBuf::from("gmod.exe"));
        assert_eq!(args, vec!["-dev", "-console", "-allowdebug", "-condebug", "-game", "C:/jeu/garrysmod"]);
    }

    #[test]
    fn menu_du_jeu_vu_dans_la_console() {
        // lignes de la console de GMod du 14/11 : le premier montage (0 sur 41) n'est pas encore le menu
        assert!(!menu_affiche("Mounted 0 of 41 workshop addons!\nMounting game 'Half-Life 2 & Episodes' (hl2, 220)...\nAddons have changes - remounting"));
        assert!(menu_affiche("Addons have changes - remounting\r\nMounted 41 of 41 workshop addons!\r\n"));
        assert!(menu_affiche("Mounted 0 of 0 workshop addons!"));
    }
}
