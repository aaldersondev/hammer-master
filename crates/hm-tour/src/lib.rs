//! Le tour automatique : choisit seul les points de vue (pièces, façades, bouts de réseau), les rend et les
//! assemble en planches légendées. Rien ici ne connaît une map en particulier : tout vient de la géométrie.
use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use glam::Vec3;
use hm_espace::{Field, Region};
use hm_render::planche::{planche, Case, Image, Police};
use hm_render::{Camera, GpuScene, Lighting, Renderer, View};
use hm_scene::{ObjKind, Scene};
use serde::Serialize;

pub mod constat;
pub mod suivi;

/// Hauteur des yeux d'un joueur debout.
pub const OEIL: f32 = 64.0;

#[derive(Clone, Debug)]
pub struct Vue {
    pub nom: String,
    /// `pieces`, `facades` ou `reseau`.
    pub groupe: &'static str,
    /// Case de la grille de lecture, pour lire la map zone par zone.
    pub zone: String,
    pub camera: Camera,
    pub legende: Vec<String>,
    /// Ce que la vue vise : sert au tri et aux légendes.
    pub cible: Vec3,
    /// Toujours gardée (première vue d'une pièce, rasante d'une façade, bout de réseau). Les autres ne le sont
    /// que si elles montrent des objets qu'aucune vue gardée ne montre.
    pub obligatoire: bool,
}

/// Case d'une grille de 8 192 unités, avec le niveau : `B3 rue`, `C2 étages`…
pub fn zone_de(p: Vec3) -> String {
    let col = (((p.x + 16384.0) / 8192.0).floor().clamp(0.0, 3.0)) as u8;
    let row = (((16384.0 - p.y) / 8192.0).floor().clamp(0.0, 3.0)) as u8;
    let niveau = if p.z < -600.0 {
        "sous-sol"
    } else if p.z < 1100.0 {
        "rue"
    } else if p.z < 6000.0 {
        "étages"
    } else {
        "hauts"
    };
    format!("{}{} {}", (b'A' + col) as char, row + 1, niveau)
}

fn cap(dir: Vec3) -> &'static str {
    let a = dir.y.atan2(dir.x).to_degrees();
    const NOMS: [&str; 8] = ["est", "nord-est", "nord", "nord-ouest", "ouest", "sud-ouest", "sud", "sud-est"];
    NOMS[(((a + 360.0 + 22.5) % 360.0) / 45.0) as usize % 8]
}

fn pos_txt(p: Vec3) -> String {
    format!("({:.0} {:.0} {:.0})", p.x, p.y, p.z)
}

/// Nombre de modèles posés dans la boîte d'une région.
fn objets_dans(scene: &Scene, r: &Region) -> usize {
    scene
        .objects
        .iter()
        .filter(|o| o.kind == ObjKind::Prop)
        .filter(|o| {
            let c = (o.min + o.max) * 0.5;
            c.x >= r.min.x && c.x <= r.max.x && c.y >= r.min.y && c.y <= r.max.y && c.z >= r.floor - 8.0 && c.z <= r.ceil + 8.0
        })
        .count()
}

/// Vues des pièces : depuis un coin vers le centre, depuis le coin opposé si la pièce est grande, et sur une
/// grille de points pour les très grandes salles.
pub fn vues_pieces(field: &Field, scene: &Scene) -> Vec<Vue> {
    let mut vues = Vec::new();
    let fov = 80.0;
    for r in field.regions.iter().filter(|r| r.is_room()) {
        let h = (r.ceil - r.floor).max(40.0);
        let oeil = OEIL.min(h - 10.0);
        let dims = r.max - r.min;
        let n_obj = objets_dans(scene, r);
        let base = format!("pièce {} · {:.0}×{:.0} h{:.0} · {} objets · sol {:.0}", r.id, dims.x, dims.y, h, n_obj, r.floor);
        let zone = zone_de(r.centre);
        let mut premiere = false;
        let mut push = |nom: String, from: Vec3, to: Vec3| {
            let mut eye = from + Vec3::Z * oeil;
            let mut target = to + Vec3::Z * (oeil * 0.62);
            if field.degage(eye, target) < 0.7 {
                // le coin choisi est derrière un obstacle : repartir du point le plus dégagé
                eye = r.centre + Vec3::Z * oeil;
                target = if (r.far_a - r.centre).truncate().length() > 48.0 { r.far_a } else { r.far_b } + Vec3::Z * (oeil * 0.62);
            }
            if (target - eye).truncate().length() < 24.0 {
                // pièce minuscule : regarder vers le plus grand côté
                target = eye + if dims.x >= dims.y { Vec3::X } else { Vec3::Y } * 64.0 - Vec3::Z * 12.0;
            }
            let camera = Camera::look_at(eye, target, fov);
            let obligatoire = !premiere;
            premiere = true;
            vues.push(Vue { nom, groupe: "pieces", zone: zone.clone(), camera, legende: vec![base.clone(), format!("{} regard {}", pos_txt(eye), cap(camera.dir))], cible: r.centre, obligatoire });
        };
        let grande = dims.x.max(dims.y);
        if r.clearance < 48.0 {
            // boyau : depuis un bout, dans l'axe
            push(format!("piece_{}_a", r.id), r.far_a, r.far_b);
        } else if grande <= 1100.0 {
            push(format!("piece_{}_a", r.id), r.far_a, r.centre);
            if r.area() > 420.0 * 420.0 || grande > 640.0 {
                push(format!("piece_{}_b", r.id), r.far_b, r.centre);
            }
        } else {
            // grande salle : un point de vue tous les 640, regard vers le centre puis à l'opposé
            let pas = 640.0;
            let (nx, ny) = ((dims.x / pas).ceil().max(1.0) as i32, (dims.y / pas).ceil().max(1.0) as i32);
            let mut k = 0;
            for iy in 0..ny {
                for ix in 0..nx {
                    let p = Vec3::new(r.min.x + (ix as f32 + 0.5) * dims.x / nx as f32, r.min.y + (iy as f32 + 0.5) * dims.y / ny as f32, r.floor + 4.0);
                    let Some(i) = field.free_at(p) else { continue };
                    if field.region[i] != r.id || field.dist[i] < 2 {
                        continue;
                    }
                    let from = field.pos(i);
                    let vers = if (r.centre - from).truncate().length() > 200.0 { r.centre } else { r.far_a };
                    push(format!("piece_{}_g{k}", r.id), from, vers);
                    push(format!("piece_{}_g{k}r", r.id), from, from + (from - vers));
                    k += 1;
                }
            }
            if k == 0 {
                push(format!("piece_{}_a", r.id), r.far_a, r.centre);
                push(format!("piece_{}_b", r.id), r.far_b, r.centre);
            }
        }
    }
    vues
}

/// Vues des façades : de face depuis la rue, par tranches, et rasante au pied du mur sur toute la longueur.
pub fn vues_facades(field: &Field) -> Vec<Vue> {
    let mut vues = Vec::new();
    for (k, f) in field.facades.iter().enumerate() {
        let len = f.a.distance(f.b);
        let along = (f.b - f.a).normalize_or(Vec3::X);
        let haut = (f.wall_top - f.a.z).clamp(120.0, 4000.0);
        let zone = zone_de((f.a + f.b) * 0.5);
        let titre = format!("façade {k} · {:.0} de long, mur de {:.0} · côté {}", len, haut, cap(-f.dir));
        // de face : le recul disponible fixe la largeur vue ; une vue par tranche
        let recul = f.back.clamp(96.0, 760.0);
        let largeur = (recul * 2.4).max(400.0);
        let n = (len / largeur).ceil().max(1.0) as usize;
        for i in 0..n {
            let m = f.a + along * (len * (i as f32 + 0.5) / n as f32);
            let eye = m - f.dir * (recul - 24.0) + Vec3::Z * (OEIL + 24.0);
            let target = m + f.dir * 16.0 + Vec3::Z * (OEIL + (haut * 0.35).min(recul * 0.45));
            if !field.libre(eye) {
                continue;
            }
            let camera = Camera::look_at(eye, target, 84.0);
            vues.push(Vue { nom: format!("facade_{k}_face_{i}"), groupe: "facades", zone: zone.clone(), camera, legende: vec![format!("{titre} · de face {}/{}", i + 1, n), format!("{} regard {}", pos_txt(eye), cap(camera.dir))], cible: m, obligatoire: false });
        }
        // rasante : le long du mur, à hauteur d'yeux, pour ce qui dépasse du mur ou traîne à son pied
        let n = (len / 1400.0).ceil().max(1.0) as usize;
        for i in 0..n {
            let s = f.a + along * (len * i as f32 / n as f32);
            let e = f.a + along * (len * (i as f32 + 1.0) / n as f32);
            let eye = s - f.dir * 40.0 + Vec3::Z * OEIL;
            let target = e - f.dir * 8.0 + Vec3::Z * (OEIL - 14.0);
            if !field.libre(eye) {
                continue;
            }
            let camera = Camera::look_at(eye, target, 70.0);
            vues.push(Vue { nom: format!("facade_{k}_rase_{i}"), groupe: "facades", zone: zone.clone(), camera, legende: vec![format!("{titre} · rasante {}/{}", i + 1, n), format!("{} regard {}", pos_txt(eye), cap(camera.dir))], cible: (s + e) * 0.5, obligatoire: len >= 640.0 && haut >= 160.0 });
        }
    }
    vues
}

/// Motifs de noms de modèles qui forment un réseau linéaire (voies, routes, ponts).
pub const MOTIFS_RESEAU: &[&str] = &["voie", "rail", "track", "viaduc", "pont_", "bridge", "road", "route_"];

/// Bouts de réseau : extrémités des chaînes de `path_track`, et extrémités des chaînes de modèles de voie qui se
/// touchent bout à bout. Chaque bout reçoit quatre vues : dans l'axe, de l'au-delà, de profil, de dessous.
pub fn vues_reseau(field: &Field, scene: &Scene, motifs: &[&str]) -> Vec<Vue> {
    // la recherche des bouts est celle des règles de réseau (hm-regles) : une seule pour le tour et pour les règles
    let _ = field;
    let garde: Vec<(String, Vec3, Vec3)> = hm_regles::reseau::bouts(scene, motifs).into_iter().map(|b| (b.nom, b.pos, b.dir)).collect();
    let mut vues = Vec::new();
    for (k, (nom, p, d)) in garde.iter().enumerate() {
        let lat = Vec3::new(-d.y, d.x, 0.0);
        let zone = zone_de(*p);
        let titre = format!("bout de réseau {k} · {nom} · {}", pos_txt(*p));
        // Chaque vue a une position de principe ; si elle tombe dans un plein ou si le bout n'en est pas visible,
        // on essaie l'autre côté puis plus près. Faute de mieux, la vue est abandonnée plutôt que rendue dans un mur.
        let mut push = |suffixe: &str, quoi: &str, essais: &[Vec3], fov: f32| {
            let voit = *p + Vec3::Z * 40.0;
            if let Some(&eye) = essais.iter().find(|&&e| field.libre(e) && field.degage(e, voit) >= 0.85) {
                let camera = Camera::look_at(eye, *p + Vec3::Z * 20.0, fov);
                vues.push(Vue { nom: format!("bout_{k}_{suffixe}"), groupe: "reseau", zone: zone.clone(), camera, legende: vec![format!("{titre} · {quoi}"), format!("{} regard {}", pos_txt(eye), cap(camera.dir))], cible: *p, obligatoire: true });
            }
        };
        let up = Vec3::Z;
        // le sol sous la voie, s'il est à portée de regard (une voie souterraine n'a pas de « dessous » utile)
        let sol = field.free_at(*p - up * 80.0).map(|i| field.free[i].floor as f32).filter(|z| *z < p.z - 100.0 && *z > p.z - 1600.0);
        push("axe", "dans l'axe, depuis la voie", &[*p - *d * 420.0 + up * 72.0, *p - *d * 250.0 + up * 72.0, *p - *d * 140.0 + up * 64.0], 74.0);
        push("au_dela", "depuis l'au-delà du bout", &[*p + *d * 520.0 + lat * 240.0 + up * 170.0, *p + *d * 520.0 - lat * 240.0 + up * 170.0, *p + *d * 300.0 + lat * 140.0 + up * 120.0, *p + *d * 300.0 - lat * 140.0 + up * 120.0, *p + *d * 150.0 + up * 90.0], 74.0);
        push("profil", "de profil", &[*p + lat * 700.0 + up * 90.0, *p - lat * 700.0 + up * 90.0, *p + lat * 380.0 + up * 80.0, *p - lat * 380.0 + up * 80.0, *p + lat * 180.0 + up * 70.0, *p - lat * 180.0 + up * 70.0], 70.0);
        if let Some(z) = sol {
            // voie en hauteur : depuis le sol dessous, en biais
            let bas = Vec3::new(p.x, p.y, z + OEIL);
            push("dessous", "depuis le sol, en dessous", &[bas + *d * 380.0 - lat * 300.0, bas + *d * 380.0 + lat * 300.0, bas + *d * 200.0 - lat * 160.0, bas + *d * 200.0 + lat * 160.0, bas - lat * 300.0, bas + lat * 300.0], 80.0);
        }
    }
    vues
}

/// Objets que le tour doit montrer : les modèles posés et les solides qui ont au moins une face visible.
pub fn objets_a_voir(scene: &Scene) -> Vec<bool> {
    scene.objects.iter().enumerate().map(|(i, o)| i != 0 && !o.hidden && (o.kind == ObjKind::Prop || !o.material.is_empty())).collect()
}

/// Objets occupant au moins `seuil` pixels d'une vue rendue en petit.
fn objets_vus(renderer: &Renderer, gs: &GpuScene, light: Lighting, camera: Camera, seuil: u32) -> Vec<u32> {
    let frame = renderer.render(gs, &View { camera, width: 384, height: 216, show_tools: false, clip: None, light });
    let mut compte: HashMap<u32, u32> = HashMap::new();
    for &id in &frame.ids {
        if id != 0 {
            *compte.entry(id).or_insert(0) += 1;
        }
    }
    compte.into_iter().filter(|&(_, n)| n >= seuil).map(|(i, _)| i).collect()
}

#[derive(Serialize, Clone)]
pub struct NonVu {
    pub classe: String,
    pub modele: String,
    pub materiau: String,
    pub vmf_id: u64,
    pub centre: [f32; 3],
}

pub struct Selection {
    pub vues: Vec<Vue>,
    pub candidates: usize,
    pub complements: usize,
    pub a_voir: usize,
    pub vus: usize,
    pub non_vus: Vec<NonVu>,
}

/// Choisit les vues du tour parmi les candidates, par la couverture mesurée :
/// 1. les vues obligatoires sont gardées ;
/// 2. une vue facultative est gardée si elle montre au moins 4 objets qu'aucune vue gardée ne montre ;
/// 3. pour chaque objet encore jamais vu, on cherche un point de vue dégagé à proximité (« complément ») ;
/// 4. ce qui reste est listé : invisible pour un joueur, ou hors de portée de cette recherche.
pub fn selectionner(renderer: &Renderer, gs: &GpuScene, scene: &Scene, field: &Field, candidates: Vec<Vue>) -> Selection {
    let light = Lighting::neutral(scene);
    let a_voir = objets_a_voir(scene);
    let n_cand = candidates.len();
    let mut vu = vec![false; scene.objects.len()];
    // « vu » : au moins 5 pixels sur une image de 384 × 216, soit une vingtaine sur la vue de la planche
    const SEUIL: u32 = 5;
    let sets: Vec<Vec<u32>> = candidates.iter().map(|v| objets_vus(renderer, gs, light, v.camera, SEUIL)).collect();
    let mut garde = vec![false; n_cand];
    for (i, v) in candidates.iter().enumerate() {
        if v.obligatoire {
            garde[i] = true;
            for &o in &sets[i] {
                vu[o as usize] = true;
            }
        }
    }
    let gain = |i: usize, vu: &[bool]| sets[i].iter().filter(|&&o| a_voir[o as usize] && !vu[o as usize]).count();
    let mut ordre: Vec<usize> = (0..n_cand).filter(|&i| !garde[i]).collect();
    ordre.sort_by_key(|&i| std::cmp::Reverse(gain(i, &vu)));
    for i in ordre {
        if gain(i, &vu) >= 4 {
            garde[i] = true;
            for &o in &sets[i] {
                vu[o as usize] = true;
            }
        }
    }
    let mut vues: Vec<Vue> = candidates.into_iter().zip(&garde).filter(|(_, g)| **g).map(|(v, _)| v).collect();
    // ---- compléments
    let mut complements = 0;
    let mut reste: Vec<usize> = (1..scene.objects.len()).filter(|&i| a_voir[i] && !vu[i]).collect();
    reste.sort_by(|&a, &b| {
        let (pa, pb) = ((scene.objects[a].min + scene.objects[a].max) * 0.5, (scene.objects[b].min + scene.objects[b].max) * 0.5);
        (zone_de(pa), (pa.x / 256.0) as i32, (pa.y / 256.0) as i32).cmp(&(zone_de(pb), (pb.x / 256.0) as i32, (pb.y / 256.0) as i32))
    });
    for i in reste {
        if vu[i] || complements >= 600 {
            continue;
        }
        let o = &scene.objects[i];
        let c = (o.min + o.max) * 0.5;
        let taille = (o.max - o.min).length();
        if taille < 48.0 {
            continue; // trop petit pour mériter une vue à lui seul ; il reste dans la liste des non vus
        }
        let r0 = (taille * 0.9).clamp(96.0, 500.0);
        let mut trouve = None;
        'cherche: for r in [r0, r0 * 2.0, r0 * 0.5] {
            for dz in [24.0f32, 96.0, -40.0] {
                for k in 0..8 {
                    let a = k as f32 * std::f32::consts::FRAC_PI_4 + 0.3;
                    let eye = c + Vec3::new(a.cos() * r, a.sin() * r, dz);
                    if field.libre(eye) && field.degage(eye, c) >= 0.8 {
                        let camera = Camera::look_at(eye, c, 70.0);
                        let set = objets_vus(renderer, gs, light, camera, SEUIL);
                        if set.contains(&(i as u32)) {
                            trouve = Some((camera, set));
                            break 'cherche;
                        }
                    }
                }
            }
        }
        if let Some((camera, set)) = trouve {
            for &o2 in &set {
                vu[o2 as usize] = true;
            }
            let quoi = if o.model.is_empty() { format!("solide {} ({})", o.vmf_id, o.material) } else { format!("{} n° {}", o.model.rsplit('/').next().unwrap_or(""), o.vmf_id) };
            vues.push(Vue { nom: format!("complement_{i}"), groupe: "complements", zone: zone_de(c), camera, legende: vec![format!("complément · {quoi} · {}", pos_txt(c)), format!("{} regard {}", pos_txt(camera.eye), cap(camera.dir))], cible: c, obligatoire: false });
            complements += 1;
        }
    }
    let total = a_voir.iter().filter(|&&b| b).count();
    let vus = (0..scene.objects.len()).filter(|&i| a_voir[i] && vu[i]).count();
    let non_vus = (1..scene.objects.len())
        .filter(|&i| a_voir[i] && !vu[i])
        .map(|i| {
            let o = &scene.objects[i];
            NonVu { classe: o.class.clone(), modele: o.model.clone(), materiau: o.material.clone(), vmf_id: o.vmf_id, centre: ((o.min + o.max) * 0.5).to_array() }
        })
        .collect();
    Selection { vues, candidates: n_cand, complements, a_voir: total, vus, non_vus }
}

#[derive(Serialize)]
pub struct VueRendue {
    pub nom: String,
    pub groupe: String,
    pub zone: String,
    pub planche: String,
    pub case: usize,
    pub oeil: [f32; 3],
    pub regard: [f32; 3],
    pub fov_y: f32,
    pub legende: Vec<String>,
    /// Objets occupant au moins 24 pixels de la vue rendue à 896 × 504 (rangs dans la table des objets).
    pub objets: Vec<u32>,
    /// Empreinte de la vue : la même tant que rien de ce qu'elle montre ne change.
    pub empreinte: String,
    /// Une lecture de cette vue existe déjà (empreinte connue) : elle n'est sur aucune planche.
    pub deja_lue: bool,
}

/// Une planche et l'empreinte de son code de lecture. Le code lui-même n'est écrit que dans les pixels de l'image.
#[derive(Serialize, serde::Deserialize, Clone)]
pub struct PlancheInfo {
    pub fichier: String,
    pub zone: String,
    pub groupe: String,
    pub vues: usize,
    pub empreinte_code: String,
}

/// Empreinte d'un code de lecture pour une planche d'une révision.
pub fn empreinte_code(revision: &str, fichier: &str, code: &str) -> String {
    blake3::hash(format!("{revision}|{fichier}|{}", code.trim().to_ascii_uppercase()).as_bytes()).to_hex()[..24].to_string()
}

#[derive(Serialize)]
pub struct Bilan {
    pub revision: String,
    pub carte: String,
    pub vues: Vec<VueRendue>,
    pub planches: Vec<String>,
    pub infos: Vec<PlancheInfo>,
    pub secondes: f32,
    /// Objets visibles (non masqués) vus au moins une fois, et leur nombre total.
    pub objets_vus: usize,
    pub objets_total: usize,
    /// Vues dont la lecture est reprise d'une révision précédente.
    pub deja_lues: usize,
}

/// Rend les vues et écrit les planches (9 vues par planche) dans `dossier`. Les vues sont triées par zone puis
/// par groupe, pour qu'une planche montre des endroits voisins.
pub fn rendre(renderer: &Renderer, gs: &GpuScene, scene: &Scene, mut vues: Vec<Vue>, dossier: &Path, registre: Option<&suivi::Registre>) -> Result<Bilan> {
    let t0 = std::time::Instant::now();
    std::fs::create_dir_all(dossier)?;
    vues.sort_by(|a, b| (a.zone.as_str(), a.groupe, (a.cible.z / 128.0) as i32, (a.cible.x / 512.0) as i32, a.nom.as_str()).cmp(&(b.zone.as_str(), b.groupe, (b.cible.z / 128.0) as i32, (b.cible.x / 512.0) as i32, b.nom.as_str())));
    let police = Police::systeme();
    let light = Lighting::neutral(scene);
    let (cw, ch) = (448u32, 252u32);
    let mut vus = vec![false; scene.objects.len()];
    let mut rendues = Vec::with_capacity(vues.len());
    // tour différentiel : une vue dont l'empreinte a déjà une lecture ne va sur aucune planche
    let mut identites = std::collections::BTreeMap::new();
    let empreintes: Vec<String> = vues.iter().map(|v| suivi::empreinte_camera(renderer, gs, scene, v.camera, &mut identites)).collect();
    let mut a_lire = Vec::with_capacity(vues.len());
    let mut emp_a_lire = Vec::with_capacity(vues.len());
    let mut deja_lues = 0;
    for (v, e) in vues.into_iter().zip(empreintes) {
        if registre.map_or(false, |r| r.lectures.contains_key(&e)) {
            deja_lues += 1;
            rendues.push(VueRendue { nom: v.nom.clone(), groupe: v.groupe.to_string(), zone: v.zone.clone(), planche: String::new(), case: 0, oeil: v.camera.eye.to_array(), regard: v.camera.dir.to_array(), fov_y: match v.camera.proj { hm_render::Projection::Perspective { fov_y } => fov_y, _ => 0.0 }, legende: v.legende.clone(), objets: Vec::new(), empreinte: e, deja_lue: true });
        } else {
            a_lire.push(v);
            emp_a_lire.push(e);
        }
    }
    let vues = a_lire;
    let mut planches = Vec::new();
    let mut infos = Vec::new();
    // graine du jour : les codes changent à chaque rendu du tour
    let graine = format!("{:?}{}", std::time::SystemTime::now(), std::process::id());
    let mut k = 0;
    let mut numero: HashMap<String, usize> = HashMap::new();
    while k < vues.len() {
        // une planche ne mélange ni les zones ni les groupes
        let mut e = k;
        while e < vues.len() && e - k < 9 && vues[e].zone == vues[k].zone && vues[e].groupe == vues[k].groupe {
            e += 1;
        }
        let cle = format!("{}_{}", vues[k].zone.replace(' ', "_"), vues[k].groupe);
        let n = numero.entry(cle.clone()).or_insert(0);
        *n += 1;
        let fichier = format!("{cle}_{:03}.jpg", *n);
        let mut cases = Vec::new();
        for (c, v) in vues[k..e].iter().enumerate() {
            // rendu au double puis réduit : les arêtes fines restent lisibles
            let frame = renderer.render(gs, &View { camera: v.camera, width: cw * 2, height: ch * 2, show_tools: false, clip: None, light });
            let mut compte: HashMap<u32, u32> = HashMap::new();
            for &id in &frame.ids {
                if id != 0 {
                    *compte.entry(id).or_insert(0) += 1;
                }
            }
            let mut objets: Vec<u32> = compte.into_iter().filter(|&(_, n)| n >= 24).map(|(i, _)| i).collect();
            objets.sort_unstable();
            for &o in &objets {
                vus[o as usize] = true;
            }
            cases.push(Case { image: reduire2(&frame.rgba, frame.width, frame.height), legende: v.legende.clone() });
            rendues.push(VueRendue { nom: v.nom.clone(), groupe: v.groupe.to_string(), zone: v.zone.clone(), planche: fichier.clone(), case: c, oeil: v.camera.eye.to_array(), regard: v.camera.dir.to_array(), fov_y: match v.camera.proj { hm_render::Projection::Perspective { fov_y } => fov_y, _ => 0.0 }, legende: v.legende.clone(), objets, empreinte: emp_a_lire[k + c].clone(), deja_lue: false });
        }
        let titre = format!("{} · {} · planche {} · révision {}", vues[k].zone, vues[k].groupe, *n, scene.revision);
        let mut img = planche(&cases, 3, &titre, &police);
        // code de lecture : quatre caractères sans ambiguïté, écrits seulement dans l'image
        const ALPHABET: &[u8] = b"ACDEFHJKLMNPRTUVWXY34679";
        let h = blake3::hash(format!("{graine}|{fichier}").as_bytes());
        let code: String = h.as_bytes()[..4].iter().map(|b| ALPHABET[*b as usize % ALPHABET.len()] as char).collect();
        let texte = format!("code de lecture : {code}");
        let x = img.width as f32 - police.largeur(&texte, 15.0) - 8.0;
        police.ecrire(&mut img, x, 2.0, 15.0, &texte, [120, 230, 160]);
        infos.push(PlancheInfo { fichier: fichier.clone(), zone: vues[k].zone.clone(), groupe: vues[k].groupe.to_string(), vues: e - k, empreinte_code: empreinte_code(&scene.revision, &fichier, &code) });
        std::fs::write(dossier.join(&fichier), img.to_jpeg(86)?)?;
        planches.push(fichier);
        k = e;
    }
    let total = scene.objects.iter().skip(1).filter(|o| !o.hidden).count();
    let objets_vus = scene.objects.iter().enumerate().skip(1).filter(|(i, o)| !o.hidden && vus[*i]).count();
    let bilan = Bilan { revision: scene.revision.clone(), carte: scene.path.clone(), vues: rendues, planches, infos, secondes: t0.elapsed().as_secs_f32(), objets_vus, objets_total: total, deja_lues };
    std::fs::write(dossier.join("tour.json"), serde_json::to_vec(&bilan)?)?;
    // la table des objets : le rang d'un objet est son identifiant dans les vues
    let objets: Vec<serde_json::Value> = scene
        .objects
        .iter()
        .map(|o| serde_json::json!({"genre": format!("{:?}", o.kind), "classe": o.class, "nom": o.name, "modele": o.model, "materiau": o.material, "vmf_id": o.vmf_id, "min": o.min.to_array(), "max": o.max.to_array(), "masque": o.hidden}))
        .collect();
    std::fs::write(dossier.join("objets.json"), serde_json::to_vec(&objets)?)?;
    Ok(bilan)
}

/// Réduit une image RGBA de moitié (moyenne de 2 × 2) et la rend en RGB.
pub fn reduire2(rgba: &[u8], w: u32, h: u32) -> Image {
    let (ow, oh) = (w / 2, h / 2);
    let mut rgb = Vec::with_capacity((ow * oh * 3) as usize);
    for y in 0..oh {
        for x in 0..ow {
            for c in 0..3 {
                let s: u32 = [(0, 0), (1, 0), (0, 1), (1, 1)].iter().map(|(dx, dy)| rgba[(((y * 2 + dy) * w + x * 2 + dx) * 4 + c) as usize] as u32).sum();
                rgb.push((s / 4) as u8);
            }
        }
    }
    Image::from_rgb(ow, oh, rgb)
}
