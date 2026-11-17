//! Décals (`infodecal`) et overlays (`info_overlay`). Un overlay est un quadrilatère posé sur des faces : origine de
//! sa base (`BasisOrigin`), axes U, V et normale (`BasisU`, `BasisV`, `BasisNormal`), coins `uv0`…`uv3` en unités
//! dans cette base, plage de texture `StartU`…`EndV`, faces portées (`sides`) — wiki Valve, « info_overlay ». Un
//! décal n'a qu'un matériau (`texture`) et une origine ; le jeu le pose sur la surface la plus proche.
use glam::DVec3;

use crate::modele::{lire_vec3, texte_vec3, Document, Entite, Face};

/// Les clés d'un overlay posé en `point` sur une face : base tirée de la face (U le long de l'axe U de sa texture),
/// un quadrilatère de `taille` unités centré sur le point.
pub fn cles_overlay(point: DVec3, face: &Face, materiau: &str, taille: [f64; 2]) -> Vec<(String, String)> {
    let n = face.normale();
    let mut u = face.u.dir - n * n.dot(face.u.dir);
    if u.length() < 1e-6 {
        u = crate::modele::axes_monde(n).0;
    }
    let u = u.normalize();
    // V dans le plan, du côté de l'axe V de la texture (l'image n'est pas retournée)
    let mut v = n.cross(u);
    if v.dot(face.v.dir) < 0.0 {
        v = -v;
    }
    let (w, h) = (taille[0] / 2.0, taille[1] / 2.0);
    vec![
        ("material".into(), materiau.to_string()),
        ("sides".into(), face.id.to_string()),
        ("RenderOrder".into(), "0".into()),
        ("StartU".into(), "0".into()),
        ("EndU".into(), "1".into()),
        ("StartV".into(), "0".into()),
        ("EndV".into(), "1".into()),
        ("BasisOrigin".into(), texte_vec3(point)),
        ("BasisU".into(), texte_vec3(u)),
        ("BasisV".into(), texte_vec3(v)),
        ("BasisNormal".into(), texte_vec3(n)),
        ("uv0".into(), texte_vec3(DVec3::new(-w, -h, 0.0))),
        ("uv1".into(), texte_vec3(DVec3::new(-w, h, 0.0))),
        ("uv2".into(), texte_vec3(DVec3::new(w, h, 0.0))),
        ("uv3".into(), texte_vec3(DVec3::new(w, -h, 0.0))),
    ]
}

/// Les quatre coins d'un overlay dans le monde (un peu au-dessus de sa surface, `ecart`) et leurs coordonnées de
/// texture (0–1).
pub fn coins_overlay(e: &Entite, ecart: f64) -> Option<[(DVec3, [f64; 2]); 4]> {
    let v = |k: &str| e.get(k).and_then(lire_vec3);
    let f = |k: &str, d: f64| e.get(k).and_then(|x| x.trim().parse::<f64>().ok()).unwrap_or(d);
    let (o, u, vv, n) = (v("BasisOrigin").or(e.origine())?, v("BasisU")?, v("BasisV")?, v("BasisNormal")?);
    let (su, eu, sv, ev) = (f("StartU", 0.0), f("EndU", 1.0), f("StartV", 0.0), f("EndV", 1.0));
    let uvs = [[su, sv], [su, ev], [eu, ev], [eu, sv]];
    let mut out = [(DVec3::ZERO, [0.0; 2]); 4];
    for k in 0..4 {
        let c = v(&format!("uv{k}"))?;
        out[k] = (o + u * c.x + vv * c.y + n * ecart, uvs[k]);
    }
    Some(out)
}

/// La surface la plus proche d'un point, à moins de `portee` unités : (point projeté, normale sortante). Calcul sur
/// les plans des faces, sans polygones : pour chaque solide, la face qui porte la plus grande distance signée.
pub fn surface_proche(doc: &Document, p: DVec3, portee: f64) -> Option<(DVec3, DVec3)> {
    surface_proche_parmi(doc.tous_les_solides().map(|(_, s)| s), p, portee)
}

/// Comme `surface_proche`, parmi les solides donnés (ceux dont la boîte est assez proche, sur une grosse map).
pub fn surface_proche_parmi<'a>(solides: impl Iterator<Item = &'a crate::Solide>, p: DVec3, portee: f64) -> Option<(DVec3, DVec3)> {
    let mut meilleur: Option<(f64, DVec3, DVec3)> = None;
    for s in solides {
        if s.cache {
            continue;
        }
        let mut pire = f64::MIN;
        let mut n_pire = DVec3::ZERO;
        for f in &s.faces {
            let n = f.normale();
            if n == DVec3::ZERO {
                continue;
            }
            let d = n.dot(p) - n.dot(f.plan[1]);
            if d > pire {
                pire = d;
                n_pire = n;
            }
        }
        // devant une seule face (ou presque dedans) et assez près
        if pire > -1.0 && pire < portee && meilleur.map_or(true, |(m, _, _)| pire.abs() < m) {
            meilleur = Some((pire.abs(), p - n_pire * pire, n_pire));
        }
    }
    meilleur.map(|(_, q, n)| (q, n))
}
