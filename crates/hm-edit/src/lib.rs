//! Édition « métier » au-dessus du noyau `hm-doc` : aménager une pièce d'après un modèle d'aménagement, construire
//! une map depuis une spec, comparer deux VMF. Toute modification d'une map est une commande du noyau
//! ([`hm_doc::Commande`]) : celles d'ici en fabriquent, elles n'écrivent jamais un fichier elles-mêmes.
use anyhow::{anyhow, Result};
use hm_doc::{Commande, Ref};
use hm_vmf::Node;
use serde::{Deserialize, Serialize};

pub mod construction;
/// Aligner, caler chaque objet, transformer par valeurs, infos de la map (menus Outils et Carte de Hammer).
pub mod disposition;
/// Export de la géométrie en DXF (« Export to .DXF » de Hammer).
pub mod dxf;
pub mod difference;
/// Fichiers qu'un BSP doit embarquer : ceux que les joueurs n'ont pas (partagé par la fenêtre et le MCP).
pub mod embarque;
pub mod mesure;
/// Propper++ : des brosses en un modèle (SMD, collision, QC, VMT de modèle).
pub mod propper;
/// Semer des entités au pinceau (« Sprinkle » de Hammer++, types au même format).
pub mod semis;
/// Contrôles façon Hammer (Alt+P) et règles, en problèmes avec leur correction (partagé par la fenêtre et le MCP).
pub mod verification;

/// Ce qui désigne une entité d'une génération à l'autre : sa classe, son modèle, son nom et sa place.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Ancre {
    pub classe: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub nom: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub modele: String,
    #[serde(default)]
    pub origine: Option<[f64; 3]>,
    /// Numéro dans le VMF le jour de la retouche : un indice pour départager deux jumeaux, pas une identité.
    #[serde(default)]
    pub id: u64,
}

impl Ancre {
    pub fn decrire(&self) -> String {
        let quoi = if !self.modele.is_empty() { self.modele.rsplit('/').next().unwrap_or("").trim_end_matches(".mdl").to_string() } else { self.classe.clone() };
        let nom = if self.nom.is_empty() { String::new() } else { format!(" « {} »", self.nom) };
        match self.origine {
            Some(o) => format!("{quoi}{nom} en ({:.0} {:.0} {:.0})", o[0], o[1], o[2]),
            None => format!("{quoi}{nom}"),
        }
    }
}

/// Ce qu'on sait d'une entité sans relire son bloc.
#[derive(Clone, Debug, Default)]
pub struct Ent {
    pub id: u64,
    pub classe: String,
    pub nom: String,
    pub modele: String,
    pub origine: Option<[f64; 3]>,
    pub angles: Option<[f64; 3]>,
    /// Sans solides : on peut la déplacer en changeant son origine.
    pub ponctuelle: bool,
}

impl Ent {
    fn de(n: &Node) -> Ent {
        Ent {
            id: n.id().unwrap_or(0),
            classe: n.classname().to_string(),
            nom: n.get("targetname").unwrap_or("").to_string(),
            modele: n.get("model").unwrap_or("").replace('\\', "/").to_ascii_lowercase(),
            origine: n.vec3("origin"),
            angles: n.vec3("angles"),
            ponctuelle: n.solids().next().is_none(),
        }
    }

    pub fn ancre(&self) -> Ancre {
        Ancre { classe: self.classe.clone(), nom: self.nom.clone(), modele: self.modele.clone(), origine: self.origine, id: self.id }
    }
}

/// Un nombre comme Hammer l'écrit : sans zéros inutiles.
pub fn nombre(v: f64) -> String {
    let r = (v * 1000.0).round() / 1000.0;
    let r = if r == 0.0 { 0.0 } else { r };
    format!("{r}")
}

pub fn triplet(v: [f64; 3]) -> String {
    format!("{} {} {}", nombre(v[0]), nombre(v[1]), nombre(v[2]))
}

/// Le repère d'une pièce vue depuis sa porte, dans le monde : origine, angle, largeur, profondeur.
pub fn repere(emprise: [f32; 4], porte: &str) -> Result<([f32; 2], f32, f32, f32), String> {
    let [x0, y0, x1, y1] = emprise;
    Ok(match porte {
        "y-" => ([x0, y0], 0.0, x1 - x0, y1 - y0),
        "x+" => ([x1, y0], 90.0, y1 - y0, x1 - x0),
        "y+" => ([x1, y1], 180.0, x1 - x0, y1 - y0),
        "x-" => ([x0, y1], -90.0, y1 - y0, x1 - x0),
        autre => return Err(format!("mur de porte « {autre} » inconnu (x-, x+, y- ou y+)")),
    })
}

fn vers_monde(o: [f32; 2], angle: f32, p: [f32; 2]) -> [f32; 2] {
    // quarts de tour exacts : pas d'arrondi de sinus sur des cotes entières
    let (c, s) = match angle.rem_euclid(360.0) as i32 {
        0 => (1.0, 0.0),
        90 => (0.0, 1.0),
        180 => (-1.0, 0.0),
        _ => (0.0, -1.0),
    };
    [o[0] + p[0] * c - p[1] * s, o[1] + p[0] * s + p[1] * c]
}


/// Les entités (poses) et coffrages (blocs, en func_detail) d'un aménagement disposé, ramenés du repère de la pièce
/// (origine `o`, angle, sol) au monde ; noms uniques dans `doc`.
fn poses_et_blocs(a: &hm_model::amenagement::Amenagement, poses: &[hm_model::amenagement::PosePiece], blocs: &[hm_model::amenagement::Bloc], o: [f32; 2], angle: f32, sol: f32, mur: &str, doc: &hm_doc::Document) -> Vec<Commande> {
    let mut commandes = Vec::new();
    // un nom unique dans la map : le nom du modèle, puis un rang libre
    let pris: std::collections::HashSet<String> = doc.entites().map(|e| e.nom().to_string()).filter(|n| !n.is_empty()).collect();
    let mut rang = 0u32;
    for q in poses {
        let m = vers_monde(o, angle, [q.pos[0], q.pos[1]]);
        let mut cles = vec![
            ("model".to_string(), q.modele.clone()),
            ("angles".to_string(), triplet([0.0, (q.lacet + angle).rem_euclid(360.0) as f64, 0.0])),
            ("skin".to_string(), q.skin.to_string()),
        ];
        if q.classe == "prop_static" {
            cles.push(("solid".to_string(), "6".to_string()));
        }
        if !q.nom.is_empty() {
            let nom = loop {
                rang += 1;
                let n = format!("{}_{rang:02}", q.nom);
                if !pris.contains(&n) {
                    break n;
                }
            };
            cles.push(("targetname".to_string(), nom));
        }
        commandes.push(Commande::CreerEntite { classe: q.classe.clone(), origine: [m[0] as f64, m[1] as f64, (sol + q.pos[2]) as f64], cles });
    }
    for b in blocs {
        let (u, v) = (vers_monde(o, angle, [b.min[0], b.min[1]]), vers_monde(o, angle, [b.max[0], b.max[1]]));
        let lo = [u[0].min(v[0]) as f64, u[1].min(v[1]) as f64, (sol + b.min[2]) as f64];
        let hi = [u[0].max(v[0]) as f64, u[1].max(v[1]) as f64, (sol + b.max[2]) as f64];
        let mat = if !b.materiau.is_empty() { b.materiau.clone() } else if !mur.is_empty() { mur.to_string() } else { a.decor.mur.clone() };
        let mat = if mat.is_empty() { "DEV/DEV_MEASUREWALL01A".to_string() } else { mat };
        commandes.push(Commande::CreerBloc { min: lo, max: hi, materiau: mat, classe: Some("func_detail".into()) });
    }
    commandes
}

/// La commande qui aménage une boîte (la sélection, dans la fenêtre) d'après le modèle `a`, la porte sur le mur
/// `porte` (`x-`, `x+`, `y-`, `y+`) : poses et coffrages en une seule étape d'annulation. Refusée si la boîte n'a pas
/// les dimensions que le modèle accepte.
pub fn commande_amenagement_boite(a: &hm_model::amenagement::Amenagement, lo: [f32; 3], hi: [f32; 3], porte: &str, doc: &hm_doc::Document) -> Result<Commande> {
    let (o, angle, largeur, profondeur) = repere([lo[0], lo[1], hi[0], hi[1]], porte).map_err(|e| anyhow!(e))?;
    let hauteur = hi[2] - lo[2];
    if let Some(r) = a.refuse(largeur, profondeur, hauteur) {
        return Err(anyhow!("« {} » refuse une pièce de {largeur:.0} × {profondeur:.0} × {hauteur:.0} : {r}", a.nom));
    }
    let (poses, blocs) = a.disposer(largeur, profondeur, hauteur).map_err(|e| anyhow!("{e:#}"))?;
    let commandes = poses_et_blocs(a, &poses, &blocs, o, angle, lo[2], "", doc);
    Ok(Commande::Lot { nom: format!("aménager d'après « {} »", a.nom), commandes })
}

/// La commande qui aménage la pièce mesurée par `plan` : retire le mobilier relevé, pose celui du modèle et ses
/// coffrages, en une seule étape d'annulation. Les numéros des objets à retirer sont ceux du document mesuré.
pub fn commande_amenagement(plan: &mesure::Plan, amenagements: &std::path::Path, doc: &hm_doc::Document) -> Result<Commande> {
    use hm_model::amenagement;
    let p = &plan.piece;
    let (a, _) = amenagement::trouver(amenagements, &plan.modele).map_err(|e| anyhow!("modèle d'aménagement « {} » : {e:#}", plan.modele))?;
    let (o, angle, largeur, profondeur) = repere(p.emprise, &plan.porte).map_err(|e| anyhow!(e))?;
    let (poses, blocs) = a.disposer(largeur, profondeur, plan.hauteur).map_err(|e| anyhow!("pièce « {} » ({largeur:.0} × {profondeur:.0} × {:.0}) : {e:#}", p.id, plan.hauteur))?;
    let mut commandes = Vec::new();
    let retirer: Vec<Ref> = plan.contenu.retirer.iter().filter(|x| doc.entite(x.id).is_some()).map(|x| Ref::Entite(x.id)).collect();
    if retirer.len() != plan.contenu.retirer.len() {
        return Err(anyhow!("la pièce « {} » a changé depuis sa mesure : la mesurer de nouveau", p.id));
    }
    if !retirer.is_empty() {
        commandes.push(Commande::Supprimer { objets: retirer });
    }
    commandes.extend(poses_et_blocs(&a, &poses, &blocs, o, angle, p.sol, &plan.mur, doc));
    Ok(Commande::Lot { nom: format!("aménager « {} » d'après « {} »", p.id, plan.modele), commandes })
}
