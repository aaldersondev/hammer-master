//! Les commandes du menu Outils de Hammer qui disposent la sélection (wiki Valve « Hammer Tools Menu ») : aligner
//! des objets entre eux, caler chacun sur la grille, transformer par valeurs (« Transform », Ctrl+M) ; et ce que
//! compte « Map > Show Information ». Chacune rend une commande du noyau : une seule étape d'annulation.
use std::collections::BTreeMap;

use glam::DVec3;
use hm_doc::{boite_de, Commande, Document, Ref};

fn lot(nom: &str, mut commandes: Vec<Commande>) -> Option<Commande> {
    match commandes.len() {
        0 => None,
        1 => commandes.pop(),
        _ => Some(Commande::Lot { nom: nom.into(), commandes }),
    }
}

fn boites(doc: &Document, objets: &[Ref]) -> Vec<(Ref, DVec3, DVec3)> {
    objets.iter().filter_map(|r| boite_de(doc, std::slice::from_ref(r)).map(|(lo, hi)| (*r, lo, hi))).collect()
}

fn le_long(axe: usize, d: f64) -> [f64; 3] {
    let mut v = [0.0; 3];
    v[axe] = d;
    v
}

/// « Align objects » : l'objet le plus loin dans la direction (axe `axe`, côté haut si `vers_max`) ne bouge pas, les
/// autres sont déplacés jusqu'à son bord.
pub fn aligner(doc: &Document, objets: &[Ref], axe: usize, vers_max: bool, verrou_texture: bool) -> Option<Commande> {
    let b = boites(doc, objets);
    let bord = |lo: DVec3, hi: DVec3| if vers_max { hi[axe] } else { lo[axe] };
    let cible = b.iter().map(|(_, lo, hi)| bord(*lo, *hi)).reduce(if vers_max { f64::max } else { f64::min })?;
    let commandes = b
        .iter()
        .filter(|(_, lo, hi)| (cible - bord(*lo, *hi)).abs() > 1e-9)
        .map(|(r, lo, hi)| Commande::Deplacer { objets: vec![*r], delta: le_long(axe, cible - bord(*lo, *hi)), verrou_texture })
        .collect();
    lot("aligner", commandes)
}

/// « Snap Selected to Grid Individually » : le coin bas (x, y et z les plus petits) de chaque objet sur la grille.
pub fn caler_chacun(doc: &Document, objets: &[Ref], grille: f64, verrou_texture: bool) -> Option<Commande> {
    let commandes = boites(doc, objets)
        .into_iter()
        .filter_map(|(r, lo, _)| {
            let d = (lo / grille).round() * grille - lo;
            (d.length() > 1e-9).then(|| Commande::Deplacer { objets: vec![r], delta: d.to_array(), verrou_texture })
        })
        .collect();
    lot(hm_langue::t("caler chaque objet sur la grille"), commandes)
}

/// Ce que fait le dialogue « Transform » : tourner (degrés autour des axes X, puis Y, puis Z du monde, sens direct),
/// mettre à l'échelle (facteurs par axe) ou déplacer (unités), autour du centre de la sélection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Transformation {
    Tourner([f64; 3]),
    Echelle([f64; 3]),
    Deplacer([f64; 3]),
}

/// La commande de la transformation, `None` s'il n'y a rien à faire ; une échelle nulle ou négative est refusée.
pub fn transformer(doc: &Document, objets: &[Ref], t: Transformation, verrou_texture: bool) -> Result<Option<Commande>, String> {
    let Some((lo, hi)) = boite_de(doc, objets) else { return Ok(None) };
    let centre = (lo + hi) * 0.5;
    let objets = objets.to_vec();
    Ok(match t {
        Transformation::Tourner(a) => {
            let commandes = (0..3).filter(|i| a[*i].abs() > 1e-9).map(|i| Commande::Tourner { objets: objets.clone(), centre: centre.to_array(), axe: le_long(i, 1.0), degres: a[i], verrou_texture }).collect();
            lot(hm_langue::t("transformer (tourner)"), commandes)
        }
        Transformation::Echelle(s) => {
            if s.iter().any(|f| *f <= 0.0) {
                return Err(hm_langue::t("facteur d'échelle nul ou négatif (pour retourner, Outils → Miroir)").into());
            }
            let s = DVec3::from(s);
            if (s - DVec3::ONE).length() < 1e-12 {
                return Ok(None);
            }
            let vers = [(centre + (lo - centre) * s).to_array(), (centre + (hi - centre) * s).to_array()];
            Some(Commande::Redimensionner { objets, de: [lo.to_array(), hi.to_array()], vers, verrou_texture })
        }
        Transformation::Deplacer(d) => (DVec3::from(d).length() > 1e-12).then(|| Commande::Deplacer { objets, delta: d, verrou_texture }),
    })
}

/// Ce que montre « Map > Show Information ».
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Infos {
    pub solides: usize,
    pub faces: usize,
    pub displacements: usize,
    pub entites_ponctuelles: usize,
    pub entites_a_brosses: usize,
    /// Matériaux employés et leur nombre de faces, du plus employé au moins employé.
    pub materiaux: Vec<(String, usize)>,
}

pub fn infos(doc: &Document) -> Infos {
    let mut i = Infos::default();
    let mut mats: BTreeMap<String, usize> = BTreeMap::new();
    for (_, s) in doc.tous_les_solides() {
        i.solides += 1;
        for f in &s.faces {
            i.faces += 1;
            i.displacements += f.disp.is_some() as usize;
            *mats.entry(f.materiau.to_ascii_lowercase()).or_default() += 1;
        }
    }
    for e in doc.entites() {
        if e.solides.is_empty() {
            i.entites_ponctuelles += 1;
        } else {
            i.entites_a_brosses += 1;
        }
    }
    i.materiaux = mats.into_iter().collect();
    i.materiaux.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    i
}
