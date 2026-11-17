//! Aides visuelles des entités, tirées des aides de leur classe dans le FGD (wiki Valve, « FGD ») :
//! - `line(couleur, clé, valeur, …)` : un trait vers l'entité dont la clé (souvent `targetname`) vaut la valeur
//!   lue sur celle-ci ; toujours montré ;
//! - `keyframe()` / `animator()` (cordes) : la corde jusqu'à l'entité `NextKey`, avec son mou (`Slack`) ;
//! - `sphere(clé)` (rayon, `radius` par défaut) et `lightcone()` (`_inner_cone`, `_cone`, `pitch`) : sur l'entité
//!   sélectionnée seulement, comme Hammer (« Show Helpers ») ;
//! (les icônes `iconsprite` sont dans la géométrie de la vue 3D : `scene.rs`)
use std::collections::HashMap;

use glam::DVec3;
use hm_doc::Ref;
use hm_render::lignes::Sommet;

use super::Editeur;

/// Ce qu'on garde d'une image à l'autre : les aides par classe, les traits permanents.
#[derive(Default)]
pub struct CacheAides {
    classes: HashMap<String, Vec<(String, Vec<String>)>>,
    traits: Option<(u64, Vec<Sommet>)>,
}

fn rgb(s: &str) -> Option<[u8; 4]> {
    let v: Vec<u8> = s.split_whitespace().filter_map(|x| x.parse::<f32>().ok()).map(|x| x.clamp(0.0, 255.0) as u8).collect();
    (v.len() >= 3).then(|| [v[0], v[1], v[2], 255])
}

fn trait_(out: &mut Vec<Sommet>, a: DVec3, b: DVec3, c: [u8; 4]) {
    out.push(Sommet { pos: a.as_vec3().to_array(), couleur: c });
    out.push(Sommet { pos: b.as_vec3().to_array(), couleur: c });
}

fn cercle(out: &mut Vec<Sommet>, c: DVec3, u: DVec3, v: DVec3, r: f64, coul: [u8; 4]) {
    let n = 32;
    for k in 0..n {
        let (a0, a1) = (std::f64::consts::TAU * k as f64 / n as f64, std::f64::consts::TAU * (k + 1) as f64 / n as f64);
        trait_(out, c + (u * a0.cos() + v * a0.sin()) * r, c + (u * a1.cos() + v * a1.sin()) * r, coul);
    }
}

/// Direction d'un angle Source (tangage positif vers le bas, lacet autour de Z).
fn direction(tangage: f64, lacet: f64) -> DVec3 {
    let (p, y) = (tangage.to_radians(), lacet.to_radians());
    DVec3::new(p.cos() * y.cos(), p.cos() * y.sin(), -p.sin())
}

impl Editeur {
    fn aides_classe(&mut self, classe: &str) -> Vec<(String, Vec<String>)> {
        if let Some(a) = self.aides.classes.get(classe) {
            return a.clone();
        }
        let a = self.fgd.classe(classe).map(|c| c.aides).unwrap_or_default();
        self.aides.classes.insert(classe.to_string(), a.clone());
        a
    }

    /// Les traits des aides : permanents (traits, cordes), puis ceux de la sélection (sphères, cônes).
    pub(super) fn lignes_aides(&mut self) -> Vec<Sommet> {
        let v = self.session.version();
        if self.aides.traits.as_ref().map_or(true, |(x, _)| *x != v) {
            let doc = self.session.doc();
            // noms → origines, une fois
            let mut par_cle: HashMap<(String, String), Vec<DVec3>> = HashMap::new();
            for e in doc.entites() {
                let Some(o) = e.origine() else { continue };
                for (k, val) in &e.cles {
                    if !val.trim().is_empty() {
                        par_cle.entry((k.to_ascii_lowercase(), val.to_ascii_lowercase())).or_default().push(o);
                    }
                }
            }
            let ents: Vec<(String, Option<DVec3>, Vec<(String, String)>)> = doc.entites().map(|e| (e.classe().to_string(), e.origine(), e.cles.clone())).collect();
            let mut out = Vec::new();
            for (classe, origine, cles) in ents {
                let Some(o) = origine else { continue };
                let get = |k: &str| cles.iter().find(|(x, _)| x.eq_ignore_ascii_case(k)).map(|(_, v)| v.clone());
                for (nom, args) in self.aides_classe(&classe) {
                    match nom.as_str() {
                        "line" | "nodelink" | "cylinder" if args.len() >= 3 => {
                            let couleur = rgb(&args[0]).unwrap_or([255, 0, 255, 255]);
                            let Some(cible) = get(&args[2]) else { continue };
                            let depart = par_cle.get(&(args[1].to_ascii_lowercase(), cible.to_ascii_lowercase())).cloned().unwrap_or_default();
                            // l'autre bout : l'entité de la seconde paire, sinon celle-ci
                            // line(…, clé, valeur, clé, valeur) ; cylinder(…, clé, valeur, rayon, clé, valeur, rayon)
                            let (ka, va) = if nom == "cylinder" { (4, 5) } else { (3, 4) };
                            let arrivee: Vec<DVec3> = if args.len() > va {
                                get(&args[va]).and_then(|x| par_cle.get(&(args[ka].to_ascii_lowercase(), x.to_ascii_lowercase())).cloned()).unwrap_or_default()
                            } else {
                                vec![o]
                            };
                            for a in &depart {
                                for b in &arrivee {
                                    trait_(&mut out, *a, *b, couleur);
                                }
                            }
                        }
                        "keyframe" | "animator" => {
                            // la corde jusqu'à NextKey, avec un mou : flèche d'une parabole de longueur portée + Slack
                            let Some(suivant) = get("NextKey") else { continue };
                            let slack = get("Slack").and_then(|x| x.trim().parse::<f64>().ok()).unwrap_or(25.0).max(0.0);
                            for b in par_cle.get(&("targetname".into(), suivant.to_ascii_lowercase())).cloned().unwrap_or_default() {
                                let portee = (b - o).length();
                                let fleche = (3.0 * portee * slack / 8.0).sqrt();
                                let n = 16;
                                let point = |t: f64| o + (b - o) * t - DVec3::Z * fleche * 4.0 * t * (1.0 - t);
                                for k in 0..n {
                                    trait_(&mut out, point(k as f64 / n as f64), point((k + 1) as f64 / n as f64), [210, 180, 120, 255]);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            self.aides.traits = Some((v, out));
        }
        let mut out = self.aides.traits.as_ref().map(|(_, t)| t.clone()).unwrap_or_default();
        // sphères et cônes de la sélection
        let sel: Vec<u64> = self.session.selection.iter().filter_map(|r| if let Ref::Entite(e) = r { Some(*e) } else { None }).collect();
        for id in sel {
            let Some(e) = self.session.doc().entite(id) else { continue };
            let Some(o) = e.origine() else { continue };
            let classe = e.classe().to_string();
            let cles = e.cles.clone();
            let angles = e.angles();
            let get = |k: &str| cles.iter().find(|(x, _)| x.eq_ignore_ascii_case(k)).and_then(|(_, v)| v.trim().split_whitespace().next().and_then(|x| x.parse::<f64>().ok()));
            for (nom, args) in self.aides_classe(&classe) {
                match nom.as_str() {
                    "sphere" => {
                        let cle = args.first().filter(|a| !a.is_empty()).cloned().unwrap_or_else(|| "radius".into());
                        if let Some(r) = get(&cle).filter(|r| *r > 0.0) {
                            let c = [120, 200, 255, 255];
                            cercle(&mut out, o, DVec3::X, DVec3::Y, r, c);
                            cercle(&mut out, o, DVec3::X, DVec3::Z, r, c);
                            cercle(&mut out, o, DVec3::Y, DVec3::Z, r, c);
                        }
                    }
                    "lightcone" => {
                        let (k_int, k_ext) = (args.first().filter(|a| !a.is_empty()).cloned().unwrap_or_else(|| "_inner_cone".into()), args.get(1).filter(|a| !a.is_empty()).cloned().unwrap_or_else(|| "_cone".into()));
                        // la clé `pitch` se compte à l'envers des angles (wiki Valve ; vérifié contre vrad, `hm_doc::lumiere`)
                        let tangage = get("pitch").map(|p| -p).unwrap_or(angles.x);
                        let d = direction(tangage, angles.y);
                        let (u, w) = {
                            let a = if d.z.abs() < 0.9 { DVec3::Z } else { DVec3::X };
                            let u = d.cross(a).normalize();
                            (u, d.cross(u))
                        };
                        let longueur = 128.0;
                        for (cle, coul) in [(k_int, [255, 240, 120, 255]), (k_ext, [255, 160, 60, 255])] {
                            let Some(angle) = get(&cle) else { continue };
                            let r = longueur * angle.clamp(0.0, 89.0).to_radians().tan();
                            let fin = o + d * longueur;
                            cercle(&mut out, fin, u, w, r, coul);
                            for k in 0..4 {
                                let a = std::f64::consts::FRAC_PI_2 * k as f64;
                                trait_(&mut out, o, fin + (u * a.cos() + w * a.sin()) * r, coul);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        out
    }
}
