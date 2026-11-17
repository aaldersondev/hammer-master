//! Scintillement (z-fighting) où un modèle est en cause. `geo.zfighting` compare les faces des solides entre elles ;
//! ici, les faces planes des modèles posés sont comparées aux faces des solides, à celles des autres modèles, et
//! entre elles à l'intérieur d'un même modèle. Deux faces dessinées dans le même plan, tournées du même côté et qui
//! se recouvrent : la carte graphique ne sait pas laquelle est devant, et la surface clignote dès que l'œil bouge.
//!
//! Né de trois signalements faits en jeu le 14/11/2026 sur rp_megalopole (gradins, portes de l'hôpital, H-7).
use std::collections::HashMap;

use glam::DVec3;
use hm_scene::Scene;
use rayon::prelude::*;

use crate::monde::Monde;
use crate::{Constat, Niveau, Regle};

pub const REGLES: &[Regle] = &[
    Regle {
        id: "phys.zfighting_modele",
        niveau: Niveau::Avertissement,
        titre: "Face de modèle qui scintille",
        explication: "Une face plane d'un modèle est dans le même plan qu'une face de solide ou qu'une face d'un autre modèle, tournée du même côté, et elles se recouvrent : elles clignoteront l'une sur l'autre en jeu. Décaler l'une des deux (0,25 suffit de près, 1 pour une surface vue de loin), ou retirer celle qui est en double.",
        source: "contrôle propre à l'outil ; signalements 003, 005 et 012 de rp_megalopole (14/11/2026)",
    },
    Regle {
        id: "asset.modele_scintille",
        niveau: Niveau::Avertissement,
        titre: "Modèle dont deux faces se recouvrent",
        explication: "Dans le modèle lui-même, deux faces de matériaux différents sont dans le même plan, tournées du même côté, et se recouvrent : elles clignoteront partout où le modèle est posé. À corriger dans le modèle.",
        source: "contrôle propre à l'outil ; signalements 003, 005 et 012 de rp_megalopole (14/11/2026)",
    },
];

/// Écart entre deux plans en dessous duquel ils sont tenus pour confondus.
const ECART: f64 = 0.02;
/// Surface commune à partir de laquelle un recouvrement est signalé.
const AIRE_MIN: f64 = 16.0;
/// Surface d'une plaque de modèle à partir de laquelle elle est regardée.
const PLAQUE_MIN: f64 = 16.0;

/// Une plaque : les triangles d'un maillage de modèle qui sont dans un même plan, tournés du même côté.
struct Plaque {
    maille: usize,
    n: DVec3,
    tris: Vec<[DVec3; 3]>,
    aire: f64,
}

fn normale(t: &[DVec3; 3]) -> (DVec3, f64) {
    let c = (t[1] - t[0]).cross(t[2] - t[0]);
    let l = c.length();
    if l < 1e-9 {
        (DVec3::Z, 0.0)
    } else {
        (c / l, l * 0.5)
    }
}

/// Les plaques d'un modèle, dans son repère.
fn plaques(m: &hm_assets::Model) -> Vec<Plaque> {
    let mut par: HashMap<(usize, i32, i32, i32, i64), Plaque> = HashMap::new();
    for (k, maille) in m.meshes.iter().enumerate() {
        for t in maille.indices.chunks_exact(3) {
            let p = |i: u32| {
                let v = m.vertices[i as usize].pos;
                DVec3::new(v[0] as f64, v[1] as f64, v[2] as f64)
            };
            let mut tri = [p(t[0]), p(t[1]), p(t[2])];
            let (mut n, aire) = normale(&tri);
            if aire < 0.05 {
                continue;
            }
            // le côté visible est celui que disent les normales des sommets, pas l'ordre des sommets (les triangles
            // d'un .mdl tournent dans le sens des aiguilles vus de dehors) : la plaque est rangée face visible devant
            let vers = t.iter().fold(DVec3::ZERO, |s, i| {
                let q = m.vertices[*i as usize].normal;
                s + DVec3::new(q[0] as f64, q[1] as f64, q[2] as f64)
            });
            if n.dot(vers) < 0.0 {
                n = -n;
                tri.swap(1, 2);
            }
            let d = n.dot(tri[0]);
            let cle = (k, (n.x * 2000.0).round() as i32, (n.y * 2000.0).round() as i32, (n.z * 2000.0).round() as i32, (d * 50.0).round() as i64);
            let e = par.entry(cle).or_insert_with(|| Plaque { maille: k, n, tris: Vec::new(), aire: 0.0 });
            e.tris.push(tri);
            e.aire += aire;
        }
    }
    let mut v: Vec<Plaque> = par.into_values().filter(|p| p.aire >= PLAQUE_MIN).collect();
    v.sort_by(|a, b| b.aire.total_cmp(&a.aire));
    v
}

/// Surface commune de deux polygones convexes du même plan (de normale `n`), et sa boîte.
pub fn aire_commune(a: &[DVec3], b: &[DVec3], n: DVec3) -> Option<(f64, DVec3, DVec3)> {
    aire_commune_milieu(a, b, n).map(|(s, lo, hi, _)| (s, lo, hi))
}

/// Comme [`aire_commune`], avec un point au milieu de la surface commune.
pub fn aire_commune_milieu(a: &[DVec3], b: &[DVec3], n: DVec3) -> Option<(f64, DVec3, DVec3, DVec3)> {
    if a.len() < 3 || b.len() < 3 {
        return None;
    }
    // sens de b autour de n : la normale d'arête doit sortir du polygone
    let mut tour = DVec3::ZERO;
    for i in 0..b.len() {
        tour += b[i].cross(b[(i + 1) % b.len()]);
    }
    let sens = if tour.dot(n) >= 0.0 { 1.0 } else { -1.0 };
    let mut poly: Vec<DVec3> = a.to_vec();
    for e in 0..b.len() {
        let (p, q) = (b[e], b[(e + 1) % b.len()]);
        let en = (q - p).cross(n) * sens;
        let l = en.length();
        if l < 1e-9 {
            continue;
        }
        let en = en / l;
        let d0 = en.dot(p);
        let mut suite = Vec::with_capacity(poly.len() + 2);
        for s in 0..poly.len() {
            let (x, y) = (poly[s], poly[(s + 1) % poly.len()]);
            let (dx, dy) = (en.dot(x) - d0, en.dot(y) - d0);
            if dx <= 0.0 {
                suite.push(x);
            }
            if (dx < 0.0 && dy > 0.0) || (dx > 0.0 && dy < 0.0) {
                suite.push(x + (y - x) * (dx / (dx - dy)));
            }
        }
        poly = suite;
        if poly.len() < 3 {
            return None;
        }
    }
    let mut s = DVec3::ZERO;
    for i in 1..poly.len() - 1 {
        s += (poly[i] - poly[0]).cross(poly[i + 1] - poly[0]);
    }
    let aire = s.length() * 0.5;
    if aire < 1e-6 {
        return None;
    }
    let lo = poly.iter().fold(poly[0], |x, p| x.min(*p));
    let hi = poly.iter().fold(poly[0], |x, p| x.max(*p));
    let milieu = poly.iter().fold(DVec3::ZERO, |x, p| x + *p) / poly.len() as f64;
    Some((aire, lo, hi, milieu))
}

fn boite(pts: impl Iterator<Item = DVec3>) -> (DVec3, DVec3) {
    pts.fold((DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)), |(lo, hi), p| (lo.min(p), hi.max(p)))
}

fn se_croisent(a: &(DVec3, DVec3), b: &(DVec3, DVec3), marge: f64) -> bool {
    (0..3).all(|k| a.1[k].min(b.1[k]) - a.0[k].max(b.0[k]) >= -marge)
}

/// Ce qui est rangé par plan : une face de solide, ou une plaque d'un modèle posé.
struct Piece {
    objet: u32,
    /// Rang du modèle posé dans `scene.props`, et de la plaque dans celles de son modèle ; `None` : face de solide.
    plaque: Option<(u32, u32)>,
    n: DVec3,
    d: f64,
    boite: (DVec3, DVec3),
    /// Les polygones de la pièce, dans le monde.
    polys: Vec<Vec<DVec3>>,
    materiau: String,
}

fn cle(n: DVec3, d: f64) -> (i32, i32, i32, i64) {
    ((n.x * 200.0).round() as i32, (n.y * 200.0).round() as i32, (n.z * 200.0).round() as i32, (d / 0.5).floor() as i64)
}

/// Surface commune de deux pièces du même plan, la boîte de ce qu'elles ont en commun, et un point au milieu de
/// leurs plus grands morceaux communs (pour savoir si la surface se voit).
fn recouvrement(a: &Piece, b: &Piece) -> Option<(f64, DVec3, DVec3, Vec<DVec3>)> {
    let mut total = 0.0;
    let mut morceaux: Vec<(f64, DVec3)> = Vec::new();
    let (mut lo, mut hi) = (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY));
    let bb: Vec<(DVec3, DVec3)> = b.polys.iter().map(|p| boite(p.iter().copied())).collect();
    for pa in &a.polys {
        let ba = boite(pa.iter().copied());
        for (pb, bx) in b.polys.iter().zip(&bb) {
            if !se_croisent(&ba, bx, 0.0) {
                continue;
            }
            if let Some((s, l, h, c)) = aire_commune_milieu(pa, pb, a.n) {
                total += s;
                lo = lo.min(l);
                hi = hi.max(h);
                morceaux.push((s, c));
            }
        }
    }
    morceaux.sort_by(|x, y| y.0.total_cmp(&x.0));
    (total >= AIRE_MIN).then(|| (total, lo, hi, morceaux.iter().take(5).map(|m| m.1).collect()))
}

/// Vrai si le point est dans du plein : un solide visible autre que les deux objets en cause, ou la collision d'un
/// modèle posé (celle des deux modèles en cause comprise : une face prise dans le volume de son propre modèle, ou
/// sous le modèle posé dessus, ne se voit pas). La surface regardée est alors cachée, elle ne scintille pour personne.
fn enfoui(monde: &Monde, q: glam::Vec3, a: u32, b: u32) -> bool {
    let e = glam::Vec3::splat(0.05);
    monde.pres(q - e, q + e).into_iter().any(|r| {
        let c = &monde.corps[r.0 as usize];
        let cv = monde.convexe(r);
        if !(0..3).all(|k| q[k] >= cv.min[k] && q[k] <= cv.max[k]) {
            return false;
        }
        if crate::physique::plein(c) {
            c.objet != a && c.objet != b && cv.profondeur(q) > 0.25
        } else {
            c.genre == crate::monde::Genre::Prop && cv.profondeur(q) > 0.5
        }
    })
}

pub fn verifier(scene: &Scene, monde: &Monde, out: &mut Vec<Constat>) {
    let outil: HashMap<&str, bool> = scene.materials.iter().map(|m| (m.name.as_str(), m.tool.is_some())).collect();
    // ---- les plaques de chaque modèle distinct, une fois
    let utilises: Vec<bool> = {
        let mut u = vec![false; scene.models.len()];
        for p in &scene.props {
            u[p.model as usize] = true;
        }
        u
    };
    let plaques_de: Vec<Vec<Plaque>> = scene.models.par_iter().enumerate().map(|(i, m)| if utilises[i] { m.model.as_ref().map(plaques).unwrap_or_default() } else { Vec::new() }).collect();
    let nom_materiau = |modele: u32, skin: u32, maille: usize| -> String {
        let m = &scene.models[modele as usize];
        m.materials.get(skin as usize).or_else(|| m.materials.first()).and_then(|s| s.get(maille)).and_then(|i| scene.materials.get(*i as usize)).map(|x| x.name.clone()).unwrap_or_default()
    };

    // ---- à l'intérieur d'un modèle : deux mailles (deux matériaux) dans le même plan
    let premier: HashMap<u32, (u32, usize)> = {
        let mut h: HashMap<u32, (u32, usize)> = HashMap::new();
        for p in &scene.props {
            if scene.objects[p.object as usize].hidden {
                continue;
            }
            h.entry(p.model).or_insert((p.object, 0)).1 += 1;
        }
        h
    };
    let mut internes: Vec<Constat> = plaques_de
        .par_iter()
        .enumerate()
        .filter_map(|(mi, pl)| {
            let (objet, poses) = *premier.get(&(mi as u32))?;
            let mut total = 0.0;
            let mut paire: Option<(usize, usize, f64)> = None;
            for i in 0..pl.len() {
                for j in i + 1..pl.len() {
                    let (a, b) = (&pl[i], &pl[j]);
                    if a.maille == b.maille || a.n.dot(b.n) < 0.99999 || (a.n.dot(a.tris[0][0]) - a.n.dot(b.tris[0][0])).abs() > ECART {
                        continue;
                    }
                    if a.tris.len() * b.tris.len() > 400_000 {
                        continue;
                    }
                    let mut s = 0.0;
                    for ta in &a.tris {
                        let ba = boite(ta.iter().copied());
                        for tb in &b.tris {
                            if se_croisent(&ba, &boite(tb.iter().copied()), 0.0) {
                                s += aire_commune(ta, tb, a.n).map_or(0.0, |x| x.0);
                            }
                        }
                    }
                    if s >= AIRE_MIN {
                        total += s;
                        if paire.map_or(true, |p| s > p.2) {
                            paire = Some((a.maille, b.maille, s));
                        }
                    }
                }
            }
            let (ma, mb, _) = paire?;
            let skin = scene.props.iter().find(|p| p.object == objet).map_or(0, |p| p.skin);
            Some(
                Constat::objet(
                    scene,
                    "asset.modele_scintille",
                    objet,
                    format!("{} : dans le modèle lui-même, des faces de « {} » et de « {} » sont dans le même plan et se recouvrent sur {total:.0} unités carrées ; posé {poses} fois dans la map", scene.models[mi].name, nom_materiau(mi as u32, skin, ma), nom_materiau(mi as u32, skin, mb)),
                )
                .mesure(total as f32),
            )
        })
        .collect();
    internes.sort_by(|a, b| b.mesure.total_cmp(&a.mesure));
    out.append(&mut internes);

    // ---- les pièces du monde, rangées par plan puis par cellule
    let mut pieces: Vec<Piece> = Vec::new();
    for b in &scene.brushes {
        if b.object == 0 || scene.objects[b.object as usize].hidden {
            continue;
        }
        for f in &b.brush.faces {
            if f.disp.is_some() || f.poly.len() < 3 || outil.get(f.material.as_str()).copied().unwrap_or(false) {
                continue;
            }
            let n = f.plane.n;
            pieces.push(Piece { objet: b.object, plaque: None, n, d: n.dot(f.poly[0]), boite: boite(f.poly.iter().copied()), polys: vec![f.poly.clone()], materiau: f.material.clone() });
        }
    }
    let solides = pieces.len();
    let mut des_modeles: Vec<Piece> = scene
        .props
        .par_iter()
        .enumerate()
        .flat_map_iter(|(k, p)| {
            let mut v = Vec::new();
            if scene.objects[p.object as usize].hidden {
                return v;
            }
            let t = p.transform.as_dmat4();
            for (r, pl) in plaques_de[p.model as usize].iter().enumerate() {
                let polys: Vec<Vec<DVec3>> = pl.tris.iter().map(|tri| tri.iter().map(|q| t.transform_point3(*q)).collect()).collect();
                let tri0 = [polys[0][0], polys[0][1], polys[0][2]];
                // la normale suit la transformation (une rotation, parfois une échelle), pas l'ordre des sommets
                let nt = t.transform_vector3(pl.n);
                let n = if nt.length() > 1e-9 { nt.normalize() } else { normale(&tri0).0 };
                v.push(Piece { objet: p.object, plaque: Some((k as u32, r as u32)), n, d: n.dot(tri0[0]), boite: boite(polys.iter().flatten().copied()), polys, materiau: nom_materiau(p.model, p.skin, pl.maille) });
            }
            v
        })
        .collect();
    pieces.append(&mut des_modeles);
    if pieces.len() == solides {
        return;
    }
    let mut par_plan: HashMap<(i32, i32, i32, i64), Vec<u32>> = HashMap::new();
    for (i, p) in pieces.iter().enumerate() {
        par_plan.entry(cle(p.n, p.d)).or_default().push(i as u32);
    }
    // une plaque de modèle cherche ses voisines dans son plan et dans les deux tranches d'à côté
    let trouves: Vec<(u32, u32, f64, DVec3, DVec3)> = (solides..pieces.len())
        .into_par_iter()
        .flat_map_iter(|i| {
            let a = &pieces[i];
            let c = cle(a.n, a.d);
            let mut v = Vec::new();
            for dz in -1..=1i64 {
                let Some(g) = par_plan.get(&(c.0, c.1, c.2, c.3 + dz)) else { continue };
                for &j in g {
                    let b = &pieces[j as usize];
                    // chaque paire de plaques une seule fois ; une plaque contre un solide toujours
                    if b.plaque.is_some() && (j as usize) <= i {
                        continue;
                    }
                    if b.objet == a.objet || (a.d - b.d).abs() > ECART || a.n.dot(b.n) < 0.99999 || !se_croisent(&a.boite, &b.boite, 0.0) {
                        continue;
                    }
                    if a.polys.len() * b.polys.len() > 400_000 {
                        continue;
                    }
                    if let Some((s, lo, hi, milieux)) = recouvrement(a, b) {
                        // deux faces adossées à un mur (le dos d'un modèle contre un pylône) sont dans le même plan
                        // et tournées du même côté, mais vers le plein : personne ne les voit
                        if milieux.iter().all(|c| enfoui(monde, (*c + a.n).as_vec3(), a.objet, b.objet)) {
                            continue;
                        }
                        v.push((i as u32, j, s, lo, hi));
                    }
                }
            }
            v
        })
        .collect();
    // un constat par paire d'objets et par orientation : les plans parallèles additionnés
    let mut par_paire: HashMap<(u32, u32, (i32, i32, i32)), (f64, DVec3, DVec3, u32, u32)> = HashMap::new();
    for (i, j, s, lo, hi) in trouves {
        let (a, b) = (&pieces[i as usize], &pieces[j as usize]);
        let c = cle(a.n, 0.0);
        let e = par_paire.entry((a.objet, b.objet, (c.0, c.1, c.2))).or_insert((0.0, lo, hi, i, j));
        e.0 += s;
        e.1 = e.1.min(lo);
        e.2 = e.2.max(hi);
    }
    let mut constats: Vec<Constat> = par_paire
        .into_iter()
        .map(|((oa, ob, _), (s, lo, hi, i, j))| {
            let (a, b) = (&pieces[i as usize], &pieces[j as usize]);
            Constat::objet(scene, "phys.zfighting_modele", oa, format!("{} (« {} ») et {} (« {} ») ont des faces dans le même plan, tournées vers ({:.0} {:.0} {:.0}), qui se recouvrent sur {s:.0} unités carrées : elles scintilleront", monde.nommer(oa), a.materiau, monde.nommer(ob), b.materiau, a.n.x, a.n.y, a.n.z))
                .boite(lo.as_vec3(), hi.as_vec3())
                .mesure(s as f32)
                .autres_de(scene, vec![ob])
                .plan(a.n.as_vec3())
        })
        .collect();
    constats.sort_by(|a, b| b.mesure.total_cmp(&a.mesure).then(a.objet.cmp(&b.objet)));
    out.append(&mut constats);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn carre(x: f64, y: f64, c: f64, z: f64) -> Vec<DVec3> {
        vec![DVec3::new(x, y, z), DVec3::new(x + c, y, z), DVec3::new(x + c, y + c, z), DVec3::new(x, y + c, z)]
    }

    #[test]
    fn la_surface_commune_ne_depend_pas_du_sens_des_polygones() {
        let a = carre(0.0, 0.0, 10.0, 5.0);
        let b = carre(5.0, 5.0, 10.0, 5.0);
        let mut b_inverse = b.clone();
        b_inverse.reverse();
        for (p, q) in [(&a, &b), (&a, &b_inverse), (&b_inverse, &a)] {
            let (s, lo, hi) = aire_commune(p, q, DVec3::Z).unwrap();
            assert!((s - 25.0).abs() < 1e-9, "{s}");
            assert_eq!((lo, hi), (DVec3::new(5.0, 5.0, 5.0), DVec3::new(10.0, 10.0, 5.0)));
        }
        // deux carrés qui ne font que se toucher par un bord n'ont rien en commun
        assert!(aire_commune(&a, &carre(10.0, 0.0, 10.0, 5.0), DVec3::Z).is_none());
        // un triangle dans un carré : sa surface entière
        let t = vec![DVec3::new(1.0, 1.0, 5.0), DVec3::new(5.0, 1.0, 5.0), DVec3::new(1.0, 5.0, 5.0)];
        assert!((aire_commune(&t, &a, DVec3::Z).unwrap().0 - 8.0).abs() < 1e-9);
    }

    #[test]
    fn les_triangles_d_un_meme_plan_font_une_plaque() {
        use hm_assets::model::{Model, ModelMesh, ModelVertex};
        let v = |x: f32, y: f32, z: f32| ModelVertex { pos: [x, y, z], normal: [0.0, 0.0, -1.0], uv: [0.0, 0.0] };
        // un carré de 8 de côté en deux triangles à z = 0, et un petit triangle à z = 3 (trop petit pour compter)
        let m = Model {
            name: "x".into(),
            vertices: vec![v(0.0, 0.0, 0.0), v(8.0, 0.0, 0.0), v(8.0, 8.0, 0.0), v(0.0, 8.0, 0.0), v(0.0, 0.0, 3.0), v(1.0, 0.0, 3.0), v(0.0, 1.0, 3.0)],
            meshes: vec![ModelMesh { indices: vec![0, 1, 2, 0, 2, 3, 4, 5, 6] }],
            skins: Vec::new(),
            bounds: ([0.0; 3], [8.0, 8.0, 3.0]),
            static_prop: true,
            prop_data: false,
            triangles: 3,
            mount: 0,
            parties: Vec::new(),
        };
        let p = plaques(&m);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].tris.len(), 2);
        assert!((p[0].aire - 64.0).abs() < 1e-6);
        // les sommets sont rangés dans le sens inverse des aiguilles vus de dessus, mais leurs normales disent que la
        // face se voit de dessous : c'est elles qui comptent
        assert_eq!(p[0].n, DVec3::NEG_Z);
    }
}
