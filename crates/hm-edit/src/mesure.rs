//! Ce qu'il faut mesurer sur la map avant d'aménager une pièce : où est sa porte, quelle est sa hauteur, de quoi
//! sont ses murs, et ce qu'elle contient déjà. La mesure se fait une fois, à la création de la retouche ; le
//! journal garde le résultat, et le rejouer ne demande plus que le VMF.
use glam::Vec3;
use hm_model::{Modele, Piece};
use hm_regles::monde::{Corps, Genre};
use hm_regles::Monde;
use hm_scene::Scene;

use crate::{Ancre, Ent};

/// Ce qui n'est pas du mobilier : menuiseries, éclairage, équipements pris dans les murs. Un modèle dont le nom
/// contient l'un de ces mots reste en place quand on vide une pièce.
pub const BATI: [&str; 30] = [
    "cadre_", "porte", "fenetre", "vitre", "vitrage", "barreaux", "grille", "escalier", "garde_corps", "main_courante", "asc_", "cloison", "radiateur", "clim", "ventilation", "camera", "lampe_", "suspension", "applique", "neon", "dalle_led",
    "plaque", "affiche", "tableau", "interrupteur", "pilastre", "pilier", "poteau", "rideau_force", "ecran_mural",
];

fn dur(k: &Corps, _: u32) -> bool {
    matches!(k.genre, Genre::Monde | Genre::Detail | Genre::Brosse)
}

/// Hauteur sous plafond : celle du modèle sémantique s'il la donne, sinon la plus basse mesurée en cinq points.
pub fn hauteur(monde: &Monde, piece: &Piece) -> Option<f32> {
    if let Some(h) = piece.hauteur.filter(|h| *h > 0.0) {
        return Some(h);
    }
    let [x0, y0, x1, y1] = piece.emprise;
    let mut h: Option<f32> = None;
    for (fx, fy) in [(0.5f32, 0.5f32), (0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
        let a = Vec3::new(x0 + (x1 - x0) * fx, y0 + (y1 - y0) * fy, piece.sol + 8.0);
        if let Some((t, _)) = monde.rayon(a, a + Vec3::Z * 1024.0, dur) {
            let v = 8.0 + t * 1024.0;
            h = Some(h.map_or(v, |x| x.min(v)));
        }
    }
    h.map(|v| (v + 0.01).floor())
}

/// Les quatre murs d'une pièce : nom, point de départ, direction le long du mur, direction vers le dehors, longueur.
fn murs(piece: &Piece) -> [(&'static str, Vec3, Vec3, Vec3, f32); 4] {
    let [x0, y0, x1, y1] = piece.emprise;
    let z = piece.sol;
    [
        ("y-", Vec3::new(x0, y0, z), Vec3::X, -Vec3::Y, x1 - x0),
        ("y+", Vec3::new(x0, y1, z), Vec3::X, Vec3::Y, x1 - x0),
        ("x-", Vec3::new(x0, y0, z), Vec3::Y, -Vec3::X, y1 - y0),
        ("x+", Vec3::new(x1, y0, z), Vec3::Y, Vec3::X, y1 - y0),
    ]
}

/// Une ouverture ou une porte dans un mur de la pièce.
#[derive(Clone, Debug, PartialEq)]
pub struct Passage {
    pub mur: &'static str,
    /// Milieu du passage, compté le long du mur depuis son début.
    pub milieu: f32,
    pub largeur: f32,
    /// Écart entre le milieu du passage et le milieu du mur.
    pub decentre: f32,
    pub porte: bool,
}

/// Les passages de la pièce : les baies vues par des rayons tirés à travers chaque mur à hauteur de poitrine, et
/// les portes posées dans un mur.
pub fn passages(scene: &Scene, monde: &Monde, piece: &Piece) -> Vec<Passage> {
    let mut out: Vec<Passage> = Vec::new();
    for (nom, depart, le_long, dehors, longueur) in murs(piece) {
        // une baie : au moins 32 de large où un rayon sort de la pièce sans rien toucher de dur
        let mut debut: Option<f32> = None;
        let mut s = 6.0f32;
        while s <= longueur - 2.0 {
            let p = depart + le_long * s + Vec3::Z * 40.0;
            let libre = s <= longueur - 6.0 && monde.rayon(p - dehors * 12.0, p + dehors * 40.0, dur).is_none();
            match (libre, debut) {
                (true, None) => debut = Some(s),
                (false, Some(d)) => {
                    if s - d >= 32.0 {
                        out.push(Passage { mur: nom, milieu: (d + s - 4.0) * 0.5, largeur: s - d, decentre: (d + s - 4.0) * 0.5 - longueur * 0.5, porte: false });
                    }
                    debut = None;
                }
                _ => {}
            }
            s += 4.0;
        }
    }
    // les portes : un objet de classe « door » dont le centre est dans l'épaisseur d'un mur
    for o in scene.objects.iter().filter(|o| o.class.contains("door")) {
        let c = (o.min + o.max) * 0.5;
        if c.z < piece.sol - 8.0 || c.z > piece.sol + 160.0 {
            continue;
        }
        for (nom, depart, le_long, dehors, longueur) in murs(piece) {
            let (s, d) = ((c - depart).dot(le_long), (c - depart).dot(dehors));
            if s > 0.0 && s < longueur && d > -20.0 && d < 28.0 {
                match out.iter_mut().find(|p| p.mur == nom && (p.milieu - s).abs() < p.largeur * 0.5 + 16.0) {
                    Some(p) => p.porte = true,
                    None => out.push(Passage { mur: nom, milieu: s, largeur: (o.max - o.min).dot(le_long).abs().max(32.0), decentre: s - longueur * 0.5, porte: true }),
                }
            }
        }
    }
    out
}

/// Matériau du mur du fond (celui d'en face de la porte), pour que les coffrages s'y fondent.
pub fn materiau_du_fond(scene: &Scene, monde: &Monde, piece: &Piece, porte: &str, hauteur: f32) -> String {
    let c = Vec3::from(piece.centre()) + Vec3::Z * (hauteur * 0.5);
    let (dir, portee) = match porte {
        "y-" => (Vec3::Y, piece.profondeur()),
        "y+" => (-Vec3::Y, piece.profondeur()),
        "x-" => (Vec3::X, piece.largeur()),
        _ => (-Vec3::X, piece.largeur()),
    };
    match monde.rayon(c, c + dir * (portee * 0.5 + 64.0), |k, _| matches!(k.genre, Genre::Monde | Genre::Detail)) {
        Some((_, i)) => scene.objects[monde.corps[i as usize].objet as usize].material.clone(),
        None => String::new(),
    }
}

/// Ce que la pièce contient déjà, trié.
#[derive(Clone, Debug, Default)]
pub struct Contenu {
    /// Le mobilier, à retirer avant d'aménager.
    pub retirer: Vec<Ancre>,
    /// Ce qui reste en place, avec la raison.
    pub gardes: Vec<String>,
    /// Modèles fusionnés dont une partie seulement est dans la pièce : les retirer viderait aussi la pièce d'à côté.
    pub partages: Vec<String>,
    /// Portes de cloisons intérieures : la pièce n'est pas un rectangle libre.
    pub cloisons: usize,
}

fn dedans(piece: &Piece, p: Vec3, hauteur: f32) -> bool {
    p.x >= piece.emprise[0] && p.x <= piece.emprise[2] && p.y >= piece.emprise[1] && p.y <= piece.emprise[3] && p.z >= piece.sol - 8.0 && p.z <= piece.sol + hauteur + 1.0
}

pub fn contenu(scene: &Scene, monde: &Monde, sem: &Modele, piece: &Piece, hauteur: f32, garder: &[String]) -> Contenu {
    let mut out = Contenu::default();
    let court = |m: &str| m.rsplit('/').next().unwrap_or(m).trim_end_matches(".mdl").to_string();
    // les cadres de porte posés dans la map : une porte à côté de son cadre est une porte de cloison
    let cadres: Vec<Vec3> = scene.props.iter().filter(|p| court(&scene.objects[p.object as usize].model).starts_with("cadre_")).map(|p| p.transform.w_axis.truncate()).collect();
    for p in &scene.props {
        let o = &scene.objects[p.object as usize];
        let pos = p.transform.w_axis.truncate();
        let ancre = || monde.entites.get(&o.vmf_id).map(|n| Ent::de(n).ancre());
        if let Some(a) = sem.assemblages.iter().find(|a| a.modele.eq_ignore_ascii_case(&o.model)) {
            let n = a.membres.iter().filter(|m| dedans(piece, p.transform.transform_point3(Vec3::from(m.pos)), hauteur)).count();
            if n == 0 {
                continue;
            }
            let bati = a.membres.iter().filter(|m| BATI.iter().any(|b| court(&m.modele).contains(b))).count();
            if n < a.membres.len() {
                out.partages.push(format!("{} : {n} meuble(s) sur {} dans la pièce", court(&o.model), a.membres.len()));
            } else if bati == a.membres.len() {
                if a.membres.iter().any(|m| court(&m.modele).contains("cloison")) {
                    out.cloisons += 1;
                }
                out.gardes.push(format!("{} ({} pièce(s) de bâti fusionnées, dont {}) : bâti", court(&o.model), bati, court(&a.membres[0].modele)));
            } else if bati > 0 {
                out.partages.push(format!("{} : mêle {bati} pièce(s) de bâti et {} meuble(s)", court(&o.model), a.membres.len() - bati));
            } else if let Some(an) = ancre() {
                out.retirer.push(an);
            }
            continue;
        }
        if !dedans(piece, pos, hauteur) {
            continue;
        }
        let nom = court(&o.model);
        // une porte prise dans un mur de la pièce reste ; une porte de cabine, au milieu de la pièce, est du mobilier
        let au_mur = (pos.x - piece.emprise[0]).min(piece.emprise[2] - pos.x).min(pos.y - piece.emprise[1]).min(piece.emprise[3] - pos.y) < 24.0;
        let porte = o.class.contains("door");
        // … sauf si elle est prise entre deux murs en dur : c'est la porte d'une cloison intérieure
        let en_cloison = porte && !au_mur && (cadres.iter().any(|c| (c.truncate() - pos.truncate()).length() < 48.0 && (c.z - pos.z).abs() < 96.0)) || porte && !au_mur && {
            let (c, e) = ((o.min + o.max) * 0.5, (o.max - o.min) * 0.5);
            let axe = if e.x >= e.y { Vec3::X } else { Vec3::Y };
            let demi = e.x.max(e.y);
            [axe, -axe].iter().all(|d| monde.dans(c + *d * (demi + 8.0), dur).is_some() || monde.rayon(c + *d * (demi - 2.0), c + *d * (demi + 20.0), dur).is_some())
        };
        let raison = if porte && au_mur {
            Some("porte dans un mur")
        } else if en_cloison {
            out.cloisons += 1;
            Some("porte d'une cloison intérieure")
        } else if porte {
            // même nommée : la laisser, ce serait une porte sans cabine au milieu de la pièce
            None
        } else if !o.name.is_empty() {
            Some("porte un nom : peut servir à un script")
        } else if BATI.iter().any(|b| nom.contains(b)) || garder.iter().any(|g| nom.contains(g.as_str())) {
            Some("bâti ou éclairage")
        } else if o.min.z > piece.sol + 64.0 && o.max.z >= piece.sol + hauteur - 4.0 {
            Some("accroché au plafond")
        } else {
            None
        };
        match (raison, ancre()) {
            (Some(r), _) => out.gardes.push(format!("{nom} en ({:.0} {:.0} {:.0}) : {r}", pos.x, pos.y, pos.z)),
            (None, Some(a)) => out.retirer.push(a),
            (None, None) => out.gardes.push(format!("{nom} : entité introuvable dans le VMF")),
        }
    }
    out
}

/// Tout ce qu'il faut pour aménager une pièce, mesuré sur la map : à montrer avant d'écrire la retouche.
#[derive(Clone, Debug)]
pub struct Plan {
    pub piece: Piece,
    pub modele: String,
    pub statut: hm_model::amenagement::Statut,
    pub porte: String,
    pub passages: Vec<Passage>,
    pub hauteur: f32,
    pub mur: String,
    pub largeur: f32,
    pub profondeur: f32,
    pub poses: usize,
    pub coffrages: usize,
    pub contenu: Contenu,
    pub avertissements: Vec<String>,
}

/// Mesure la pièce et prépare son aménagement. `porte` : le mur de la porte si on veut l'imposer (`x-`, `x+`, `y-`,
/// `y+`) ; sinon il est cherché, et s'il y a des passages sur plusieurs murs il faut le dire.
pub fn preparer(scene: &Scene, monde: &Monde, sem: &Modele, amenagements: &std::path::Path, piece: &str, modele: &str, porte: Option<&str>, garder: &[String]) -> anyhow::Result<Plan> {
    use anyhow::{anyhow, bail};
    let p = sem.pieces.iter().find(|p| p.id == piece).ok_or_else(|| anyhow!("la pièce « {piece} » n'est pas dans le modèle sémantique"))?;
    let (a, statut) = hm_model::amenagement::trouver(amenagements, modele)?;
    if a.role != p.role {
        bail!("le modèle « {modele} » meuble le rôle « {} », la pièce « {piece} » est de rôle « {} »", a.role, p.role);
    }
    let h = hauteur(monde, p).ok_or_else(|| anyhow!("pièce « {piece} » : aucun plafond trouvé au-dessus de son sol"))?;
    let passages = passages(scene, monde, p);
    let mut avertissements = Vec::new();
    let porte = match porte {
        Some(m) => m.to_string(),
        None => {
            let murs_de = |portes: bool| {
                let mut m: Vec<&str> = passages.iter().filter(|x| x.porte || !portes).map(|x| x.mur).collect();
                m.sort();
                m.dedup();
                m
            };
            // d'abord les murs qui ont une vraie porte, sinon ceux qui ont une baie
            let candidats = if murs_de(true).is_empty() { murs_de(false) } else { murs_de(true) };
            match candidats.as_slice() {
                [un] => un.to_string(),
                [] => bail!("pièce « {piece} » : aucune porte ni baie trouvée dans ses murs ; dire le mur de la porte (x-, x+, y- ou y+)"),
                plusieurs => bail!("pièce « {piece} » : des passages sur plusieurs murs ({}) ; dire lequel est celui de la porte", plusieurs.join(", ")),
            }
        }
    };
    let (_, _, largeur, profondeur) = crate::repere(p.emprise, &porte).map_err(|e| anyhow!(e))?;
    let (poses, blocs) = a.disposer(largeur, profondeur, h).map_err(|e| anyhow!("pièce « {piece} » ({largeur:.0} × {profondeur:.0} × {h:.0}, porte {porte}) : {e:#}"))?;
    for x in &passages {
        if x.mur == porte && x.decentre.abs() > 24.0 {
            avertissements.push(format!("le passage du mur {} est à {:.0} du milieu : le modèle suppose une porte au milieu, les règles diront si un meuble la gêne", x.mur, x.decentre.abs()));
        }
        if x.mur != porte {
            avertissements.push(format!("autre passage ({}) sur le mur {} : un meuble peut tomber devant", if x.porte { "porte" } else { "baie" }, x.mur));
        }
    }
    let contenu = contenu(scene, monde, sem, p, h, garder);
    let mur = materiau_du_fond(scene, monde, p, &porte, h);
    Ok(Plan { piece: p.clone(), modele: modele.to_string(), statut, porte, passages, hauteur: h, mur, largeur, profondeur, poses: poses.len(), coffrages: blocs.len(), contenu, avertissements })
}

impl Plan {
    /// Ce qui empêche d'aménager : un modèle fusionné à cheval sur la pièce et sa voisine.
    pub fn bloque(&self) -> Option<String> {
        if !self.contenu.partages.is_empty() {
            return Some(format!("le mobilier de cette pièce est pris dans des modèles fusionnés qu'on ne peut pas retirer seuls ({}) : ils débordent sur d'autres pièces ou mêlent bâti et mobilier. Défaire cette fusion dans le générateur d'abord.", self.contenu.partages.join(" ; ")));
        }
        (self.contenu.cloisons > 0).then(|| "la pièce est recoupée par une cloison intérieure (bureau vitré, local) : un modèle d'aménagement meuble un rectangle libre. Déclarer les deux parties comme deux pièces dans le modèle sémantique, ou choisir une autre pièce.".to_string())
    }

    pub fn decrire(&self) -> String {
        let mut t = format!(
            "pièce « {} » ({}) : {:.0} × {:.0} × {:.0} vue de sa porte (mur {}), sol à {:.0}\nmodèle « {} » ({}) : {} pose(s), {} coffrage(s) ; murs : {}",
            self.piece.id,
            self.piece.role,
            self.largeur,
            self.profondeur,
            self.hauteur,
            self.porte,
            self.piece.sol,
            self.modele,
            self.statut.nom(),
            self.poses,
            self.coffrages,
            if self.mur.is_empty() { "matériau non trouvé" } else { &self.mur }
        );
        for x in &self.passages {
            t += &format!("\npassage : mur {}, {} de {:.0} de large, son milieu à {:.0} du début du mur ({:.0} du milieu)", x.mur, if x.porte { "porte" } else { "baie" }, x.largeur, x.milieu, x.decentre);
        }
        t += &format!("\nà retirer : {} objet(s)", self.contenu.retirer.len());
        let mut par_modele: std::collections::BTreeMap<String, usize> = Default::default();
        for a in &self.contenu.retirer {
            *par_modele.entry(a.modele.rsplit('/').next().unwrap_or("").trim_end_matches(".mdl").to_string()).or_default() += 1;
        }
        if !par_modele.is_empty() {
            t += &format!(" ({})", par_modele.iter().map(|(m, n)| if *n > 1 { format!("{n} {m}") } else { m.clone() }).collect::<Vec<_>>().join(", "));
        }
        t += &format!("\nlaissé en place : {} objet(s)", self.contenu.gardes.len());
        for g in self.contenu.gardes.iter().take(12) {
            t += &format!("\n  {g}");
        }
        if self.contenu.gardes.len() > 12 {
            t += &format!("\n  … et {} autre(s)", self.contenu.gardes.len() - 12);
        }
        for a in &self.avertissements {
            t += &format!("\nattention : {a}");
        }
        if let Some(b) = self.bloque() {
            t += &format!("\nBLOQUÉ : {b}");
        }
        if self.statut == hm_model::amenagement::Statut::Brouillon {
            t += &format!("\nBROUILLON : le modèle « {} » n'est pas validé. La pièce sera meublée, mais la map ne passera pas la validation (sem.amenagement_brouillon) tant que l'utilisateur ne l'a pas validé dans un terminal.", self.modele);
        }
        t
    }
}
