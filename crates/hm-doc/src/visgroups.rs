//! Visgroups (groupes de visibilité) et cordon.
//!
//! Format relevé dans les VMF de Hammer++ (14/11/2026) : bloc de tête `visgroups { visgroup { "name" "visgroupid"
//! "color" visgroup { … } } }` ; un objet en fait partie par une clé `visgroupid` (une par groupe) de son bloc
//! `editor`, avec `visgroupshown`. Un visgroup décoché ne se compile pas (wiki Valve, « Grouping and VisGrouping ») :
//! ses objets sont rangés dans des blocs `hidden`, que vbsp ignore. Cordons (Hammer++, une liste reprise de Hammer
//! de CS:GO) : `cordons { "active" cordon { "name" "active" box { "mins" "(x y z)" "maxs" "(x y z)" } … } … }` ;
//! le Hammer d'origine écrit `cordon { "mins" "maxs" "active" }` (relevé dans rp_rockford_v2b et les maps de TTT).
use glam::DVec3;
use serde::{Deserialize, Serialize};

use crate::kv::{nombre, nombres, Bloc, Element};
use crate::modele::{Document, Proprio, Solide};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Visgroup {
    pub id: u64,
    pub nom: String,
    pub couleur: [u8; 3],
    /// 0 : à la racine.
    pub parent: u64,
}

/// Les visgroups d'un bloc `visgroups`, à plat (parents avant enfants).
pub fn lire(b: Option<&Bloc>) -> Vec<Visgroup> {
    fn parcourir(b: &Bloc, parent: u64, out: &mut Vec<Visgroup>) {
        for v in b.enfants().filter(|c| c.nom.eq_ignore_ascii_case("visgroup")) {
            let id = v.get("visgroupid").and_then(|x| x.trim().parse().ok()).unwrap_or(0);
            let c = v.get("color").map(nombres).filter(|n| n.len() >= 3).map(|n| [n[0] as u8, n[1] as u8, n[2] as u8]).unwrap_or([255, 255, 255]);
            out.push(Visgroup { id, nom: v.get("name").unwrap_or("").to_string(), couleur: c, parent });
            parcourir(v, id, out);
        }
    }
    let mut out = Vec::new();
    if let Some(b) = b {
        parcourir(b, 0, &mut out);
    }
    out
}

/// Les visgroups dont un objet fait partie (clés `visgroupid` de son bloc `editor`).
pub fn de(editeur: &Bloc) -> Vec<u64> {
    editeur.paires().filter(|(k, _)| k.eq_ignore_ascii_case("visgroupid")).filter_map(|(_, v)| v.trim().parse().ok()).collect()
}

pub fn ajouter(editeur: &mut Bloc, id: u64) {
    if de(editeur).contains(&id) {
        return;
    }
    // après la dernière clé visgroupid, sinon avant visgroupshown (l'ordre de Hammer)
    let pos = editeur
        .elements
        .iter()
        .rposition(|e| matches!(e, Element::Paire(k, _) if k.eq_ignore_ascii_case("visgroupid")))
        .map(|i| i + 1)
        .or_else(|| editeur.elements.iter().position(|e| matches!(e, Element::Paire(k, _) if k.eq_ignore_ascii_case("visgroupshown"))))
        .unwrap_or(editeur.elements.len());
    editeur.elements.insert(pos, Element::Paire("visgroupid".into(), id.to_string()));
}

pub fn retirer(editeur: &mut Bloc, id: u64) {
    editeur.elements.retain(|e| !matches!(e, Element::Paire(k, v) if k.eq_ignore_ascii_case("visgroupid") && v.trim().parse::<u64>().ok() == Some(id)));
}

/// Ajoute un visgroup au bloc `visgroups` (à la racine, ou sous `parent`).
pub fn creer(b: &mut Bloc, v: &Visgroup) {
    let mut nv = Bloc::nouveau("visgroup");
    nv.elements.push(Element::Paire("name".into(), v.nom.clone()));
    nv.elements.push(Element::Paire("visgroupid".into(), v.id.to_string()));
    nv.elements.push(Element::Paire("color".into(), format!("{} {} {}", v.couleur[0], v.couleur[1], v.couleur[2])));
    fn inserer(b: &mut Bloc, parent: u64, nv: &mut Option<Bloc>) {
        for e in &mut b.elements {
            if let Element::Bloc(c) = e {
                if c.nom.eq_ignore_ascii_case("visgroup") {
                    if c.get("visgroupid").and_then(|x| x.trim().parse::<u64>().ok()) == Some(parent) {
                        if let Some(x) = nv.take() {
                            c.elements.push(Element::Bloc(x));
                        }
                        return;
                    }
                    inserer(c, parent, nv);
                }
            }
        }
    }
    let mut nv = Some(nv);
    if v.parent != 0 {
        inserer(b, v.parent, &mut nv);
    }
    if let Some(x) = nv {
        b.elements.push(Element::Bloc(x));
    }
}

/// Retire un visgroup (et ses enfants) du bloc ; rend les identifiants retirés.
pub fn supprimer(b: &mut Bloc, id: u64) -> Vec<u64> {
    let mut retires = Vec::new();
    fn ids(b: &Bloc, out: &mut Vec<u64>) {
        if let Some(i) = b.get("visgroupid").and_then(|x| x.trim().parse().ok()) {
            out.push(i);
        }
        for c in b.enfants() {
            ids(c, out);
        }
    }
    fn parcourir(b: &mut Bloc, id: u64, retires: &mut Vec<u64>) {
        let mut garde = Vec::new();
        for e in std::mem::take(&mut b.elements) {
            match e {
                Element::Bloc(c) if c.nom.eq_ignore_ascii_case("visgroup") && c.get("visgroupid").and_then(|x| x.trim().parse::<u64>().ok()) == Some(id) => ids(&c, retires),
                Element::Bloc(mut c) => {
                    parcourir(&mut c, id, retires);
                    garde.push(Element::Bloc(c));
                }
                autre => garde.push(autre),
            }
        }
        b.elements = garde;
    }
    parcourir(b, id, &mut retires);
    retires
}

/// Monte ou descend un visgroup d'une place parmi ceux de même parent (l'ordre du fichier, donc du panneau).
/// `None` : introuvable ; `Some(false)` : déjà au bout.
pub fn deplacer(b: &mut Bloc, id: u64, monter: bool) -> Option<bool> {
    let est = |e: &Element| matches!(e, Element::Bloc(c) if c.nom.eq_ignore_ascii_case("visgroup"));
    let freres: Vec<usize> = (0..b.elements.len()).filter(|&i| est(&b.elements[i])).collect();
    let k = freres.iter().position(|&i| matches!(&b.elements[i], Element::Bloc(c) if c.get("visgroupid").and_then(|x| x.trim().parse::<u64>().ok()) == Some(id)));
    if let Some(k) = k {
        let j = if monter { k.checked_sub(1) } else { Some(k + 1).filter(|&j| j < freres.len()) };
        if let Some(j) = j {
            b.elements.swap(freres[k], freres[j]);
        }
        return Some(j.is_some());
    }
    b.elements.iter_mut().find_map(|e| match e {
        Element::Bloc(c) if c.nom.eq_ignore_ascii_case("visgroup") => deplacer(c, id, monter),
        _ => None,
    })
}

pub fn renommer(b: &mut Bloc, id: u64, nom: &str) -> bool {
    for e in &mut b.elements {
        if let Element::Bloc(c) = e {
            if c.get("visgroupid").and_then(|x| x.trim().parse::<u64>().ok()) == Some(id) {
                c.set("name", nom);
                return true;
            }
            if renommer(c, id, nom) {
                return true;
            }
        }
    }
    false
}

/// Un cordon de la liste (Hammer++, repris de Hammer de CS:GO) : un nom, coché ou non, ses boîtes `[min, max]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Cordon {
    pub nom: String,
    pub actif: bool,
    pub boites: Vec<[[f64; 3]; 2]>,
}

/// Les cordons de la map : l'état du cordon (« Toggle cordon state ») et la liste.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Cordons {
    pub actif: bool,
    pub liste: Vec<Cordon>,
}

impl Cordons {
    /// Les boîtes qui limitent la compilation, en (min, max) : celles des cordons cochés, quand le cordon est actif.
    pub fn boites_actives(&self) -> Vec<(DVec3, DVec3)> {
        if !self.actif {
            return Vec::new();
        }
        self.liste.iter().filter(|c| c.actif).flat_map(|c| &c.boites).map(|[a, b]| (DVec3::from(*a).min(DVec3::from(*b)), DVec3::from(*a).max(DVec3::from(*b)))).collect()
    }
}

/// La boîte `a`–`b` est au moins en partie dans une des `boites` (ce qui est compilé sous un cordon actif).
pub fn touche(boites: &[(DVec3, DVec3)], a: DVec3, b: DVec3) -> bool {
    boites.iter().any(|(lo, hi)| a.cmple(*hi).all() && b.cmpge(*lo).all())
}

fn boite_lue(b: &Bloc) -> Option<[[f64; 3]; 2]> {
    let (lo, hi) = (nombres(b.get("mins")?), nombres(b.get("maxs")?));
    (lo.len() == 3 && hi.len() == 3).then(|| [[lo[0], lo[1], lo[2]], [hi[0], hi[1], hi[2]]])
}

/// Lit le bloc `cordons` ; vide, le bloc `cordon` du Hammer d'origine (une boîte : `mins`, `maxs`, `active`).
pub fn lire_cordons(cordons: Option<&Bloc>, ancien: Option<&Bloc>) -> Cordons {
    let un = |v: Option<&str>, defaut: bool| v.map_or(defaut, |v| v.trim() == "1");
    match (cordons.filter(|b| !b.elements.is_empty()), ancien) {
        (Some(b), _) => Cordons {
            actif: un(b.get("active"), false),
            liste: b
                .enfants()
                .filter(|c| c.nom.eq_ignore_ascii_case("cordon"))
                .map(|c| Cordon { nom: c.get("name").unwrap_or("").to_string(), actif: un(c.get("active"), true), boites: c.enfants().filter(|x| x.nom.eq_ignore_ascii_case("box")).filter_map(boite_lue).collect() })
                .collect(),
        },
        (None, Some(v)) => Cordons { actif: un(v.get("active"), false), liste: boite_lue(v).map(|x| Cordon { nom: "cordon".into(), actif: true, boites: vec![x] }).into_iter().collect() },
        (None, None) => Cordons::default(),
    }
}

fn texte_point(p: [f64; 3]) -> String {
    format!("({} {} {})", nombre(p[0]), nombre(p[1]), nombre(p[2]))
}

/// Écrit les cordons dans le bloc `cordons` (format de Hammer++), en gardant ses autres clés.
pub fn ecrire_cordons(b: &mut Bloc, c: &Cordons) {
    b.set("active", if c.actif { "1" } else { "0" });
    b.elements.retain(|e| !matches!(e, Element::Bloc(x) if x.nom.eq_ignore_ascii_case("cordon")));
    for k in &c.liste {
        let mut x = Bloc::nouveau("cordon");
        x.set("name", &k.nom);
        x.set("active", if k.actif { "1" } else { "0" });
        for [lo, hi] in &k.boites {
            let mut y = Bloc::nouveau("box");
            y.set("mins", &texte_point(*lo));
            y.set("maxs", &texte_point(*hi));
            x.elements.push(Element::Bloc(y));
        }
        b.elements.push(Element::Bloc(x));
    }
}

/// Garde le bloc `cordon` du Hammer d'origine d'accord avec la liste : la première boîte d'un cordon coché, et l'état.
pub fn ecrire_ancien_cordon(v: &mut Bloc, c: &Cordons) {
    if let Some([lo, hi]) = c.liste.iter().filter(|k| k.actif).chain(&c.liste).find_map(|k| k.boites.first()) {
        v.set("mins", &texte_point(*lo));
        v.set("maxs", &texte_point(*hi));
    }
    v.set("active", if c.actif { "1" } else { "0" });
}

impl Document {
    pub fn visgroups(&self) -> Vec<Visgroup> {
        lire(self.bloc_haut("visgroups"))
    }

    /// Les objets de chaque visgroup (appartenance directe) : solides du monde, entités, et solides d'entités
    /// quand c'est le solide qui porte l'appartenance ; avec, pour chacun, s'il est caché.
    pub fn membres_visgroups(&self) -> std::collections::HashMap<u64, Vec<(crate::Ref, bool)>> {
        let mut m: std::collections::HashMap<u64, Vec<(crate::Ref, bool)>> = std::collections::HashMap::new();
        for (_, s) in self.tous_les_solides() {
            for id in de(&s.editeur) {
                m.entry(id).or_default().push((crate::Ref::Solide(s.id), s.cache));
            }
        }
        for e in self.entites() {
            if let Some(ed) = &e.editeur {
                for id in de(ed) {
                    m.entry(id).or_default().push((crate::Ref::Entite(e.id), e.cache));
                }
            }
        }
        m
    }

    pub fn cordons(&self) -> Cordons {
        lire_cordons(self.bloc_haut("cordons"), self.bloc_haut("cordon"))
    }

    /// Comme `pour_compilation`, pour un VMF écrit dans `dossier` alors que la map est en `carte` : les chemins
    /// relatifs des `func_instance` (relatifs au fichier de la map, wiki Valve) sont refaits depuis `dossier`.
    pub fn pour_compilation_dans(&self, texture_cordon: &str, carte: Option<&std::path::Path>, dossier: &std::path::Path) -> Vec<u8> {
        let Some(base) = carte.and_then(|c| std::path::absolute(c).ok()).and_then(|c| c.parent().map(|p| p.to_path_buf())) else {
            return self.pour_compilation(texture_cordon);
        };
        let dossier = std::path::absolute(dossier).unwrap_or_else(|_| dossier.to_path_buf());
        // chemin de `dossier` vers `base` : remonter jusqu'à l'ancêtre commun, puis descendre
        let a: Vec<_> = base.components().collect();
        let b: Vec<_> = dossier.components().collect();
        let commun = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
        let mut rel = std::path::PathBuf::new();
        for _ in commun..b.len() {
            rel.push("..");
        }
        for c in &a[commun..] {
            rel.push(c.as_os_str());
        }
        let mut d = self.clone();
        for e in d.entites.values_mut() {
            if !e.classe().eq_ignore_ascii_case("func_instance") {
                continue;
            }
            let Some(f) = e.get("file").map(String::from) else { continue };
            if f.trim().is_empty() || std::path::Path::new(&f).is_absolute() {
                continue;
            }
            let p = rel.join(&f);
            e.set("file", &p.to_string_lossy().replace('\\', "/"));
        }
        d.pour_compilation(texture_cordon)
    }

    /// Compilation rapide d'une zone : ce que vbsp doit compiler pour la seule boîte `min`–`max`, comme sous un cordon
    /// actif, sans toucher au cordon de la map. Sans point d'apparition dans la zone, un `info_player_start` est
    /// ajouté à la copie, posé sur le sol au milieu de la zone : vbsp a besoin d'une entité dedans pour savoir où est
    /// l'intérieur.
    pub fn pour_compilation_zone(&self, texture_cordon: &str, carte: Option<&std::path::Path>, dossier: &std::path::Path, min: [f64; 3], max: [f64; 3]) -> Vec<u8> {
        let (lo, hi) = (DVec3::from(min).min(DVec3::from(max)), DVec3::from(min).max(DVec3::from(max)));
        let mut d = self.clone();
        let i = match d.haut.iter().position(|h| matches!(h, crate::modele::Haut::Bloc(b) | crate::modele::Haut::Ajoute(b) if b.nom.eq_ignore_ascii_case("cordons"))) {
            Some(i) => i,
            None => {
                d.haut.push(crate::modele::Haut::Ajoute(Bloc::nouveau("cordons")));
                d.haut.len() - 1
            }
        };
        if let crate::modele::Haut::Bloc(b) | crate::modele::Haut::Ajoute(b) = &mut d.haut[i] {
            ecrire_cordons(b, &Cordons { actif: true, liste: vec![Cordon { nom: "zone".into(), actif: true, boites: vec![[lo.to_array(), hi.to_array()]] }] });
        }
        let dedans = |p: DVec3| p.cmpge(lo).all() && p.cmple(hi).all();
        let a_depart = d.entites.values().any(|e| e.classe().to_ascii_lowercase().starts_with("info_player") && e.origine().map_or(false, dedans));
        let mut vmf = d.pour_compilation_dans(texture_cordon, carte, dossier);
        if !a_depart {
            let c = (lo + hi) * 0.5;
            let haut = DVec3::new(c.x, c.y, hi.z - 1.0);
            let sol = crate::geo::rayon(self, haut, DVec3::NEG_Z, &|_| false).map(|i| i.point.z).filter(|z| *z > lo.z).unwrap_or(c.z);
            let z = (sol + 16.0).min(hi.z - 1.0);
            let e = format!("entity\n{{\n\t\"id\" \"999999999\"\n\t\"classname\" \"info_player_start\"\n\t\"origin\" \"{} {} {}\"\n}}\n", nombre(c.x), nombre(c.y), nombre(z));
            vmf.extend_from_slice(e.as_bytes());
        }
        vmf
    }

    /// Ce que vbsp doit compiler. Sans cordon actif, le fichier tel qu'il est (les objets des visgroups décochés
    /// sont dans des blocs `hidden`, que vbsp ignore). Avec un cordon actif : seulement les objets au moins en
    /// partie dans une boîte d'un cordon coché, chaque boîte fermée par six brosses de `texture_cordon` (wiki
    /// Valve, « Hammer Cordon Usage » : « Only objects that are at least partially within these boundaries will be
    /// compiled »). shortcut: une boîte par six murs, sans union : deux boîtes qui se chevauchent ont des murs
    /// l'une dans l'autre ; le code de Hammer de CS:GO n'est pas public pour vérifier ce qu'il fait.
    pub fn pour_compilation(&self, texture_cordon: &str) -> Vec<u8> {
        let boites = self.cordons().boites_actives();
        if boites.is_empty() {
            return self.ecrire();
        }
        let touche = |a: DVec3, b: DVec3| touche(&boites, a, b);
        let mut d = self.clone();
        let dehors: Vec<u64> = d.solides.values().filter(|s| s.boite().map_or(true, |(a, b)| !touche(a, b))).map(|s| s.id).collect();
        for id in dehors {
            d.solides.shift_remove(&id);
            d.proprio.remove(&id);
        }
        let ents_dehors: Vec<u64> = d
            .entites
            .values()
            .filter(|e| {
                if e.solides.is_empty() {
                    e.origine().map_or(false, |o| !touche(o, o))
                } else {
                    e.solides.iter().all(|s| s.boite().map_or(true, |(a, b)| !touche(a, b)))
                }
            })
            .map(|e| e.id)
            .collect();
        for id in ents_dehors {
            d.entites.shift_remove(&id);
        }
        // six murs de 16 autour de chaque boîte
        let e = 16.0;
        for (lo, hi) in boites.iter().copied() {
            let murs = [
                (DVec3::new(lo.x - e, lo.y - e, lo.z - e), DVec3::new(hi.x + e, hi.y + e, lo.z)),
                (DVec3::new(lo.x - e, lo.y - e, hi.z), DVec3::new(hi.x + e, hi.y + e, hi.z + e)),
                (DVec3::new(lo.x - e, lo.y - e, lo.z), DVec3::new(lo.x, hi.y + e, hi.z)),
                (DVec3::new(hi.x, lo.y - e, lo.z), DVec3::new(hi.x + e, hi.y + e, hi.z)),
                (DVec3::new(lo.x, lo.y - e, lo.z), DVec3::new(hi.x, lo.y, hi.z)),
                (DVec3::new(lo.x, hi.y, lo.z), DVec3::new(hi.x, hi.y + e, hi.z)),
            ];
            for (a, b) in murs {
                let id = d.nouvel_id();
                let f = [d.nouvelle_face(), d.nouvelle_face(), d.nouvelle_face(), d.nouvelle_face(), d.nouvelle_face(), d.nouvelle_face()];
                let s: Solide = crate::geo::boite(id, f, a, b, texture_cordon);
                d.proprio.insert(id, Proprio::Monde);
                d.solides.insert(id, s);
            }
        }
        d.ecrire()
    }
}
