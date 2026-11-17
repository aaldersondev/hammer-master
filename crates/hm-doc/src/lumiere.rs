//! Les lumières de la map pour l'aperçu de l'éclairage : lues dans les entités `light`, `light_spot` et
//! `light_environment`, et la formule de leur éclairement direct, que le shader de la vue 3D reproduit à l'identique.
//!
//! Ce que dit le wiki Valve (pages « light », « light_spot », « light_environment ») et que l'on suit ici :
//! - `_light` = « r g b luminosité » : couleur 0-255 corrigée du gamma, puis multipliée par luminosité / 255 ; une
//!   seule valeur = gris ; sans quatrième valeur, pas de mise à l'échelle ;
//! - atténuation `_constant_attn`, `_linear_attn`, `_quadratic_attn` (« Constant-Linear-Quadratic Falloff »), ou
//!   `_fifty_percent_distance` / `_zero_percent_distance` (50 % et 1/256 de la luminosité) qui la remplacent, et
//!   `_hardfalloff` qui coupe à zéro au-delà de la distance 0 % ;
//! - projecteur : `_inner_cone`, `_cone` (bornés à 90°), `_exponent` (« Focus »), `target` qui l'oriente ;
//! - le `pitch` d'une light_environment (et d'un light_spot) remplace celui des angles et se compte à l'envers :
//!   -90 = droit vers le bas.
//! La façon dont vrad combine ces valeurs (mise à l'échelle à 100 unités, rampe du cône) est vérifiée contre une
//! vraie compilation : test `eclairage_compare_a_vrad` de `hm-compile`, qui lit les lightmaps du BSP.
use glam::DVec3;

use crate::modele::{Document, Entite};

/// Le cône d'un projecteur.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cone {
    /// Direction de l'axe du faisceau.
    pub direction: DVec3,
    /// Cosinus des demi-angles intérieur et extérieur.
    pub cos_int: f64,
    pub cos_ext: f64,
    pub exposant: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lumiere {
    /// L'entité d'où elle vient.
    pub entite: u64,
    pub position: DVec3,
    /// Intensité linéaire, où 1 = une surface blanche éclairée en plein (lightmap à 1).
    pub couleur: DVec3,
    /// Éclairement = couleur × cos / (c + l·d + q·d²).
    pub attenuation: [f64; 3],
    /// Fondu linéaire jusqu'à zéro entre ces deux distances.
    pub fondu: Option<(f64, f64)>,
    pub cone: Option<Cone>,
}

/// Le soleil et le ciel d'une `light_environment`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Soleil {
    pub entite: u64,
    /// Vers le soleil.
    pub direction: DVec3,
    pub couleur: DVec3,
    /// La lumière diffuse du ciel.
    pub ambiance: DVec3,
}

/// `"r g b luminosité"` → intensité linéaire (1 = blanc plein).
pub fn intensite(v: &str) -> DVec3 {
    let n: Vec<f64> = v.split_whitespace().filter_map(|x| x.parse::<f64>().ok()).collect();
    let g = |c: f64| (c.max(0.0) / 255.0).powf(2.2);
    match n.len() {
        0 => DVec3::ZERO,
        _ if n.iter().take(4).any(|x| *x < 0.0) => DVec3::ZERO,
        1 | 2 => DVec3::splat(g(n[0])),
        3 => DVec3::new(g(n[0]), g(n[1]), g(n[2])),
        _ => DVec3::new(g(n[0]), g(n[1]), g(n[2])) * (n[3] / 255.0),
    }
}

/// Direction d'avant pour des angles Source (tangage positif = vers le bas).
pub fn avant(tangage: f64, lacet: f64) -> DVec3 {
    let (p, y) = (tangage.to_radians(), lacet.to_radians());
    DVec3::new(p.cos() * y.cos(), p.cos() * y.sin(), -p.sin())
}

/// Le polynôme `f(x) = a x² + b x + c` qui passe par trois points.
fn quadratique(p: [(f64, f64); 3]) -> (f64, f64, f64) {
    let [(x1, y1), (x2, y2), (x3, y3)] = p;
    let den = (x1 - x2) * (x1 - x3) * (x2 - x3);
    if den.abs() < 1e-12 {
        return (0.0, 0.0, y1);
    }
    let a = (x3 * (y2 - y1) + x2 * (y1 - y3) + x1 * (y3 - y2)) / den;
    let b = (x3 * x3 * (y1 - y2) + x2 * x2 * (y3 - y1) + x1 * x1 * (y2 - y3)) / den;
    let c = (x2 * x3 * (x2 - x3) * y1 + x3 * x1 * (x3 - x1) * y2 + x1 * x2 * (x1 - x2) * y3) / den;
    (a, b, c)
}

/// Atténuation par distances 50 % et 0 % : le dénominateur vaut 1 à 0, 2 à 50 %, 256 à 0 %, croissant sur
/// l'intervalle (le point du milieu est rapproché de la droite tant que la courbe redescend).
fn attenuation_50(d50: f64, d0: f64) -> ([f64; 3], Option<(f64, f64)>) {
    let d0 = if d0 <= d50 { 2.0 * d50 } else { d0 };
    let mut y2 = 2.0;
    let mut abc = quadratique([(0.0, 1.0), (d50, y2), (d0, 256.0)]);
    for _ in 0..100 {
        let (a, b, _) = abc;
        if b >= 0.0 && 2.0 * a * d0 + b >= 0.0 {
            break;
        }
        y2 = 0.9 * y2 + 0.1 * (1.0 + 255.0 * d50 / d0);
        abc = quadratique([(0.0, 1.0), (d50, y2), (d0, 256.0)]);
    }
    let (a, b, c) = abc;
    let v50 = c + d50 * (b + d50 * a);
    let k = if v50 > 1e-9 { 2.0 / v50 } else { 1.0 };
    let fondu = if a < 0.0 {
        let m = b / (-2.0 * a);
        (m > 0.0).then_some((m, 10.0 * m))
    } else {
        None
    };
    ([c * k, b * k, a * k], fondu)
}

fn nombre(e: &Entite, k: &str) -> Option<f64> {
    e.get(k).and_then(|x| x.split_whitespace().next()).and_then(|x| x.parse::<f64>().ok())
}

impl Document {
    /// Les lumières ponctuelles et projecteurs de la map, et son soleil s'il y en a un (la première
    /// light_environment : vrad n'en prend qu'une). Les entités cachées sont laissées de côté.
    pub fn lumieres(&self) -> (Vec<Lumiere>, Option<Soleil>) {
        let mut v = Vec::new();
        let mut soleil = None;
        for e in self.entites() {
            if e.cache {
                continue;
            }
            let c = e.classe().to_ascii_lowercase();
            match c.as_str() {
                "light" | "light_spot" => {
                    let Some(o) = e.origine() else { continue };
                    let i = intensite(e.get("_light").unwrap_or("255 255 255 200"));
                    if i.max_element() <= 0.0 {
                        continue;
                    }
                    let d50 = nombre(e, "_fifty_percent_distance").unwrap_or(0.0);
                    let (attenuation, mut fondu, couleur) = if d50 > 0.0 {
                        let d0 = nombre(e, "_zero_percent_distance").unwrap_or(0.0);
                        let (a, f) = attenuation_50(d50, d0);
                        let dur = nombre(e, "_hardfalloff").unwrap_or(0.0) != 0.0;
                        let d0 = if d0 <= d50 { 2.0 * d50 } else { d0 };
                        (a, if dur { Some((0.75 * d0 + 0.25 * d50, d0)) } else { f }, i)
                    } else {
                        let q = |k| nombre(e, k).unwrap_or(0.0).max(0.0);
                        let (mut cc, l, qq) = (q("_constant_attn"), q("_linear_attn"), q("_quadratic_attn"));
                        if cc < 1e-6 && l < 1e-6 && qq < 1e-6 {
                            cc = 1.0;
                        }
                        // intensité ramenée à 100 unités
                        let r = cc + 100.0 * l + 10000.0 * qq;
                        ([cc, l, qq], None, i * r)
                    };
                    if fondu.is_some_and(|(a, b)| b <= a) {
                        fondu = None;
                    }
                    let cone = (c == "light_spot").then(|| {
                        let cible = e.get("target").filter(|t| !t.trim().is_empty()).and_then(|t| self.entites().find(|x| x.get("targetname") == Some(t)).and_then(|x| x.origine()));
                        let a = e.angles();
                        let direction = match cible {
                            Some(p) if (p - o).length() > 1e-6 => (p - o).normalize(),
                            _ => avant(nombre(e, "pitch").map(|p| -p).unwrap_or(a.x), a.y),
                        };
                        let ext = nombre(e, "_cone").unwrap_or(45.0).clamp(0.0, 90.0);
                        let int = nombre(e, "_inner_cone").unwrap_or(30.0).clamp(0.0, ext);
                        Cone { direction, cos_int: int.to_radians().cos(), cos_ext: ext.to_radians().cos(), exposant: nombre(e, "_exponent").unwrap_or(1.0) }
                    });
                    v.push(Lumiere { entite: e.id, position: o, couleur, attenuation, fondu, cone });
                }
                "light_environment" if soleil.is_none() => {
                    let a = e.angles();
                    // le pitch remplace celui des angles même absent, compté à l'envers
                    let tangage = -nombre(e, "pitch").unwrap_or(0.0);
                    soleil = Some(Soleil { entite: e.id, direction: -avant(tangage, a.y), couleur: intensite(e.get("_light").unwrap_or("255 255 255 200")), ambiance: intensite(e.get("_ambient").unwrap_or("255 255 255 20")) });
                }
                _ => {}
            }
        }
        (v, soleil)
    }
}

impl Lumiere {
    /// La part de l'intensité qui arrive à la distance `d` (sans le cosinus de la surface ni le cône).
    pub fn attenuation(&self, d: f64) -> f64 {
        let [c, l, q] = self.attenuation;
        let den = c + l * d + q * d * d;
        let mut k = if den <= 1e-9 { 0.0 } else { 1.0 / den };
        if let Some((a, b)) = self.fondu {
            if d > a {
                k *= (1.0 - (d - a) / (b - a)).clamp(0.0, 1.0);
            }
        }
        k
    }

    /// L'éclairement direct d'un point `p` de normale `n` (sans ombre).
    pub fn eclairement(&self, p: DVec3, n: DVec3) -> DVec3 {
        let v = self.position - p;
        let d = v.length();
        if d < 1e-6 {
            return DVec3::ZERO;
        }
        let l = v / d;
        let ndl = n.dot(l);
        if ndl <= 0.0 {
            return DVec3::ZERO;
        }
        let mut k = self.attenuation(d) * ndl;
        if let Some(c) = self.cone {
            let axe = -l.dot(c.direction);
            if axe <= c.cos_ext {
                return DVec3::ZERO;
            }
            k *= axe;
            if axe <= c.cos_int {
                let t = (axe - c.cos_ext) / (c.cos_int - c.cos_ext).max(1e-9);
                k *= if c.exposant == 0.0 || c.exposant == 1.0 { t } else { t.powf(c.exposant) };
            }
        }
        self.couleur * k
    }
}

impl Soleil {
    /// L'éclairement direct du soleil sur une normale `n` (sans ombre).
    pub fn eclairement(&self, n: DVec3) -> DVec3 {
        self.couleur * n.dot(self.direction).max(0.0)
    }
}
