//! Modèles d'aménagement : un agencement dessiné une fois, qui s'adapte à la pièce où on le pose.
//!
//! Un modèle est un fichier TOML : le rôle qu'il meuble, les dimensions de pièce qu'il accepte, et des ensembles de
//! poses dans le **repère de la pièce vue depuis sa porte** : origine au pied du mur de la porte, à gauche ; `x` le
//! long de ce mur, vers la droite ; `y` vers le fond ; lacet 0 = vers la droite, 90 = vers le fond.
//!
//! Un modèle est en **brouillon** tant que l'utilisateur ne l'a pas validé. La validation est rattachée à
//! l'empreinte du modèle : le modifier le repasse en brouillon. Elle se pose par une commande lancée dans un
//! terminal (`hm amenagement-valider`), jamais par le serveur MCP.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};

/// Une cote le long du mur de la porte : depuis le mur de gauche, depuis celui de droite, ou depuis le milieu.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub enum CoteX {
    Gauche(f32),
    Droite(f32),
    Centre(f32),
}

/// Une cote en profondeur : depuis le mur de la porte, depuis le mur du fond, ou depuis le milieu.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub enum CoteY {
    Porte(f32),
    Fond(f32),
    Centre(f32),
}

impl CoteX {
    pub fn en(self, largeur: f32) -> f32 {
        match self {
            CoteX::Gauche(a) => a,
            CoteX::Droite(a) => largeur - a,
            CoteX::Centre(a) => largeur * 0.5 + a,
        }
    }
}

impl CoteY {
    pub fn en(self, profondeur: f32) -> f32 {
        match self {
            CoteY::Porte(a) => a,
            CoteY::Fond(a) => profondeur - a,
            CoteY::Centre(a) => profondeur * 0.5 + a,
        }
    }
}

/// Dimensions de pièce acceptées (air intérieur).
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Contraintes {
    /// Largeur le long du mur de la porte : `[min, max]`.
    pub largeur: [f32; 2],
    /// Profondeur, de la porte au fond : `[min, max]`.
    pub profondeur: [f32; 2],
    #[serde(default = "hauteur_defaut")]
    pub hauteur_min: f32,
}

fn hauteur_defaut() -> f32 {
    112.0
}

/// Une pose dans le repère de son ensemble.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Pose {
    pub modele: String,
    /// Position dans le repère de l'ensemble.
    #[serde(default)]
    pub pos: [f32; 3],
    #[serde(default)]
    pub lacet: f32,
    /// Classe de l'entité (`prop_static` par défaut ; `prop_door_rotating` pour une porte).
    #[serde(default = "classe_defaut")]
    pub classe: String,
    #[serde(default)]
    pub skin: u32,
    /// Début du nom (`targetname`) de l'entité créée ; un numéro unique lui est ajouté (`<nom>_<numéro>`). Pour les
    /// entités que les scripts de la map reconnaissent à leur nom : une porte de cabine, par exemple.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub nom: String,
}

fn classe_defaut() -> String {
    "prop_static".to_string()
}

/// Répétition d'un ensemble le long d'un mur, tant qu'il reste de la place.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Repete {
    /// Pas entre deux exemplaires.
    pub pas: f32,
    /// `x` (le long du mur de la porte) ou `y` (en profondeur).
    #[serde(default = "sens_defaut")]
    pub sens: String,
    #[serde(default = "max_defaut")]
    pub max: u32,
    #[serde(default = "un")]
    pub min: u32,
    /// Encombrement d'un exemplaire le long du sens, de part et d'autre de son origine : `[avant, après]`.
    pub emprise: [f32; 2],
    /// Place à garder libre au bout de la rangée.
    #[serde(default)]
    pub marge: f32,
    /// Centrer la rangée entière dans la pièce plutôt que de la faire partir de la cote donnée.
    #[serde(default)]
    pub centrer: bool,
    /// Plusieurs rangées : la rangée est répétée dans l'autre sens.
    #[serde(default)]
    pub rangs: Option<Rangs>,
    /// Ferme par un coffrage le vide qui reste entre un bout de la rangée et le mur.
    #[serde(default)]
    pub bouche: Option<Bouche>,
}

/// Coffrage qui ferme le vide laissé entre un bout de rangée et le mur : un solide, pas un modèle. Sans lui, une
/// rangée de cabines qui ne tombe pas juste laisse un recoin où l'on ne passe pas.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Bouche {
    /// Étendue du coffrage en travers de la rangée, comptée depuis la cote de la rangée dans le repère de la pièce
    /// (pour une rangée contre le mur du fond : `[-81, 0]` = 81 d'épaisseur en avant du mur).
    pub travers: [f32; 2],
    /// Hauteur du coffrage ; 0 = jusqu'au plafond.
    #[serde(default)]
    pub hauteur: f32,
    /// Matériau ; vide = celui des murs de la pièce.
    #[serde(default)]
    pub materiau: String,
    /// En dessous de cette largeur, le vide est laissé tel quel.
    #[serde(default = "un_f")]
    pub min: f32,
}

fn un_f() -> f32 {
    1.0
}

/// Condition sur la pièce : l'ensemble n'est posé que si elle est remplie.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Condition {
    #[serde(default)]
    pub largeur_min: Option<f32>,
    #[serde(default)]
    pub largeur_max: Option<f32>,
    #[serde(default)]
    pub profondeur_min: Option<f32>,
    #[serde(default)]
    pub profondeur_max: Option<f32>,
    #[serde(default)]
    pub hauteur_min: Option<f32>,
}

impl Condition {
    pub fn remplie(&self, largeur: f32, profondeur: f32, hauteur: f32) -> bool {
        self.largeur_min.map_or(true, |v| largeur >= v)
            && self.largeur_max.map_or(true, |v| largeur <= v)
            && self.profondeur_min.map_or(true, |v| profondeur >= v)
            && self.profondeur_max.map_or(true, |v| profondeur <= v)
            && self.hauteur_min.map_or(true, |v| hauteur >= v)
    }
}

/// Répétition d'une rangée dans l'autre sens (en profondeur pour une rangée le long du mur de la porte).
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Rangs {
    /// Pas entre deux rangées ; négatif pour avancer vers le mur de la porte (ou vers la gauche).
    pub pas: f32,
    #[serde(default = "max_defaut")]
    pub max: u32,
    /// Place à garder libre entre la dernière rangée et le mur vers lequel on avance.
    #[serde(default)]
    pub marge: f32,
}

fn sens_defaut() -> String {
    "x".to_string()
}
fn max_defaut() -> u32 {
    32
}
fn un() -> u32 {
    1
}

/// Un groupe de poses placé d'un bloc : une cabine avec sa cuvette et sa porte, un poste de travail.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Ensemble {
    pub nom: String,
    pub x: CoteX,
    pub y: CoteY,
    #[serde(default)]
    pub z: f32,
    #[serde(default)]
    pub lacet: f32,
    #[serde(default)]
    pub repete: Option<Repete>,
    /// L'ensemble n'est posé que dans les pièces qui remplissent cette condition.
    #[serde(default)]
    pub si: Option<Condition>,
    pub pose: Vec<Pose>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Decor {
    #[serde(default)]
    pub sol: String,
    #[serde(default)]
    pub mur: String,
    #[serde(default)]
    pub plafond: String,
}

/// Un modèle d'aménagement.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Amenagement {
    pub nom: String,
    /// Rôle de pièce qu'il meuble (`regles/roles.yaml`).
    pub role: String,
    #[serde(default)]
    pub description: String,
    pub piece: Contraintes,
    /// Matériaux de la pièce neutre où le modèle est montré (ils ne font pas partie de l'aménagement).
    #[serde(default)]
    pub decor: Decor,
    pub ensemble: Vec<Ensemble>,
}

/// Un coffrage résolu, dans le repère de la pièce : une boîte pleine.
#[derive(Clone, Debug, PartialEq)]
pub struct Bloc {
    pub nom: String,
    pub min: [f32; 3],
    pub max: [f32; 3],
    /// Vide = le matériau des murs de la pièce.
    pub materiau: String,
}

/// Une pose résolue, dans le repère de la pièce.
#[derive(Clone, Debug, PartialEq)]
pub struct PosePiece {
    pub ensemble: String,
    pub modele: String,
    pub pos: [f32; 3],
    pub lacet: f32,
    pub classe: String,
    pub skin: u32,
    /// Début du nom de l'entité, ou vide.
    pub nom: String,
}

impl Amenagement {
    pub fn lire(chemin: &Path) -> Result<Amenagement> {
        let texte = std::fs::read_to_string(chemin).with_context(|| hm_langue::tr!("lecture de {}", "reading {}", chemin.display()))?;
        let a: Amenagement = toml::from_str(&texte).map_err(|e| anyhow::anyhow!("{} : {e}", chemin.display()))?;
        a.controler().with_context(|| format!("{}", chemin.display()))?;
        Ok(a)
    }

    pub fn controler(&self) -> Result<()> {
        ensure!(!self.nom.trim().is_empty() && !self.nom.contains(char::is_whitespace), hm_langue::tr!("nom de modèle vide ou avec un espace", "model name empty or containing a space"));
        ensure!(!self.role.trim().is_empty(), hm_langue::tr!("modèle « {} » sans rôle", "model \"{}\" has no role", self.nom));
        let p = &self.piece;
        ensure!(p.largeur[0] > 0.0 && p.largeur[0] <= p.largeur[1] && p.profondeur[0] > 0.0 && p.profondeur[0] <= p.profondeur[1], hm_langue::tr!("modèle « {} » : dimensions de pièce à l'envers", "model \"{}\": room dimensions reversed", self.nom));
        ensure!(!self.ensemble.is_empty(), hm_langue::tr!("modèle « {} » sans aucune pose", "model \"{}\" has no placement", self.nom));
        for e in &self.ensemble {
            ensure!(!e.pose.is_empty(), hm_langue::tr!("ensemble « {} » vide", "set \"{}\" is empty", e.nom));
            if let Some(r) = &e.repete {
                ensure!(r.pas.abs() >= 1.0 && (r.sens == "x" || r.sens == "y") && r.min >= 1 && r.min <= r.max && r.emprise[0] <= r.emprise[1], hm_langue::tr!("ensemble « {} » : répétition mal décrite", "set \"{}\": repetition badly described", e.nom));
                if let Some(b) = &r.bouche {
                    ensure!(b.travers[0] < b.travers[1] && b.hauteur >= 0.0, hm_langue::tr!("ensemble « {} » : coffrage mal décrit", "set \"{}\": casing badly described", e.nom));
                }
            }
        }
        Ok(())
    }

    /// Empreinte du modèle : elle change dès que son contenu change (pas pour un commentaire ou une mise en page).
    pub fn empreinte(&self) -> String {
        let canon = serde_json::to_vec(self).expect("modèle sérialisable");
        blake3::hash(&canon).to_hex()[..16].to_string()
    }

    /// Dit pourquoi une pièce de ces dimensions ne convient pas, ou `None` si elle convient.
    pub fn refuse(&self, largeur: f32, profondeur: f32, hauteur: f32) -> Option<String> {
        let p = &self.piece;
        if largeur < p.largeur[0] || largeur > p.largeur[1] {
            return Some(hm_langue::tr!("largeur {largeur:.0} hors de {:.0} à {:.0}", "width {largeur:.0} outside {:.0} to {:.0}", p.largeur[0], p.largeur[1]));
        }
        if profondeur < p.profondeur[0] || profondeur > p.profondeur[1] {
            return Some(hm_langue::tr!("profondeur {profondeur:.0} hors de {:.0} à {:.0}", "depth {profondeur:.0} outside {:.0} to {:.0}", p.profondeur[0], p.profondeur[1]));
        }
        if hauteur < p.hauteur_min {
            return Some(hm_langue::tr!("hauteur {hauteur:.0} sous {:.0}", "height {hauteur:.0} below {:.0}", p.hauteur_min));
        }
        None
    }

    /// Les poses du modèle dans une pièce de ces dimensions, dans le repère de la pièce vue depuis sa porte.
    pub fn poser(&self, largeur: f32, profondeur: f32, hauteur: f32) -> Result<Vec<PosePiece>> {
        Ok(self.disposer(largeur, profondeur, hauteur)?.0)
    }

    /// Les coffrages du modèle dans une pièce de ces dimensions.
    pub fn blocs(&self, largeur: f32, profondeur: f32, hauteur: f32) -> Result<Vec<Bloc>> {
        Ok(self.disposer(largeur, profondeur, hauteur)?.1)
    }

    /// Poses et coffrages du modèle dans une pièce de ces dimensions.
    pub fn disposer(&self, largeur: f32, profondeur: f32, hauteur: f32) -> Result<(Vec<PosePiece>, Vec<Bloc>)> {
        if let Some(raison) = self.refuse(largeur, profondeur, hauteur) {
            bail!(hm_langue::tr!("le modèle « {} » ne convient pas à cette pièce : {raison}", "model \"{}\" does not suit this room: {raison}", self.nom));
        }
        let mut out = Vec::new();
        let mut blocs = Vec::new();
        for e in &self.ensemble {
            if e.si.as_ref().map_or(false, |c| !c.remplie(largeur, profondeur, hauteur)) {
                continue;
            }
            let (mut ox, mut oy) = (e.x.en(largeur), e.y.en(profondeur));
            let (mut n, mut pas, mut le_long_x) = (1u32, 0.0f32, true);
            if let Some(r) = &e.repete {
                le_long_x = r.sens == "x";
                pas = r.pas;
                let total = if le_long_x { largeur } else { profondeur };
                let depart = if le_long_x { ox } else { oy };
                // place disponible devant le premier exemplaire, dans le sens du pas
                // une rangée centrée ne dépend pas de sa cote de départ : elle tient entre les deux murs, marge de
                // chaque côté
                let libre = if r.centrer {
                    total - 2.0 * r.marge - (r.emprise[1] - r.emprise[0])
                } else if pas > 0.0 {
                    total - r.marge - depart - r.emprise[1]
                } else {
                    depart + r.emprise[0] - r.marge
                };
                let tient = if libre < 0.0 { 0 } else { (libre / pas.abs()).floor() as u32 + 1 };
                n = tient.min(r.max);
                if n < r.min {
                    bail!(hm_langue::tr!("le modèle « {} » ne tient pas : l'ensemble « {} » demande {} exemplaire(s), il y a la place pour {}", "model \"{}\" does not fit: set \"{}\" needs {} copy(ies), there is room for {}", self.nom, e.nom, r.min, tient));
                }
                if r.centrer {
                    let etendue = (n - 1) as f32 * pas.abs() + (r.emprise[1] - r.emprise[0]);
                    let debut = (total - etendue) * 0.5 - r.emprise[0];
                    if le_long_x {
                        ox = debut;
                    } else {
                        oy = debut;
                    }
                    pas = pas.abs();
                }
            }
            // rangées : la rangée entière répétée dans l'autre sens, tant qu'elle reste à `marge` du mur
            let mut rangs: Vec<f32> = vec![0.0];
            if let Some(rg) = e.repete.as_ref().and_then(|r| r.rangs.as_ref()) {
                let (depart, total) = if le_long_x { (oy, profondeur) } else { (ox, largeur) };
                for j in 1..rg.max {
                    let ici = depart + j as f32 * rg.pas;
                    if rg.pas.abs() < 1.0 || ici < rg.marge || ici > total - rg.marge {
                        break;
                    }
                    rangs.push(j as f32 * rg.pas);
                }
            }
            if let Some((r, b)) = e.repete.as_ref().and_then(|r| r.bouche.as_ref().map(|b| (r, b))) {
                let (total, depart, cote) = if le_long_x { (largeur, ox, oy) } else { (profondeur, oy, ox) };
                let fin = depart + (n - 1) as f32 * pas;
                let (lo, hi) = (depart.min(fin) + r.emprise[0], depart.max(fin) + r.emprise[1]);
                let haut = if b.hauteur > 0.0 { b.hauteur.min(hauteur) } else { hauteur };
                for decal in &rangs {
                    let (t0, t1) = (cote + decal + b.travers[0], cote + decal + b.travers[1]);
                    for (a, z) in [(0.0, lo), (hi, total)] {
                        if z - a < b.min {
                            continue;
                        }
                        let (min, max) = if le_long_x { ([a, t0, 0.0], [z, t1, haut]) } else { ([t0, a, 0.0], [t1, z, haut]) };
                        blocs.push(Bloc { nom: format!("{} (coffrage)", e.nom), min, max, materiau: b.materiau.clone() });
                    }
                }
            }
            let (s, c) = e.lacet.to_radians().sin_cos();
            for (j, decal) in rangs.iter().enumerate() {
              for k in 0..n {
                let (bx, by) = if le_long_x { (ox + k as f32 * pas, oy + decal) } else { (ox + decal, oy + k as f32 * pas) };
                for p in &e.pose {
                    out.push(PosePiece {
                        ensemble: if n > 1 || rangs.len() > 1 { format!("{} {}", e.nom, j as u32 * n + k + 1) } else { e.nom.clone() },
                        modele: p.modele.clone(),
                        pos: [bx + p.pos[0] * c - p.pos[1] * s, by + p.pos[0] * s + p.pos[1] * c, e.z + p.pos[2]],
                        lacet: e.lacet + p.lacet,
                        classe: p.classe.clone(),
                        skin: p.skin,
                        nom: p.nom.clone(),
                    });
                }
              }
            }
        }
        Ok((out, blocs))
    }
}

// ---------------------------------------------------------------------------------------------- validations

/// La validation d'un modèle par l'utilisateur.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Validation {
    pub empreinte: String,
    pub quand: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Statut {
    Brouillon,
    Valide,
}

impl Statut {
    pub fn nom(self) -> &'static str {
        match self {
            Statut::Brouillon => "brouillon",
            Statut::Valide => "validé",
        }
    }
}

/// Les validations d'un dossier de modèles : `validations.json`, à côté des `.toml`.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Validations(pub BTreeMap<String, Validation>);

impl Validations {
    pub fn fichier(dossier: &Path) -> PathBuf {
        dossier.join("validations.json")
    }

    pub fn charger(dossier: &Path) -> Validations {
        std::fs::read(Validations::fichier(dossier)).ok().and_then(|o| serde_json::from_slice(&o).ok()).unwrap_or_default()
    }

    pub fn enregistrer(&self, dossier: &Path) -> Result<()> {
        std::fs::write(Validations::fichier(dossier), serde_json::to_string_pretty(self)? + "\n")?;
        Ok(())
    }

    /// Validé seulement si l'empreinte gardée est celle du modèle d'aujourd'hui.
    pub fn statut(&self, a: &Amenagement) -> Statut {
        match self.0.get(&a.nom) {
            Some(v) if v.empreinte == a.empreinte() => Statut::Valide,
            _ => Statut::Brouillon,
        }
    }

    pub fn valider(&mut self, a: &Amenagement, quand: &str) {
        self.0.insert(a.nom.clone(), Validation { empreinte: a.empreinte(), quand: quand.to_string() });
    }
}

/// Le modèle nommé d'un dossier, et son statut.
pub fn trouver(dossier: &Path, nom: &str) -> Result<(Amenagement, Statut)> {
    let a = Amenagement::lire(&dossier.join(format!("{nom}.toml")))?;
    ensure!(a.nom == nom, hm_langue::tr!("{}.toml déclare le nom « {} »", "{}.toml declares the name \"{}\"", nom, a.nom));
    let statut = Validations::charger(dossier).statut(&a);
    Ok((a, statut))
}

// ---------------------------------------------------------------------------------------------- pièce neutre

/// Le texte VMF d'une boîte pleine (un `solid` et ses six faces), numérotée à la suite de `id`.
pub fn boite(id: &mut u32, lo: [f32; 3], hi: [f32; 3], mat: &str) -> String {
    let ([x0, y0, z0], [x1, y1, z1]) = (lo, hi);
    let faces = [
        ([x0, y1, z1], [x1, y1, z1], [x1, y0, z1], "[1 0 0 0] 0.25", "[0 -1 0 0] 0.25"),
        ([x0, y0, z0], [x1, y0, z0], [x1, y1, z0], "[1 0 0 0] 0.25", "[0 -1 0 0] 0.25"),
        ([x0, y1, z1], [x0, y0, z1], [x0, y0, z0], "[0 1 0 0] 0.25", "[0 0 -1 0] 0.25"),
        ([x1, y1, z0], [x1, y0, z0], [x1, y0, z1], "[0 1 0 0] 0.25", "[0 0 -1 0] 0.25"),
        ([x1, y1, z1], [x0, y1, z1], [x0, y1, z0], "[1 0 0 0] 0.25", "[0 0 -1 0] 0.25"),
        ([x1, y0, z0], [x0, y0, z0], [x0, y0, z1], "[1 0 0 0] 0.25", "[0 0 -1 0] 0.25"),
    ];
    *id += 1;
    let mut s = format!("\tsolid\n\t{{\n\t\t\"id\" \"{id}\"\n");
    for (a, b, c, u, v) in faces {
        *id += 1;
        let p = |q: [f32; 3]| format!("({} {} {})", q[0], q[1], q[2]);
        s += &format!("\t\tside\n\t\t{{\n\t\t\t\"id\" \"{id}\"\n\t\t\t\"plane\" \"{} {} {}\"\n\t\t\t\"material\" \"{mat}\"\n\t\t\t\"uaxis\" \"{u}\"\n\t\t\t\"vaxis\" \"{v}\"\n\t\t\t\"lightmapscale\" \"16\"\n\t\t}}\n", p(a), p(b), p(c));
    }
    s + "\t}\n"
}

/// Largeur et hauteur de la baie de porte de la pièce neutre.
pub const BAIE: (f32, f32) = (56.0, 112.0);

/// Le VMF d'une pièce neutre meublée par le modèle : quatre murs, un sol, un plafond, une baie de porte au milieu du
/// mur de la porte, et les poses du modèle. L'air intérieur va de (0, 0, 0) à (largeur, profondeur, hauteur) ; le
/// repère de la pièce est celui du monde.
pub fn piece_neutre(a: &Amenagement, largeur: f32, profondeur: f32, hauteur: f32) -> Result<String> {
    let (poses, blocs) = a.disposer(largeur, profondeur, hauteur)?;
    let ou = |m: &str, defaut: &str| if m.is_empty() { defaut.to_string() } else { m.to_string() };
    let (sol, mur, plafond) = (ou(&a.decor.sol, "DEV/DEV_MEASUREGENERIC01B"), ou(&a.decor.mur, "DEV/DEV_MEASUREWALL01A"), ou(&a.decor.plafond, "DEV/DEV_MEASUREGENERIC01"));
    let (l, p, h, e) = (largeur, profondeur, hauteur, 16.0);
    let (g, d) = (l * 0.5 - BAIE.0 * 0.5, l * 0.5 + BAIE.0 * 0.5);
    let mut id = 1u32;
    let mut monde = String::new();
    monde += &boite(&mut id, [-e, -e - 192.0, -e], [l + e, p + e, 0.0], &sol);
    monde += &boite(&mut id, [-e, -e - 192.0, h], [l + e, p + e, h + e], &plafond);
    monde += &boite(&mut id, [-e, -e - 192.0, 0.0], [0.0, p + e, h], &mur);
    monde += &boite(&mut id, [l, -e - 192.0, 0.0], [l + e, p + e, h], &mur);
    monde += &boite(&mut id, [0.0, p, 0.0], [l, p + e, h], &mur);
    // le mur de la porte, en trois morceaux autour de la baie ; derrière lui, un bout de couloir fermé
    monde += &boite(&mut id, [0.0, -e, 0.0], [g, 0.0, h], &mur);
    monde += &boite(&mut id, [d, -e, 0.0], [l, 0.0, h], &mur);
    monde += &boite(&mut id, [g, -e, BAIE.1], [d, 0.0, h], &mur);
    monde += &boite(&mut id, [0.0, -e - 192.0 - e, 0.0], [l, -e - 192.0, h], &mur);
    let mut s = format!("versioninfo\n{{\n\t\"mapversion\" \"1\"\n}}\nworld\n{{\n\t\"id\" \"1\"\n\t\"classname\" \"worldspawn\"\n{monde}}}\n");
    id += 1;
    s += &format!("entity\n{{\n\t\"id\" \"{id}\"\n\t\"classname\" \"info_player_start\"\n\t\"origin\" \"{} -96 1\"\n\t\"angles\" \"0 90 0\"\n}}\n", l * 0.5);
    // la porte de la pièce, fermée : un vantail de 4 d'épaisseur dans la baie (elle n'appartient pas à l'aménagement)
    id += 1;
    let porte = boite(&mut id, [g, -10.0, 0.0], [d, -6.0, BAIE.1], &mur);
    id += 1;
    s += &format!("entity
{{
	\"id\" \"{id}\"
	\"classname\" \"func_door\"
	\"targetname\" \"porte_piece\"
	\"movedir\" \"0 0 0\"
{porte}}}
");
    for q in &poses {
        id += 1;
        s += &format!(
            "entity\n{{\n\t\"id\" \"{id}\"\n\t\"classname\" \"{}\"\n\t\"model\" \"{}\"\n\t\"origin\" \"{} {} {}\"\n\t\"angles\" \"0 {} 0\"\n\t\"skin\" \"{}\"\n\t\"solid\" \"6\"\n}}\n",
            q.classe, q.modele, q.pos[0], q.pos[1], q.pos[2], q.lacet, q.skin
        );
    }
    for b in &blocs {
        id += 1;
        let solide = boite(&mut id, b.min, b.max, if b.materiau.is_empty() { &mur } else { &b.materiau });
        id += 1;
        s += &format!("entity\n{{\n\t\"id\" \"{id}\"\n\t\"classname\" \"func_detail\"\n{solide}}}\n");
    }
    s += "cameras\n{\n\t\"activecamera\" \"-1\"\n}\n";
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WC: &str = r#"
nom = "wc_essai"
role = "wc"
[piece]
largeur = [200, 600]
profondeur = [200, 400]

[[ensemble]]
nom = "cabine"
x = { gauche = 50 }
y = { fond = 0 }
lacet = -90
repete = { pas = 100, emprise = [-50, 50], max = 4, min = 2 }
[[ensemble.pose]]
modele = "models/x/cabine.mdl"
[[ensemble.pose]]
modele = "models/x/cuvette.mdl"
pos = [20, 0, 0]

[[ensemble]]
nom = "lavabo"
x = { droite = 30 }
y = { porte = 12 }
lacet = 90
[[ensemble.pose]]
modele = "models/x/lavabo.mdl"
"#;

    fn wc() -> Amenagement {
        let a: Amenagement = toml::from_str(WC).unwrap();
        a.controler().unwrap();
        a
    }

    #[test]
    fn les_poses_suivent_la_piece() {
        let a = wc();
        // 320 de large : trois cabines de 100 tiennent (50, 150, 250) ; 600 : quatre au plus (max)
        let p = a.poser(320.0, 300.0, 128.0).unwrap();
        let cabines: Vec<&PosePiece> = p.iter().filter(|q| q.modele.ends_with("cabine.mdl")).collect();
        assert_eq!(cabines.iter().map(|q| q.pos[0]).collect::<Vec<_>>(), vec![50.0, 150.0, 250.0]);
        assert!(cabines.iter().all(|q| q.pos[1] == 300.0 && q.lacet == -90.0));
        assert_eq!(a.poser(600.0, 300.0, 128.0).unwrap().iter().filter(|q| q.modele.ends_with("cabine.mdl")).count(), 4);
        // la cuvette, 20 devant le dos de la cabine : tournée de -90°, elle est 20 plus près de la porte
        let cuvette = p.iter().find(|q| q.modele.ends_with("cuvette.mdl")).unwrap();
        assert!((cuvette.pos[0] - 50.0).abs() < 1e-3 && (cuvette.pos[1] - 280.0).abs() < 1e-3);
        // le lavabo reste à 30 du mur de droite, quelle que soit la largeur
        assert_eq!(p.iter().find(|q| q.modele.ends_with("lavabo.mdl")).unwrap().pos, [290.0, 12.0, 0.0]);
        assert_eq!(a.poser(500.0, 300.0, 128.0).unwrap().iter().find(|q| q.modele.ends_with("lavabo.mdl")).unwrap().pos[0], 470.0);
    }

    #[test]
    fn plusieurs_rangees_tant_qu_il_reste_de_la_place() {
        let texte = WC.replace("repete = { pas = 100, emprise = [-50, 50], max = 4, min = 2 }", "repete = { pas = 100, emprise = [-50, 50], max = 4, min = 2, rangs = { pas = -120, max = 3, marge = 100 } }");
        let a: Amenagement = toml::from_str(&texte).unwrap();
        let rangees = |profondeur: f32| {
            let mut y: Vec<i32> = a.poser(320.0, profondeur, 128.0).unwrap().iter().filter(|q| q.modele.ends_with("cabine.mdl")).map(|q| q.pos[1] as i32).collect();
            y.dedup();
            y
        };
        // 300 de profondeur : la rangée du fond (300) et une seconde à 180 ; la troisième tomberait à 60, sous la marge
        assert_eq!(rangees(300.0), vec![300, 180]);
        // 400 : trois rangées (400, 280, 160), le maximum
        assert_eq!(rangees(400.0), vec![400, 280, 160]);
        // 200 : la seconde tomberait à 80, sous la marge de 100
        assert_eq!(rangees(200.0), vec![200]);
    }

    #[test]
    fn le_coffrage_ferme_le_vide_au_bout_de_la_rangee() {
        let texte = WC.replace("max = 4, min = 2 }", "max = 4, min = 2, bouche = { travers = [-80, 0] } }");
        let a: Amenagement = toml::from_str(&texte).unwrap();
        // 320 de large : trois cabines de 100 (de 0 à 300), il reste 20 contre le mur de droite
        let b = a.blocs(320.0, 300.0, 128.0).unwrap();
        assert_eq!(b.len(), 1);
        assert_eq!((b[0].min, b[0].max), ([300.0, 220.0, 0.0], [320.0, 300.0, 128.0]));
        // 400 de large : quatre cabines, la rangée tombe juste, pas de coffrage
        assert!(a.blocs(400.0, 300.0, 128.0).unwrap().is_empty());
        // le coffrage est dans la pièce neutre, en func_detail
        assert!(piece_neutre(&a, 320.0, 300.0, 128.0).unwrap().contains("func_detail"));
        assert!(!piece_neutre(&a, 400.0, 300.0, 128.0).unwrap().contains("func_detail"));
    }

    #[test]
    fn un_ensemble_conditionnel_ne_vient_que_si_la_piece_s_y_prete() {
        let texte = WC.replace("[[ensemble]]\nnom = \"lavabo\"", "[[ensemble]]\nnom = \"lavabo\"\nsi = { largeur_min = 300, profondeur_max = 350 }");
        let a: Amenagement = toml::from_str(&texte).unwrap();
        let lavabos = |l: f32, p: f32| a.poser(l, p, 128.0).unwrap().iter().filter(|q| q.ensemble == "lavabo").count();
        assert_eq!(lavabos(320.0, 300.0), 1);
        assert_eq!(lavabos(280.0, 300.0), 0, "pièce trop étroite pour la condition");
        assert_eq!(lavabos(320.0, 380.0), 0, "pièce trop profonde pour la condition");
    }

    #[test]
    fn une_rangee_centree_ne_deborde_jamais_des_murs() {
        let texte = WC.replace("max = 4, min = 2 }", "max = 4, centrer = true }").replace("x = { gauche = 50 }", "x = { gauche = 0 }");
        let a: Amenagement = toml::from_str(&texte).unwrap();
        for largeur in [200.0f32, 230.0, 299.0, 300.0, 399.0, 400.0, 600.0] {
            let x: Vec<f32> = a.poser(largeur, 300.0, 128.0).unwrap().iter().filter(|q| q.modele.ends_with("cabine.mdl")).map(|q| q.pos[0]).collect();
            let (lo, hi) = (x.iter().cloned().fold(f32::MAX, f32::min) - 50.0, x.iter().cloned().fold(f32::MIN, f32::max) + 50.0);
            assert!(lo >= -0.01 && hi <= largeur + 0.01, "largeur {largeur} : rangée de {lo} à {hi}");
            assert!((lo - (largeur - hi)).abs() < 0.01, "largeur {largeur} : rangée pas centrée");
            assert_eq!(x.len() as u32, ((largeur / 100.0).floor() as u32).min(4));
        }
    }

    #[test]
    fn une_piece_qui_ne_convient_pas_est_refusee_avec_sa_raison() {
        let a = wc();
        assert!(a.poser(700.0, 300.0, 128.0).unwrap_err().to_string().contains("largeur 700"));
        assert!(a.poser(320.0, 100.0, 128.0).unwrap_err().to_string().contains("profondeur 100"));
        assert!(a.poser(320.0, 300.0, 90.0).unwrap_err().to_string().contains("hauteur 90"));
        // à la largeur minimale du modèle (200), les deux cabines exigées tiennent tout juste (en 50 et en 150)
        assert_eq!(a.poser(200.0, 300.0, 128.0).unwrap().iter().filter(|q| q.modele.ends_with("cabine.mdl")).count(), 2);
    }

    #[test]
    fn un_modele_valide_puis_modifie_repasse_en_brouillon() {
        let d = std::env::temp_dir().join(format!("hm_amenagement_{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("wc_essai.toml"), WC).unwrap();
        let (a, statut) = trouver(&d, "wc_essai").unwrap();
        assert_eq!(statut, Statut::Brouillon);
        let mut v = Validations::charger(&d);
        v.valider(&a, "2026-11-13 21:00:00");
        v.enregistrer(&d).unwrap();
        assert_eq!(trouver(&d, "wc_essai").unwrap().1, Statut::Valide);
        // un commentaire ou une ligne vide ne changent pas le modèle
        std::fs::write(d.join("wc_essai.toml"), format!("# relu le 14/11\n{WC}\n")).unwrap();
        assert_eq!(trouver(&d, "wc_essai").unwrap().1, Statut::Valide);
        // déplacer le lavabo de 2 unités : brouillon
        std::fs::write(d.join("wc_essai.toml"), WC.replace("y = { porte = 12 }", "y = { porte = 14 }")).unwrap();
        assert_eq!(trouver(&d, "wc_essai").unwrap().1, Statut::Brouillon);
        // remis comme avant : la validation d'origine vaut de nouveau
        std::fs::write(d.join("wc_essai.toml"), WC).unwrap();
        assert_eq!(trouver(&d, "wc_essai").unwrap().1, Statut::Valide);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn la_piece_neutre_est_un_vmf_avec_ses_poses() {
        let a = wc();
        let vmf = piece_neutre(&a, 320.0, 300.0, 128.0).unwrap();
        assert_eq!(vmf.matches("\tsolid\n").count(), 10); // neuf solides du monde et le vantail de la porte
        assert_eq!(vmf.matches("\"classname\" \"prop_static\"").count(), 7); // 3 cabines, 3 cuvettes, 1 lavabo
        assert!(vmf.contains("\"origin\" \"290 12 0\"") && vmf.contains("info_player_start"));
        assert_eq!(vmf.matches('{').count(), vmf.matches('}').count());
    }
}
