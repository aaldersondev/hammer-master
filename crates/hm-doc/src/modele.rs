//! Le document : une map en objets typés, lue et écrite sans perte.
//!
//! Chaque objet garde son **gabarit** : la suite des éléments de son bloc d'origine. À l'écriture, le gabarit donne
//! l'ordre, et chaque valeur dont le sens n'a pas changé ressort avec son texte d'origine (un plan relu égal au plan
//! courant ressort tel qu'il était écrit). Un fichier ouvert puis enregistré sans modification ressort donc à
//! l'octet, quand il a été écrit dans la mise en page de Hammer.
use std::collections::HashMap;

use glam::DVec3;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::kv::{self, nombre, nombres, Bloc, Ecrivain, Element, Format};

// ---------------------------------------------------------------------------------------------- faces

/// Axe de texture : `u = (P · dir) / echelle + decalage`, en texels.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Axe {
    pub dir: DVec3,
    pub decalage: f64,
    pub echelle: f64,
}

impl Axe {
    pub fn lire(s: &str) -> Option<Axe> {
        let v = nombres(s);
        (v.len() == 5).then(|| Axe { dir: DVec3::new(v[0], v[1], v[2]), decalage: v[3], echelle: v[4] })
    }

    pub fn texte(&self) -> String {
        format!("[{} {} {} {}] {}", nombre(self.dir.x), nombre(self.dir.y), nombre(self.dir.z), nombre(self.decalage), nombre(self.echelle))
    }
}

pub fn lire_plan(s: &str) -> Option<[DVec3; 3]> {
    let v = nombres(s);
    (v.len() == 9).then(|| [DVec3::new(v[0], v[1], v[2]), DVec3::new(v[3], v[4], v[5]), DVec3::new(v[6], v[7], v[8])])
}

pub fn texte_plan(p: &[DVec3; 3]) -> String {
    let t = |v: DVec3| format!("({} {} {})", nombre(v.x), nombre(v.y), nombre(v.z));
    format!("{} {} {}", t(p[0]), t(p[1]), t(p[2]))
}

pub fn lire_vec3(s: &str) -> Option<DVec3> {
    let v = nombres(s);
    (v.len() >= 3).then(|| DVec3::new(v[0], v[1], v[2]))
}

pub fn texte_vec3(v: DVec3) -> String {
    format!("{} {} {}", nombre(v.x), nombre(v.y), nombre(v.z))
}

/// Une face (`side`) d'un solide.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Face {
    pub id: u64,
    /// Trois points du plan, en sens horaire vus de l'extérieur du solide.
    pub plan: [DVec3; 3],
    pub materiau: String,
    pub u: Axe,
    pub v: Axe,
    pub rotation: f64,
    pub echelle_lumiere: f64,
    pub lissage: String,
    /// Bloc `dispinfo` d'un displacement, gardé tel quel.
    pub disp: Option<Bloc>,
    #[serde(skip)]
    pub(crate) gabarit: Vec<Element>,
}

impl Face {
    /// Face neuve, avec ses axes de texture alignés sur le monde (comme Hammer).
    pub fn nouvelle(id: u64, plan: [DVec3; 3], materiau: &str) -> Face {
        let n = (plan[0] - plan[1]).cross(plan[2] - plan[1]).normalize_or_zero();
        let (u, v) = axes_monde(n);
        Face {
            id,
            plan,
            materiau: materiau.to_string(),
            u: Axe { dir: u, decalage: 0.0, echelle: 0.25 },
            v: Axe { dir: v, decalage: 0.0, echelle: 0.25 },
            rotation: 0.0,
            echelle_lumiere: 16.0,
            lissage: "0".into(),
            disp: None,
            gabarit: Vec::new(),
        }
    }

    /// Normale sortante (unitaire), selon la règle de vbsp : (p0 − p1) × (p2 − p1).
    pub fn normale(&self) -> DVec3 {
        (self.plan[0] - self.plan[1]).cross(self.plan[2] - self.plan[1]).normalize_or_zero()
    }

    pub fn plan_geo(&self) -> Option<hm_geom::Plane> {
        hm_geom::Plane::from_points([self.plan[0].to_array(), self.plan[1].to_array(), self.plan[2].to_array()])
    }

    fn lire(b: &Bloc) -> Face {
        let mut f = Face {
            id: 0,
            plan: [DVec3::ZERO; 3],
            materiau: String::new(),
            u: Axe { dir: DVec3::X, decalage: 0.0, echelle: 0.25 },
            v: Axe { dir: DVec3::NEG_Y, decalage: 0.0, echelle: 0.25 },
            rotation: 0.0,
            echelle_lumiere: 16.0,
            lissage: "0".into(),
            disp: None,
            gabarit: b.elements.clone(),
        };
        for e in &b.elements {
            match e {
                Element::Paire(k, v) => match k.to_ascii_lowercase().as_str() {
                    "id" => f.id = v.trim().parse().unwrap_or(0),
                    "plane" => f.plan = lire_plan(v).unwrap_or(f.plan),
                    "material" => f.materiau = v.clone(),
                    "uaxis" => f.u = Axe::lire(v).unwrap_or(f.u),
                    "vaxis" => f.v = Axe::lire(v).unwrap_or(f.v),
                    "rotation" => f.rotation = v.trim().parse().unwrap_or(0.0),
                    "lightmapscale" => f.echelle_lumiere = v.trim().parse().unwrap_or(16.0),
                    "smoothing_groups" => f.lissage = v.clone(),
                    _ => {}
                },
                Element::Bloc(c) if c.nom.eq_ignore_ascii_case("dispinfo") => f.disp = Some(c.clone()),
                _ => {}
            }
        }
        f
    }

    /// Le plan relu dans le texte d'origine est-il le plan courant ?
    fn plan_inchange(&self) -> bool {
        self.gabarit.iter().any(|e| matches!(e, Element::Paire(k, v) if k.eq_ignore_ascii_case("plane") && lire_plan(v) == Some(self.plan)))
    }

    fn ecrire(&self, w: &mut Ecrivain, niveau: usize, geometrie_inchangee: bool, poly: &[DVec3]) {
        w.ouvrir("side", niveau);
        let n = niveau + 1;
        let mut faits = [false; 8];
        const CANON: [&str; 8] = ["id", "plane", "material", "uaxis", "vaxis", "rotation", "lightmapscale", "smoothing_groups"];
        let mut disp_ecrit = false;
        let valeur = |i: usize| -> String {
            match i {
                0 => self.id.to_string(),
                1 => texte_plan(&self.plan),
                2 => self.materiau.clone(),
                3 => self.u.texte(),
                4 => self.v.texte(),
                5 => nombre(self.rotation),
                6 => nombre(self.echelle_lumiere),
                _ => self.lissage.clone(),
            }
        };
        let meme = |i: usize, orig: &str| -> bool {
            match i {
                0 => orig.trim().parse::<u64>().ok() == Some(self.id),
                1 => lire_plan(orig) == Some(self.plan),
                2 => orig == self.materiau,
                3 => Axe::lire(orig) == Some(self.u),
                4 => Axe::lire(orig) == Some(self.v),
                5 => orig.trim().parse::<f64>().ok() == Some(self.rotation),
                6 => orig.trim().parse::<f64>().ok() == Some(self.echelle_lumiere),
                _ => orig == self.lissage,
            }
        };
        for e in &self.gabarit {
            match e {
                Element::Paire(k, v) => match CANON.iter().position(|c| k.eq_ignore_ascii_case(c)) {
                    Some(i) => {
                        if !faits[i] {
                            faits[i] = true;
                            if meme(i, v) {
                                w.paire(k, v, n);
                            } else {
                                w.paire(k, &valeur(i), n);
                            }
                        }
                    }
                    None => w.paire(k, v, n),
                },
                Element::Bloc(c) if c.nom.eq_ignore_ascii_case("dispinfo") => {
                    if let Some(d) = &self.disp {
                        w.bloc(d, n);
                    }
                    disp_ecrit = true;
                }
                Element::Bloc(c) if c.nom.eq_ignore_ascii_case("vertices_plus") => {
                    // sommets exacts écrits par Hammer++ : faux dès que la géométrie a bougé, donc recalculés
                    if geometrie_inchangee || poly.is_empty() {
                        w.bloc(c, n);
                    } else {
                        let mut b = Bloc::nouveau(&c.nom);
                        for p in poly {
                            b.elements.push(Element::Paire("v".into(), texte_vec3(*p)));
                        }
                        w.bloc(&b, n);
                    }
                }
                Element::Bloc(c) => w.bloc(c, n),
            }
        }
        let neuve = self.gabarit.is_empty();
        for i in 0..8 {
            let defaut = match i {
                5 => self.rotation == 0.0,
                6 => self.echelle_lumiere == 16.0,
                7 => self.lissage == "0",
                _ => false,
            };
            if !faits[i] && (neuve || !defaut) {
                w.paire(CANON[i], &valeur(i), n);
            }
        }
        if !disp_ecrit {
            if let Some(d) = &self.disp {
                w.bloc(d, n);
            }
        }
        w.fermer(niveau);
    }
}

/// Axes de texture « alignés sur le monde » de Hammer : projection sur le plan des axes le plus proche de la face.
pub fn axes_monde(n: DVec3) -> (DVec3, DVec3) {
    // table de Hammer (`TextureAxisFromPlane`) : sol/plafond, mur face à X, mur face à Y
    let (a, b, c) = (n.x.abs(), n.y.abs(), n.z.abs());
    if c >= a && c >= b {
        (DVec3::X, DVec3::NEG_Y)
    } else if a >= b {
        (DVec3::Y, DVec3::NEG_Z)
    } else {
        (DVec3::X, DVec3::NEG_Z)
    }
}

// ---------------------------------------------------------------------------------------------- solides

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Solide {
    pub id: u64,
    pub faces: Vec<Face>,
    /// Bloc `editor` : couleur, groupe, groupes de visibilité.
    pub editeur: Bloc,
    /// Rangé dans un bloc `hidden` (masqué par un groupe de visibilité).
    pub cache: bool,
    #[serde(skip)]
    pub(crate) gabarit: Vec<Element>,
}

impl Solide {
    fn lire(b: &Bloc, cache: bool) -> Solide {
        let mut s = Solide { id: 0, faces: Vec::new(), editeur: Bloc::nouveau("editor"), cache, gabarit: b.elements.clone() };
        for e in &b.elements {
            match e {
                Element::Paire(k, v) if k.eq_ignore_ascii_case("id") => s.id = v.trim().parse().unwrap_or(0),
                Element::Bloc(c) if c.nom.eq_ignore_ascii_case("side") => s.faces.push(Face::lire(c)),
                Element::Bloc(c) if c.nom.eq_ignore_ascii_case("editor") => s.editeur = c.clone(),
                _ => {}
            }
        }
        s
    }

    pub fn plans(&self) -> Vec<Option<hm_geom::Plane>> {
        self.faces.iter().map(|f| f.plan_geo()).collect()
    }

    /// Polygone de chaque face, dans l'ordre des faces (vide pour une face sans surface).
    pub fn polygones(&self) -> Vec<Vec<DVec3>> {
        hm_geom::polygones(&self.plans())
    }

    pub fn boite(&self) -> Option<(DVec3, DVec3)> {
        let polys = self.polygones();
        let mut it = polys.iter().flatten();
        let p0 = *it.next()?;
        Some(it.fold((p0, p0), |(a, b), p| (a.min(*p), b.max(*p))))
    }

    /// Le solide ferme un volume convexe : au moins quatre faces ont une surface, aucune n'est dégénérée.
    pub fn valide(&self) -> bool {
        let polys = self.polygones();
        polys.iter().filter(|p| !p.is_empty()).count() >= 4 && self.faces.iter().all(|f| f.plan_geo().is_some())
    }

    pub fn groupe(&self) -> u64 {
        self.editeur.get("groupid").and_then(|v| v.trim().parse().ok()).unwrap_or(0)
    }

    pub fn couleur(&self) -> [u8; 3] {
        couleur(&self.editeur).unwrap_or([0, 100, 220])
    }

    fn ecrire(&self, w: &mut Ecrivain, niveau: usize) {
        let (niveau, cache) = if self.cache {
            w.ouvrir("hidden", niveau);
            (niveau + 1, true)
        } else {
            (niveau, false)
        };
        w.ouvrir("solid", niveau);
        let n = niveau + 1;
        let inchangee = self.faces.iter().all(|f| f.plan_inchange());
        let polys = if inchangee { Vec::new() } else { self.polygones() };
        let poly = |i: usize| polys.get(i).map(|p| p.as_slice()).unwrap_or(&[]);
        let (mut id_fait, mut faces_faites, mut editeur_fait) = (false, false, false);
        for e in &self.gabarit {
            match e {
                Element::Paire(k, v) if k.eq_ignore_ascii_case("id") => {
                    if !id_fait {
                        id_fait = true;
                        w.paire(k, &texte_id(v, self.id), n);
                    }
                }
                Element::Paire(k, v) => w.paire(k, v, n),
                Element::Bloc(c) if c.nom.eq_ignore_ascii_case("side") => {
                    if !faces_faites {
                        faces_faites = true;
                        if !id_fait {
                            id_fait = true;
                            w.paire("id", &self.id.to_string(), n);
                        }
                        for (i, f) in self.faces.iter().enumerate() {
                            f.ecrire(w, n, inchangee, poly(i));
                        }
                    }
                }
                Element::Bloc(c) if c.nom.eq_ignore_ascii_case("editor") => {
                    if !editeur_fait {
                        editeur_fait = true;
                        w.bloc(&self.editeur, n);
                    }
                }
                Element::Bloc(c) => w.bloc(c, n),
            }
        }
        if !id_fait {
            w.paire("id", &self.id.to_string(), n);
        }
        if !faces_faites {
            for (i, f) in self.faces.iter().enumerate() {
                f.ecrire(w, n, inchangee, poly(i));
            }
        }
        if !editeur_fait && (self.gabarit.is_empty() || !self.editeur.elements.is_empty()) {
            w.bloc(&self.editeur, n);
        }
        w.fermer(niveau);
        if cache {
            w.fermer(niveau - 1);
        }
    }
}

/// Le numéro tel qu'il était écrit s'il n'a pas changé.
fn texte_id(orig: &str, id: u64) -> String {
    if orig.trim().parse::<u64>().ok() == Some(id) { orig.to_string() } else { id.to_string() }
}

pub fn couleur(editeur: &Bloc) -> Option<[u8; 3]> {
    let v = nombres(editeur.get("color")?);
    (v.len() >= 3).then(|| [v[0] as u8, v[1] as u8, v[2] as u8])
}

// ---------------------------------------------------------------------------------------------- entités

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entite {
    pub id: u64,
    /// Toutes les clés sauf `id`, dans l'ordre du fichier (doublons permis).
    pub cles: Vec<(String, String)>,
    /// Bloc `connections` : sorties `"OnTrigger" "cible,Entrée,param,délai,fois"`. `None` : pas de bloc.
    pub sorties: Option<Vec<(String, String)>>,
    pub solides: Vec<Solide>,
    pub editeur: Option<Bloc>,
    pub cache: bool,
    #[serde(skip)]
    pub(crate) gabarit: Vec<Element>,
}

impl Entite {
    pub fn nouvelle(id: u64, classe: &str) -> Entite {
        Entite { id, cles: vec![("classname".into(), classe.into())], sorties: None, solides: Vec::new(), editeur: None, cache: false, gabarit: Vec::new() }
    }

    pub fn get(&self, cle: &str) -> Option<&str> {
        self.cles.iter().find(|(k, _)| k.eq_ignore_ascii_case(cle)).map(|(_, v)| v.as_str())
    }

    /// Remplace la valeur (la première si la clé est en double), ou ajoute la clé à la fin.
    pub fn set(&mut self, cle: &str, valeur: &str) {
        match self.cles.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(cle)) {
            Some((_, v)) => *v = valeur.to_string(),
            None => self.cles.push((cle.to_string(), valeur.to_string())),
        }
    }

    pub fn retirer(&mut self, cle: &str) {
        self.cles.retain(|(k, _)| !k.eq_ignore_ascii_case(cle));
    }

    pub fn classe(&self) -> &str {
        self.get("classname").unwrap_or("")
    }

    pub fn nom(&self) -> &str {
        self.get("targetname").unwrap_or("")
    }

    pub fn origine(&self) -> Option<DVec3> {
        self.get("origin").and_then(lire_vec3)
    }

    pub fn angles(&self) -> DVec3 {
        self.get("angles").and_then(lire_vec3).unwrap_or(DVec3::ZERO)
    }

    pub fn ponctuelle(&self) -> bool {
        self.solides.is_empty()
    }

    pub fn groupe(&self) -> u64 {
        self.editeur.as_ref().and_then(|e| e.get("groupid")).and_then(|v| v.trim().parse().ok()).unwrap_or(0)
    }

    fn lire(b: &Bloc, cache: bool) -> Entite {
        let mut e = Entite { id: 0, cles: Vec::new(), sorties: None, solides: Vec::new(), editeur: None, cache, gabarit: b.elements.clone() };
        for el in &b.elements {
            match el {
                Element::Paire(k, v) if k.eq_ignore_ascii_case("id") => e.id = v.trim().parse().unwrap_or(0),
                Element::Paire(k, v) => e.cles.push((k.clone(), v.clone())),
                Element::Bloc(c) if c.nom.eq_ignore_ascii_case("connections") => e.sorties = Some(c.paires().map(|(k, v)| (k.to_string(), v.to_string())).collect()),
                Element::Bloc(c) if c.nom.eq_ignore_ascii_case("solid") => e.solides.push(Solide::lire(c, false)),
                Element::Bloc(c) if c.nom.eq_ignore_ascii_case("hidden") => {
                    for s in c.enfants().filter(|s| s.nom.eq_ignore_ascii_case("solid")) {
                        e.solides.push(Solide::lire(s, true));
                    }
                }
                Element::Bloc(c) if c.nom.eq_ignore_ascii_case("editor") => e.editeur = Some(c.clone()),
                _ => {}
            }
        }
        e
    }

    /// Écrit le bloc. `solides_monde` et `groupes` ne servent qu'au monde, dont les solides vivent à part.
    fn ecrire(&self, nom: &str, w: &mut Ecrivain, niveau: usize, solides_monde: Option<&IndexMap<u64, Solide>>, groupes: Option<&IndexMap<u64, Groupe>>) {
        let (niveau, cache) = if self.cache {
            w.ouvrir("hidden", niveau);
            (niveau + 1, true)
        } else {
            (niveau, false)
        };
        w.ouvrir(nom, niveau);
        let n = niveau + 1;
        let (mut id_fait, mut cles_faites, mut sorties_faites, mut solides_faits, mut groupes_faits, mut editeur_fait) = (false, false, false, false, false, false);
        let ecrire_solides = |w: &mut Ecrivain| match solides_monde {
            Some(m) => m.values().for_each(|s| s.ecrire(w, n)),
            None => self.solides.iter().for_each(|s| s.ecrire(w, n)),
        };
        let ecrire_cles = |w: &mut Ecrivain| self.cles.iter().for_each(|(k, v)| w.paire(k, v, n));
        let ecrire_sorties = |w: &mut Ecrivain| {
            if let Some(s) = &self.sorties {
                w.ouvrir("connections", n);
                s.iter().for_each(|(k, v)| w.paire(k, v, n + 1));
                w.fermer(n);
            }
        };
        let ecrire_groupes = |w: &mut Ecrivain| {
            if let Some(g) = groupes {
                g.values().for_each(|g| g.ecrire(w, n));
            }
        };
        // quelles clés courantes ont une place dans le gabarit (dans l'ordre, doublons compris)
        let mut dans_gabarit = vec![false; self.cles.len()];
        for el in &self.gabarit {
            if let Element::Paire(k, _) = el {
                if !k.eq_ignore_ascii_case("id") {
                    if let Some(j) = (0..self.cles.len()).find(|&j| !dans_gabarit[j] && self.cles[j].0.eq_ignore_ascii_case(k)) {
                        dans_gabarit[j] = true;
                    }
                }
            }
        }
        let mut ecrites = vec![false; self.cles.len()];
        let derniere_paire = self.gabarit.iter().rposition(|e| matches!(e, Element::Paire(k, _) if !k.eq_ignore_ascii_case("id")));
        for (rang, el) in self.gabarit.iter().enumerate() {
            match el {
                Element::Paire(k, v) if k.eq_ignore_ascii_case("id") => {
                    if !id_fait {
                        id_fait = true;
                        w.paire(k, &texte_id(v, self.id), n);
                    }
                }
                Element::Paire(k, _) => {
                    if !id_fait {
                        // Hammer écrit le numéro en premier
                        id_fait = true;
                        w.paire("id", &self.id.to_string(), n);
                    }
                    cles_faites = true;
                    if let Some(j) = (0..self.cles.len()).find(|&j| !ecrites[j] && self.cles[j].0.eq_ignore_ascii_case(k)) {
                        // d'abord les clés neuves rangées avant celle-ci
                        for i in 0..j {
                            if !ecrites[i] && !dans_gabarit[i] {
                                ecrites[i] = true;
                                w.paire(&self.cles[i].0, &self.cles[i].1, n);
                            }
                        }
                        ecrites[j] = true;
                        w.paire(&self.cles[j].0, &self.cles[j].1, n);
                    }
                    if derniere_paire == Some(rang) {
                        for i in 0..self.cles.len() {
                            if !ecrites[i] {
                                ecrites[i] = true;
                                w.paire(&self.cles[i].0, &self.cles[i].1, n);
                            }
                        }
                    }
                }
                Element::Bloc(c) => match c.nom.to_ascii_lowercase().as_str() {
                    "connections" => {
                        if !sorties_faites {
                            sorties_faites = true;
                            ecrire_sorties(w);
                        }
                    }
                    "solid" | "hidden" => {
                        if !solides_faits {
                            solides_faits = true;
                            ecrire_solides(w);
                        }
                    }
                    "group" if groupes.is_some() => {
                        if !groupes_faits {
                            groupes_faits = true;
                            ecrire_groupes(w);
                        }
                    }
                    "editor" => {
                        if !editeur_fait {
                            editeur_fait = true;
                            if let Some(e) = &self.editeur {
                                w.bloc(e, n);
                            }
                        }
                    }
                    _ => w.bloc(c, n),
                },
            }
        }
        if !id_fait {
            w.paire("id", &self.id.to_string(), n);
        }
        if !cles_faites {
            ecrire_cles(w);
        }
        if !sorties_faites {
            ecrire_sorties(w);
        }
        if !solides_faits {
            ecrire_solides(w);
        }
        if !groupes_faits {
            ecrire_groupes(w);
        }
        if !editeur_fait {
            if let Some(e) = &self.editeur {
                w.bloc(e, n);
            }
        }
        w.fermer(niveau);
        if cache {
            w.fermer(niveau - 1);
        }
    }
}

// ---------------------------------------------------------------------------------------------- groupes

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Groupe {
    pub id: u64,
    pub editeur: Bloc,
    #[serde(skip)]
    pub(crate) gabarit: Vec<Element>,
}

impl Groupe {
    fn lire(b: &Bloc) -> Groupe {
        let mut g = Groupe { id: 0, editeur: Bloc::nouveau("editor"), gabarit: b.elements.clone() };
        for e in &b.elements {
            match e {
                Element::Paire(k, v) if k.eq_ignore_ascii_case("id") => g.id = v.trim().parse().unwrap_or(0),
                Element::Bloc(c) if c.nom.eq_ignore_ascii_case("editor") => g.editeur = c.clone(),
                _ => {}
            }
        }
        g
    }

    fn ecrire(&self, w: &mut Ecrivain, niveau: usize) {
        w.ouvrir("group", niveau);
        let n = niveau + 1;
        let (mut id_fait, mut ed_fait) = (false, false);
        for e in &self.gabarit {
            match e {
                Element::Paire(k, v) if k.eq_ignore_ascii_case("id") => {
                    id_fait = true;
                    w.paire(k, &texte_id(v, self.id), n);
                }
                Element::Paire(k, v) => w.paire(k, v, n),
                Element::Bloc(c) if c.nom.eq_ignore_ascii_case("editor") => {
                    ed_fait = true;
                    w.bloc(&self.editeur, n);
                }
                Element::Bloc(c) => w.bloc(c, n),
            }
        }
        if !id_fait {
            w.paire("id", &self.id.to_string(), n);
        }
        if !ed_fait {
            w.bloc(&self.editeur, n);
        }
        w.fermer(niveau);
    }
}

// ---------------------------------------------------------------------------------------------- document

/// Ce qui se trouve au premier niveau du fichier, dans l'ordre.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) enum Haut {
    Bloc(Bloc),
    /// Bloc absent du fichier lu (`visgroups`, `cordons`), ajouté pour que les commandes aient où écrire ; il n'est
    /// écrit que s'il a reçu un contenu, pour que le fichier ressorte à l'octet.
    Ajoute(Bloc),
    Monde,
    /// Place des entités (toutes écrites là, dans leur ordre).
    Entites,
}

/// À qui appartient un solide.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Proprio {
    Monde,
    Entite(u64),
}

/// Ce que la lecture a dû corriger : un numéro en double ne peut pas désigner un objet.
#[derive(Clone, Debug, Default)]
pub struct RapportLecture {
    pub renumerotes: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Document {
    pub(crate) haut: Vec<Haut>,
    pub(crate) monde: Entite,
    pub(crate) solides: IndexMap<u64, Solide>,
    pub(crate) entites: IndexMap<u64, Entite>,
    pub(crate) groupes: IndexMap<u64, Groupe>,
    pub(crate) format: Format,
    pub(crate) proprio: HashMap<u64, Proprio>,
    pub(crate) prochain_id: u64,
    pub(crate) prochaine_face: u64,
}

#[derive(Debug)]
pub struct ErreurLecture(pub String);
impl std::fmt::Display for ErreurLecture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ErreurLecture {}

impl Document {
    /// Une map vide, comme Hammer++ en crée une.
    pub fn nouveau() -> Document {
        let bloc = |nom: &str, paires: &[(&str, &str)]| {
            let mut b = Bloc::nouveau(nom);
            for (k, v) in paires {
                b.elements.push(Element::Paire(k.to_string(), v.to_string()));
            }
            b
        };
        let haut = vec![
            Haut::Bloc(bloc("versioninfo", &[("editorversion", "400"), ("editorbuild", "10493"), ("mapversion", "0"), ("formatversion", "100"), ("prefab", "0")])),
            Haut::Bloc(bloc("visgroups", &[])),
            Haut::Bloc(bloc("viewsettings", &[("bSnapToGrid", "1"), ("bShowGrid", "1"), ("bShowLogicalGrid", "0"), ("nGridSpacing", "64"), ("bShow3DGrid", "0")])),
            Haut::Monde,
            Haut::Entites,
            Haut::Bloc(bloc("cameras", &[("activecamera", "-1")])),
            Haut::Bloc(bloc("cordons", &[("active", "0")])),
        ];
        let mut monde = Entite::nouvelle(1, "worldspawn");
        monde.cles = vec![
            ("mapversion".into(), "0".into()),
            ("classname".into(), "worldspawn".into()),
            ("skyname".into(), "sky_day01_01".into()),
            ("maxpropscreenwidth".into(), "-1".into()),
            ("detailvbsp".into(), "detail.vbsp".into()),
            ("detailmaterial".into(), "detail/detailsprites".into()),
        ];
        Document { haut, monde, solides: IndexMap::new(), entites: IndexMap::new(), groupes: IndexMap::new(), format: Format::default(), proprio: HashMap::new(), prochain_id: 2, prochaine_face: 1 }
    }

    pub fn lire(src: &[u8]) -> Result<(Document, RapportLecture), ErreurLecture> {
        let (blocs, format) = kv::lire(src).map_err(|e| ErreurLecture(e.to_string()))?;
        let mut doc = Document { haut: Vec::new(), monde: Entite::nouvelle(1, "worldspawn"), solides: IndexMap::new(), entites: IndexMap::new(), groupes: IndexMap::new(), format, proprio: HashMap::new(), prochain_id: 1, prochaine_face: 1 };
        let mut rapport = RapportLecture::default();
        let mut entites: Vec<Entite> = Vec::new();
        let mut solides_monde: Vec<Solide> = Vec::new();
        let mut monde_vu = false;
        for b in blocs {
            match b.nom.to_ascii_lowercase().as_str() {
                "world" if !monde_vu => {
                    monde_vu = true;
                    let mut m = Entite::lire(&b, false);
                    solides_monde = std::mem::take(&mut m.solides);
                    for c in b.enfants().filter(|c| c.nom.eq_ignore_ascii_case("group")) {
                        let g = Groupe::lire(c);
                        doc.groupes.insert(g.id, g);
                    }
                    doc.monde = m;
                    doc.haut.push(Haut::Monde);
                }
                "entity" => {
                    if !doc.haut.contains(&Haut::Entites) {
                        doc.haut.push(Haut::Entites);
                    }
                    entites.push(Entite::lire(&b, false));
                }
                "hidden" if b.enfants().any(|c| c.nom.eq_ignore_ascii_case("entity")) => {
                    if !doc.haut.contains(&Haut::Entites) {
                        doc.haut.push(Haut::Entites);
                    }
                    for c in b.enfants().filter(|c| c.nom.eq_ignore_ascii_case("entity")) {
                        entites.push(Entite::lire(c, true));
                    }
                }
                _ => doc.haut.push(Haut::Bloc(b)),
            }
        }
        if !monde_vu {
            return Err(ErreurLecture("le fichier n'a pas de bloc « world » : ce n'est pas une map".into()));
        }
        if !doc.haut.contains(&Haut::Entites) {
            let i = doc.haut.iter().position(|h| *h == Haut::Monde).unwrap() + 1;
            doc.haut.insert(i, Haut::Entites);
        }
        for nom in ["visgroups", "cordons"] {
            if doc.bloc_haut(nom).is_none() {
                doc.haut.push(Haut::Ajoute(Bloc::nouveau(nom)));
            }
        }
        // numéros : le plus grand de tous, puis les doublons renumérotés
        let mut max = doc.monde.id;
        let mut max_face = 0;
        let voir = |s: &Solide, max: &mut u64, max_face: &mut u64| {
            *max = (*max).max(s.id);
            for f in &s.faces {
                *max_face = (*max_face).max(f.id);
            }
        };
        for s in &solides_monde {
            voir(s, &mut max, &mut max_face);
        }
        for e in &entites {
            max = max.max(e.id);
            for s in &e.solides {
                voir(s, &mut max, &mut max_face);
            }
        }
        for g in doc.groupes.values() {
            max = max.max(g.id);
        }
        doc.prochain_id = max + 1;
        doc.prochaine_face = max_face + 1;
        for mut s in solides_monde {
            if s.id == 0 || doc.proprio.contains_key(&s.id) {
                let ancien = s.id;
                s.id = doc.nouvel_id();
                rapport.renumerotes.push(format!("solide du monde n° {ancien} en double : renuméroté {}", s.id));
            }
            doc.proprio.insert(s.id, Proprio::Monde);
            doc.solides.insert(s.id, s);
        }
        for mut e in entites {
            if e.id == 0 || doc.entites.contains_key(&e.id) {
                let ancien = e.id;
                e.id = doc.nouvel_id();
                rapport.renumerotes.push(format!("entité n° {ancien} en double : renumérotée {}", e.id));
            }
            for s in &mut e.solides {
                if s.id == 0 || doc.proprio.contains_key(&s.id) {
                    let ancien = s.id;
                    s.id = doc.nouvel_id();
                    rapport.renumerotes.push(format!("solide n° {ancien} en double : renuméroté {}", s.id));
                }
                doc.proprio.insert(s.id, Proprio::Entite(e.id));
            }
            doc.entites.insert(e.id, e);
        }
        Ok((doc, rapport))
    }

    pub fn ecrire(&self) -> Vec<u8> {
        let mut w = Ecrivain::new(self.format);
        let mut entites_faites = false;
        for h in &self.haut {
            match h {
                Haut::Bloc(b) => w.bloc(b, 0),
                Haut::Ajoute(b) => {
                    if !b.elements.is_empty() {
                        w.bloc(b, 0)
                    }
                }
                Haut::Monde => self.monde.ecrire("world", &mut w, 0, Some(&self.solides), Some(&self.groupes)),
                Haut::Entites => {
                    if !entites_faites {
                        entites_faites = true;
                        for e in self.entites.values() {
                            e.ecrire("entity", &mut w, 0, None, None);
                        }
                    }
                }
            }
        }
        w.out
    }

    pub fn format(&self) -> Format {
        self.format
    }

    pub(crate) fn nouvel_id(&mut self) -> u64 {
        let i = self.prochain_id;
        self.prochain_id += 1;
        i
    }

    pub(crate) fn nouvelle_face(&mut self) -> u64 {
        let i = self.prochaine_face;
        self.prochaine_face += 1;
        i
    }

    // ------------------------------------------------------------------ lecture seule, pour la fenêtre et le MCP

    pub fn monde(&self) -> &Entite {
        &self.monde
    }

    /// Solides du monde, dans l'ordre du fichier.
    pub fn solides_monde(&self) -> impl Iterator<Item = &Solide> + '_ {
        self.solides.values()
    }

    pub fn entites(&self) -> impl Iterator<Item = &Entite> + '_ {
        self.entites.values()
    }

    pub fn groupes(&self) -> impl Iterator<Item = &Groupe> + '_ {
        self.groupes.values()
    }

    pub fn entite(&self, id: u64) -> Option<&Entite> {
        self.entites.get(&id)
    }

    pub fn groupe(&self, id: u64) -> Option<&Groupe> {
        self.groupes.get(&id)
    }

    pub fn proprio(&self, solide: u64) -> Option<Proprio> {
        self.proprio.get(&solide).copied()
    }

    pub fn solide(&self, id: u64) -> Option<&Solide> {
        match self.proprio.get(&id)? {
            Proprio::Monde => self.solides.get(&id),
            Proprio::Entite(e) => self.entites.get(e)?.solides.iter().find(|s| s.id == id),
        }
    }

    /// Tous les solides (monde puis entités), avec leur propriétaire.
    pub fn tous_les_solides(&self) -> impl Iterator<Item = (Proprio, &Solide)> + '_ {
        self.solides.values().map(|s| (Proprio::Monde, s)).chain(self.entites.values().flat_map(|e| e.solides.iter().map(move |s| (Proprio::Entite(e.id), s))))
    }

    /// La caméra active du bloc `cameras` (celle où Hammer rouvre la vue 3D) : (position, point visé).
    pub fn camera_active(&self) -> Option<([f64; 3], [f64; 3])> {
        let b = self.bloc_haut("cameras")?;
        let i: usize = b.get("activecamera")?.trim().parse().ok()?;
        let c = b.enfants().filter(|e| e.nom.eq_ignore_ascii_case("camera")).nth(i)?;
        let v = |k: &str| -> Option<[f64; 3]> {
            let n = crate::kv::nombres(c.get(k)?);
            (n.len() == 3).then(|| [n[0], n[1], n[2]])
        };
        Some((v("position")?, v("look")?)).filter(|(p, l)| p != l)
    }

    /// Un bloc de premier niveau (`versioninfo`, `viewsettings`, `cameras`, `cordons`…).
    pub fn bloc_haut(&self, nom: &str) -> Option<&Bloc> {
        self.haut.iter().find_map(|h| match h {
            Haut::Bloc(b) | Haut::Ajoute(b) if b.nom.eq_ignore_ascii_case(nom) => Some(b),
            _ => None,
        })
    }

    /// Les solides invalides (moins de quatre faces avec une surface, ou un plan dégénéré) : vbsp les rejette.
    pub fn solides_invalides(&self) -> Vec<u64> {
        self.tous_les_solides().filter(|(_, s)| !s.valide()).map(|(_, s)| s.id).collect()
    }

    pub fn nombre_objets(&self) -> (usize, usize) {
        (self.solides.len() + self.entites.values().map(|e| e.solides.len()).sum::<usize>(), self.entites.len())
    }
}
