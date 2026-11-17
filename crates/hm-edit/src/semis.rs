//! Semer des entités au pinceau (« Sprinkle » de Hammer++) : les types se lisent dans le format de Hammer++
//! (`hammerplusplus/scripts/sprinkle/*.txt` : un nom, un bloc `base` de clés communes avec la grille `grid "x y"`,
//! puis des cas pondérés `poids { clés }`). Sous le pinceau, chaque case libre de la grille reçoit une entité tirée
//! au sort, posée sur la surface trouvée par un rayon vers le bas ; une case est occupée si une entité de ce type a
//! déjà son origine dedans. Retirer enlève les entités du type sous le pinceau.
use glam::DVec3;
use hm_doc::{Commande, Document, Ref};

#[derive(Clone, Debug, PartialEq)]
pub struct TypeSemis {
    pub nom: String,
    pub base: Vec<(String, String)>,
    pub grille: [f64; 2],
    pub cas: Vec<(f64, Vec<(String, String)>)>,
}

/// Les types d'un fichier de Hammer++.
pub fn lire_types(texte: &str) -> Vec<TypeSemis> {
    let m = hm_assets::sons::mots(texte);
    let mut types = Vec::new();
    let mut i = 0;
    // un bloc de clés simples `k v … }` à partir de m[i] (après l'accolade ouvrante) ; rend l'indice après `}`
    let cles = |i: &mut usize| -> Vec<(String, String)> {
        let mut v = Vec::new();
        while *i + 1 < m.len() && m[*i] != "}" {
            v.push((m[*i].clone(), m[*i + 1].clone()));
            *i += 2;
        }
        *i += 1;
        v
    };
    while i + 1 < m.len() {
        if m[i + 1] != "{" {
            i += 1;
            continue;
        }
        let mut t = TypeSemis { nom: m[i].clone(), base: Vec::new(), grille: [64.0, 64.0], cas: Vec::new() };
        i += 2;
        while i + 1 < m.len() && m[i] != "}" {
            let tete = m[i].clone();
            if m[i + 1] != "{" {
                i += 2;
                continue;
            }
            i += 2;
            let bloc = cles(&mut i);
            if tete.eq_ignore_ascii_case("base") {
                for (k, v) in bloc {
                    if k.eq_ignore_ascii_case("grid") {
                        let n: Vec<f64> = v.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                        if n.len() >= 2 && n[0] > 0.0 && n[1] > 0.0 {
                            t.grille = [n[0], n[1]];
                        }
                    } else {
                        t.base.push((k, v));
                    }
                }
            } else if let Ok(p) = tete.parse::<f64>() {
                t.cas.push((p.max(0.0), bloc));
            }
        }
        i += 1;
        if !t.cas.is_empty() || !t.base.is_empty() {
            types.push(t);
        }
    }
    types
}

impl TypeSemis {
    /// Les clés d'une entité tirée : la base, puis le cas choisi par `h` (dans [0, 1)) selon les poids.
    pub fn tirer(&self, h: f64) -> Vec<(String, String)> {
        let total: f64 = self.cas.iter().map(|(p, _)| p).sum();
        let mut v = self.base.clone();
        let mut x = h * total;
        for (p, c) in &self.cas {
            if x < *p || std::ptr::eq(c, &self.cas.last().unwrap().1) {
                for (k, val) in c {
                    v.retain(|(a, _)| !a.eq_ignore_ascii_case(k));
                    v.push((k.clone(), val.clone()));
                }
                break;
            }
            x -= p;
        }
        v
    }

    /// Les modèles (et la classe) du type : ce qui reconnaît ses entités.
    fn reconnait(&self, e: &hm_doc::Entite) -> bool {
        let modeles: Vec<&str> = self.cas.iter().flat_map(|(_, c)| c.iter()).chain(&self.base).filter(|(k, _)| k.eq_ignore_ascii_case("model")).map(|(_, v)| v.as_str()).collect();
        let classe = self.base.iter().find(|(k, _)| k.eq_ignore_ascii_case("classname")).map(|(_, v)| v.as_str());
        if modeles.is_empty() {
            classe.map_or(false, |c| e.classe().eq_ignore_ascii_case(c))
        } else {
            e.get("model").map_or(false, |m| modeles.iter().any(|x| x.eq_ignore_ascii_case(m)))
        }
    }
}

/// Réglages du pinceau.
#[derive(Clone, Copy, Debug)]
pub struct Pinceau {
    pub rayon: f64,
    /// Lacet au hasard, et un écart au hasard dans la case (jusqu'à un quart de case).
    pub lacet_hasard: bool,
    pub ecart: bool,
    /// Le haut de l'entité suit la normale de la surface.
    pub aligner: bool,
}

/// Hasard reproductible à partir d'une graine (pas de dépendance) : nombre dans [0, 1).
fn hasard(graine: u64, k: u64) -> f64 {
    let mut x = graine ^ k.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    x ^= x >> 33;
    (x >> 11) as f64 / (1u64 << 53) as f64
}

/// Les centres des cases de la grille dans le cercle du pinceau.
pub fn cases(centre: [f64; 2], rayon: f64, grille: [f64; 2]) -> Vec<[f64; 2]> {
    let mut v = Vec::new();
    let (i0, i1) = (((centre[0] - rayon) / grille[0]).floor() as i64, ((centre[0] + rayon) / grille[0]).ceil() as i64);
    let (j0, j1) = (((centre[1] - rayon) / grille[1]).floor() as i64, ((centre[1] + rayon) / grille[1]).ceil() as i64);
    for i in i0..=i1 {
        for j in j0..=j1 {
            let c = [(i as f64 + 0.5) * grille[0], (j as f64 + 0.5) * grille[1]];
            if (c[0] - centre[0]).powi(2) + (c[1] - centre[1]).powi(2) <= rayon * rayon {
                v.push(c);
            }
        }
    }
    v
}

/// Les entités à créer sous le pinceau posé en `centre` (un point de la surface) : une par case libre.
pub fn semer(doc: &Document, t: &TypeSemis, centre: DVec3, p: Pinceau, graine: u64) -> Vec<Commande> {
    let g = t.grille;
    let occupees: Vec<[i64; 2]> = doc.entites().filter(|e| t.reconnait(e)).filter_map(|e| e.origine()).map(|o| [(o.x / g[0]).floor() as i64, (o.y / g[1]).floor() as i64]).collect();
    let mut out = Vec::new();
    for (k, c) in cases([centre.x, centre.y], p.rayon, g).into_iter().enumerate() {
        let case = [(c[0] / g[0]).floor() as i64, (c[1] / g[1]).floor() as i64];
        if occupees.contains(&case) {
            continue;
        }
        // cases négatives comprises : tout en arithmétique modulaire
        let k = (k as u64).wrapping_mul(8).wrapping_add((case[0] as u64).wrapping_mul(31)).wrapping_add((case[1] as u64).wrapping_mul(1009));
        let (dx, dy) = if p.ecart { ((hasard(graine, k.wrapping_add(1)) - 0.5) * g[0] * 0.5, (hasard(graine, k.wrapping_add(2)) - 0.5) * g[1] * 0.5) } else { (0.0, 0.0) };
        let (x, y) = (c[0] + dx, c[1] + dy);
        // depuis au-dessus du pinceau, vers le bas : la surface de cette case
        let haut = DVec3::new(x, y, centre.z + p.rayon.max(64.0));
        let Some(imp) = hm_doc::geo::rayon(doc, haut, -DVec3::Z, &|_| false) else { continue };
        let lacet = if p.lacet_hasard { (hasard(graine, k.wrapping_add(3)) * 360.0).floor() } else { 0.0 };
        let (o, a) = hm_doc::geo::pose_sur(imp.point, imp.normale, 0.0, p.aligner, DVec3::new(0.0, lacet, 0.0));
        let mut cles = t.tirer(hasard(graine, k.wrapping_add(4)));
        let classe = cles.iter().position(|(k, _)| k.eq_ignore_ascii_case("classname")).map(|i| cles.remove(i).1).unwrap_or_else(|| "prop_static".into());
        cles.retain(|(k, _)| !k.eq_ignore_ascii_case("angles") && !k.eq_ignore_ascii_case("origin"));
        cles.push(("angles".into(), format!("{} {} {}", a.x.round(), a.y.round(), a.z.round())));
        out.push(Commande::CreerEntite { classe, origine: o.to_array(), cles });
    }
    out
}

/// Les entités du type dont l'origine est sous le pinceau (distance horizontale), à retirer.
pub fn retirer(doc: &Document, t: &TypeSemis, centre: DVec3, rayon: f64) -> Option<Commande> {
    let objets: Vec<Ref> = doc
        .entites()
        .filter(|e| t.reconnait(e))
        .filter(|e| e.origine().map_or(false, |o| (o.x - centre.x).powi(2) + (o.y - centre.y).powi(2) <= rayon * rayon))
        .map(|e| Ref::Entite(e.id))
        .collect();
    (!objets.is_empty()).then_some(Commande::Supprimer { objets })
}
