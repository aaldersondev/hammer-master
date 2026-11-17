//! Règles de géométrie : ce que vbsp ou vrad refuseront, et ce qu'ils acceptent mais qui se voit.
use std::collections::HashMap;

use glam::{DVec3, Vec3};
use hm_assets::Tool;
use hm_geom::{aire, boite, polytope, volume, BrushIssue, Plane};
use hm_scene::Scene;
use hm_vmf::parse_plane;
use rayon::prelude::*;

use crate::monde::{Genre, Monde};
use crate::{Config, Constat, Niveau, Regle};

pub const REGLES: &[Regle] = &[
    Regle { id: "geo.plan_degenere", niveau: Niveau::Erreur, titre: "Plan dégénéré", explication: "Les trois points d'un plan sont alignés : la face n'a pas de normale.", source: "vbsp map.cpp : « FloatPlane: bad normal », « plane with no normal »" },
    Regle { id: "geo.sans_volume", niveau: Niveau::Erreur, titre: "Solide sans volume", explication: "Les plans du solide ne ferment aucun volume.", source: "vbsp map.cpp l. 661 : « no visible sides on brush »" },
    Regle { id: "geo.face_inutile", niveau: Niveau::Avertissement, titre: "Face inutile", explication: "Une face ne garde aucun polygone : plan en double, ou plan qui ne coupe pas le solide (solide non convexe à l'origine).", source: "vbsp map.cpp l. 2771 : « duplicate plane », « mirrored plane »" },
    Regle { id: "geo.micro_solide", niveau: Niveau::Avertissement, titre: "Solide minuscule", explication: "Volume inférieur à 1 unité cube : vbsp le signale et peut le retirer.", source: "vbsp brushbsp.cpp l. 1417 : « WARNING, microbrush »" },
    Regle { id: "geo.hors_limites", niveau: Niveau::Erreur, titre: "Hors des limites de la map", explication: "Une coordonnée dépasse ±16 376 : au-delà de ±16 384, vbsp refuse la brosse ; entre les deux, il borde l'arbre de 8 puis encore de 8, trouve des nœuds sans volume borné et plante.", source: "SDK public/worldsize.h l. 19 (MAX_COORD_INTEGER) ; vbsp map.cpp l. 659 : « bounds out of range » ; vbsp portals.cpp (SIDESPACE, « BSP node with unbounded volume »), plantage vu le 14/11/2026" },
    Regle { id: "geo.hors_grille", niveau: Niveau::Information, titre: "Hors grille", explication: "Une face droite n'est pas sur la grille de sa classe, ou les points d'un plan oblique ne sont pas entiers : risque de fente entre deux solides.", source: "contrôle propre à l'outil : aucun message de vbsp ne le signale" },
    Regle { id: "geo.chevauchement", niveau: Niveau::Information, titre: "Solides du monde qui se chevauchent", explication: "Deux solides du monde occupent le même volume : faces perdues, découpe inutile.", source: "contrôle propre à l'outil" },
    Regle { id: "geo.zfighting", niveau: Niveau::Avertissement, titre: "Faces superposées qui scintillent", explication: "Deux faces dessinées sont dans le même plan, tournées du même côté, et se recouvrent, alors qu'elles appartiennent à deux modèles de brosses distincts (une entité à solides et le monde, ou deux entités) : le moteur dessine les deux et l'image scintille.", source: "contrôle propre à l'outil ; les faces du monde et des func_detail passent par le même arbre BSP et ne sont pas concernées" },
    Regle { id: "geo.faces_superposees", niveau: Niveau::Information, titre: "Deux matériaux sur la même surface", explication: "Deux faces du monde ou de détail, de matériaux différents, se recouvrent dans le même plan : le compilateur n'en garde qu'une, sans qu'on choisisse laquelle.", source: "contrôle propre à l'outil" },
    Regle { id: "geo.texture_perpendiculaire", niveau: Niveau::Erreur, titre: "Texture perpendiculaire à sa face", explication: "Les axes de texture sont dans un plan perpendiculaire à la face : pas de coordonnées de texture ni de lumière.", source: "vrad lightmap.cpp l. 453 : « face vectors parallel to face normal »" },
    Regle { id: "geo.displacement_invalide", niveau: Niveau::Erreur, titre: "Displacement invalide", explication: "Displacement sur une entité, ou sur une face qui n'a pas quatre côtés.", source: "vbsp disp_vbsp.cpp l. 631 et 641" },
    Regle { id: "geo.areaportal_multiple", niveau: Niveau::Erreur, titre: "Areaportal à plusieurs solides", explication: "Un func_areaportal doit être fait d'un seul solide.", source: "vbsp map.cpp l. 1697 : « func_areaportal can only be a single brush »" },
    Regle { id: "geo.fuite", niveau: Niveau::Erreur, titre: "Fuite", explication: "Depuis une entité, on atteint le vide extérieur sans traverser de solide du monde : la map n'est pas fermée.", source: "vbsp : « **** leaked **** ». Trouvée par vbsp lancé à part (`hm vbsp`, 22 s sur rp_megalopole), avec le chemin de son fichier de points" },
];

fn multiple(x: f64, pas: f64) -> bool {
    pas <= 0.0 || ((x / pas).round() * pas - x).abs() < 0.005
}

pub fn verifier(scene: &Scene, monde: &Monde, config: &Config, out: &mut Vec<Constat>) {
    let outil: HashMap<&str, Option<Tool>> = scene.materials.iter().map(|m| (m.name.as_str(), m.tool)).collect();
    // les nœuds « solid » du VMF, dans l'ordre de `scene.brushes`
    let noeuds: Vec<&hm_vmf::Node> = scene.vmf.world().into_iter().flat_map(|w| w.solids()).chain(scene.vmf.entities().flat_map(|e| e.solids())).collect();

    // ---------------------------------------------------------------- solide par solide
    for (k, b) in scene.brushes.iter().enumerate() {
        let quoi = if b.world { hm_langue::t("solide du monde").to_string() } else { hm_langue::tr!("solide de {}", "solid of {}", b.class) };
        let ici = |regle: &'static str, msg: String| {
            if b.object != 0 {
                Constat::objet(scene, regle, b.object, msg)
            } else {
                let mut c = Constat::lieu(regle, Vec3::ZERO, Vec3::ZERO, msg);
                c.vmf_id = b.brush.id;
                c.classe = b.class.clone();
                c
            }
        };
        for issue in &b.brush.issues {
            match issue {
                BrushIssue::DegeneratePlane { side } => out.push(ici("geo.plan_degenere", hm_langue::tr!("{quoi} n° {} : les trois points du plan de la face {} sont alignés", "{quoi} #{}: the three plane points of face {} are aligned", b.brush.id, side + 1))),
                BrushIssue::EmptyFace { side } => out.push(ici("geo.face_inutile", hm_langue::tr!("{quoi} n° {} : la face {} ne garde aucun polygone", "{quoi} #{}: face {} keeps no polygon", b.brush.id, side + 1))),
                BrushIssue::NoVolume => out.push(ici("geo.sans_volume", hm_langue::tr!("{quoi} n° {} : ses plans ne ferment aucun volume", "{quoi} #{}: its planes enclose no volume", b.brush.id))),
            }
        }
        if b.object == 0 {
            continue;
        }
        let o = &scene.objects[b.object as usize];
        let a_disp = b.brush.faces.iter().any(|f| f.disp.is_some());
        let polys: Vec<Vec<DVec3>> = b.brush.faces.iter().filter(|f| f.disp.is_none()).map(|f| f.poly.clone()).collect();
        if !a_disp && b.brush.faces.len() >= 4 {
            let v = volume(&polys);
            if v < 1.0 {
                out.push(ici("geo.micro_solide", hm_langue::tr!("{quoi} n° {} : volume de {v:.2} unité cube ({:.1} × {:.1} × {:.1})", "{quoi} #{}: volume of {v:.2} cubic unit ({:.1} × {:.1} × {:.1})", b.brush.id, o.max.x - o.min.x, o.max.y - o.min.y, o.max.z - o.min.z)).mesure(1.0 - v as f32));
            }
        }
        let loin = o.min.abs().max(o.max.abs()).max_element();
        // l'étendue utile : MAX_COORD_INTEGER moins la marge de vbsp (portals.cpp, SIDESPACE)
        if loin > 16376.0 {
            let pourquoi = if loin > 16384.0 { hm_langue::t("vbsp la refuse (« bounds out of range »)") } else { hm_langue::t("vbsp plante (« BSP node with unbounded volume »)") };
            out.push(ici("geo.hors_limites", hm_langue::tr!("{quoi} n° {} : va jusqu'à {loin:.0}, au-delà de ±16 376 : {pourquoi}", "{quoi} #{}: reaches {loin:.0}, beyond ±16,376: {pourquoi}", b.brush.id)).mesure(loin));
        }
        // displacements
        if let Some(node) = noeuds.get(k) {
            for (i, s) in node.children_named("side").enumerate() {
                if s.child("dispinfo").is_none() {
                    continue;
                }
                if !b.world {
                    out.push(ici("geo.displacement_invalide", hm_langue::tr!("displacement sur un solide de {} n° {}", "displacement on a solid of {} #{}", b.class, b.brush.id)));
                } else if !b.brush.faces.iter().any(|f| f.side == i && f.disp.is_some()) {
                    out.push(ici("geo.displacement_invalide", hm_langue::tr!("solide n° {} : la face {} porte un displacement mais n'a pas quatre côtés, ou sa puissance n'est pas entre 2 et 4", "solid #{}: face {} carries a displacement but does not have four sides, or its power is not between 2 and 4", b.brush.id, i + 1)));
                }
            }
        }
        // grille : seulement ce que le joueur heurte ou voit
        if !o.hidden && !a_disp {
            let pas = if b.world { config.grille_monde } else { config.grille_detail };
            let mut hors: Vec<String> = Vec::new();
            for f in &b.brush.faces {
                let n = f.plane.n;
                let axe = (0..3).find(|&a| n[a].abs() > 0.99999);
                match axe {
                    Some(a) => {
                        let pos = f.plane.d * n[a].signum();
                        if !multiple(pos, pas) {
                            hors.push(hm_langue::tr!("face {} à {} = {:.3}", "face {} at {} = {:.3}", f.side + 1, ["x", "y", "z"][a], pos));
                        }
                    }
                    None => {
                        let pts = noeuds.get(k).and_then(|nd| nd.children_named("side").nth(f.side)).and_then(|s| s.get("plane")).and_then(parse_plane);
                        if let Some(pts) = pts {
                            if pts.iter().flatten().any(|c| !multiple(*c, 1.0)) {
                                hors.push(hm_langue::tr!("face oblique {} : points de plan non entiers", "sloped face {}: non-integer plane points", f.side + 1));
                            }
                        }
                    }
                }
            }
            if !hors.is_empty() {
                out.push(ici("geo.hors_grille", hm_langue::tr!("{quoi} n° {} : {}{}", "{quoi} #{}: {}{}", b.brush.id, hors.iter().take(3).cloned().collect::<Vec<_>>().join(" ; "), if hors.len() > 3 { " ; …" } else { "" })).mesure(hors.len() as f32));
            }
        }
        // texture perpendiculaire à la face
        for f in &b.brush.faces {
            if outil.get(f.material.as_str()).copied().flatten().is_some() {
                continue;
            }
            let (u, v) = (DVec3::from(f.uaxis.dir), DVec3::from(f.vaxis.dir));
            let w = u.cross(v);
            if w.length() < 1e-6 || (w.normalize().dot(f.plane.n)).abs() < 1e-3 {
                out.push(ici("geo.texture_perpendiculaire", hm_langue::tr!("{quoi} n° {}, face {} ({}) : les axes de texture sont perpendiculaires à la face", "{quoi} #{}, face {} ({}): the texture axes are perpendicular to the face", b.brush.id, f.side + 1, f.material)));
            }
        }
    }

    // ---------------------------------------------------------------- areaportals
    for e in scene.vmf.entities() {
        if e.classname().eq_ignore_ascii_case("func_areaportal") && e.solids().count() != 1 {
            let mut c = Constat::lieu("geo.areaportal_multiple", Vec3::ZERO, Vec3::ZERO, hm_langue::tr!("func_areaportal n° {} : {} solides au lieu d'un seul", "func_areaportal #{}: {} solids instead of one", e.id().unwrap_or(0), e.solids().count()));
            c.vmf_id = e.id().unwrap_or(0);
            c.classe = "func_areaportal".into();
            if let Some(b) = scene.brushes.iter().find(|b| b.entity_id == c.vmf_id && b.object != 0) {
                let o = &scene.objects[b.object as usize];
                c = c.boite(o.min, o.max);
            }
            out.push(c);
        }
    }

    // ---------------------------------------------------------------- chevauchement de solides du monde
    let monde_solides: Vec<usize> = monde.corps.iter().enumerate().filter(|(_, c)| c.genre == Genre::Monde && !c.ciel).map(|(i, _)| i).collect();
    let plans_de = |i: usize| -> Vec<Plane> { monde.corps[i].convexes[0].plans.iter().map(|p| Plane { n: DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64), d: p[3] as f64 }).collect() };
    let mut chev: Vec<Constat> = monde_solides
        .par_iter()
        .flat_map_iter(|&i| {
            let a = &monde.corps[i];
            let mut v = Vec::new();
            for (j, _) in monde.pres(a.min + Vec3::splat(1.0), a.max - Vec3::splat(1.0)) {
                let j = j as usize;
                let b = &monde.corps[j];
                if j <= i || b.genre != Genre::Monde || b.ciel {
                    continue;
                }
                // recouvrement des boîtes d'au moins 1 sur chaque axe, puis volume exact
                if (0..3).any(|k| a.max[k].min(b.max[k]) - a.min[k].max(b.min[k]) < 1.0) {
                    continue;
                }
                let mut plans = plans_de(i);
                plans.extend(plans_de(j));
                let faces = polytope(&plans);
                let vol = volume(&faces);
                let Some((lo, hi)) = boite(&faces) else { continue };
                let e = hi - lo;
                if vol >= 64.0 && e.min_element() >= 1.0 {
                    v.push(
                        Constat::objet(scene, "geo.chevauchement", a.objet, hm_langue::tr!("{} et {} occupent le même volume sur {:.0} × {:.0} × {:.0}", "{} and {} occupy the same volume over {:.0} × {:.0} × {:.0}", monde.nommer(a.objet), monde.nommer(b.objet), e.x, e.y, e.z))
                            .boite(lo.as_vec3(), hi.as_vec3())
                            .mesure(vol as f32)
                            .autres(vec![b.objet]),
                    );
                }
            }
            v
        })
        .collect();
    out.append(&mut chev);

    // ---------------------------------------------------------------- faces coplanaires superposées
    struct F<'a> {
        objet: u32,
        poly: &'a [DVec3],
        plan: Plane,
        materiau: &'a str,
        /// Modèle de brosses : 0 pour le monde et le détail, sinon l'entité.
        bmodele: u64,
        min: DVec3,
        max: DVec3,
    }
    let mut par_plan: HashMap<(i32, i32, i32, i64), Vec<F>> = HashMap::new();
    for b in &scene.brushes {
        if b.object == 0 || scene.objects[b.object as usize].hidden {
            continue;
        }
        let bmodele = if b.world || b.class == "func_detail" { 0 } else { b.entity_id };
        for f in &b.brush.faces {
            if f.disp.is_some() || f.poly.len() < 3 || outil.get(f.material.as_str()).copied().flatten().is_some() {
                continue;
            }
            let n = f.plane.n;
            let cle = ((n.x * 500.0).round() as i32, (n.y * 500.0).round() as i32, (n.z * 500.0).round() as i32, (f.plane.d * 4.0).round() as i64);
            let lo = f.poly.iter().fold(f.poly[0], |a, p| a.min(*p));
            let hi = f.poly.iter().fold(f.poly[0], |a, p| a.max(*p));
            par_plan.entry(cle).or_default().push(F { objet: b.object, poly: &f.poly, plan: f.plane, materiau: &f.material, bmodele, min: lo, max: hi });
        }
    }
    let groupes: Vec<&Vec<F>> = par_plan.values().filter(|g| g.len() >= 2).collect();
    let mut sup: Vec<Constat> = groupes
        .par_iter()
        .flat_map_iter(|g| {
            let mut v = Vec::new();
            for i in 0..g.len() {
                for j in i + 1..g.len() {
                    let (a, b) = (&g[i], &g[j]);
                    if a.objet == b.objet || (0..3).any(|k| a.max[k].min(b.max[k]) - a.min[k].max(b.min[k]) < -0.01) {
                        continue;
                    }
                    let scintille = a.bmodele != b.bmodele;
                    if !scintille && a.materiau == b.materiau {
                        continue;
                    }
                    // découpe du polygone de a par les arêtes de b, dans le plan commun
                    let n = a.plan.n;
                    let mut poly: Vec<DVec3> = a.poly.to_vec();
                    let m = b.poly.len();
                    for e in 0..m {
                        let (p, q) = (b.poly[e], b.poly[(e + 1) % m]);
                        // normale d'arête sortante : les polygones sont horaires vus de l'extérieur
                        let en = n.cross(q - p);
                        let len = en.length();
                        if len < 1e-9 {
                            continue;
                        }
                        let en = en / len;
                        let d0 = en.dot(p);
                        let mut next = Vec::with_capacity(poly.len() + 2);
                        for s in 0..poly.len() {
                            let (x, y) = (poly[s], poly[(s + 1) % poly.len()]);
                            let (dx, dy) = (en.dot(x) - d0, en.dot(y) - d0);
                            if dx <= 0.0 {
                                next.push(x);
                            }
                            if (dx < 0.0 && dy > 0.0) || (dx > 0.0 && dy < 0.0) {
                                next.push(x + (y - x) * (dx / (dx - dy)));
                            }
                        }
                        poly = next;
                        if poly.len() < 3 {
                            break;
                        }
                    }
                    if poly.len() < 3 {
                        continue;
                    }
                    let s = aire(&poly);
                    if s < 16.0 {
                        continue;
                    }
                    let lo = poly.iter().fold(poly[0], |x, p| x.min(*p)).as_vec3();
                    let hi = poly.iter().fold(poly[0], |x, p| x.max(*p)).as_vec3();
                    let (regle, verbe) = if scintille { ("geo.zfighting", hm_langue::t("se recouvrent dans le même plan et scintilleront")) } else { ("geo.faces_superposees", hm_langue::t("portent deux matériaux sur la même surface")) };
                    v.push(
                        Constat::objet(scene, regle, a.objet, hm_langue::tr!("{} ({}) et {} ({}) {verbe}, sur {s:.0} unités carrées", "{} ({}) and {} ({}) {verbe}, over {s:.0} square units", monde.nommer(a.objet), a.materiau, monde.nommer(b.objet), b.materiau))
                            .boite(lo, hi)
                            .mesure(s as f32)
                            .autres(vec![b.objet]),
                    );
                }
            }
            v
        })
        .collect();
    out.append(&mut sup);
}
