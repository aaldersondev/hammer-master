//! Réseaux linéaires (voies, viaducs, routes en modèles) : où ils finissent, et sur quoi.
use std::collections::HashMap;

use glam::Vec3;
use hm_scene::{ObjKind, Scene};

use crate::monde::{Genre, Monde};
use crate::physique::contient;
use crate::{Config, Constat, Niveau, Regle};

pub const REGLES: &[Regle] = &[
    Regle { id: "phys.reseau_coupe", niveau: Niveau::Avertissement, titre: "Réseau coupé net au-dessus du vide", explication: "Une voie ou un viaduc s'arrête sans terminus, sans mur ni téléporteur, et le sol est plus bas au-delà du bout.", source: "contrôle propre à l'outil ; défaut de référence : le viaduc du M1 coupé à trois bouts, 10/11/2026" },
    Regle { id: "phys.reseau_sans_terminus", niveau: Niveau::Information, titre: "Réseau qui s'arrête sans terminus", explication: "Une voie s'arrête au niveau du sol sans butoir, mur ni téléporteur.", source: "contrôle propre à l'outil" },
];

/// Une extrémité libre d'un réseau.
#[derive(Clone, Debug)]
pub struct Bout {
    pub nom: String,
    /// Milieu du bout, sur le dessus de la pièce.
    pub pos: Vec3,
    /// Direction sortante.
    pub dir: Vec3,
    /// Rang de la pièce dans la scène ; `None` pour un bout de ligne de `path_track`.
    pub objet: Option<u32>,
}

/// Bouts de réseau : extrémités des chaînes de `path_track`, et extrémités des chaînes de modèles de voie qui se
/// touchent bout à bout. Les bouts à moins de 300 l'un de l'autre n'en font qu'un (deux rails parallèles, voie et
/// `path_track`). L'ordre est stable : le tour automatique numérote ses vues d'après lui.
pub fn bouts(scene: &Scene, motifs: &[&str]) -> Vec<Bout> {
    bouts_selon(scene, motifs, false)
}

/// Comme `bouts`, avec un contrôle de plus quand `strict` est vrai : un bout n'est libre que si aucune autre pièce
/// du réseau ne se trouve juste au-delà (une courbe, dont le centre n'est pas dans l'axe, continue la voie).
/// Le tour automatique garde la forme non stricte : ses vues sont numérotées d'après elle.
pub fn bouts_selon(scene: &Scene, motifs: &[&str], strict: bool) -> Vec<Bout> {
    let mut bouts: Vec<Bout> = Vec::new();
    // ---- path_track : un nœud que personne ne vise, ou qui ne vise personne
    let tracks: Vec<(&str, Vec3, Option<&str>)> = scene
        .vmf
        .entities()
        .filter(|e| e.classname().eq_ignore_ascii_case("path_track"))
        .filter_map(|e| Some((e.get("targetname")?, e.vec3("origin").map(|o| Vec3::new(o[0] as f32, o[1] as f32, o[2] as f32))?, e.get("target").filter(|t| !t.is_empty()))))
        .collect();
    let by_name: HashMap<&str, usize> = tracks.iter().enumerate().map(|(i, t)| (t.0, i)).collect();
    let mut targeted = vec![false; tracks.len()];
    for t in &tracks {
        if let Some(&j) = t.2.and_then(|n| by_name.get(n)) {
            targeted[j] = true;
        }
    }
    for (i, t) in tracks.iter().enumerate() {
        let next = t.2.and_then(|n| by_name.get(n)).map(|&j| tracks[j].1);
        let prev = tracks.iter().find(|o| o.2 == Some(t.0)).map(|o| o.1);
        if !targeted[i] {
            if let Some(n) = next {
                bouts.push(Bout { nom: format!("{} (départ de la ligne)", t.0), pos: t.1, dir: (t.1 - n).normalize_or(Vec3::X), objet: None });
            }
        }
        if next.is_none() {
            if let Some(p) = prev {
                bouts.push(Bout { nom: format!("{} (fin de la ligne)", t.0), pos: t.1, dir: (t.1 - p).normalize_or(Vec3::X), objet: None });
            }
        }
    }
    // ---- chaînes de modèles : graphe des boîtes qui se touchent
    let pieces: Vec<(usize, Vec3, Vec3)> = scene
        .objects
        .iter()
        .enumerate()
        .filter(|(_, o)| o.kind == ObjKind::Prop && motifs.iter().any(|m| o.model.rsplit('/').next().unwrap_or("").contains(m)))
        .map(|(i, o)| (i, o.min, o.max))
        .collect();
    let touche = |a: &(usize, Vec3, Vec3), b: &(usize, Vec3, Vec3)| {
        let g = 12.0;
        a.1.x <= b.2.x + g && b.1.x <= a.2.x + g && a.1.y <= b.2.y + g && b.1.y <= a.2.y + g && a.1.z <= b.2.z + 48.0 && b.1.z <= a.2.z + 48.0
    };
    for (k, a) in pieces.iter().enumerate() {
        let voisins: Vec<&(usize, Vec3, Vec3)> = pieces.iter().enumerate().filter(|(j, b)| *j != k && touche(a, b)).map(|(_, b)| b).collect();
        if voisins.is_empty() {
            continue; // une pièce seule n'est pas un réseau
        }
        let ca = (a.1 + a.2) * 0.5;
        let e = a.2 - a.1;
        // axe de la pièce : son plus grand côté en plan. En mode strict, la direction de sa voisine la plus proche :
        // un tronçon court (64 de long pour 160 de large) est plus large que long, et son grand côté est en travers.
        let mut axe = if e.x >= e.y { Vec3::X } else { Vec3::Y };
        if strict {
            let proche = voisins.iter().min_by(|p, q| (((p.1 + p.2) * 0.5) - ca).length().total_cmp(&(((q.1 + q.2) * 0.5) - ca).length())).unwrap();
            let d = ((proche.1 + proche.2) * 0.5) - ca;
            axe = if d.x.abs() >= d.y.abs() { Vec3::X } else { Vec3::Y };
        }
        let demi = if axe == Vec3::X { e.x } else { e.y } * 0.5;
        for sens in [1.0f32, -1.0] {
            let bout = ca + axe * sens * demi;
            // un bout est libre si aucun voisin ne se trouve de ce côté
            let au_dela = bout + axe * sens * 24.0;
            let occupe = voisins.iter().any(|b| (((b.1 + b.2) * 0.5) - ca).dot(axe * sens) > demi * 0.5)
                || (strict && pieces.iter().enumerate().any(|(j, b)| j != k && au_dela.x >= b.1.x - 8.0 && au_dela.x <= b.2.x + 8.0 && au_dela.y >= b.1.y - 8.0 && au_dela.y <= b.2.y + 8.0 && a.2.z >= b.1.z - 48.0 && a.1.z <= b.2.z + 48.0));
            if !occupe {
                let o = &scene.objects[a.0];
                bouts.push(Bout { nom: format!("{} n° {}", o.model.rsplit('/').next().unwrap_or(""), o.vmf_id), pos: Vec3::new(bout.x, bout.y, a.2.z), dir: axe * sens, objet: Some(a.0 as u32) });
            }
        }
    }
    let mut garde: Vec<Bout> = Vec::new();
    for b in bouts {
        if !garde.iter().any(|g| g.pos.distance(b.pos) < 300.0) {
            garde.push(b);
        }
    }
    garde
}

/// Dénivelé au-delà duquel le sol « manque » après le bout d'une voie.
const VIDE: f32 = 96.0;

/// Les bouts de voie que rien ne termine : ni terminus posé, ni obstacle devant, ni téléporteur. Avec, pour chacun,
/// le vide sous son prolongement.
pub fn bouts_libres(scene: &Scene, monde: &Monde, config: &Config) -> Vec<(Bout, u32, f32)> {
    let mut libres = Vec::new();
    let motifs: Vec<&str> = config.reseau.iter().map(|s| s.as_str()).collect();
    let est_reseau = |c: &crate::monde::Corps| c.genre == Genre::Prop && contient(&config.reseau, &scene.objects[c.objet as usize].model);
    for b in bouts_selon(scene, &motifs, true) {
        let Some(objet) = b.objet else { continue }; // une ligne de path_track peut finir où elle veut
        // 1. un terminus posé au bout
        let zone = (b.pos - Vec3::splat(128.0), b.pos + Vec3::splat(128.0));
        if monde.pres(zone.0, zone.1).iter().any(|r| {
            let c = &monde.corps[r.0 as usize];
            c.genre == Genre::Prop && contient(&config.terminus, &scene.objects[c.objet as usize].model)
        }) {
            continue;
        }
        // 2. quelque chose se dresse devant le bout, au niveau de la voie : mur, heurtoir, bâtiment. Si la voie le
        //    traverse, « phys.traverse » le dit.
        let obstacle = |c: &crate::monde::Corps, _: u32| c.visible && c.genre != Genre::Mobile && c.genre != Genre::Terrain && !est_reseau(c);
        let devant = [12.0f32, 28.0, 44.0].iter().filter(|h| {
            let depart = b.pos - b.dir * 8.0 + Vec3::Z * **h;
            monde.rayon(depart, depart + b.dir * 136.0, obstacle).is_some()
        });
        if devant.count() >= 2 {
            continue;
        }
        // 3. un téléporteur au bout (les déclencheurs sont des objets masqués de la scène)
        if scene.objects.iter().any(|t| t.class == "trigger_teleport" && (0..3).all(|a| b.pos[a] >= t.min[a] - 160.0 && b.pos[a] <= t.max[a] + 160.0)) {
            continue;
        }
        // 4. que trouve-t-on sous le prolongement de la voie ?
        let mut chute = f32::MAX;
        for avance in [48.0f32, 128.0, 224.0] {
            let haut = b.pos + b.dir * avance + Vec3::Z * 8.0;
            let d = monde.rayon(haut, haut - Vec3::Z * 4096.0, |c, _| c.genre != Genre::Mobile && !c.libre && c.visible && !est_reseau(c)).map_or(4096.0, |(t, _)| t * 4096.0 - 8.0);
            chute = chute.min(d);
        }
        libres.push((b.clone(), objet, chute));
    }
    libres
}

pub fn verifier(scene: &Scene, monde: &Monde, config: &Config, out: &mut Vec<Constat>) {
    for (b, objet, chute) in bouts_libres(scene, monde, config) {
        let o = &scene.objects[objet as usize];
        let epaisseur = o.max.z - o.min.z;
        let boite = (b.pos - Vec3::splat(200.0), b.pos + Vec3::splat(200.0));
        if chute - epaisseur > VIDE {
            out.push(
                Constat::objet(scene, "phys.reseau_coupe", objet, format!("le réseau s'arrête net à {} ({:.0} {:.0} {:.0}) : ni terminus, ni mur, ni téléporteur, et {:.0} de vide sous son prolongement", b.nom, b.pos.x, b.pos.y, b.pos.z, chute))
                    .boite(boite.0, boite.1)
                    .mesure(chute),
            );
        } else {
            out.push(
                Constat::objet(scene, "phys.reseau_sans_terminus", objet, format!("le réseau s'arrête à {} ({:.0} {:.0} {:.0}) sans butoir, mur ni téléporteur", b.nom, b.pos.x, b.pos.y, b.pos.z))
                    .boite(boite.0, boite.1)
                    .mesure(chute.max(0.0)),
            );
        }
    }
}
