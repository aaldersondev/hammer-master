//! Ce que les vues dessinent, tiré du document du noyau : triangles du monde par matériau, modèles posés, arêtes
//! pour les vues 2D, et la table « identifiant de pixel → objet » pour désigner à la souris.
use glam::{DMat3, DVec3, Mat4, Vec3};
use hm_assets::{Tool, Vfs};
use hm_doc::{Document, Entite, Proprio, Ref};
use hm_render::lignes::Sommet;
use hm_render::vivant::{Bibliotheque, InfoMateriau, LotMonde, Pose, WVertex};
use hm_render::Renderer;

/// Ce qu'un pixel désigne.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Designe {
    /// Ce que le clic sélectionne : l'entité pour un solide d'entité à brosses.
    pub objet: Ref,
    /// Le solide touché, s'il y en a un.
    pub solide: Option<u64>,
}

/// Une forme en fil de fer pour les vues 2D et les contours de sélection.
#[derive(Clone, Debug)]
pub struct Forme {
    pub designe: Designe,
    /// Arêtes en coordonnées du monde.
    pub aretes: Vec<[Vec3; 2]>,
    pub min: Vec3,
    pub max: Vec3,
    pub couleur: [u8; 4],
}

pub struct Geometrie {
    pub sommets: Vec<WVertex>,
    pub lots: Vec<LotMonde>,
    pub poses: Vec<Pose>,
    /// `objets[i]` : ce que désigne l'identifiant i (0 = rien).
    pub objets: Vec<Option<Designe>>,
    pub formes: Vec<Forme>,
    pub min: Vec3,
    pub max: Vec3,
}

/// Classes d'entités à brosses que le joueur ne voit pas : dessinées en couleur d'outil.
fn classe_outil(c: &str) -> bool {
    c.starts_with("trigger_") || matches!(c, "func_areaportal" | "func_areaportalwindow" | "func_viscluster" | "func_occluder" | "func_clip_vphysics" | "func_ladder" | "func_nav_blocker" | "func_nav_avoid" | "func_precipitation" | "func_vehicleclip")
}

/// Une entité à brosses dont les solides font de l'ombre dans vrad : celles que vbsp fond dans le monde
/// (func_detail, func_group) et celles qui le demandent (`vrad_brush_cast_shadows`).
fn entite_ombre(e: &Entite) -> bool {
    let c = e.classe().to_ascii_lowercase();
    c.starts_with("func_detail") || c == "func_group" || e.get("vrad_brush_cast_shadows").map_or(false, |v| v.trim() == "1")
}

/// Une face de ce matériau fait de l'ombre dans vrad : opaque et solide (nodraw compris), pas le ciel, l'eau ni les
/// outils sans matière (clip, trigger, hint, skip, areaportal…), sauf `toolsblocklight`. Les translucides sont
/// écartés plus loin, d'après leur passe.
fn materiau_ombre(nom: &str, info: &InfoMateriau) -> bool {
    if info.eau {
        return false;
    }
    match info.outil {
        None | Some(Tool::Nodraw) | Some(Tool::Invisible) => true,
        Some(Tool::Other) => nom.to_ascii_lowercase().contains("blocklight"),
        _ => false,
    }
}

fn v3(v: DVec3) -> [f32; 3] {
    [v.x as f32, v.y as f32, v.z as f32]
}

/// Transformation d'une entité ponctuelle (origine, angles, échelle du modèle).
/// Les fichiers du projecteur de Hammer++ (`bin/win64/hammerplusplus`) : il montre `spot_color.mdl`, teinté de la
/// couleur `_light`, à la place du `models/editor/spot.mdl` du FGD (vu sur des captures de Hammer++ et dans son dossier).
pub const PROJECTEUR_HAMMERPP: [&str; 6] = [
    "models/editor/spot_color.mdl",
    "models/editor/spot_color.vvd",
    "models/editor/spot_color.dx90.vtx",
    "models/editor/spot_color.dx80.vtx",
    "materials/models/editor/spot_sheet_color.vmt",
    "materials/models/editor/spot_sheet_color.vtf",
];

pub fn transformation(e: &Entite) -> Mat4 {
    transformation_tangage(e, 1.0)
}

/// `signe` -1 : le tangage inversé, pour un modèle de `lightprop`.
fn transformation_tangage(e: &Entite, signe: f64) -> Mat4 {
    let o = e.origine().unwrap_or(DVec3::ZERO);
    let mut a = e.angles();
    a.x *= signe;
    let s = e.get("modelscale").and_then(|v| v.trim().parse::<f64>().ok()).filter(|s| *s > 0.0).unwrap_or(1.0);
    let r: DMat3 = hm_geom::angles_to_mat(a.x, a.y, a.z) * DMat3::from_diagonal(DVec3::splat(s));
    glam::DMat4::from_cols(r.x_axis.extend(0.0), r.y_axis.extend(0.0), r.z_axis.extend(0.0), o.extend(1.0)).as_mat4()
}


/// `classe` : couleur et boîte (sans modèle) d'une classe d'entité, tirées du FGD. `visible` : l'objet est montré
/// (sa boîte, ou son origine pour une entité ponctuelle) ; un objet caché n'est ni dessiné ni désignable.
/// `instance` : le document d'un fichier de `func_instance` (chemin relatif à la map), s'il se lit.
/// `aretes_modele` : les arêtes vives d'un modèle (et son corps) pour les vues 2D, ou `None` pour sa boîte.
pub fn construire(doc: &Document, r: &Renderer, bib: &mut Bibliotheque, vfs: &Vfs, classe: &dyn Fn(&str) -> ([u8; 3], [f32; 3], [f32; 3]), modele_classe: &dyn Fn(&str) -> Option<(String, bool)>, icone_classe: &dyn Fn(&str) -> Option<String>, visible: &dyn Fn(&Designe, DVec3, DVec3) -> bool, instance: &dyn Fn(&str) -> Option<std::sync::Arc<Document>>, aretes_modele: &dyn Fn(&str, u32) -> Option<std::sync::Arc<Vec<[[f32; 3]; 2]>>>) -> Geometrie {
    let mut g = Geometrie { sommets: Vec::new(), lots: Vec::new(), poses: Vec::new(), objets: vec![None], formes: Vec::new(), min: Vec3::splat(f32::MAX), max: Vec3::splat(f32::MIN) };
    let mut lot_de: std::collections::HashMap<(u32, bool, bool), usize> = std::collections::HashMap::new();
    let mut ajouter_tri = |g: &mut Geometrie, mat: u32, outil: bool, ombre: bool, idx: &[u32]| {
        let i = *lot_de.entry((mat, outil, ombre)).or_insert_with(|| {
            g.lots.push(LotMonde { materiau: mat, outil, ombre, indices: Vec::new() });
            g.lots.len() - 1
        });
        g.lots[i].indices.extend_from_slice(idx);
    };
    // boîte de chaque solide montré ou non : les décals cherchent leur surface parmi les solides proches
    let mut boites: Vec<(u64, DVec3, DVec3)> = Vec::new();
    // ------------------------------------------------------------------ solides
    for (proprio, s) in doc.tous_les_solides() {
        let (designe, classe, couleur, ombre_ent) = match proprio {
            Proprio::Monde => (Designe { objet: Ref::Solide(s.id), solide: Some(s.id) }, String::new(), s.couleur(), true),
            Proprio::Entite(e) => {
                let ent = doc.entite(e).unwrap();
                (Designe { objet: Ref::Entite(e), solide: Some(s.id) }, ent.classe().to_ascii_lowercase(), classe(ent.classe()).0, entite_ombre(ent))
            }
        };
        // d'une brosse à displacement, vbsp ne garde que le displacement, et Hammer ne dessine que lui (ses autres
        // faces restent dans les arêtes) : vu le 15/11 sur rp_rockford, le dessus d'un bloc d'arbres en carton
        let a_disp = s.faces.iter().any(|f| f.disp.is_some());
        let polys = s.polygones();
        let mut pts = polys.iter().flatten();
        let Some(p0) = pts.next().copied() else { continue };
        let (b0, b1) = pts.fold((p0, p0), |(a, b), p| (a.min(*p), b.max(*p)));
        if !s.cache {
            boites.push((s.id, b0, b1));
        }
        if !visible(&designe, b0, b1) {
            continue;
        }
        let objet = g.objets.len() as u32;
        g.objets.push(Some(designe));
        let outil_ent = classe_outil(&classe);
        let mut aretes = Vec::new();
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for (f, poly) in s.faces.iter().zip(&polys) {
            if poly.len() < 3 {
                continue;
            }
            for (i, p) in poly.iter().enumerate() {
                let q = poly[(i + 1) % poly.len()];
                aretes.push([p.as_vec3(), q.as_vec3()]);
                lo = lo.min(p.as_vec3());
                hi = hi.max(p.as_vec3());
            }
            let info = bib.materiau(r, vfs, &f.materiau);
            let outil = outil_ent || info.outil.is_some();
            let ombre = ombre_ent && !outil_ent && materiau_ombre(&f.materiau, &info) && (!a_disp || f.disp.is_some());
            let (tw, th) = (info.taille.0.max(1) as f64, info.taille.1.max(1) as f64);
            let n = f.normale();
            let uv = |p: DVec3| {
                let u = p.dot(f.u.dir) / f.u.echelle.max(1e-6) + f.u.decalage;
                let v = p.dot(f.v.dir) / f.v.echelle.max(1e-6) + f.v.decalage;
                [(u / tw) as f32, (v / th) as f32]
            };
            let base = g.sommets.len() as u32;
            match f.disp.as_ref().and_then(|d| deplacement(d, poly, n)) {
                Some(d) => {
                    let k = d.k();
                    let mut normales = vec![DVec3::ZERO; k * k];
                    let mut tris = Vec::with_capacity((k - 1) * (k - 1) * 2);
                    for i in 0..k - 1 {
                        for j in 0..k - 1 {
                            let (a, b, c, e) = (i * k + j, (i + 1) * k + j, (i + 1) * k + j + 1, i * k + j + 1);
                            tris.push([a, b, c]);
                            tris.push([a, c, e]);
                        }
                    }
                    for t in &tris {
                        let nn = (d.points[t[2]] - d.points[t[0]]).cross(d.points[t[1]] - d.points[t[0]]);
                        for &v in t {
                            normales[v] += nn;
                        }
                    }
                    for v in 0..k * k {
                        g.sommets.push(WVertex { pos: v3(d.points[v]), normal: v3(normales[v].normalize_or(n)), uv: uv(d.flat[v]), object: objet, alpha: d.alphas[v] / 255.0 });
                        lo = lo.min(d.points[v].as_vec3());
                        hi = hi.max(d.points[v].as_vec3());
                    }
                    let idx: Vec<u32> = tris.iter().flat_map(|t| t.iter().map(|&v| base + v as u32)).collect();
                    ajouter_tri(&mut g, info.indice, outil, ombre, &idx);
                }
                None if a_disp => {}
                None => {
                    for p in poly {
                        g.sommets.push(WVertex { pos: v3(*p), normal: v3(n), uv: uv(*p), object: objet, alpha: 0.0 });
                    }
                    let idx: Vec<u32> = (1..poly.len() as u32 - 1).flat_map(|i| [base, base + i, base + i + 1]).collect();
                    ajouter_tri(&mut g, info.indice, outil, ombre, &idx);
                }
            }
        }
        if lo.x <= hi.x {
            g.min = g.min.min(lo);
            g.max = g.max.max(hi);
            g.formes.push(Forme { designe, aretes, min: lo, max: hi, couleur: [couleur[0], couleur[1], couleur[2], 255] });
        }
    }
    // ------------------------------------------------------------------ entités ponctuelles
    for e in doc.entites() {
        if !e.ponctuelle() {
            continue;
        }
        let Some(o) = e.origine() else { continue };
        let designe = Designe { objet: Ref::Entite(e.id), solide: None };
        if !visible(&designe, o, o) {
            continue;
        }
        let objet = g.objets.len() as u32;
        g.objets.push(Some(designe));
        let (c, taille_min, taille_max) = classe(e.classe());
        // overlays et décals : un quadrilatère texturé sur leur surface (un peu au-dessus, pour ne pas s'y mêler)
        let quad = match e.classe().to_ascii_lowercase().as_str() {
            "info_overlay" => e.get("material").map(String::from).zip(hm_doc::decals::coins_overlay(e, 0.4)),
            "infodecal" => e.get("texture").map(String::from).and_then(|m| {
                let pres = boites.iter().filter(|(_, lo, hi)| (o - *lo).min_element() >= -16.0 && (*hi - o).min_element() >= -16.0).filter_map(|(id, _, _)| doc.solide(*id));
                let (p, n) = hm_doc::decals::surface_proche_parmi(pres, o, 16.0)?;
                let info = bib.materiau(r, vfs, &m);
                let t = info.taille;
                // taille en unités : texture × $decalscale
                let k = info.decalscale as f64;
                let (u, v) = hm_doc::modele::axes_monde(n);
                let (w, h) = (t.0.max(1) as f64 * k / 2.0, t.1.max(1) as f64 * k / 2.0);
                let c = p + n * 0.4;
                Some((m, [(c - u * w - v * h, [0.0, 0.0]), (c - u * w + v * h, [0.0, 1.0]), (c + u * w + v * h, [1.0, 1.0]), (c + u * w - v * h, [1.0, 0.0])]))
            }),
            _ => None,
        };
        // une instance : les solides de son fichier, posés selon son origine et ses angles, désignés par elle
        if e.classe().eq_ignore_ascii_case("func_instance") {
            if let Some(idoc) = e.get("file").filter(|f| !f.trim().is_empty()).and_then(|f| instance(f)) {
                let t = transformation(e).as_dmat4();
                let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
                let mut aretes = Vec::new();
                for (proprio, s) in idoc.tous_les_solides() {
                    // une instance est fondue dans la map par vbsp : ses solides font de l'ombre comme les autres
                    let ombre_ent = match proprio {
                        Proprio::Monde => true,
                        Proprio::Entite(i) => idoc.entite(i).map_or(false, entite_ombre),
                    };
                    let a_disp = s.faces.iter().any(|f| f.disp.is_some());
                    for (f, poly) in s.faces.iter().zip(s.polygones()) {
                        if poly.len() < 3 {
                            continue;
                        }
                        if a_disp && f.disp.is_none() {
                            continue;
                        }
                        let info = bib.materiau(r, vfs, &f.materiau);
                        let (tw, th) = (info.taille.0.max(1) as f64, info.taille.1.max(1) as f64);
                        let n = t.transform_vector3(f.normale()).normalize_or_zero();
                        let base = g.sommets.len() as u32;
                        for p in &poly {
                            let w = t.transform_point3(*p);
                            let uv = [((p.dot(f.u.dir) / f.u.echelle.max(1e-6) + f.u.decalage) / tw) as f32, ((p.dot(f.v.dir) / f.v.echelle.max(1e-6) + f.v.decalage) / th) as f32];
                            g.sommets.push(WVertex { pos: v3(w), normal: v3(n), uv, object: objet, alpha: 0.0 });
                            lo = lo.min(w.as_vec3());
                            hi = hi.max(w.as_vec3());
                        }
                        for k in 0..poly.len() {
                            aretes.push([t.transform_point3(poly[k]).as_vec3(), t.transform_point3(poly[(k + 1) % poly.len()]).as_vec3()]);
                        }
                        let idx: Vec<u32> = (1..poly.len() as u32 - 1).flat_map(|i| [base, base + i, base + i + 1]).collect();
                        let ombre = ombre_ent && materiau_ombre(&f.materiau, &info) && (!a_disp || f.disp.is_some());
                        ajouter_tri(&mut g, info.indice, info.outil.is_some(), ombre, &idx);
                    }
                }
                if lo.x <= hi.x {
                    g.min = g.min.min(lo);
                    g.max = g.max.max(hi);
                    g.formes.push(Forme { designe, aretes, min: lo, max: hi, couleur: [c[0], c[1], c[2], 255] });
                    continue;
                }
            }
        }
        if let Some((m, coins)) = quad {
            let info = bib.materiau(r, vfs, &m);
            let n = (coins[1].0 - coins[0].0).cross(coins[3].0 - coins[0].0).normalize_or(DVec3::Z);
            let base = g.sommets.len() as u32;
            for (p, uv) in &coins {
                g.sommets.push(WVertex { pos: v3(*p), normal: v3(-n), uv: [uv[0] as f32, uv[1] as f32], object: objet, alpha: 0.0 });
            }
            // les deux faces : un overlay se voit de son côté quel que soit le sens de ses coins
            ajouter_tri(&mut g, info.indice, false, false, &[base, base + 1, base + 2, base, base + 2, base + 3, base, base + 2, base + 1, base, base + 3, base + 2]);
            let pts: Vec<Vec3> = coins.iter().map(|(p, _)| p.as_vec3()).collect();
            let (lo, hi) = pts.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(a, b), p| (a.min(*p), b.max(*p)));
            let aretes = (0..4).map(|k| [pts[k], pts[(k + 1) % 4]]).collect();
            g.min = g.min.min(lo);
            g.max = g.max.max(hi);
            g.formes.push(Forme { designe, aretes, min: lo, max: hi, couleur: [c[0], c[1], c[2], 255] });
            continue;
        }
        // le modèle de l'entité, sinon celui que son FGD montre (`studio("…")`)
        let defaut = modele_classe(e.classe());
        let propre = e.get("model").filter(|m| m.to_ascii_lowercase().ends_with(".mdl"));
        let modele = propre.or(defaut.as_ref().map(|(m, _)| m.as_str()));
        let inverse = propre.is_none() && defaut.as_ref().map_or(false, |(_, i)| *i);
        let mut aretes_props = None;
        // le projecteur de Hammer++ s'il est monté, teinté de la couleur de la lumière
        let projecteur = inverse && modele.map_or(false, |m| m.eq_ignore_ascii_case("models/editor/spot.mdl")) && vfs.locate(PROJECTEUR_HAMMERPP[0]).is_some();
        let modele = if projecteur { Some(PROJECTEUR_HAMMERPP[0]) } else { modele };
        let (lo, hi) = match modele {
            Some(m) => {
                let t = transformation_tangage(e, if inverse { -1.0 } else { 1.0 });
                // `body` l'emporte sur `SetBodyGroup` quand les deux sont là (wiki Valve, « prop_dynamic »)
                let corps = e.get("body").or(e.get("SetBodyGroup")).and_then(|v| v.trim().parse::<f64>().ok()).unwrap_or(0.0).max(0.0) as u32;
                let bornes = bib.modele_corps(r, vfs, m, corps).unwrap_or(([-16.0; 3], [16.0; 3]));
                let skin = e.get("skin").and_then(|v| v.trim().parse::<f64>().ok()).unwrap_or(0.0).max(0.0) as u32;
                let teinte = e.get(if projecteur { "_light" } else { "rendercolor" }).map(hm_doc::kv::nombres).filter(|v| v.len() >= 3).map(|v| [v[0] as f32 / 255.0, v[1] as f32 / 255.0, v[2] as f32 / 255.0, 1.0]).unwrap_or([1.0; 4]);
                // vrad ombre les prop_static (sauf `disableshadows`), pas les modèles qui bougent
                let ombre = e.classe().eq_ignore_ascii_case("prop_static") && e.get("disableshadows").map_or(true, |v| v.trim() != "1");
                g.poses.push(Pose { modele: m.to_string(), skin, corps, transform: t, objet, teinte, ombre });
                aretes_props = aretes_modele(m, corps).map(|a| a.iter().map(|[p, q]| [t.transform_point3(Vec3::from(*p)), t.transform_point3(Vec3::from(*q))]).collect::<Vec<_>>());
                let (b0, b1) = (Vec3::from(bornes.0), Vec3::from(bornes.1));
                let mut lo = Vec3::splat(f32::MAX);
                let mut hi = Vec3::splat(f32::MIN);
                for k in 0..8 {
                    let p = t.transform_point3(Vec3::new(if k & 1 == 0 { b0.x } else { b1.x }, if k & 2 == 0 { b0.y } else { b1.y }, if k & 4 == 0 { b0.z } else { b1.z }));
                    lo = lo.min(p);
                    hi = hi.max(p);
                }
                (lo, hi)
            }
            None if icone_classe(e.classe()).is_some() => {
                // son icône, comme Hammer : 16 unités (64 pixels à l'échelle 0,25) face à la caméra, cachée par ce qui
                // est devant (le shader tourne le panneau : sommets marqués -2, coin dans la normale)
                let info = bib.materiau(r, vfs, &icone_classe(e.classe()).unwrap_or_default());
                let base = g.sommets.len() as u32;
                for (x, y, uv) in [(-8.0, -8.0, [0.0, 1.0]), (-8.0, 8.0, [0.0, 0.0]), (8.0, 8.0, [1.0, 0.0]), (8.0, -8.0, [1.0, 1.0])] {
                    g.sommets.push(WVertex { pos: v3(o), normal: [x, y, 0.0], uv, object: objet, alpha: -2.0 });
                }
                // les deux sens : celui que garde l'élimination des faces arrière dépend du matériau
                ajouter_tri(&mut g, info.indice, false, false, &[base, base + 1, base + 2, base, base + 2, base + 3, base, base + 2, base + 1, base, base + 3, base + 2]);
                let o = o.as_vec3();
                (o + Vec3::from(taille_min), o + Vec3::from(taille_max))
            }
            None => {
                let o = o.as_vec3();
                let (lo, hi) = (o + Vec3::from(taille_min), o + Vec3::from(taille_max));
                let (centre, demi) = ((lo + hi) * 0.5, (hi - lo) * 0.5);
                // une boîte pleine de la couleur de la classe
                let info = bib.couleur(r, [c[0], c[1], c[2], 255]);
                let base = g.sommets.len() as u32;
                let mut idx = Vec::new();
                for (n, u, v) in [(Vec3::X, Vec3::Y, Vec3::Z), (-Vec3::X, Vec3::Z, Vec3::Y), (Vec3::Y, Vec3::Z, Vec3::X), (-Vec3::Y, Vec3::X, Vec3::Z), (Vec3::Z, Vec3::X, Vec3::Y), (-Vec3::Z, Vec3::Y, Vec3::X)] {
                    let b = g.sommets.len() as u32;
                    for (a, bb) in [(-1.0, -1.0), (-1.0, 1.0), (1.0, 1.0), (1.0, -1.0)] {
                        let p = centre + (n + u * a + v * bb) * demi;
                        g.sommets.push(WVertex { pos: p.to_array(), normal: n.to_array(), uv: [0.0, 0.0], object: objet, alpha: 0.0 });
                    }
                    idx.extend([b, b + 1, b + 2, b, b + 2, b + 3]);
                }
                let _ = base;
                ajouter_tri(&mut g, info, false, false, &idx);
                (lo, hi)
            }
        };
        g.min = g.min.min(lo);
        g.max = g.max.max(hi);
        // le fil de fer d'un modèle en cyan, comme Hammer++ dans ses vues 2D (sa boîte garde la couleur du FGD)
        let couleur = if aretes_props.is_some() { [0, 255, 255, 255] } else { [c[0], c[1], c[2], 255] };
        g.formes.push(Forme { designe, aretes: aretes_props.unwrap_or_else(|| aretes_boite(lo, hi)), min: lo, max: hi, couleur });
    }
    if g.min.x > g.max.x {
        g.min = Vec3::splat(-512.0);
        g.max = Vec3::splat(512.0);
    }
    g
}

pub fn aretes_boite(lo: Vec3, hi: Vec3) -> Vec<[Vec3; 2]> {
    let c = |i: u32| Vec3::new(if i & 1 == 0 { lo.x } else { hi.x }, if i & 2 == 0 { lo.y } else { hi.y }, if i & 4 == 0 { lo.z } else { hi.z });
    let mut out = Vec::with_capacity(12);
    for i in 0..8u32 {
        for b in [1u32, 2, 4] {
            if i & b == 0 {
                out.push([c(i), c(i | b)]);
            }
        }
    }
    out
}

/// Les arêtes de toutes les formes, en sommets de lignes.
pub fn lignes(formes: &[Forme], filtre: impl Fn(&Forme) -> Option<[u8; 4]>) -> Vec<Sommet> {
    let mut out = Vec::new();
    for f in formes {
        let Some(c) = filtre(f) else { continue };
        for a in &f.aretes {
            out.push(Sommet { pos: a[0].to_array(), couleur: c });
            out.push(Sommet { pos: a[1].to_array(), couleur: c });
        }
    }
    out
}

/// La grille d'un displacement, par le calcul de `hm-geom` (le bloc `dispinfo` lui est passé en texte).
fn deplacement(d: &hm_doc::kv::Bloc, poly: &[DVec3], n: DVec3) -> Option<hm_geom::Displacement> {
    let mut w = hm_doc::kv::Ecrivain::new(hm_doc::kv::Format { crlf: false, latin1: false });
    w.bloc(d, 0);
    let v = hm_vmf::Vmf::parse(w.out).ok()?;
    hm_geom::build_displacement(v.root.children.first()?, poly, n)
}

/// Le solide sous un point d'une vue 2D (axes `ax`, `ay`) : celui dont une arête passe à moins de `tol`, sinon le
/// plus petit dont la boîte contient le point. Rend tous les candidats, du meilleur au moins bon.
pub fn sous_le_point_2d(formes: &[Forme], ax: usize, ay: usize, p: [f32; 2], tol: f32) -> Vec<Designe> {
    let mut cands: Vec<(f32, Designe)> = Vec::new();
    for f in formes {
        let (lo, hi) = (f.min, f.max);
        let dedans = p[0] >= lo[ax] - tol && p[0] <= hi[ax] + tol && p[1] >= lo[ay] - tol && p[1] <= hi[ay] + tol;
        if !dedans {
            continue;
        }
        let mut d_arete = f32::MAX;
        for a in &f.aretes {
            let (x0, y0, x1, y1) = (a[0][ax], a[0][ay], a[1][ax], a[1][ay]);
            let (dx, dy) = (x1 - x0, y1 - y0);
            let l2 = dx * dx + dy * dy;
            let t = if l2 > 1e-9 { (((p[0] - x0) * dx + (p[1] - y0) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
            let (qx, qy) = (x0 + t * dx - p[0], y0 + t * dy - p[1]);
            d_arete = d_arete.min((qx * qx + qy * qy).sqrt());
        }
        let aire = (hi[ax] - lo[ax]).max(1.0) * (hi[ay] - lo[ay]).max(1.0);
        // une arête sous le curseur passe devant ; sinon le plus petit objet qui contient le point
        let score = if d_arete <= tol { d_arete } else { tol + aire.sqrt() };
        cands.push((score, f.designe));
    }
    cands.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<Designe> = Vec::new();
    for (_, d) in cands {
        if !out.iter().any(|x| x.objet == d.objet) {
            out.push(d);
        }
    }
    out
}

