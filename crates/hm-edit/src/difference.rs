//! Différence entre deux versions d'une map : ce qui a été ajouté, retiré, déplacé, sans tenir compte des numéros
//! (deux générations numérotent autrement les mêmes objets).
use std::collections::HashMap;

use anyhow::{anyhow, Result};
use hm_model::Modele;
use hm_vmf::{Node, Vmf};
use serde::Serialize;

use crate::Ent;

/// Clés posées par d'anciennes versions de l'outil, sans effet sur la map.
const MARQUES: [&str; 2] = ["hm_retouche", "hm_zone"];

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Element {
    pub classe: String,
    pub nom: String,
    pub modele: String,
    pub origine: Option<[f64; 3]>,
    pub id: u64,
}

impl Element {
    pub fn decrire(&self) -> String {
        let quoi = if self.modele.is_empty() { self.classe.clone() } else { format!("{} {}", self.classe, self.modele.rsplit('/').next().unwrap_or("").trim_end_matches(".mdl")) };
        let nom = if self.nom.is_empty() { String::new() } else { format!(" « {} »", self.nom) };
        match self.origine {
            Some(o) => format!("{quoi}{nom} en ({:.0} {:.0} {:.0})", o[0], o[1], o[2]),
            None => format!("{quoi}{nom}"),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Difference {
    pub ajoutes: Vec<Element>,
    pub retires: Vec<Element>,
    /// L'élément d'avant, et sa nouvelle place.
    pub deplaces: Vec<(Element, [f64; 3])>,
    /// Même place, d'autres réglages.
    pub modifies: Vec<Element>,
    /// Centres des solides du monde ajoutés et retirés.
    pub solides_ajoutes: Vec<[f64; 3]>,
    pub solides_retires: Vec<[f64; 3]>,
}

impl Difference {
    pub fn vide(&self) -> bool {
        self.ajoutes.is_empty() && self.retires.is_empty() && self.deplaces.is_empty() && self.modifies.is_empty() && self.solides_ajoutes.is_empty() && self.solides_retires.is_empty()
    }

    pub fn resume(&self) -> String {
        if self.vide() {
            return "aucune différence (aux numéros près)".to_string();
        }
        format!(
            "entités : {} ajoutée(s), {} retirée(s), {} déplacée(s), {} modifiée(s) ; solides du monde et du détail : {} ajouté(s), {} retiré(s)",
            self.ajoutes.len(),
            self.retires.len(),
            self.deplaces.len(),
            self.modifies.len(),
            self.solides_ajoutes.len(),
            self.solides_retires.len()
        )
    }

    /// Tous les endroits où quelque chose a changé.
    fn points(&self) -> Vec<[f64; 3]> {
        let mut p: Vec<[f64; 3]> = Vec::new();
        p.extend(self.ajoutes.iter().chain(&self.retires).chain(&self.modifies).filter_map(|e| e.origine));
        for (e, vers) in &self.deplaces {
            p.extend(e.origine);
            p.push(*vers);
        }
        p.extend(self.solides_ajoutes.iter().chain(&self.solides_retires));
        p
    }

    /// Les zones touchées, les plus changées d'abord : la pièce du modèle sémantique qui contient le changement,
    /// sinon un carré de 512 de côté.
    pub fn zones(&self, sem: Option<&Modele>) -> Vec<Zone> {
        let mut zones: Vec<Zone> = Vec::new();
        for p in self.points() {
            let q = [p[0] as f32, p[1] as f32, p[2] as f32];
            let piece = sem.and_then(|m| m.pieces.iter().find(|x| x.contient(q, 8.0)));
            let (nom, min, max) = match piece {
                Some(x) => (format!("pièce « {} » ({})", x.id, x.role), [x.emprise[0], x.emprise[1], x.sol], [x.emprise[2], x.emprise[3], x.sol + x.hauteur.unwrap_or(128.0)]),
                None => {
                    let (cx, cy) = ((q[0] / 512.0).floor() * 512.0, (q[1] / 512.0).floor() * 512.0);
                    (format!("carré ({cx:.0} {cy:.0})"), [cx, cy, q[2] - 16.0], [cx + 512.0, cy + 512.0, q[2] + 128.0])
                }
            };
            match zones.iter_mut().find(|z| z.nom == nom) {
                Some(z) => {
                    z.n += 1;
                    if piece.is_none() {
                        z.min[2] = z.min[2].min(q[2] - 16.0);
                        z.max[2] = z.max[2].max(q[2] + 128.0);
                    }
                }
                None => zones.push(Zone { nom, min, max, n: 1 }),
            }
        }
        zones.sort_by(|a, b| b.n.cmp(&a.n).then(a.nom.cmp(&b.nom)));
        zones
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Zone {
    pub nom: String,
    pub min: [f32; 3],
    pub max: [f32; 3],
    /// Nombre de changements dans la zone.
    pub n: usize,
}

/// Empreinte d'un bloc sans ses numéros : deux générations qui écrivent le même objet donnent la même.
fn empreinte(n: &Node, h: &mut blake3::Hasher) {
    h.update(n.name.to_ascii_lowercase().as_bytes());
    h.update(b"{");
    for p in &n.pairs {
        if p.key.eq_ignore_ascii_case("id") || MARQUES.iter().any(|m| p.key.eq_ignore_ascii_case(m)) {
            continue;
        }
        h.update(p.key.to_ascii_lowercase().as_bytes());
        h.update(b"=");
        // un plan est le même quels que soient les trois points qui le disent
        match p.key.eq_ignore_ascii_case("plane").then(|| plan_canonique(&p.value)).flatten() {
            Some(c) => h.update(c.as_bytes()),
            None => h.update(p.value.as_bytes()),
        };
        h.update(b";");
    }
    // l'éditeur de Hammer range ses propres réglages dans `editor` : ils ne changent pas la map
    let enfants = n.children.iter().filter(|c| !c.name.eq_ignore_ascii_case("editor"));
    if n.name.eq_ignore_ascii_case("solid") {
        // les faces d'un solide sont un ensemble : deux écritures qui les rangent autrement disent le même solide
        let mut faces: Vec<[u8; 32]> = enfants.map(cle).collect();
        faces.sort();
        for f in faces {
            h.update(&f);
        }
    } else {
        for c in enfants {
            empreinte(c, h);
        }
    }
    h.update(b"}");
}

/// Un plan dit par sa normale unitaire et sa distance à l'origine, arrondies : deux écritures du même plan par
/// trois points différents donnent le même texte.
fn plan_canonique(valeur: &str) -> Option<String> {
    let [a, b, c] = hm_vmf::parse_plane(valeur)?;
    let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
    let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if l < 1e-9 {
        return None;
    }
    let n = n.map(|x| x / l);
    let d = n[0] * a[0] + n[1] * a[1] + n[2] * a[2];
    let r = |x: f64, pas: f64| {
        let y = (x / pas).round() * pas;
        if y == 0.0 { 0.0 } else { y }
    };
    Some(format!("{:.5} {:.5} {:.5} {:.2}", r(n[0], 1e-5), r(n[1], 1e-5), r(n[2], 1e-5), r(d, 1e-2)))
}

fn cle(n: &Node) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    empreinte(n, &mut h);
    *h.finalize().as_bytes()
}

fn centre_solide(s: &Node) -> [f64; 3] {
    let (mut c, mut n) = ([0.0f64; 3], 0.0f64);
    for side in s.children_named("side") {
        if let Some(pl) = side.get("plane").and_then(hm_vmf::parse_plane) {
            for p in pl {
                for k in 0..3 {
                    c[k] += p[k];
                }
                n += 1.0;
            }
        }
    }
    if n > 0.0 {
        [c[0] / n, c[1] / n, c[2] / n]
    } else {
        c
    }
}

fn element(n: &Node) -> Element {
    let e = Ent::de(n);
    // une entité à solides sans origine : le centre de son premier solide
    let origine = e.origine.or_else(|| n.solids().next().map(centre_solide));
    Element { classe: e.classe, nom: e.nom, modele: e.modele, origine, id: e.id }
}

/// Retire de `a` et de `b` ce qu'ils ont en commun (même empreinte), et rend ce qui reste de chaque côté.
fn propres<'a>(a: Vec<&'a Node>, b: Vec<&'a Node>) -> (Vec<&'a Node>, Vec<&'a Node>) {
    let mut compte: HashMap<[u8; 32], i32> = HashMap::new();
    let ca: Vec<[u8; 32]> = a.iter().map(|n| cle(n)).collect();
    let cb: Vec<[u8; 32]> = b.iter().map(|n| cle(n)).collect();
    for k in &ca {
        *compte.entry(*k).or_default() += 1;
    }
    for k in &cb {
        *compte.entry(*k).or_default() -= 1;
    }
    // un compte positif : en trop dans `a` ; négatif : en trop dans `b`
    let mut reste = compte.clone();
    let seuls_a = a.into_iter().zip(&ca).filter(|(_, k)| { let c = reste.get_mut(*k).unwrap(); if *c > 0 { *c -= 1; true } else { false } }).map(|(n, _)| n).collect();
    let mut reste = compte;
    let seuls_b = b.into_iter().zip(&cb).filter(|(_, k)| { let c = reste.get_mut(*k).unwrap(); if *c < 0 { *c += 1; true } else { false } }).map(|(n, _)| n).collect();
    (seuls_a, seuls_b)
}

pub fn comparer(avant: &[u8], apres: &[u8]) -> Result<Difference> {
    let a = Vmf::parse(avant.to_vec()).map_err(|e| anyhow!("avant : {e}"))?;
    let b = Vmf::parse(apres.to_vec()).map_err(|e| anyhow!("après : {e}"))?;
    // l'entité qui porte la révision change à chaque version : elle ne compte pas
    // … et un func_detail n'est qu'un sac de solides que le compilateur verse dans le monde : la façon dont un
    // générateur les regroupe ne change pas la map. Ses solides sont comparés avec ceux du monde.
    fn detail(e: &Node) -> bool {
        e.classname().eq_ignore_ascii_case("func_detail")
    }
    fn entites(v: &Vmf) -> Vec<&Node> {
        v.entities().filter(|e| !detail(e) && !e.get("targetname").map_or(false, |n| n.starts_with(hm_vmf::PREFIXE_REVISION))).collect()
    }
    fn solides(v: &Vmf) -> Vec<&Node> {
        v.world().into_iter().flat_map(|w| w.solids()).chain(v.entities().filter(|e| detail(e)).flat_map(|e| e.solids())).collect()
    }
    let (seuls_a, seuls_b) = propres(entites(&a), entites(&b));
    let mut d = Difference::default();
    let mut ajoutes: Vec<Option<Element>> = seuls_b.iter().map(|n| Some(element(n))).collect();
    for n in seuls_a {
        let e = element(n);
        // le même objet ailleurs, ou réglé autrement : même classe, même modèle, même nom, le plus proche
        let ecart = |x: &Element| match (x.origine, e.origine) {
            (Some(p), Some(q)) => ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt(),
            _ => 0.0,
        };
        let pair = ajoutes
            .iter()
            .enumerate()
            .filter_map(|(i, x)| x.as_ref().map(|x| (i, x)))
            .filter(|(_, x)| x.classe == e.classe && x.modele == e.modele && x.nom == e.nom && (!e.nom.is_empty() || ecart(x) <= 256.0))
            .min_by(|(_, x), (_, y)| ecart(x).total_cmp(&ecart(y)))
            .map(|(i, _)| i);
        match pair {
            Some(i) => {
                let x = ajoutes[i].take().unwrap();
                match (ecart(&x) > 0.01, x.origine) {
                    (true, Some(vers)) => d.deplaces.push((e, vers)),
                    _ => d.modifies.push(e),
                }
            }
            None => d.retires.push(e),
        }
    }
    d.ajoutes = ajoutes.into_iter().flatten().collect();
    let (seuls_a, seuls_b) = propres(solides(&a), solides(&b));
    d.solides_retires = seuls_a.into_iter().map(centre_solide).collect();
    d.solides_ajoutes = seuls_b.into_iter().map(centre_solide).collect();
    Ok(d)
}
