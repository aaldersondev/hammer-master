//! Cohérence physique : ce qui flotte, ce qui est enfoui, ce qui traverse, les WC sans cloison, les portes
//! qui ne peuvent pas s'ouvrir.
use glam::{Mat3, Mat4, Vec3};
use hm_scene::Scene;
use rayon::prelude::*;

use crate::monde::{se_touchent, Convexe, Corps, Genre, Monde};
use crate::{Config, Constat, Niveau, Regle};

pub const REGLES: &[Regle] = &[
    Regle { id: "phys.flotte", niveau: Niveau::Avertissement, titre: "Flotte", explication: "Un groupe d'objets ne touche rien qui le relie au sol : ni solide, ni modèle, ni terrain, par aucune chaîne de contacts.", source: "contrôle propre à l'outil ; défaut de référence : l'auvent des stations aériennes du 10/11/2026" },
    Regle { id: "phys.enfoui", niveau: Niveau::Avertissement, titre: "Modèle enfoui", explication: "Une grande part des sommets d'un modèle est à l'intérieur d'un solide : il est caché ou coupé par un mur ou un sol. Un prop_static dont toute la collision est dans du plein est retiré par vbsp.", source: "contrôle propre à l'outil ; vbsp staticprop.cpp, AddStaticPropToLump : « Static prop … outside the map » quand aucune feuille non pleine ne touche sa collision (lu le 13/11/2026)" },
    Regle { id: "phys.traverse", niveau: Niveau::Avertissement, titre: "Modèle qui traverse un solide", explication: "Le maillage d'un modèle entre dans un solide et en ressort de l'autre côté.", source: "contrôle propre à l'outil ; défaut de référence : les rails sortant du mur est de la gare, 10/11/2026" },
    Regle { id: "phys.imbrique", niveau: Niveau::Information, titre: "Modèle imbriqué dans la structure", explication: "Un modèle passe à travers quatre solides ou plus : c'est en général un habillage voulu (galerie, balcons filants), à vérifier une fois.", source: "contrôle propre à l'outil" },
    Regle { id: "phys.decolle", niveau: Niveau::Information, titre: "Décollé de son appui", explication: "Un objet est à moins de 4 unités de son appui sans le toucher : un jour se voit de près.", source: "contrôle propre à l'outil ; jeu admis réglable (jeu_contact)" },
    Regle { id: "phys.porte_a_faux", niveau: Niveau::Avertissement, titre: "Panneau qui dépasse de son appui", explication: "Un solide mince posé contre un mur (enseigne, panneau, plaque) est adossé à son mur sauf à un bout, qui dépasse dans le vide au-delà de l'angle du mur.", source: "contrôle propre à l'outil ; signalement L06 (« panneau qui dépasse ») du 11/11/2026" },
    Regle { id: "phys.wc_sans_cloison", niveau: Niveau::Avertissement, titre: "Cuvettes de WC sans cloison", explication: "Deux cuvettes voisines se voient l'une l'autre : ni cabine ni cloison entre elles.", source: "contrôle propre à l'outil ; défaut de référence : les 13 salles de WC sans cabines du 10/11/2026" },
    Regle { id: "phys.porte_bloquee", niveau: Niveau::Avertissement, titre: "Porte qui ne peut pas s'ouvrir", explication: "Le vantail d'une porte battante rencontre un solide ou un modèle avant d'être ouvert à 90°, des deux côtés.", source: "contrôle propre à l'outil ; le vantail tourne autour de l'origine de l'entité" },
    Regle { id: "phys.porte_genee", niveau: Niveau::Information, titre: "Porte gênée d'un côté", explication: "Le vantail d'une porte battante rencontre un obstacle d'un seul côté.", source: "contrôle propre à l'outil" },
];

/// Vrai si le nom de fichier du modèle contient un des motifs (les `*` sont ignorés).
pub fn contient(motifs: &[String], modele: &str) -> bool {
    let base = modele.rsplit('/').next().unwrap_or(modele).to_ascii_lowercase();
    motifs.iter().any(|m| {
        let m = m.trim_matches('*').to_ascii_lowercase();
        !m.is_empty() && base.contains(&m)
    })
}

struct Union(Vec<u32>);
impl Union {
    fn racine(&mut self, mut i: u32) -> u32 {
        while self.0[i as usize] != i {
            self.0[i as usize] = self.0[self.0[i as usize] as usize];
            i = self.0[i as usize];
        }
        i
    }
    fn unir(&mut self, a: u32, b: u32) {
        let (a, b) = (self.racine(a), self.racine(b));
        if a != b {
            self.0[a.max(b) as usize] = a.min(b);
        }
    }
}

/// Un corps qui porte ou est porté : pas les mobiles (portes, trains), pas les objets libres (attachés, physiques,
/// suspendus exprès).
fn porteur(c: &Corps) -> bool {
    c.genre != Genre::Mobile && !c.libre
}

/// Un solide qui peut cacher ou couper un modèle.
pub(crate) fn plein(c: &Corps) -> bool {
    matches!(c.genre, Genre::Monde | Genre::Detail | Genre::Brosse) && c.visible && !c.ciel
}

pub fn verifier(scene: &Scene, monde: &Monde, config: &Config, out: &mut Vec<Constat>) {
    flotte(scene, monde, config, out);
    enfoui_et_traverse(scene, monde, out);
    wc(scene, monde, config, out);
    portes(scene, monde, out);
    porte_a_faux(scene, monde, out);
}

/// Taille à partir de laquelle un groupe d'objets en contact est « le sol » même sans ciel ni terrain.
const GROUPE_ANCRE: usize = 150;

fn flotte(scene: &Scene, monde: &Monde, config: &Config, out: &mut Vec<Constat>) {
    let jeu = config.jeu_contact;
    let n = monde.corps.len();
    let aretes: Vec<(u32, u32)> = (0..n as u32)
        .into_par_iter()
        .flat_map_iter(|i| {
            let a = &monde.corps[i as usize];
            let mut v = Vec::new();
            if !porteur(a) {
                return v;
            }
            let pts = monde.points_de_contact(i);
            for j in monde.corps_pres(a.min - Vec3::splat(jeu), a.max + Vec3::splat(jeu)) {
                // chaque paire est vue une fois, depuis le corps de plus petit rang
                if j <= i || !porteur(&monde.corps[j as usize]) {
                    continue;
                }
                if monde.contact(i, &pts, j, jeu) {
                    v.push((i, j));
                }
            }
            v
        })
        .collect();
    let mut u = Union((0..n as u32).collect());
    for (a, b) in aretes {
        u.unir(a, b);
    }
    let mut groupes: std::collections::HashMap<u32, Vec<u32>> = std::collections::HashMap::new();
    for i in 0..n as u32 {
        if porteur(&monde.corps[i as usize]) {
            let r = u.racine(i);
            groupes.entry(r).or_default().push(i);
        }
    }
    // Le sol : tout groupe qui contient du ciel ou du terrain, tout grand groupe, et dans tous les cas le plus
    // grand groupe de la map (une petite map d'essai n'a ni ciel ni 150 objets).
    let plus_grand = groupes.values().map(|g| g.len()).max().unwrap_or(0);
    let est_ancre = |g: &Vec<u32>| g.len() >= GROUPE_ANCRE || g.len() == plus_grand || g.iter().any(|&i| monde.corps[i as usize].ciel || monde.corps[i as usize].genre == Genre::Terrain);
    // ce qui tient au sol : seul ce genre de voisin compte pour dire à quelle distance est l'appui le plus proche
    let mut au_sol = vec![false; n];
    for g in groupes.values().filter(|g| est_ancre(g)) {
        for &i in g {
            au_sol[i as usize] = true;
        }
    }
    let mut iles: Vec<&Vec<u32>> = groupes.values().filter(|g| !est_ancre(g)).collect();
    iles.sort_by_key(|g| g[0]);
    for g in iles {
        // un groupe entièrement invisible (remplissage en nodraw) ne flotte pas aux yeux du joueur
        if !g.iter().any(|&i| monde.corps[i as usize].visible) {
            continue;
        }
        let vol = |c: &Corps| {
            let e = c.max - c.min;
            e.x * e.y * e.z
        };
        let principal = *g.iter().max_by(|a, b| vol(&monde.corps[**a as usize]).total_cmp(&vol(&monde.corps[**b as usize]))).unwrap();
        let lo = g.iter().fold(Vec3::splat(f32::MAX), |a, &i| a.min(monde.corps[i as usize].min));
        let hi = g.iter().fold(Vec3::splat(f32::MIN), |a, &i| a.max(monde.corps[i as usize].max));
        // vide sous le groupe : rayon vers le bas depuis le dessous de l'objet principal
        let p = &monde.corps[principal as usize];
        let c = (p.min + p.max) * 0.5;
        let depart = Vec3::new(c.x, c.y, lo.z - 0.5);
        let dessous = monde.rayon(depart, depart - Vec3::Z * 4096.0, |c, k| porteur(c) && !g.contains(&k)).map(|(t, k)| (t * 4096.0, k));
        let e = hi - lo;
        // à quelle distance est le premier objet qui n'est pas du groupe ?
        let mut proche: Option<(f32, u32)> = None;
        'jeux: for essai in [4.0f32, 8.0, 16.0, 32.0, 64.0] {
            for &i in g {
                let a = &monde.corps[i as usize];
                let pts = monde.points_de_contact(i);
                for j in monde.corps_pres(a.min - Vec3::splat(essai), a.max + Vec3::splat(essai)) {
                    if au_sol[j as usize] && monde.contact(i, &pts, j, essai) {
                        proche = Some((essai, j));
                        break 'jeux;
                    }
                }
            }
        }
        let autres: Vec<u32> = g.iter().filter(|&&i| i != principal).map(|&i| monde.corps[i as usize].objet).collect();
        let avec = if autres.is_empty() {
            "ne touche rien".to_string()
        } else {
            format!("ne touche que {} autre(s) objet(s) ({})", autres.len(), autres.iter().take(3).map(|&o| monde.nommer(o)).collect::<Vec<_>>().join(", "))
        };
        let vide = match dessous {
            Some((d, k)) => format!("{d:.0} de vide dessous, jusqu'à {}", monde.nommer(monde.corps[k as usize].objet)),
            None => "rien dessous".to_string(),
        };
        let voisin = match proche {
            Some((j, k)) => format!("le plus proche objet qui tient au sol, {}, est à moins de {j:.0} mais à plus de {:.0}", monde.nommer(monde.corps[k as usize].objet), if j <= 4.0 { jeu } else { j / 2.0 }),
            None => "rien qui tienne au sol à moins de 64".to_string(),
        };
        let regle = if proche.map_or(false, |x| x.0 <= 4.0) { "phys.decolle" } else { "phys.flotte" };
        out.push(
            Constat::objet(scene, regle, p.objet, format!("{} ({:.0} × {:.0} × {:.0}) {avec} : aucun contact ne le relie au sol ; {voisin} ; {vide}", monde.nommer(p.objet), e.x, e.y, e.z))
                .boite(lo, hi)
                .mesure(proche.map_or(128.0, |x| x.0))
                .autres(autres),
        );
    }
}

/// Part des sommets enfouis à partir de laquelle un modèle est dit enfoui, et profondeur minimale.
const PART_ENFOUIE: f32 = 0.5;
const PROFONDEUR_ENFOUIE: f32 = 8.0;
/// Longueur minimale qui doit dépasser de chaque côté d'un solide traversé.
const DEPASSE: f32 = 16.0;
/// Nombre de solides traversés à partir duquel un modèle est un habillage imbriqué dans la structure.
const IMBRIQUE: usize = 4;

fn enfoui_et_traverse(scene: &Scene, monde: &Monde, out: &mut Vec<Constat>) {
    let res: Vec<Constat> = monde
        .corps
        .par_iter()
        .filter(|c| c.genre == Genre::Prop && !c.libre)
        .flat_map_iter(|c| {
            let mut v = Vec::new();
            let Some(k) = c.prop else { return v };
            let p = &scene.props[k as usize];
            let Some(m) = &scene.models[p.model as usize].model else { return v };
            // les solides de moins de 2 d'épaisseur sont des marquages posés sur une surface, pas des murs
            let voisins: Vec<(u32, u16)> = monde.pres(c.min, c.max).into_iter().filter(|r| plein(&monde.corps[r.0 as usize]) && (monde.corps[r.0 as usize].max - monde.corps[r.0 as usize].min).min_element() >= 2.0).collect();
            if voisins.is_empty() {
                return v;
            }
            let pts: Vec<Vec3> = m.vertices.iter().map(|q| p.transform.transform_point3(Vec3::from(q.pos))).collect();
            if pts.is_empty() {
                return v;
            }
            // ---- enfoui : part des sommets à l'intérieur d'un solide
            let pas = (pts.len() / 600).max(1);
            let (mut dedans, mut total, mut pmax, mut dans_qui) = (0usize, 0usize, 0.0f32, 0u32);
            for q in pts.iter().step_by(pas) {
                total += 1;
                let mut prof = 0.0f32;
                for r in &voisins {
                    let cv = monde.convexe(*r);
                    if (0..3).all(|a| q[a] >= cv.min[a] && q[a] <= cv.max[a]) {
                        let d = cv.profondeur(*q);
                        if d > prof {
                            prof = d;
                            if d > pmax {
                                dans_qui = r.0;
                            }
                        }
                    }
                }
                if prof > 2.0 {
                    dedans += 1;
                }
                pmax = pmax.max(prof);
            }
            let part = dedans as f32 / total.max(1) as f32;
            if part >= PART_ENFOUIE && pmax >= PROFONDEUR_ENFOUIE {
                let dans = monde.corps[dans_qui as usize].objet;
                v.push(
                    Constat::objet(scene, "phys.enfoui", c.objet, format!("{} : {:.0} % de ses sommets sont dans un solide, jusqu'à {pmax:.0} de profondeur ({}){}", monde.nommer(c.objet), part * 100.0, monde.nommer(dans), if part >= 0.99 && o_statique(scene, c.objet) { " ; entièrement dans du plein, vbsp le déclarera « outside the map » et le retirera" } else { "" }))
                        .mesure(part * 100.0)
                        .autres(vec![dans]),
                );
                return v; // un modèle enfoui traverse forcément : un seul constat
            }
            // ---- traverse : une arête du maillage entre dans un solide et en ressort
            // par solide traversé : (longueur qui dépasse du côté court, point de passage)
            let mut perces: Vec<(u32, f32, Vec3)> = Vec::new();
            let mut refuses: Vec<u32> = Vec::new();
            for mesh in &m.meshes {
                let pas_t = (mesh.indices.len() / 3 / 4000).max(1);
                for t in mesh.indices.chunks_exact(3).step_by(pas_t) {
                    for e in 0..3 {
                        let (a, b) = (pts[t[e] as usize], pts[t[(e + 1) % 3] as usize]);
                        let long = a.distance(b);
                        if long < 2.0 * DEPASSE {
                            continue;
                        }
                        let (lo, hi) = (a.min(b), a.max(b));
                        for r in &voisins {
                            let cv = monde.convexe(*r);
                            if refuses.contains(&r.0) || (0..3).any(|x| lo[x] > cv.max[x] || hi[x] < cv.min[x]) {
                                continue;
                            }
                            let Some((t0, t1)) = cv.segment(a, b) else { continue };
                            // Percer, c'est dépasser de chaque côté de plus que l'épaisseur traversée : un rail
                            // dans un mur, un mât dans une dalle. Un chapiteau autour de son pilier a plus de
                            // solide que de dépassement.
                            let (dedans, court) = ((t1 - t0) * long, (t0 * long).min((1.0 - t1) * long));
                            if dedans < 1.0 || court < DEPASSE || dedans > court {
                                continue;
                            }
                            if let Some(x) = perces.iter_mut().find(|x| x.0 == r.0) {
                                if court > x.1 {
                                    x.1 = court;
                                }
                                continue;
                            }
                            // Et la section du modèle doit être avalée par les solides à l'endroit du passage.
                            // Un îlot autour d'un pilier, une jardinière autour d'un poteau se voient tout autour
                            // du solide : ce sont eux qui l'entourent.
                            let ou = a.lerp(b, (t0 + t1) * 0.5);
                            let dir = (b - a).abs();
                            let axe = if dir.x >= dir.y && dir.x >= dir.z { 0 } else if dir.y >= dir.z { 1 } else { 2 };
                            let (u, w) = ((axe + 1) % 3, (axe + 2) % 3);
                            let (mut dans, mut total) = (0, 0);
                            for i in 0..5 {
                                for j in 0..5 {
                                    let mut q = ou;
                                    q[u] = c.min[u] + (c.max[u] - c.min[u]) * (0.1 + 0.2 * i as f32);
                                    q[w] = c.min[w] + (c.max[w] - c.min[w]) * (0.1 + 0.2 * j as f32);
                                    total += 1;
                                    if voisins.iter().any(|v| monde.convexe(*v).profondeur(q) > 0.0) {
                                        dans += 1;
                                    }
                                }
                            }
                            if dans as f32 / total as f32 >= 0.85 {
                                perces.push((r.0, court, ou));
                            } else {
                                refuses.push(r.0);
                            }
                        }
                    }
                }
            }
            if let Some(&(qui, court, ou)) = perces.iter().max_by(|a, b| a.1.total_cmp(&b.1)) {
                let q = monde.corps[qui as usize].objet;
                let autres: Vec<u32> = perces.iter().map(|x| monde.corps[x.0 as usize].objet).collect();
                if perces.len() >= IMBRIQUE {
                    // un modèle qui passe à travers beaucoup de solides est un habillage imbriqué dans la structure
                    v.push(Constat::objet(scene, "phys.imbrique", c.objet, format!("{} passe à travers {} solides (le plus net : {}, dépassé de {court:.0} de chaque côté)", monde.nommer(c.objet), perces.len(), monde.nommer(q))).mesure(perces.len() as f32).autres(autres));
                } else {
                    v.push(
                        Constat::objet(scene, "phys.traverse", c.objet, format!("{} traverse {} de part en part : il dépasse d'au moins {court:.0} de chaque côté", monde.nommer(c.objet), monde.nommer(q)))
                            .boite(ou - Vec3::splat(96.0), ou + Vec3::splat(96.0))
                            .mesure(court)
                            .autres(autres),
                    );
                }
            }
            v
        })
        .collect();
    out.extend(res);
}

/// Distance en plan au-delà de laquelle deux cuvettes ne sont plus voisines, et écart de niveau admis.
const WC_VOISINS: f32 = 200.0;

fn wc(scene: &Scene, monde: &Monde, config: &Config, out: &mut Vec<Constat>) {
    let cuvettes: Vec<u32> = monde.corps.iter().enumerate().filter(|(_, c)| c.genre == Genre::Prop && contient(&config.cuvettes, &scene.objects[c.objet as usize].model)).map(|(i, _)| i as u32).collect();
    let centre = |i: u32| {
        let c = &monde.corps[i as usize];
        (c.min + c.max) * 0.5
    };
    let mut exposees: Vec<(u32, u32)> = Vec::new();
    for (x, &a) in cuvettes.iter().enumerate() {
        for &b in &cuvettes[x + 1..] {
            let (pa, pb) = (centre(a), centre(b));
            if (pa.z - pb.z).abs() > 48.0 || pa.truncate().distance(pb.truncate()) > WC_VOISINS {
                continue;
            }
            // à hauteur d'assise : quelque chose se dresse-t-il entre les deux ?
            let coupe = monde.rayon(pa, pb, |c, k| {
                if k == a || k == b || !c.visible || c.genre == Genre::Terrain {
                    return false;
                }
                // une boîte approchée qui englobe les deux cuvettes ne dit rien
                !(c.approche && c.convexes[0].profondeur(pa) > 0.0 && c.convexes[0].profondeur(pb) > 0.0)
            });
            if coupe.is_none() {
                exposees.push((a, b));
            }
        }
    }
    // une alerte par groupe de cuvettes qui se voient
    let mut u = Union((0..monde.corps.len() as u32).collect());
    for &(a, b) in &exposees {
        u.unir(a, b);
    }
    let mut groupes: std::collections::BTreeMap<u32, Vec<u32>> = Default::default();
    for &(a, b) in &exposees {
        let r = u.racine(a);
        let g = groupes.entry(r).or_default();
        for x in [a, b] {
            if !g.contains(&x) {
                g.push(x);
            }
        }
    }
    for g in groupes.values() {
        let lo = g.iter().fold(Vec3::splat(f32::MAX), |a, &i| a.min(monde.corps[i as usize].min));
        let hi = g.iter().fold(Vec3::splat(f32::MIN), |a, &i| a.max(monde.corps[i as usize].max));
        let premier = monde.corps[g[0] as usize].objet;
        out.push(
            Constat::objet(scene, "phys.wc_sans_cloison", premier, format!("{} cuvettes voisines se voient l'une l'autre, sans cabine ni cloison entre elles ({})", g.len(), g.iter().take(4).map(|&i| monde.nommer(monde.corps[i as usize].objet)).collect::<Vec<_>>().join(", ")))
                .boite(lo, hi)
                .mesure(g.len() as f32)
                .autres(g[1..].iter().map(|&i| monde.corps[i as usize].objet).collect()),
        );
    }
}

fn portes(scene: &Scene, monde: &Monde, out: &mut Vec<Constat>) {
    for (i, c) in monde.corps.iter().enumerate() {
        let o = &scene.objects[c.objet as usize];
        if c.genre != Genre::Prop || o.class != "prop_door_rotating" {
            continue;
        }
        let Some(k) = c.prop else { continue };
        let p = &scene.props[k as usize];
        let Some(m) = &scene.models[p.model as usize].model else { continue };
        let ent = monde.entites.get(&o.vmf_id);
        let angle = ent.and_then(|e| e.f64("distance")).filter(|d| *d > 0.0).unwrap_or(90.0) as f32;
        // le vantail : la boîte du modèle, rétrécie pour ne pas compter le frottement contre l'huisserie et le sol
        let (lo, hi) = (Vec3::from(m.bounds.0), Vec3::from(m.bounds.1));
        let e = hi - lo;
        let marge = Vec3::new(3.0f32.min(e.x * 0.25), 3.0f32.min(e.y * 0.25), 4.0f32.min(e.z * 0.25));
        let boite = Convexe::boite(lo + marge, hi - marge);
        let pose = |deg: f32| -> Convexe {
            // rotation autour de l'axe vertical passant par l'origine de l'entité
            let t = p.transform * Mat4::from_rotation_z(deg.to_radians());
            let rot = Mat3::from_mat4(t);
            let tr = t.w_axis.truncate();
            let pts: Vec<Vec3> = boite.pts.iter().map(|q| t.transform_point3(*q)).collect();
            let plans = boite.plans.iter().map(|pl| {
                let n = rot * Vec3::new(pl[0], pl[1], pl[2]);
                [n.x, n.y, n.z, pl[3] + n.dot(tr)]
            });
            let lo = pts.iter().fold(Vec3::splat(f32::MAX), |a, q| a.min(*q));
            let hi = pts.iter().fold(Vec3::splat(f32::MIN), |a, q| a.max(*q));
            Convexe { plans: plans.collect(), pts, min: lo, max: hi }
        };
        let heurte = |cv: &Convexe, ignore: &[u32]| -> Option<u32> {
            monde.pres(cv.min, cv.max).into_iter().find_map(|r| {
                let autre = &monde.corps[r.0 as usize];
                let bloque = r.0 as usize != i && !ignore.contains(&r.0) && autre.genre != Genre::Mobile && autre.genre != Genre::Terrain && !autre.libre && (autre.genre != Genre::Prop || (scene.props[autre.prop.unwrap() as usize].solid && !autre.approche));
                // pénétration franche : au moins 2 unités
                (bloque && se_touchent(cv, monde.convexe(r), -2.0)).then_some(r.0)
            })
        };
        // ce que le vantail touche déjà fermé (huisserie, mur du cadre) ne compte pas
        let ferme = pose(0.0);
        let cadre: Vec<u32> = monde.pres(ferme.min, ferme.max).into_iter().filter(|r| se_touchent(&ferme, monde.convexe(*r), -2.0)).map(|r| r.0).collect();
        let bloque_vers = |sens: f32| -> Option<(f32, u32)> { [0.5f32, 1.0].iter().find_map(|f| heurte(&pose(sens * angle * f), &cadre).map(|q| (angle * f, q))) };
        let (a, b) = (bloque_vers(1.0), bloque_vers(-1.0));
        let dire = |x: &(f32, u32)| format!("{} à {:.0}°", monde.nommer(monde.corps[x.1 as usize].objet), x.0);
        match (a, b) {
            (Some(x), Some(y)) => out.push(Constat::objet(scene, "phys.porte_bloquee", c.objet, format!("{} ne peut s'ouvrir d'aucun côté : elle rencontre {} dans un sens et {} dans l'autre", monde.nommer(c.objet), dire(&x), dire(&y))).mesure(180.0 - x.0 - y.0).autres(vec![monde.corps[x.1 as usize].objet, monde.corps[y.1 as usize].objet])),
            (Some(x), None) | (None, Some(x)) => out.push(Constat::objet(scene, "phys.porte_genee", c.objet, format!("{} rencontre {} en s'ouvrant d'un côté ; l'autre côté est libre", monde.nommer(c.objet), dire(&x))).mesure(90.0 - x.0).autres(vec![monde.corps[x.1 as usize].objet])),
            (None, None) => {}
        }
    }
}

fn o_statique(scene: &Scene, objet: u32) -> bool {
    scene.objects[objet as usize].class == "prop_static"
}

/// Panneaux minces posés contre un mur dont une partie dépasse de l'appui.
fn porte_a_faux(scene: &Scene, monde: &Monde, out: &mut Vec<Constat>) {
    for (i, c) in monde.corps.iter().enumerate() {
        if !matches!(c.genre, Genre::Detail | Genre::Brosse) || !c.visible || c.libre || c.convexes.len() != 1 {
            continue;
        }
        let e = c.max - c.min;
        // un panneau : mince dans un sens horizontal (8 au plus), et grand dans les deux autres
        let k = if e.x <= e.y { 0 } else { 1 };
        let (u, w) = (1 - k, 2);
        if e[k] > 8.0 || e[u] < 48.0 || e[w] < 24.0 || e[u] * e[w] < 2048.0 {
            continue;
        }
        // une cloison debout sur le sol n'est pas un panneau accroché : le dessous du milieu repose sur du plein
        let mut dessous = (c.min + c.max) * 0.5;
        dessous.z = c.min.z - 3.0;
        if monde.dans(dessous, |x, j| j as usize != i && plein(x)).is_some() {
            continue;
        }
        // De chaque côté, à 3 unités de la face, six colonnes de quatre points : lesquelles sont adossées à du
        // plein ? Seul un dépassement sur le côté compte (des colonnes entières dans le vide à un bout, les autres
        // adossées) : une enseigne dressée sur le bord d'une marquise dépasse vers le haut, et c'est voulu.
        let colonnes = |cote: f32| -> [u8; 6] {
            let mut n = [0u8; 6];
            for (a, col) in n.iter_mut().enumerate() {
                for b in 0..4 {
                    let mut p = (c.min + c.max) * 0.5;
                    p[k] = if cote > 0.0 { c.max[k] + 3.0 } else { c.min[k] - 3.0 };
                    p[u] = c.min[u] + e[u] * (a as f32 + 0.5) / 6.0;
                    p[w] = c.min[w] + e[w] * (b as f32 + 0.5) / 4.0;
                    if monde.dans(p, |x, j| j as usize != i && plein(x)).is_some() {
                        *col += 1;
                    }
                }
            }
            n
        };
        let (a, b) = (colonnes(1.0), colonnes(-1.0));
        let (sa, sb): (u32, u32) = (a.iter().map(|x| *x as u32).sum(), b.iter().map(|x| *x as u32).sum());
        let (dos, autre) = if sa >= sb { (a, sb) } else { (b, sa) };
        let pleines = dos.iter().filter(|x| **x >= 3).count();
        let vides_au_bout = dos.iter().take_while(|x| **x == 0).count() + dos.iter().rev().take_while(|x| **x == 0).count();
        // adossé sur deux colonnes au moins, le reste étant des colonnes entières dans le vide à un bout ou aux deux ;
        // rien derrière l'autre face
        if autre == 0 && pleines >= 2 && vides_au_bout >= 1 && pleines + vides_au_bout == 6 {
            let part = pleines as f32 / 6.0;
            out.push(Constat::objet(scene, "phys.porte_a_faux", c.objet, format!("{} ({:.0} × {:.0}, épaisseur {:.0}) n'est adossé que sur {:.0} % de sa longueur : il dépasse de {:.0} au bout de son mur", monde.nommer(c.objet), e[u], e[w], e[k], part * 100.0, e[u] * (1.0 - part))).mesure(e[u] * (1.0 - part)));
        }
    }
}
