//! Constructeur : une spec (volumes d'air, ouvertures, éléments, entités) → un VMF.
//!
//! La spec est celle du skill gmod-map-maker (`references/spec.md`), lue telle quelle : mêmes champs, mêmes valeurs
//! par défaut. Ce fichier est le portage de son `generer.py`, fonction par fonction et dans le même ordre, pour
//! que les deux écrivent la même map : c'est ce que vérifie la comparaison avec la démo du skill. Les murs, sols
//! et plafonds ne sont jamais décrits : chaque volume d'air reçoit une enveloppe d'épaisseur `murs` / `dalles`,
//! découpée en boîtes sur la grille. Un mur est un solide, rien ne se chevauche, la map est scellée par
//! construction.
//!
//! Ce que le portage ne refait pas, parce que les règles de Hammer Master le font sur la map écrite : l'existence
//! des matériaux.
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use serde_yaml::Value;

pub type V3 = [f64; 3];

const NODRAW: &str = "tools/toolsnodraw";
const SKY: &str = "tools/toolsskybox";
const SKY2D: &str = "tools/toolsskybox2d";
const GRID_WORLD: f64 = 16.0;
const GRID_DETAIL: f64 = 8.0;
/// Battant 48,4 × 108,4, charnière à l'origine, le long de +Y.
const DOOR_MODEL: &str = "models/props_c17/door01_left.mdl";
const DOOR_HALF_H: f64 = 54.2;
/// Plafonnier, origine = point de fixation.
const LIGHT_MODEL: &str = "models/props_c17/light_domelight02_on.mdl";
const FINISH_ROLES: [&str; 8] = ["plinthe", "cadre", "marche", "garde_corps", "porte", "pilastre", "corniche", "soubassement"];
/// La liste des matériaux du skill : textures de blockout et échelle de chaque matériau (copie contrôlée par un test).
const WHITELIST: &str = include_str!("../../../regles/whitelist.yaml");
/// Les ciels du jeu, et ceux pour lesquels vbsp sait faire une cubemap par défaut (copie de la table du skill).
const CIELS: &str = include_str!("../../../regles/ciels_disponibles.tsv");
/// La table des rôles de pièce du skill (la même copie que lisent les règles).
const ROLES: &str = include_str!("../../../regles/roles.yaml");

fn cote(nom: &str) -> Option<(usize, i32)> {
    Some(match nom {
        "est" => (0, 1),
        "ouest" => (0, -1),
        "nord" => (1, 1),
        "sud" => (1, -1),
        "dessus" => (2, 1),
        "dessous" => (2, -1),
        _ => return None,
    })
}

const COTES: &str = "est, ouest, nord, sud, dessus, dessous";
const DIRS: [(usize, i32); 6] = [(0, 1), (0, -1), (1, 1), (1, -1), (2, 1), (2, -1)];

fn dirkey(d: (usize, i32)) -> &'static str {
    match d {
        (0, 1) => "+x",
        (0, _) => "-x",
        (1, 1) => "+y",
        (1, _) => "-y",
        (2, 1) => "+z",
        _ => "-z",
    }
}

// ---------------------------------------------------------------------------------------------- valeurs YAML

fn get<'a>(v: &'a Value, k: &str) -> Option<&'a Value> {
    v.as_mapping().and_then(|m| m.get(k)).filter(|x| !x.is_null())
}
fn num(v: &Value, k: &str, d: f64) -> f64 {
    get(v, k).and_then(Value::as_f64).unwrap_or(d)
}
fn texte<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    get(v, k).and_then(Value::as_str)
}
/// Vrai comme Python l'entend : ni absent, ni faux, ni zéro, ni vide.
fn vrai(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().map_or(false, |x| x != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Sequence(s)) => !s.is_empty(),
        Some(Value::Mapping(m)) => !m.is_empty(),
        Some(_) => true,
    }
}
fn nombres(v: Option<&Value>) -> Vec<f64> {
    v.and_then(Value::as_sequence).map(|s| s.iter().filter_map(Value::as_f64).collect()).unwrap_or_default()
}
fn textes(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_sequence).map(|s| s.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}
/// Un sous-dictionnaire de matériaux, en minuscules.
fn materiaux(v: Option<&Value>) -> HashMap<String, String> {
    v.and_then(Value::as_mapping).map(|m| m.iter().filter_map(|(k, x)| Some((k.as_str()?.to_string(), x.as_str()?.to_lowercase()))).collect()).unwrap_or_default()
}
/// `str(v)` de Python.
fn py_str(v: &Value) -> String {
    match v {
        Value::Bool(b) => if *b { "True" } else { "False" }.to_string(),
        Value::Number(n) => match (n.as_i64(), n.as_f64()) {
            (Some(i), _) => i.to_string(),
            (None, Some(f)) if f == f.trunc() && f.abs() < 1e16 => format!("{f:.1}"),
            (None, Some(f)) => format!("{f}"),
            _ => n.to_string(),
        },
        Value::String(s) => s.clone(),
        Value::Null => "None".to_string(),
        autre => serde_yaml::to_string(autre).unwrap_or_default().trim().to_string(),
    }
}

/// Le format `%g` de Python : six chiffres significatifs, sans zéros inutiles.
pub fn g(x: f64) -> String {
    if x == 0.0 {
        return "0".to_string();
    }
    let e = x.abs().log10().floor() as i32;
    let sans_zeros = |t: String| if t.contains('.') { t.trim_end_matches('0').trim_end_matches('.').to_string() } else { t };
    if !(-4..6).contains(&e) {
        // comme Python : mantisse à six chiffres, exposant signé sur deux chiffres au moins
        let t = format!("{x:.5e}");
        let (m, ex) = t.split_once('e').unwrap();
        let ex: i32 = ex.parse().unwrap();
        return format!("{}e{}{:02}", sans_zeros(m.to_string()), if ex < 0 { '-' } else { '+' }, ex.abs());
    }
    sans_zeros(format!("{x:.dec$}", dec = (5 - e).max(0) as usize))
}

/// `round()` de Python : au plus proche, le demi vers le pair.
fn arrondi(x: f64) -> f64 {
    let r = x.round();
    if (x - x.trunc()).abs() == 0.5 && r % 2.0 != 0.0 {
        r - x.signum()
    } else {
        r
    }
}

fn plus(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn moins(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn croix(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn scal(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn norme(a: V3) -> f64 {
    scal(a, a).sqrt()
}
fn liste_py(v: V3) -> String {
    format!("[{}]", v.iter().map(|c| if *c == c.trunc() { format!("{c:.1}") } else { format!("{c}") }).collect::<Vec<_>>().join(", "))
}

fn soustraire(spans: Vec<(f64, f64)>, a: f64, b: f64) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for (s0, s1) in spans {
        if b <= s0 || a >= s1 {
            out.push((s0, s1));
            continue;
        }
        if a > s0 {
            out.push((s0, a));
        }
        if b < s1 {
            out.push((b, s1));
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------- solides

#[derive(Clone, Debug, Default)]
pub struct Face {
    pub n: V3,
    pub poly: Vec<V3>,
    pub mat: String,
    pub echelle: f64,
    pub lm: i64,
    /// La texture est ajustée à la face (une enseigne, un plan) au lieu d'être répétée.
    pub ajuste: bool,
    /// La lightmap a été demandée par l'élément : la table des textures ne la remplace pas.
    pub lm_fixe: bool,
    /// Axes de texture calculés par la pose « par face » : (u, décalage, échelle, v, décalage, échelle).
    pub uv: Option<(V3, f64, f64, V3, f64, f64)>,
}

/// Ce que la table des textures d'un projet dit d'un matériau : taille de l'image, taille d'une répétition dans le
/// monde, lightmap conseillée.
#[derive(Clone, Copy, Debug)]
struct InfoTexture {
    w: f64,
    h: f64,
    tuile: [f64; 2],
    lm: Option<i64>,
}

#[derive(Clone, Debug, Default)]
pub struct Entite {
    pub classe: String,
    pub cles: Vec<(String, String)>,
    pub solides: Vec<Solide>,
}

/// Un solide et ses numéros, pris à sa création : le sien, et celui qui précède sa première face.
#[derive(Clone, Debug, Default)]
pub struct Solide {
    pub id: u64,
    pub face0: u64,
    pub faces: Vec<Face>,
}

/// Les six faces d'une boîte ; matériaux, échelles et lightmaps par direction.
fn faces_boite(lo: V3, hi: V3, mats: &HashMap<&'static str, String>, echelles: &HashMap<&'static str, f64>, lms: &HashMap<&'static str, i64>) -> Vec<Face> {
    let ([x0, y0, z0], [x1, y1, z1]) = (lo, hi);
    let p: [(&'static str, V3, [V3; 4]); 6] = [
        ("+z", [0.0, 0.0, 1.0], [[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]]),
        ("-z", [0.0, 0.0, -1.0], [[x0, y0, z0], [x0, y1, z0], [x1, y1, z0], [x1, y0, z0]]),
        ("+x", [1.0, 0.0, 0.0], [[x1, y0, z0], [x1, y1, z0], [x1, y1, z1], [x1, y0, z1]]),
        ("-x", [-1.0, 0.0, 0.0], [[x0, y0, z0], [x0, y0, z1], [x0, y1, z1], [x0, y1, z0]]),
        ("+y", [0.0, 1.0, 0.0], [[x0, y1, z0], [x0, y1, z1], [x1, y1, z1], [x1, y1, z0]]),
        ("-y", [0.0, -1.0, 0.0], [[x0, y0, z0], [x1, y0, z0], [x1, y0, z1], [x0, y0, z1]]),
    ];
    p.iter().map(|(d, n, poly)| Face { n: *n, poly: poly.to_vec(), mat: mats[d].clone(), echelle: echelles[d], lm: lms[d], ..Default::default() }).collect()
}

/// (normale sortante, polygone) de chaque face de l'enveloppe convexe d'un petit nuage de points.
fn faces_enveloppe(points: &[V3]) -> Vec<(V3, Vec<V3>)> {
    let mut plans: Vec<(V3, f64)> = Vec::new();
    let n = points.len();
    for a in 0..n {
        for b in a + 1..n {
            for c in b + 1..n {
                let mut nrm = croix(moins(points[b], points[a]), moins(points[c], points[a]));
                let l = norme(nrm);
                if l < 1e-9 {
                    continue;
                }
                nrm = [nrm[0] / l, nrm[1] / l, nrm[2] / l];
                let mut d = scal(nrm, points[a]);
                let cotes: Vec<f64> = points.iter().map(|p| scal(nrm, *p) - d).collect();
                if cotes.iter().any(|x| *x > 1e-6) {
                    if cotes.iter().any(|x| *x < -1e-6) {
                        continue;
                    }
                    nrm = [-nrm[0], -nrm[1], -nrm[2]];
                    d = -d;
                }
                if !plans.iter().any(|(m, e)| norme(moins(*m, nrm)) < 1e-6 && (d - e).abs() < 1e-3) {
                    plans.push((nrm, d));
                }
            }
        }
    }
    let mut out = Vec::new();
    for (nrm, d) in plans {
        let mut sur: Vec<V3> = Vec::new();
        for p in points {
            if (scal(nrm, *p) - d).abs() < 1e-3 && !sur.iter().any(|q| norme(moins(*q, *p)) < 1e-6) {
                sur.push(*p);
            }
        }
        let k = sur.len() as f64;
        let c = sur.iter().fold([0.0; 3], |s, p| plus(s, *p)).map(|x| x / k);
        let u0 = moins(sur[0], c);
        let lu = norme(u0);
        let u = [u0[0] / lu, u0[1] / lu, u0[2] / lu];
        let w = croix(nrm, u);
        sur.sort_by(|p, q| {
            let ang = |v: &V3| scal(moins(*v, c), w).atan2(scal(moins(*v, c), u));
            ang(p).total_cmp(&ang(q))
        });
        out.push((nrm.map(|x| (x * 1e9).round() / 1e9), sur));
    }
    out
}

/// Trois points d'un polygone dans l'ordre du VMF (sens horaire vu de l'extérieur) : le plus grand triangle.
fn points_de_face(poly: &[V3], normale: V3) -> [V3; 3] {
    let mut meilleur: Option<(f64, usize, usize, usize)> = None;
    for a in 0..poly.len() {
        for b in a + 1..poly.len() {
            for c in b + 1..poly.len() {
                let aire = norme(croix(moins(poly[c], poly[a]), moins(poly[b], poly[a])));
                if aire > 1e-6 && meilleur.map_or(true, |m| aire > m.0) {
                    meilleur = Some((aire, a, b, c));
                }
            }
        }
    }
    let (_, a, b, c) = meilleur.expect("face sans surface");
    let (p0, mut p1, mut p2) = (poly[a], poly[b], poly[c]);
    if scal(croix(moins(p2, p0), moins(p1, p0)), normale) < 0.0 {
        std::mem::swap(&mut p1, &mut p2);
    }
    [p0, p1, p2]
}

/// Alignement « monde » de Hammer : axes de texture selon l'axe dominant de la normale.
fn axes_uv(n: V3) -> ([i32; 3], [i32; 3]) {
    let a = n.map(f64::abs);
    if a[2] >= a[0] && a[2] >= a[1] {
        ([1, 0, 0], [0, -1, 0])
    } else if a[0] >= a[1] {
        ([0, 1, 0], [0, 0, -1])
    } else {
        ([1, 0, 0], [0, 0, -1])
    }
}

// ---------------------------------------------------------------------------------------------- modèle

#[derive(Clone, Debug)]
struct Zone {
    nom: String,
    typ: String,
    lo: V3,
    hi: V3,
    spec: Value,
    englobe: bool,
    /// Zone englobante : les zones qu'elle entoure ; `None` = toutes.
    cibles: Option<Vec<String>>,
    ciel2d: bool,
    /// Bâtiment fermé sans intérieur : son volume est plein.
    plein: bool,
    lm: i64,
    role: Option<String>,
    /// Hauteur des murs visibles d'un extérieur.
    hm: f64,
}

impl Zone {
    fn taille(&self) -> V3 {
        moins(self.hi, self.lo)
    }
    fn exterieur(&self) -> bool {
        self.typ == "exterieur"
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Part {
    Int,
    Bas,
    Ciel,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Cle {
    Zone(usize, Part),
    Ouv(usize),
}

#[derive(Clone, Debug)]
struct Ouv {
    typ: String,
    entre: [String; 2],
    lo: V3,
    hi: V3,
    axis: usize,
    /// Zone qui porte les matériaux.
    zone: usize,
    spec: Value,
    nom: String,
    clear: Option<(V3, V3)>,
}

#[derive(Clone, Debug, Default)]
struct El {
    typ: String,
    nom: Option<String>,
    lo: V3,
    hi: V3,
    zone: Option<usize>,
    spec: Value,
    arrive_axis: usize,
    arrive_at: f64,
    direction: Option<String>,
    ouverture: Option<String>,
}

/// Ce que le constructeur a besoin de savoir d'un modèle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InfoModele {
    /// Le bas de sa boîte (`hull_min.z`) : de combien le relever pour qu'il touche le sol.
    pub bas: f64,
    /// Accepté par vbsp en `prop_static` (compilé avec `$staticprop`) ; `None` si on ne sait pas.
    pub statique: Option<bool>,
}

/// Ce que le constructeur demande au monde extérieur.
pub struct Options<'a> {
    /// `blockout` ou `texture` ; `None` = celle de la spec.
    pub phase: Option<String>,
    /// Dossier de la spec : les modules et le contenu du projet y sont cherchés.
    pub base: PathBuf,
    /// Dossier `modules/` du skill, où chercher un module que le projet n'a pas.
    pub modules_skill: Option<PathBuf>,
    /// Ce qu'on sait d'un modèle (chemin en minuscules), ou `None` s'il n'existe pas.
    pub modele: &'a dyn Fn(&str) -> Option<InfoModele>,
}

/// Ce que le constructeur rend. S'il y a une erreur, `vmf` est vide : rien n'est à écrire.
#[derive(Default)]
pub struct Construction {
    pub vmf: String,
    pub plan: serde_json::Value,
    pub sem: crate::Modele,
    pub erreurs: Vec<String>,
    pub avertissements: Vec<String>,
    pub phase: String,
    pub zones: usize,
    pub ouvertures: usize,
    pub brosses: usize,
    pub entites: usize,
    /// Modules `func_instance` à copier dans `modules/` à côté du VMF : (source, nom du fichier).
    pub modules: Vec<(PathBuf, String)>,
}

struct Gen<'a> {
    spec: &'a Value,
    opt: &'a Options<'a>,
    phase: String,
    t: f64,
    s: f64,
    err: Vec<String>,
    warn: Vec<String>,
    zones: Vec<Zone>,
    ouvertures: Vec<Ouv>,
    elements: Vec<El>,
    blockout: HashMap<String, String>,
    echelles: HashMap<String, f64>,
    echelle_defaut: f64,
    defauts: HashMap<String, String>,
    monde: Vec<Solide>,
    id_solide: u64,
    id_face: u64,
    entites: Vec<Entite>,
    cles_monde: Vec<(String, String)>,
    /// Bâtiments propriétaires de leurs façades (indice de zone) ; le rang 0 ne désigne personne.
    proprietaires: Vec<Option<usize>>,
    coque: Vec<(V3, V3, Vec<((usize, i32), Option<Cle>)>, usize)>,
    modules: Vec<(PathBuf, String)>,
    /// Grille des éléments (8 par défaut ; `grille_detail:` de la spec).
    grille_detail: f64,
    /// Pose de texture « par face » (`textures: {pose: face, table: …}`) : la table du projet, par matériau.
    table: Option<HashMap<String, InfoTexture>>,
    /// Repère des éléments sans zone (`repere: {origine, angle}`) : un module posé et tourné à la construction.
    repere: (V3, i32),
    /// Variante construite (`variante:`) : un élément qui porte `seulement: [...]` n'est fait que pour elles.
    variante: Option<String>,
}

impl<'a> Gen<'a> {
    fn new(spec: &'a Value, opt: &'a Options<'a>) -> Gen<'a> {
        let wl: Value = serde_yaml::from_str(WHITELIST).expect("regles/whitelist.yaml lisible");
        let echelle_defaut = num(&wl, "echelle_defaut", 0.25);
        let mut echelles = HashMap::new();
        if let Some(usages) = get(&wl, "usages").and_then(Value::as_mapping) {
            for (_, liste) in usages {
                for e in liste.as_sequence().into_iter().flatten() {
                    if let Some(nom) = texte(e, "nom") {
                        echelles.insert(nom.to_string(), num(e, "echelle", echelle_defaut));
                    }
                }
            }
        }
        for k in get(spec, "kit").and_then(Value::as_sequence).into_iter().flatten() {
            if let Some(nom) = texte(k, "nom") {
                echelles.insert(nom.to_lowercase(), num(k, "echelle", echelle_defaut));
            }
        }
        Gen {
            spec,
            opt,
            phase: opt.phase.clone().or_else(|| texte(spec, "phase").map(str::to_string)).unwrap_or_else(|| "texture".to_string()),
            t: num(spec, "murs", 16.0),
            s: num(spec, "dalles", 16.0),
            err: Vec::new(),
            warn: Vec::new(),
            zones: Vec::new(),
            ouvertures: Vec::new(),
            elements: Vec::new(),
            blockout: materiaux(get(&wl, "blockout")),
            echelles,
            echelle_defaut,
            defauts: materiaux(get(spec, "materiaux")),
            monde: Vec::new(),
            id_solide: 0,
            id_face: 0,
            entites: Vec::new(),
            cles_monde: Vec::new(),
            proprietaires: vec![None],
            coque: Vec::new(),
            modules: Vec::new(),
            grille_detail: num(spec, "grille_detail", GRID_DETAIL),
            table: None,
            repere: ([0.0; 3], 0),
            variante: texte(spec, "variante").map(str::to_string),
        }
    }

    /// Lit ce que la spec dit de la pose des textures et du repère. Séparé de `new` : ces réglages peuvent échouer.
    fn reglages(&mut self) {
        if let Some(t) = get(self.spec, "textures") {
            match texte(t, "pose").unwrap_or("monde") {
                "monde" => {}
                "face" => {
                    let chemin = self.opt.base.join(texte(t, "table").unwrap_or("texinfo.json"));
                    match std::fs::read_to_string(&chemin).ok().and_then(|x| serde_json::from_str::<serde_json::Value>(&x).ok()) {
                        Some(json) => {
                            let mut table = HashMap::new();
                            for (nom, i) in json.as_object().into_iter().flatten() {
                                let f = |k: &str, d: f64| i.get(k).and_then(|v| v.as_f64()).unwrap_or(d);
                                let (w, h) = (f("w", 512.0), f("h", 512.0));
                                let tuile = i.get("tile").and_then(|v| v.as_array()).filter(|a| a.len() == 2).map(|a| [a[0].as_f64().unwrap_or(w), a[1].as_f64().unwrap_or(h)]).unwrap_or([w, h]);
                                table.insert(nom.to_lowercase(), InfoTexture { w, h, tuile, lm: i.get("lm").and_then(|v| v.as_i64()) });
                            }
                            self.table = Some(table);
                        }
                        None => self.fail(format!("textures : table {} introuvable ou illisible", chemin.display())),
                    }
                }
                autre => self.fail(format!("textures : pose '{autre}' inconnue (monde, face)")),
            }
        }
        if let Some(rp) = get(self.spec, "repere") {
            let o = nombres(get(rp, "origine"));
            let angle = num(rp, "angle", 0.0);
            if o.len() != 3 || angle % 90.0 != 0.0 {
                self.fail("repere : origine: [x, y, z] et angle: multiple de 90 attendus".to_string());
            } else {
                self.repere = ([o[0], o[1], o[2]], (angle as i32).rem_euclid(360));
            }
        }
    }

    /// Un point du repère des éléments sans zone, dans le monde.
    fn vers_monde(&self, p: V3) -> V3 {
        let (o, a) = self.repere;
        let (c, s) = match a {
            90 => (0.0, 1.0),
            180 => (-1.0, 0.0),
            270 => (0.0, -1.0),
            _ => (1.0, 0.0),
        };
        [o[0] + p[0] * c - p[1] * s, o[1] + p[0] * s + p[1] * c, o[2] + p[2]]
    }

    /// Une boîte donnée par deux coins : dans le repère de sa zone (simple décalage), ou dans le repère du module.
    fn boite_monde(&self, zi: Option<usize>, lo: V3, hi: V3) -> (V3, V3) {
        match zi {
            Some(i) => (plus(self.zones[i].lo, lo), plus(self.zones[i].lo, hi)),
            None => bornes(&[self.vers_monde(lo), self.vers_monde(hi)]),
        }
    }

    /// Le côté du monde vers lequel regarde un côté du repère du module (nord, sud, est, ouest ; les autres tels quels).
    fn cote_monde(&self, zi: Option<usize>, nom: &str) -> String {
        const TOUR: [&str; 4] = ["est", "nord", "ouest", "sud"];
        match (zi, TOUR.iter().position(|c| *c == nom)) {
            (None, Some(k)) => TOUR[(k + (self.repere.1 / 90) as usize) % 4].to_string(),
            _ => nom.to_string(),
        }
    }

    /// Faux si l'élément porte `seulement: [...]` et que la variante construite n'y est pas.
    fn retenu(&self, e: &Value) -> bool {
        match get(e, "seulement") {
            None => true,
            Some(l) => self.variante.as_ref().map_or(false, |v| textes(Some(l)).contains(v)),
        }
    }

    /// Pose « par face », comme le fait le générateur de rp_megalopole (`vmfkit.Map._side`) : la texture garde la
    /// taille que la table lui donne, part du bord de chaque face sur les murs, ou s'ajuste à la face.
    fn poser_par_face(&self, f: &mut Face) {
        let Some(table) = &self.table else { return };
        let i = table.get(&f.mat).copied().unwrap_or(InfoTexture { w: 512.0, h: 512.0, tuile: [128.0, 128.0], lm: None });
        let a = f.n.map(f64::abs);
        let plat = a[2] >= a[0].max(a[1]) - 1e-9;
        let (u, v) = if plat {
            ([1.0, 0.0, 0.0], [0.0, -1.0, 0.0])
        } else {
            let u = croix([-f.n[0], -f.n[1], -f.n[2]], [0.0, 0.0, 1.0]);
            let lu = norme(u);
            let u = [u[0] / lu, u[1] / lu, u[2] / lu];
            let v = croix(u, f.n);
            let lv = norme(v);
            (u, [v[0] / lv, v[1] / lv, v[2] / lv])
        };
        let (us, vs): (Vec<f64>, Vec<f64>) = f.poly.iter().map(|q| (scal(*q, u), scal(*q, v))).unzip();
        let (minu, maxu) = (us.iter().cloned().fold(f64::INFINITY, f64::min), us.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
        let (minv, maxv) = (vs.iter().cloned().fold(f64::INFINITY, f64::min), vs.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
        let (su, sv, shu, shv);
        if f.ajuste {
            su = (maxu - minu) / i.w;
            sv = (maxv - minv) / i.h;
            shu = -minu / su;
            shv = -minv / sv;
        } else {
            su = i.tuile[0] / i.w;
            sv = i.tuile[1] / i.h;
            shu = if plat { 0.0 } else { -minu / su };
            shv = 0.0;
        }
        // arrondis comme `round()` de Python : le demi va au pair
        let arr = |x: f64, d: f64| arrondi(x * d) / d;
        f.uv = Some((u, arr(shu.rem_euclid(i.w), 1e4), arr(su, 1e6), v, arr(shv.rem_euclid(i.h), 1e4), arr(sv, 1e6)));
        if !f.lm_fixe {
            f.lm = i.lm.unwrap_or(16);
        }
    }

    fn solide(&mut self, mut faces: Vec<Face>) -> Solide {
        for f in &mut faces {
            self.poser_par_face(f);
        }
        self.id_solide += 1;
        let face0 = self.id_face;
        self.id_face += faces.len() as u64;
        Solide { id: self.id_solide, face0, faces }
    }

    // --- messages
    fn fail(&mut self, msg: String) {
        self.err.push(msg);
    }

    /// Règle d'échelle : erreur, ou avertissement si l'élément déclare une exception.
    fn regle(&mut self, ok: bool, msg: String, item: &Value) {
        if ok {
            return;
        }
        match texte(item, "exception").filter(|e| !e.is_empty()) {
            Some(e) => self.warn.push(format!("{msg} (exception : {e})")),
            None => self.err.push(format!("{msg}  [ajouter exception: \"raison\" si c'est voulu]")),
        }
    }

    fn sur_grille(&mut self, valeurs: &[f64], grille: f64, quoi: &str) -> bool {
        // la grille des éléments est celle de la spec (un quai à la hauteur du plancher d'une rame n'est pas sur 8)
        let grille = if grille == GRID_DETAIL { self.grille_detail } else { grille };
        for v in valeurs {
            if (v / grille - (v / grille).round()).abs() > 1e-6 {
                self.fail(format!("{quoi} : {} n'est pas un multiple de {}", g(*v), g(grille)));
                return false;
            }
        }
        true
    }

    fn zone(&self, nom: &str) -> Option<usize> {
        self.zones.iter().position(|z| z.nom == nom)
    }

    // --- matériaux
    fn mat(&mut self, role: &str, zi: Option<usize>, force: Option<String>) -> String {
        let zone = zi.map(|i| self.zones[i].clone());
        let zone = zone.as_ref();
        if self.phase == "blockout" {
            let mut cle = match role {
                "toit" => "sol",
                "plinthe" | "cadre" | "marche" | "garde_corps" | "porte" | "pilastre" | "corniche" | "soubassement" => "detail",
                r => r,
            };
            if role == "mur" && zone.map_or(false, Zone::exterieur) {
                cle = "facade";
            }
            return self.blockout.get(cle).or_else(|| self.blockout.get("detail")).cloned().unwrap_or_else(|| NODRAW.to_string());
        }
        if let Some(f) = force.filter(|f| !f.is_empty()) {
            return f.to_lowercase();
        }
        let zm = zone.map(|z| materiaux(get(&z.spec, "materiaux"))).unwrap_or_default();
        if let Some(m) = zm.get(role) {
            return m.clone();
        }
        let mut chaine: Vec<&str> = if zone.map_or(false, Zone::exterieur) {
            match role {
                "sol" => vec!["sol_exterieur", "sol"],
                "mur" => vec!["facade", "mur"],
                "toit" => vec!["toit", "facade"],
                r => vec![r],
            }
        } else {
            vec![role]
        };
        if FINISH_ROLES.contains(&role) {
            chaine.push("detail");
        }
        for k in chaine {
            if let Some(m) = self.defauts.get(k) {
                return m.clone();
            }
        }
        let ou = zone.map(|z| format!(" (zone {})", z.nom)).unwrap_or_default();
        self.fail(format!("aucun matériau pour le rôle '{role}'{ou} : le déclarer dans materiaux: de la spec ou de la zone"));
        NODRAW.to_string()
    }

    fn echelle(&self, mat: &str) -> f64 {
        self.echelles.get(mat).copied().unwrap_or(self.echelle_defaut)
    }

    // --- zones
    fn resoudre_zones(&mut self) {
        let mut attente: Vec<Value> = get(self.spec, "zones").and_then(Value::as_sequence).cloned().unwrap_or_default();
        let mut englobes: Vec<Value> = Vec::new();
        while !attente.is_empty() {
            let mut avance = false;
            let mut reste = Vec::new();
            for z in std::mem::take(&mut attente) {
                if vrai(get(&z, "englobe")) {
                    englobes.push(z);
                    avance = true;
                    continue;
                }
                let t = nombres(get(&z, "taille"));
                let taille: V3 = if t.len() >= 3 { [t[0], t[1], t[2]] } else { [0.0; 3] };
                let lo = if let Some(o) = get(&z, "origine") {
                    let o = nombres(Some(o));
                    [o.first().copied().unwrap_or(0.0), o.get(1).copied().unwrap_or(0.0), o.get(2).copied().unwrap_or(0.0)]
                } else if let Some(c) = get(&z, "contre") {
                    match texte(c, "zone").and_then(|n| self.zone(n)) {
                        Some(r) => self.placer_contre(&z, r, taille),
                        None => {
                            reste.push(z);
                            continue;
                        }
                    }
                } else {
                    self.fail(format!("zone {} : ni 'origine' ni 'contre'", texte(&z, "nom").unwrap_or("None")));
                    avance = true;
                    continue;
                };
                self.ajouter_zone(&z, lo, plus(lo, taille));
                avance = true;
            }
            attente = reste;
            if !avance {
                let noms: Vec<&str> = attente.iter().map(|z| texte(z, "nom").unwrap_or("?")).collect();
                self.fail(format!("zones non placées (référence inconnue ou circulaire) : {}", noms.join(", ")));
                break;
            }
        }
        for z in englobes {
            let motifs = textes(get(&z, "englobe_zones"));
            let autres: Vec<usize> = (0..self.zones.len()).filter(|i| motifs.is_empty() || motifs.iter().any(|m| motif(m, &self.zones[*i].nom))).collect();
            if autres.is_empty() {
                self.fail(format!("zone englobante {} sans rien à englober", texte(&z, "nom").unwrap_or("None")));
                continue;
            }
            let m = get(&z, "marge");
            let marge = |c: &str| match m {
                Some(v) if v.is_mapping() => num(v, c, 512.0),
                Some(v) => v.as_f64().unwrap_or(512.0),
                None => 512.0,
            };
            let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
            for i in &autres {
                for a in 0..3 {
                    lo[a] = lo[a].min(self.zones[*i].lo[a]);
                    hi[a] = hi[a].max(self.zones[*i].hi[a]);
                }
            }
            lo[0] -= self.t + marge("ouest");
            lo[1] -= self.t + marge("sud");
            hi[0] += self.t + marge("est");
            hi[1] += self.t + marge("nord");
            lo[2] = num(&z, "sol", lo[2]);
            hi[2] = lo[2] + num(&z, "hauteur", (512.0f64).max(hi[2] - lo[2] + self.s + 256.0));
            let avant = self.zones.len();
            self.ajouter_zone(&z, lo, hi);
            if self.zones.len() > avant {
                let noms = autres.iter().map(|i| self.zones[*i].nom.clone()).collect();
                self.zones.last_mut().unwrap().cibles = Some(noms);
            }
        }
    }

    fn placer_contre(&mut self, z: &Value, r: usize, taille: V3) -> V3 {
        let c = get(z, "contre").unwrap();
        let (rlo, rhi) = (self.zones[r].lo, self.zones[r].hi);
        let rt = self.zones[r].taille();
        let nom_cote = texte(c, "cote").unwrap_or("None");
        let Some((ax, sg)) = cote(nom_cote) else {
            self.fail(format!("zone {} : contre.cote '{nom_cote}' inconnu ({COTES})", texte(z, "nom").unwrap_or("None")));
            return rlo;
        };
        let ecart = num(c, "mur", if ax == 2 { self.s } else { self.t });
        let mut lo = rlo;
        let dec = match get(c, "decalage") {
            Some(v) if v.is_sequence() => {
                let l = nombres(Some(v));
                [l.first().copied().unwrap_or(0.0), l.get(1).copied().unwrap_or(0.0)]
            }
            Some(v) => [v.as_f64().unwrap_or(0.0); 2],
            None => [0.0; 2],
        };
        let aligne = texte(c, "aligne").unwrap_or("centre");
        for (k, a) in [0usize, 1].into_iter().filter(|a| *a != ax).enumerate() {
            lo[a] = match aligne {
                "debut" => rlo[a],
                "fin" => rhi[a] - taille[a],
                _ => ((rlo[a] + (rt[a] - taille[a]) / 2.0) / GRID_WORLD).floor() * GRID_WORLD,
            };
            lo[a] += if ax == 2 { dec[k] } else { dec[0] };
        }
        if ax != 2 {
            lo[2] = rlo[2] + num(c, "dz", 0.0);
        }
        lo[ax] = if sg > 0 { rhi[ax] + ecart } else { rlo[ax] - ecart - taille[ax] };
        lo
    }

    fn ajouter_zone(&mut self, z: &Value, lo: V3, hi: V3) {
        let typ = texte(z, "type").unwrap_or("piece").to_string();
        let nom = match texte(z, "nom") {
            Some(n) if self.zone(n).is_none() => n.to_string(),
            autre => {
                self.fail(format!("zone sans nom ou nom en double : {}", autre.unwrap_or("None")));
                return;
            }
        };
        if !["piece", "couloir", "exterieur"].contains(&typ.as_str()) {
            self.fail(format!("zone {nom} : type '{typ}' inconnu (piece, couloir, exterieur)"));
        }
        let mut zone = Zone {
            nom: nom.clone(),
            typ: typ.clone(),
            lo,
            hi,
            spec: z.clone(),
            englobe: vrai(get(z, "englobe")),
            cibles: None,
            ciel2d: vrai(get(z, "ciel_2d")),
            plein: vrai(get(z, "plein")),
            lm: num(z, "lightmap", if typ == "exterieur" { 32.0 } else { 16.0 }) as i64,
            role: texte(z, "role").map(str::to_string),
            hm: 0.0,
        };
        if let Some(role) = &zone.role {
            let table: Value = serde_yaml::from_str(ROLES).expect("regles/roles.yaml lisible");
            let mut connus: Vec<String> = get(&table, "roles").and_then(Value::as_mapping).map(|m| m.keys().filter_map(|k| k.as_str().map(str::to_string)).collect()).unwrap_or_default();
            connus.sort();
            if !connus.contains(role) {
                self.fail(format!("zone {nom} : role '{role}' inconnu (data/roles.yaml : {})", connus.join(", ")));
            }
        }
        self.sur_grille(&[lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]], GRID_WORLD, &format!("zone {nom}"));
        let [sx, sy, h] = zone.taille();
        match typ.as_str() {
            "piece" => {
                self.regle(h >= 128.0, format!("zone {nom} : plafond à {} (< 128)", g(h)), z);
                self.regle(sx.min(sy) >= 64.0, format!("zone {nom} : pièce de {} de large (< 64)", g(sx.min(sy))), z);
            }
            "couloir" => {
                self.regle(sx.min(sy) >= 96.0, format!("zone {nom} : couloir de {} de large (< 96)", g(sx.min(sy))), z);
                self.regle(h >= 128.0, format!("zone {nom} : plafond à {} (< 128)", g(h)), z);
            }
            _ => {
                let hm = num(z, "hauteur_murs", (192.0f64).min(h - 64.0));
                zone.hm = hm;
                self.sur_grille(&[hm], GRID_WORLD, &format!("zone {nom} hauteur_murs"));
                self.regle(h - hm >= 64.0, format!("zone {nom} : moins de 64 de ciel au-dessus des murs"), z);
            }
        }
        self.zones.push(zone);
    }

    fn controler_paires(&mut self) {
        let zs: Vec<usize> = (0..self.zones.len()).filter(|i| !self.zones[*i].englobe).collect();
        for (i, a) in zs.iter().enumerate() {
            for b in &zs[i + 1..] {
                let (za, zb) = (self.zones[*a].clone(), self.zones[*b].clone());
                let ext: Vec<f64> = (0..3).map(|k| za.hi[k].min(zb.hi[k]) - za.lo[k].max(zb.lo[k])).collect();
                if ext.iter().all(|e| *e > 0.0) {
                    self.fail(format!("zones {} et {} se chevauchent", za.nom, zb.nom));
                    continue;
                }
                let sep: Vec<usize> = (0..3).filter(|k| ext[*k] <= 0.0).collect();
                if sep.len() == 1 {
                    let k = sep[0];
                    if (0..3).filter(|q| *q != k).all(|q| ext[q] > 0.0) {
                        let besoin = if k == 2 { self.s } else { self.t };
                        let ecart = -ext[k];
                        if ecart < besoin {
                            let quoi = if ecart == 0.0 { "zones collées sans mur : utiliser une ouverture passage" } else { "mur trop mince" };
                            self.fail(format!("zones {} et {} : {} entre elles, moins que l'épaisseur {} ({quoi})", za.nom, zb.nom, g(ecart), g(besoin)));
                        }
                    }
                }
            }
        }
        let dil = [self.t, self.t, self.s];
        let englobantes: Vec<usize> = (0..self.zones.len()).filter(|i| self.zones[*i].englobe).collect();
        for e in englobantes {
            let ze = self.zones[e].clone();
            if !ze.exterieur() {
                self.fail(format!("zone {} : englobe: true n'existe que pour type: exterieur", ze.nom));
            }
            for i in &zs {
                let z = self.zones[*i].clone();
                if ze.cibles.as_ref().map_or(false, |c| !c.contains(&z.nom)) {
                    // hors de l'enveloppe : elle ne doit pas la toucher (couche à part)
                    if (0..3).all(|k| (z.hi[k] + dil[k]).min(ze.hi[k]) - (z.lo[k] - dil[k]).max(ze.lo[k]) > 0.0) {
                        self.fail(format!("zone {} : hors de englobe_zones mais dans l'enveloppe de {}", z.nom, ze.nom));
                    }
                    continue;
                }
                if (0..2).any(|k| z.lo[k] - self.t < ze.lo[k] || z.hi[k] + self.t > ze.hi[k]) || z.hi[2] + self.s > ze.hi[2] {
                    self.fail(format!("zone {} déborde de la zone englobante {}", z.nom, ze.nom));
                }
            }
        }
    }

    // --- ouvertures
    fn resoudre_ouvertures(&mut self) {
        let liste: Vec<Value> = get(self.spec, "ouvertures").and_then(Value::as_sequence).cloned().unwrap_or_default();
        for (i, o) in liste.iter().enumerate() {
            let typ = texte(o, "type").unwrap_or("passage").to_string();
            let entre = textes(get(o, "entre"));
            let paire = if entre.len() == 2 { self.zone(&entre[0]).zip(self.zone(&entre[1])) } else { None };
            let Some((ia, ib)) = paire else {
                let ecrit = match get(o, "entre") {
                    Some(v) if v.is_sequence() => format!("[{}]", v.as_sequence().unwrap().iter().map(|x| match x.as_str() { Some(t) => format!("'{t}'"), None => py_str(x) }).collect::<Vec<_>>().join(", ")),
                    Some(v) => py_str(v),
                    None => "None".to_string(),
                };
                self.fail(format!("ouverture {i} : 'entre' doit nommer deux zones existantes ({ecrit})"));
                continue;
            };
            let (za, zb) = (self.zones[ia].clone(), self.zones[ib].clone());
            let label = format!("{typ} {}/{}", za.nom, zb.nom);
            let (ax, s0, s1, lo_rng, hi_rng, porteur);
            if za.englobe || zb.englobe {
                let (iz, z) = if za.englobe { (ib, &zb) } else { (ia, &za) };
                let c = texte(o, "cote").unwrap_or("");
                if !["est", "ouest", "nord", "sud"].contains(&c) && !(c == "dessus" && typ == "tremie") {
                    self.fail(format!("{label} : vers une zone englobante, préciser cote: (nord/sud/est/ouest de {}, ou dessus pour une trémie)", z.nom));
                    continue;
                }
                let (a, sg) = cote(c).unwrap();
                let ep = if a == 2 { self.s } else { self.t };
                ax = a;
                (s0, s1) = if sg > 0 { (z.hi[a], z.hi[a] + ep) } else { (z.lo[a] - ep, z.lo[a]) };
                (lo_rng, hi_rng) = (z.lo, z.hi);
                porteur = iz;
            } else {
                let mut sep = None;
                for a in 0..3 {
                    if za.hi[a] <= zb.lo[a] {
                        sep = Some((a, za.hi[a], zb.lo[a]));
                    } else if zb.hi[a] <= za.lo[a] {
                        sep = Some((a, zb.hi[a], za.lo[a]));
                    } else {
                        continue;
                    }
                    break;
                }
                let Some((a, t0, t1)) = sep else {
                    self.fail(format!("{label} : les zones ne sont pas séparées par un mur"));
                    continue;
                };
                let besoin = if a == 2 { self.s } else { self.t };
                if t1 - t0 > 2.0 * besoin {
                    self.fail(format!("{label} : {} entre les zones (> {}) : ajouter une zone couloir", g(t1 - t0), g(2.0 * besoin)));
                    continue;
                }
                ax = a;
                (s0, s1) = (t0, t1);
                lo_rng = [za.lo[0].max(zb.lo[0]), za.lo[1].max(zb.lo[1]), za.lo[2].max(zb.lo[2])];
                hi_rng = [za.hi[0].min(zb.hi[0]), za.hi[1].min(zb.hi[1]), za.hi[2].min(zb.hi[2])];
                porteur = if !za.exterieur() { ia } else { ib };
            }
            let (mut lo, mut hi) = ([0.0; 3], [0.0; 3]);
            if ax == 2 {
                if typ != "tremie" {
                    self.fail(format!("{label} : zones superposées : seule une ouverture 'tremie' est possible"));
                    continue;
                }
                let bas = if za.lo[2] < zb.lo[2] { &za } else { &zb };
                let r = nombres(get(o, "rect"));
                if r.len() < 4 {
                    self.fail(format!("{label} : rect: [x, y, largeur, profondeur] requis (relatif à {})", bas.nom));
                    continue;
                }
                lo = [bas.lo[0] + r[0], bas.lo[1] + r[1], s0];
                hi = [lo[0] + r[2], lo[1] + r[3], s1];
                if (0..2).any(|k| lo[k] < lo_rng[k] || hi[k] > hi_rng[k]) {
                    self.fail(format!("{label} : la trémie sort de l'emprise commune des deux zones"));
                }
            } else {
                let w = 1 - ax;
                let dispo = hi_rng[w] - lo_rng[w];
                let (dw, dh) = match typ.as_str() {
                    "fenetre" => (128.0, Some(64.0)),
                    "passage" => (dispo, None),
                    _ => (64.0, Some(128.0)),
                };
                let largeur = num(o, "largeur", dw);
                let sol = self.zones[porteur].lo[2];
                if (typ == "porte" || typ == "passage") && !(za.englobe || zb.englobe) && za.lo[2] != zb.lo[2] && get(o, "z").is_none() {
                    self.fail(format!("{label} : sols à des hauteurs différentes ({} / {}) : ajouter un escalier ou préciser z:", g(za.lo[2]), g(zb.lo[2])));
                }
                let bas = sol + match get(o, "z").and_then(Value::as_f64) {
                    Some(z) => z,
                    None if typ == "fenetre" => num(o, "allege", 48.0),
                    None => 0.0,
                };
                let hauteur = num(o, "hauteur", dh.unwrap_or(hi_rng[2] - bas));
                let debut = match get(o, "decalage").and_then(Value::as_f64) {
                    Some(d) => lo_rng[w] + d,
                    None => lo_rng[w] + ((dispo - largeur) / 2.0 / GRID_WORLD).floor() * GRID_WORLD,
                };
                (lo[ax], hi[ax]) = (s0, s1);
                (lo[w], hi[w]) = (debut, debut + largeur);
                (lo[2], hi[2]) = (bas, bas + hauteur);
                if debut < lo_rng[w] || debut + largeur > hi_rng[w] {
                    self.fail(format!("{label} : l'ouverture ({}..{}) sort du mur commun ({}..{})", g(debut), g(debut + largeur), g(lo_rng[w]), g(hi_rng[w])));
                }
                if bas < lo_rng[2] || bas + hauteur > hi_rng[2] {
                    self.fail(format!("{label} : l'ouverture ({}..{} en z) dépasse la hauteur commune ({}..{})", g(bas), g(bas + hauteur), g(lo_rng[2]), g(hi_rng[2])));
                }
                if typ == "porte" {
                    self.regle((56.0..=64.0).contains(&largeur), format!("{label} : baie de porte {} de large (56 à 64)", g(largeur)), o);
                    self.regle((112.0..=128.0).contains(&hauteur), format!("{label} : baie de porte {} de haut (112 à 128)", g(hauteur)), o);
                } else if typ == "passage" {
                    self.regle(largeur >= 56.0, format!("{label} : passage de {} de large (< 56)", g(largeur)), o);
                    self.regle(hauteur >= 112.0, format!("{label} : passage de {} de haut (< 112)", g(hauteur)), o);
                }
            }
            self.sur_grille(&[lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]], GRID_WORLD, &format!("{label} (une ouverture découpe des murs du monde)"));
            let nom = texte(o, "nom").map(str::to_string).unwrap_or_else(|| format!("{typ}_{i}"));
            self.ouvertures.push(Ouv { typ, entre: [za.nom.clone(), zb.nom.clone()], lo, hi, axis: ax, zone: porteur, spec: o.clone(), nom, clear: None });
        }
    }

    // --- enveloppe : air → boîtes de murs
    fn creuser(&mut self) {
        let dil = [self.t, self.t, self.s];
        // (lo, hi, clé, englobante)
        let mut airs: Vec<(V3, V3, Cle, bool)> = Vec::new();
        // (lo, hi, bâtiment)
        let (mut coques_e, mut coques): (Vec<(V3, V3, Option<usize>)>, Vec<(V3, V3, Option<usize>)>) = (Vec::new(), Vec::new());
        for (i, z) in self.zones.iter().enumerate() {
            let mut parts = vec![(Part::Int, z.lo, z.hi)];
            if z.exterieur() {
                let coupe = z.lo[2] + z.hm;
                parts = vec![(Part::Bas, z.lo, [z.hi[0], z.hi[1], coupe]), (Part::Ciel, [z.lo[0], z.lo[1], coupe], z.hi)];
                if z.hm <= 0.0 {
                    parts = vec![(Part::Ciel, z.lo, z.hi)];
                }
            }
            if !z.plein {
                for (part, lo, hi) in parts {
                    airs.push((lo, hi, Cle::Zone(i, part), z.englobe));
                }
            }
            let coque = (moins(z.lo, dil), plus(z.hi, dil), (!z.exterieur()).then_some(i));
            if z.englobe { coques_e.push(coque) } else { coques.push(coque) }
        }
        for (k, o) in self.ouvertures.iter().enumerate() {
            airs.push((o.lo, o.hi, Cle::Ouv(k), false));
        }
        let mut coords: [Vec<f64>; 3] = Default::default();
        for a in 0..3 {
            let mut c: Vec<f64> = airs.iter().flat_map(|b| [b.0[a], b.1[a]]).chain(coques.iter().chain(&coques_e).flat_map(|b| [b.0[a], b.1[a]])).collect();
            c.sort_by(f64::total_cmp);
            c.dedup();
            coords[a] = c;
        }
        let n = [coords[0].len() - 1, coords[1].len() - 1, coords[2].len() - 1];
        let id = |i: usize, j: usize, k: usize| (i * n[1] + j) * n[2] + k;
        let total = n[0] * n[1] * n[2];
        // 0 vide, -1 plein, k > 0 air n° k
        let mut etat = vec![0i32; total];
        // cellule pleine qui appartient à un bâtiment : n° de son propriétaire, 0 = aucun
        let mut dedans = vec![0usize; total];
        let mut cles: Vec<Option<Cle>> = vec![None];
        let plage = |lo: V3, hi: V3| -> [(usize, usize); 3] {
            let ou = |a: usize, v: f64| coords[a].partition_point(|c| *c < v);
            [(ou(0, lo[0]), ou(0, hi[0])), (ou(1, lo[1]), ou(1, hi[1])), (ou(2, lo[2]), ou(2, hi[2]))]
        };
        macro_rules! remplir {
            ($r:expr, $f:expr) => {
                for i in $r[0].0..$r[0].1 {
                    for j in $r[1].0..$r[1].1 {
                        for k in $r[2].0..$r[2].1 {
                            $f(id(i, j, k));
                        }
                    }
                }
            };
        }
        for (lo, hi, _) in &coques_e {
            let r = plage(*lo, *hi);
            remplir!(r, |c: usize| etat[c] = -1);
        }
        for (lo, hi, cle, englobe) in &airs {
            if *englobe {
                cles.push(Some(*cle));
                let (r, v) = (plage(*lo, *hi), cles.len() as i32 - 1);
                remplir!(r, |c: usize| etat[c] = v);
            }
        }
        for (lo, hi, batiment) in &coques {
            let r = plage(*lo, *hi);
            remplir!(r, |c: usize| etat[c] = -1);
            if batiment.is_some() {
                self.proprietaires.push(*batiment);
                let v = self.proprietaires.len() - 1;
                remplir!(r, |c: usize| dedans[c] = v);
            }
        }
        for (lo, hi, cle, englobe) in &airs {
            if !*englobe {
                cles.push(Some(*cle));
                let (r, v) = (plage(*lo, *hi), cles.len() as i32 - 1);
                remplir!(r, |c: usize| etat[c] = v);
            }
        }
        let plein: Vec<bool> = etat.iter().map(|e| *e == -1).collect();
        let air = |i: i64, j: i64, k: i64| -> i32 {
            if i < 0 || j < 0 || k < 0 || i >= n[0] as i64 || j >= n[1] as i64 || k >= n[2] as i64 {
                return 0;
            }
            etat[id(i as usize, j as usize, k as usize)].max(0)
        };
        // l'air que touche chaque face d'un bloc de cellules ; `None` si une face touche deux airs différents
        let faces_ok = |i0: [usize; 3], i1: [usize; 3]| -> Option<Vec<((usize, i32), i32)>> {
            let mut out = Vec::with_capacity(6);
            for (a, sg) in DIRS {
                let mut r = [(i0[0], i1[0]), (i0[1], i1[1]), (i0[2], i1[2])];
                r[a] = if sg > 0 { (i1[a] - 1, i1[a]) } else { (i0[a], i0[a] + 1) };
                let mut vu = 0i32;
                for i in r[0].0..r[0].1 {
                    for j in r[1].0..r[1].1 {
                        for k in r[2].0..r[2].1 {
                            let mut q = [i as i64, j as i64, k as i64];
                            q[a] += sg as i64;
                            let v = air(q[0], q[1], q[2]);
                            if v > 0 {
                                if vu > 0 && v != vu {
                                    return None;
                                }
                                vu = v;
                            }
                        }
                    }
                }
                out.push(((a, sg), vu));
            }
            Some(out)
        };
        let mut pris = vec![false; total];
        // graines dans l'ordre z, y, x : les sols d'abord (ils passent sous les murs), puis les murs pleine hauteur
        for k in 0..n[2] {
            for j in 0..n[1] {
                for i in 0..n[0] {
                    if !plein[id(i, j, k)] || pris[id(i, j, k)] {
                        continue;
                    }
                    let (i0, mut i1) = ([i, j, k], [i + 1, j + 1, k + 1]);
                    let marque = dedans[id(i, j, k)];
                    for a in 0..3 {
                        while i1[a] < n[a] {
                            let mut t1 = i1;
                            t1[a] += 1;
                            let mut bon = true;
                            'bloc: for x in i0[0]..t1[0] {
                                for y in i0[1]..t1[1] {
                                    for z in i0[2]..t1[2] {
                                        let c = id(x, y, z);
                                        if !plein[c] || pris[c] || dedans[c] != marque {
                                            bon = false;
                                            break 'bloc;
                                        }
                                    }
                                }
                            }
                            if !bon || faces_ok(i0, t1).is_none() {
                                break;
                            }
                            i1 = t1;
                        }
                    }
                    for x in i0[0]..i1[0] {
                        for y in i0[1]..i1[1] {
                            for z in i0[2]..i1[2] {
                                pris[id(x, y, z)] = true;
                            }
                        }
                    }
                    let lo = [coords[0][i0[0]], coords[1][i0[1]], coords[2][i0[2]]];
                    let hi = [coords[0][i1[0]], coords[1][i1[1]], coords[2][i1[2]]];
                    let contacts = faces_ok(i0, i1).unwrap().into_iter().map(|(d, v)| (d, if v > 0 { cles[v as usize] } else { None })).collect();
                    self.coque.push((lo, hi, contacts, marque));
                }
            }
        }
    }

    /// Face d'un bâtiment vue depuis un extérieur : la façade (ou le toit) que le bâtiment déclare lui-même, sinon
    /// celle de l'extérieur.
    fn mat_proprio(&mut self, role: &str, proprio: usize, zi: usize) -> String {
        let voulu = match role {
            "toit" => "toit",
            "dessous" => "sous_face",
            _ => "facade",
        };
        if let Some(o) = self.proprietaires.get(proprio).copied().flatten().filter(|_| proprio != 0 && self.phase != "blockout") {
            let om = materiaux(get(&self.zones[o].spec, "materiaux"));
            if om.contains_key(voulu) {
                return self.mat(voulu, Some(o), None);
            }
            if voulu == "sous_face" && om.contains_key("facade") {
                return self.mat("facade", Some(o), None);
            }
        }
        self.mat(if role == "dessous" { "mur" } else { role }, Some(zi), None)
    }

    fn mat_coque(&mut self, d: (usize, i32), cle: Option<Cle>, dedans: usize, hi: V3) -> (String, i64) {
        let Some(cle) = cle else { return (NODRAW.to_string(), 16) };
        let (zi, part) = match cle {
            Cle::Ouv(k) => {
                let (zi, typ) = (self.ouvertures[k].zone, self.ouvertures[k].typ.clone());
                let lm = self.zones[zi].lm;
                let role = if d == (2, 1) && (typ == "porte" || typ == "passage") { "sol" } else { "mur" };
                return (self.mat(role, Some(zi), None), lm);
            }
            Cle::Zone(zi, part) => (zi, part),
        };
        let z = self.zones[zi].clone();
        if part == Part::Ciel {
            if d == (2, 1) && hi[2] == z.lo[2] {
                // le sol d'un extérieur reste un sol, même sous le ciel
                return (self.mat("sol", Some(zi), None), z.lm);
            }
            if dedans == 0 {
                return (if z.ciel2d { SKY2D } else { SKY }.to_string(), 16);
            }
            let role = match d {
                (2, 1) => "toit",
                (2, _) => "dessous",
                _ => "mur",
            };
            return (self.mat_proprio(role, dedans, zi), z.lm);
        }
        if z.exterieur() {
            let m = match d {
                (2, 1) if hi[2] == z.lo[2] => self.mat("sol", Some(zi), None),
                (2, 1) => self.mat_proprio("toit", dedans, zi),
                (2, _) => self.mat_proprio("dessous", dedans, zi),
                _ => self.mat_proprio("mur", dedans, zi),
            };
            return (m, z.lm);
        }
        let role = match d {
            (2, 1) => "sol",
            (2, _) => "plafond",
            _ => "mur",
        };
        (self.mat(role, Some(zi), None), z.lm)
    }

    fn ecrire_coque(&mut self) {
        for (lo, hi, contacts, dedans) in std::mem::take(&mut self.coque) {
            let (mut mats, mut echelles, mut lms) = (HashMap::new(), HashMap::new(), HashMap::new());
            for (d, cle) in contacts {
                let (m, lm) = self.mat_coque(d, cle, dedans, hi);
                echelles.insert(dirkey(d), self.echelle(&m));
                mats.insert(dirkey(d), m);
                lms.insert(dirkey(d), lm);
            }
            let sol = self.solide(faces_boite(lo, hi, &mats, &echelles, &lms));
            self.monde.push(sol);
        }
    }

    // --- éléments positifs (func_detail)
    fn entite(&mut self, classe: &str, cles: Vec<(&str, String)>) -> usize {
        self.entites.push(Entite { classe: classe.to_string(), cles: cles.into_iter().map(|(k, v)| (k.to_string(), v)).collect(), solides: Vec::new() });
        self.entites.len() - 1
    }

    fn ent_detail(&mut self, nom: &str, classe: &str) -> usize {
        let cles = if classe != "func_detail" { vec![("targetname", nom.to_string())] } else { Vec::new() };
        self.entite(classe, cles)
    }

    #[allow(clippy::too_many_arguments)]
    fn boite(&mut self, ent: usize, lo: V3, hi: V3, dessus: &str, cotes: &str, dessous: Option<&str>, lm: i64, grille: f64) {
        let classe = self.entites[ent].classe.clone();
        self.sur_grille(&[lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]], grille, &format!("élément ({classe})"));
        let faces = self.faces_trois(lo, hi, dessus, cotes, dessous.filter(|d| !d.is_empty()).unwrap_or(cotes), lm);
        let sol = self.solide(faces);
        self.entites[ent].solides.push(sol);
    }

    fn faces_trois(&self, lo: V3, hi: V3, dessus: &str, cotes: &str, dessous: &str, lm: i64) -> Vec<Face> {
        let mats: HashMap<&'static str, String> = [("+z", dessus), ("-z", dessous), ("+x", cotes), ("-x", cotes), ("+y", cotes), ("-y", cotes)].into_iter().map(|(k, v)| (k, v.to_string())).collect();
        let echelles = mats.iter().map(|(k, v)| (*k, self.echelle(v))).collect();
        let lms = mats.keys().map(|k| (*k, lm)).collect();
        faces_boite(lo, hi, &mats, &echelles, &lms)
    }

    fn zone_de(&mut self, e: &Value, label: &str) -> Option<usize> {
        let nom = texte(e, "zone")?;
        let z = self.zone(nom);
        if z.is_none() {
            self.fail(format!("{label} : zone '{nom}' inconnue"));
        }
        z
    }

    fn origine_de(&self, zi: Option<usize>) -> V3 {
        zi.map_or(self.repere.0, |i| self.zones[i].lo)
    }

    /// Un élément sans zone dans un repère tourné : seuls les blocs, les dalles et les entités savent s'y poser.
    fn refuse_tourne(&mut self, zi: Option<usize>, label: &str) -> bool {
        if zi.is_none() && self.repere.1 != 0 {
            self.fail(format!("{label} : ce type d'élément ne sait pas encore se poser dans un repère tourné (repere.angle)"));
            return true;
        }
        false
    }

    fn dedans(&mut self, zi: Option<usize>, lo: V3, hi: V3, label: &str) {
        if let Some(i) = zi {
            let z = &self.zones[i];
            if (0..3).any(|k| lo[k] < z.lo[k] - 1e-6 || hi[k] > z.hi[k] + 1e-6) {
                let nom = z.nom.clone();
                self.fail(format!("{label} : sort de la zone {nom} ({}..{})", liste_py(lo), liste_py(hi)));
            }
        }
    }

    fn construire_elements(&mut self) {
        let specs: Vec<Value> = get(self.spec, "elements").and_then(Value::as_sequence).cloned().unwrap_or_default();
        // les dalles d'abord : les escaliers vérifient qu'ils y arrivent
        let mut ordre: Vec<usize> = (0..specs.len()).collect();
        ordre.sort_by_key(|i| texte(&specs[*i], "type") != Some("dalle"));
        for i in ordre {
            let e = &specs[i];
            if !self.retenu(e) {
                continue;
            }
            let typ = texte(e, "type").unwrap_or("None");
            let label = format!("élément {} ({typ})", texte(e, "nom").map(str::to_string).unwrap_or_else(|| i.to_string()));
            let zi = self.zone_de(e, &label);
            let o = self.origine_de(zi);
            match typ {
                "dalle" => self.el_dalle(e, zi, o, &label),
                "escalier" => self.el_escalier(e, zi, o, &label),
                "rampe" => self.el_rampe(e, zi, o, &label),
                "bloc" => self.el_bloc(e, zi, o, &label),
                "instance" => self.el_instance(e, zi, o, &label),
                _ => self.fail(format!("{label} : type inconnu (dalle, escalier, rampe, bloc, instance)")),
            }
        }
    }

    fn mats3(&mut self, e: &Value, zi: Option<usize>, role_dessus: &str, role_cotes: &str) -> (String, String, String) {
        let m = materiaux(get(e, "materiaux"));
        let dessus = self.mat(role_dessus, zi, m.get("dessus").cloned());
        let cotes = self.mat(role_cotes, zi, m.get("cotes").cloned());
        let dessous = self.mat(role_cotes, zi, m.get("dessous").cloned());
        (dessus, cotes, dessous)
    }

    fn lm_zone(&self, zi: Option<usize>) -> i64 {
        zi.map_or(16, |i| self.zones[i].lm)
    }

    fn el_dalle(&mut self, e: &Value, zi: Option<usize>, o: V3, label: &str) {
        let r = nombres(get(e, "rect"));
        if r.len() < 4 || get(e, "z").is_none() {
            self.fail(format!("{label} : rect: [x, y, largeur, profondeur] et z: requis"));
            return;
        }
        let ep = num(e, "epaisseur", 16.0);
        let haut = o[2] + num(e, "z", 0.0);
        let zl = num(e, "z", 0.0);
        let (lo, hi) = self.boite_monde(zi, [r[0], r[1], zl - ep], [r[0] + r[2], r[1] + r[3], zl]);
        self.dedans(zi, lo, hi, label);
        if let Some(i) = zi {
            let (zlo, zhi) = (self.zones[i].lo[2], self.zones[i].hi[2]);
            self.regle(haut - zlo - ep >= 128.0, format!("{label} : {} sous la dalle (< 128)", g(haut - zlo - ep)), e);
            self.regle(zhi - haut >= 128.0, format!("{label} : {} au-dessus de la dalle (< 128)", g(zhi - haut)), e);
        }
        let m = materiaux(get(e, "materiaux"));
        let ent = self.ent_detail(texte(e, "nom").unwrap_or("dalle"), "func_detail");
        let (dessus, cotes, dessous) = (self.mat("sol", zi, m.get("dessus").cloned()), self.mat("detail", zi, m.get("cotes").cloned()), self.mat("plafond", zi, m.get("dessous").cloned()));
        self.boite(ent, lo, hi, &dessus, &cotes, Some(&dessous), self.lm_zone(zi), GRID_DETAIL);
        self.elements.push(El { typ: "dalle".into(), nom: texte(e, "nom").map(str::to_string), lo, hi, zone: zi, spec: e.clone(), ..Default::default() });
        if let Some(i) = zi {
            if texte(e, "lumiere").unwrap_or("auto") == "auto" && get(e, "lumiere").map_or(true, Value::is_string) && self.lumieres_auto(i) {
                self.grille_lumieres(i, lo, hi, lo[2] - 8.0, lo[2], 150.0);
            }
        }
    }

    /// Garde-corps des dalles, une fois les escaliers connus (on laisse leur arrivée libre).
    fn finir_garde_corps(&mut self) {
        let escaliers: Vec<El> = self.elements.iter().filter(|el| el.typ == "escalier").cloned().collect();
        let dalles: Vec<El> = self.elements.iter().filter(|el| el.typ == "dalle").cloned().collect();
        for el in dalles {
            let (e, lo, hi, zi) = (&el.spec, el.lo, el.hi, el.zone);
            let cotes_gc: Vec<String> = textes(get(e, "garde_corps")).iter().map(|c| self.cote_monde(zi, c)).collect();
            if cotes_gc.is_empty() {
                continue;
            }
            let (h, t) = (num(e, "hauteur_garde_corps", 48.0), 8.0);
            let ent = self.ent_detail("garde_corps", "func_detail");
            let mat = self.mat("garde_corps", zi, materiaux(get(e, "materiaux")).get("garde_corps").cloned());
            for c in &cotes_gc {
                let Some((ax, sg)) = cote(c) else { continue };
                let w = 1 - ax;
                let (mut a0, mut a1) = (lo[w], hi[w]);
                if w == 1 {
                    // côtés est/ouest : raccourcis aux angles tenus par nord/sud
                    if cotes_gc.iter().any(|x| x == "sud") {
                        a0 += t;
                    }
                    if cotes_gc.iter().any(|x| x == "nord") {
                        a1 -= t;
                    }
                }
                let mut spans = vec![(a0, a1)];
                let bord = if sg > 0 { hi[ax] } else { lo[ax] };
                for st in &escaliers {
                    if st.arrive_axis == ax && (st.arrive_at - bord).abs() < 1e-6 {
                        spans = soustraire(spans, st.lo[w], st.hi[w]);
                    }
                }
                for (s0, s1) in spans {
                    if s1 - s0 < 8.0 {
                        continue;
                    }
                    let (mut blo, mut bhi) = (lo, hi);
                    (blo[2], bhi[2]) = (hi[2], hi[2] + h);
                    (blo[w], bhi[w]) = (s0, s1);
                    if sg > 0 {
                        blo[ax] = hi[ax] - t;
                    } else {
                        bhi[ax] = lo[ax] + t;
                    }
                    self.boite(ent, blo, bhi, &mat, &mat, None, 16, GRID_DETAIL);
                    self.elements.push(El { typ: "garde_corps".into(), lo: blo, hi: bhi, zone: zi, ..Default::default() });
                }
            }
        }
    }

    fn el_escalier(&mut self, e: &Value, zi: Option<usize>, o: V3, label: &str) {
        if self.refuse_tourne(zi, label) {
            return;
        }
        let sens = texte(e, "direction").unwrap_or("None").to_string();
        let Some((ax, sg)) = cote(&sens).filter(|(a, _)| *a < 2) else {
            self.fail(format!("{label} : direction '{sens}' (nord, sud, est, ouest)"));
            return;
        };
        let (w, sgf) = (1 - ax, sg as f64);
        let (marche, giron) = (num(e, "marche", 8.0), num(e, "giron", 16.0));
        let (h, largeur) = (num(e, "hauteur", 0.0), num(e, "largeur", 96.0));
        let z0 = o[2] + num(e, "z", 0.0);
        self.regle(marche <= 18.0, format!("{label} : marche de {} (> 18 : infranchissable sans sauter)", g(marche)), e);
        self.regle(giron >= 12.0, format!("{label} : giron de {} (< 12)", g(giron)), e);
        self.regle(largeur >= 64.0, format!("{label} : escalier de {} de large (< 64)", g(largeur)), e);
        let dep = nombres(get(e, "depart"));
        if dep.len() < 2 || marche <= 0.0 || h % marche != 0.0 {
            self.fail(format!("{label} : hauteur {} non divisible par la marche {}", g(h), g(marche)));
            return;
        }
        let nb = (h / marche) as i64;
        let long = (nb - 1) as f64 * giron;
        let p = [o[0] + dep[0], o[1] + dep[1], z0];
        let (dessus, cotes, _) = self.mats3(e, zi, "marche", "detail");
        let ent = self.ent_detail("escalier", "func_detail");
        // plein : marches pleines jusqu'au sol ; limon : marches de 8, dessous ouvert
        let style = texte(e, "style").unwrap_or("plein").to_string();
        if style != "plein" && style != "limon" {
            self.fail(format!("{label} : style '{style}' (plein, limon)"));
            return;
        }
        for k in 1..nb {
            let (mut lo, mut hi) = (p, p);
            (lo[w], hi[w]) = (p[w], p[w] + largeur);
            let a = p[ax] + sgf * (k - 1) as f64 * giron;
            let b = a + sgf * giron;
            (lo[ax], hi[ax]) = (a.min(b), a.max(b));
            (lo[2], hi[2]) = (if style == "plein" { z0 } else { z0 + k as f64 * marche - 8.0 }, z0 + k as f64 * marche);
            self.boite(ent, lo, hi, &dessus, &cotes, None, 16, GRID_DETAIL);
        }
        // garde-corps rampant sur les côtés libres : 48 au-dessus du nez des marches, 8 d'épaisseur ; il rejoint à
        // l'arrivée le garde-corps d'une dalle (même hauteur)
        let fin = p[ax] + sgf * long;
        for gc in textes(get(e, "garde_corps")) {
            let Some((_, gs)) = cote(&gc).filter(|(a, _)| *a == w) else {
                self.fail(format!("{label} : garde_corps '{gc}' doit être un côté de l'escalier, pas son sens"));
                continue;
            };
            if marche % 8.0 != 0.0 {
                self.fail(format!("{label} : garde-corps rampant impossible avec une marche de {} (multiple de 8)", g(marche)));
                continue;
            }
            let (q0, q1) = if gs > 0 { (p[w] + largeur, p[w] + largeur + 8.0) } else { (p[w] - 8.0, p[w]) };
            let mut pts: Vec<V3> = Vec::new();
            for q in [q0, q1] {
                for (s, zz) in [(p[ax], z0), (p[ax], z0 + marche + 48.0), (fin, z0 + h + 48.0), (fin, z0 + h - 16.0)] {
                    let mut v = [0.0; 3];
                    (v[w], v[ax], v[2]) = (q, s, zz);
                    pts.push(v);
                }
            }
            let plat: Vec<f64> = pts.iter().flatten().copied().collect();
            self.sur_grille(&plat, GRID_DETAIL, &format!("{label} garde-corps"));
            let m = self.mat("garde_corps", zi, materiaux(get(e, "materiaux")).get("garde_corps").cloned());
            let gent = self.ent_detail("garde_corps_escalier", "func_detail");
            let ech = self.echelle(&m);
            let faces = faces_enveloppe(&pts).into_iter().map(|(n, poly)| Face { n, poly, mat: m.clone(), echelle: ech, lm: 16, ..Default::default() }).collect();
            let sol = self.solide(faces);
            self.entites[gent].solides.push(sol);
            let (lo, hi) = bornes(&pts);
            self.dedans(zi, lo, hi, &format!("{label} garde-corps"));
            self.elements.push(El { typ: "garde_corps".into(), lo, hi, zone: zi, ..Default::default() });
        }
        let (mut lo, mut hi) = (p, p);
        (lo[w], hi[w]) = (p[w], p[w] + largeur);
        (lo[ax], hi[ax]) = (p[ax].min(fin), p[ax].max(fin));
        hi[2] = z0 + h - marche;
        self.dedans(zi, lo, hi, label);
        let palier = self.elements.iter().any(|el| el.typ == "dalle" && (el.hi[2] - (z0 + h)).abs() < 1e-6 && ((if sg > 0 { el.lo[ax] } else { el.hi[ax] }) - fin).abs() < 1e-6 && el.lo[w] <= lo[w] && el.hi[w] >= hi[w]);
        if !palier {
            self.warn.push(format!("{label} : la dernière marche n'arrive sur aucune dalle à z={}", g(z0 + h)));
        }
        if vrai(get(e, "clip")) {
            let c = self.entite("func_detail", Vec::new());
            let mut pts: Vec<V3> = Vec::new();
            for q in [lo[w], hi[w]] {
                for (s, zz) in [(p[ax], z0), (fin, z0 + h - marche), (fin, z0)] {
                    let mut v = [0.0; 3];
                    (v[w], v[ax], v[2]) = (q, s, zz);
                    pts.push(v);
                }
            }
            let faces = faces_enveloppe(&pts).into_iter().map(|(n, poly)| Face { n, poly, mat: "tools/toolsplayerclip".into(), echelle: 0.25, lm: 16, ..Default::default() }).collect();
            let sol = self.solide(faces);
            self.entites[c].solides.push(sol);
        }
        self.elements.push(El { typ: "escalier".into(), lo, hi, zone: zi, arrive_axis: ax, arrive_at: fin, direction: Some(sens), ..Default::default() });
    }

    fn el_rampe(&mut self, e: &Value, zi: Option<usize>, o: V3, label: &str) {
        if self.refuse_tourne(zi, label) {
            return;
        }
        let sens = texte(e, "direction").unwrap_or("None");
        let Some((ax, sg)) = cote(sens).filter(|(a, _)| *a < 2) else {
            self.fail(format!("{label} : direction '{sens}' (nord, sud, est, ouest)"));
            return;
        };
        let w = 1 - ax;
        let (long, h, largeur) = (num(e, "longueur", 0.0), num(e, "hauteur", 0.0), num(e, "largeur", 128.0));
        let dep = nombres(get(e, "depart"));
        if dep.len() < 2 || long <= 0.0 {
            self.fail(format!("{label} : depart: [x, y] et longueur: requis"));
            return;
        }
        let angle = h.atan2(long).to_degrees();
        self.regle(angle <= 45.0, format!("{label} : pente de {angle:.1}° (> 45 : on glisse)"), e);
        let p = [o[0] + dep[0], o[1] + dep[1], o[2] + num(e, "z", 0.0)];
        let fin = p[ax] + sg as f64 * long;
        let mut pts: Vec<V3> = Vec::new();
        for q in [p[w], p[w] + largeur] {
            for (s, zz) in [(p[ax], p[2]), (fin, p[2]), (fin, p[2] + h)] {
                let mut v = [0.0; 3];
                (v[w], v[ax], v[2]) = (q, s, zz);
                pts.push(v);
            }
        }
        let plat: Vec<f64> = pts.iter().flatten().copied().collect();
        self.sur_grille(&plat, GRID_DETAIL, label);
        let (dessus, cotes, _) = self.mats3(e, zi, "sol", "detail");
        let ent = self.ent_detail("rampe", "func_detail");
        let lm = self.lm_zone(zi);
        let faces = faces_enveloppe(&pts)
            .into_iter()
            .map(|(n, poly)| {
                let m = if n[2] > 0.01 { &dessus } else { &cotes };
                Face { n, poly, mat: m.clone(), echelle: self.echelle(m), lm, ..Default::default() }
            })
            .collect();
        let sol = self.solide(faces);
        self.entites[ent].solides.push(sol);
        let (lo, hi) = bornes(&pts);
        self.dedans(zi, lo, hi, label);
        self.elements.push(El { typ: "rampe".into(), lo, hi, zone: zi, ..Default::default() });
    }

    fn el_bloc(&mut self, e: &Value, zi: Option<usize>, o: V3, label: &str) {
        let r = nombres(get(e, "rect"));
        if r.len() < 4 || get(e, "hauteur").is_none() {
            self.fail(format!("{label} : rect: [x, y, largeur, profondeur] et hauteur: requis"));
            return;
        }
        let _ = o;
        let zl = num(e, "z", 0.0);
        let (lo, hi) = self.boite_monde(zi, [r[0], r[1], zl], [r[0] + r[2], r[1] + r[3], zl + num(e, "hauteur", 0.0)]);
        self.dedans(zi, lo, hi, label);
        let (dessus, cotes, dessous) = self.mats3(e, zi, "detail", "detail");
        let classe = texte(e, "classe").unwrap_or("func_detail").to_string();
        // un relief de désert en blocs se met à 128-256
        let lm = get(e, "lightmap").and_then(Value::as_f64).map_or(self.lm_zone(zi), |v| v as i64);
        let grille = if classe == "monde" { GRID_WORLD } else { GRID_DETAIL };
        self.sur_grille(&[lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]], grille, &if classe == "monde" { label.to_string() } else { format!("élément ({classe})") });
        // un matériau par côté (`est`, `ouest`, `nord`, `sud` dans le repère de l'élément), et les faces à ajuster
        let propres = materiaux(get(e, "materiaux"));
        let ajustees: Vec<String> = textes(get(e, "ajuste")).iter().map(|c| self.cote_monde(zi, c)).collect();
        let mut faces = self.faces_trois(lo, hi, &dessus, &cotes, if dessous.is_empty() { &cotes } else { &dessous }, lm);
        for f in &mut faces {
            let monde = match (f.n[0] as i32, f.n[1] as i32, f.n[2] as i32) {
                (1, _, _) => "est",
                (-1, _, _) => "ouest",
                (_, 1, _) => "nord",
                (_, -1, _) => "sud",
                (_, _, 1) => "dessus",
                _ => "dessous",
            };
            for local in ["est", "ouest", "nord", "sud"] {
                if let Some(m) = propres.get(local).filter(|_| self.cote_monde(zi, local) == monde) {
                    f.mat = self.mat("detail", zi, Some(m.clone()));
                    f.echelle = self.echelle(&f.mat);
                }
            }
            f.ajuste = ajustees.iter().any(|c| c == monde);
            f.lm_fixe = get(e, "lightmap").is_some();
        }
        let sol = self.solide(faces);
        if classe == "monde" {
            self.monde.push(sol);
        } else {
            let ent = self.ent_detail(texte(e, "nom").unwrap_or("bloc"), &classe);
            self.entites[ent].solides.push(sol);
        }
        self.elements.push(El { typ: "bloc".into(), nom: texte(e, "nom").map(str::to_string), lo, hi, zone: zi, ..Default::default() });
    }

    fn trouver_module(&self, nom: &str) -> Option<PathBuf> {
        let court = Path::new(nom).file_name().map(PathBuf::from).unwrap_or_default();
        let mut candidats = vec![self.opt.base.join(nom), self.opt.base.join("modules").join(&court)];
        if let Some(m) = &self.opt.modules_skill {
            candidats.push(m.join(nom));
            candidats.push(m.join(&court));
        }
        candidats.into_iter().find(|c| c.is_file())
    }

    fn el_instance(&mut self, e: &Value, zi: Option<usize>, o: V3, label: &str) {
        if self.refuse_tourne(zi, label) {
            return;
        }
        let fichier = texte(e, "fichier").unwrap_or("");
        let Some(mut src) = self.trouver_module(fichier) else {
            self.fail(format!("{label} : module '{fichier}' introuvable (modules/ du projet ou du skill)"));
            return;
        };
        let nom_fichier = src.file_name().unwrap_or_default().to_string_lossy().to_string();
        // une copie d'un module du skill dans le projet ne doit pas masquer une version plus récente du skill
        if let Some(du_skill) = self.opt.modules_skill.as_ref().map(|m| m.join(&nom_fichier)).filter(|p| p.is_file()) {
            let memes = std::fs::canonicalize(&src).ok() == std::fs::canonicalize(&du_skill).ok();
            if !memes && std::fs::read(&src).ok() != std::fs::read(&du_skill).ok() {
                let date = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
                if date(&du_skill) > date(&src) {
                    self.warn.push(format!("{label} : module {nom_fichier} mis à jour depuis le skill (copie du projet remplacée)"));
                    src = du_skill;
                } else {
                    self.warn.push(format!("{label} : la copie de {nom_fichier} du projet diffère du skill et elle est plus récente : gardée (la renommer si c'est un module propre au projet)"));
                }
            }
        }
        if !self.modules.iter().any(|(_, n)| *n == nom_fichier) {
            self.modules.push((src.clone(), nom_fichier.clone()));
        }
        let pos = nombres(get(e, "position"));
        let origine = plus(o, [pos.first().copied().unwrap_or(0.0), pos.get(1).copied().unwrap_or(0.0), pos.get(2).copied().unwrap_or(0.0)]);
        let angle = num(e, "angle", 0.0);
        if angle % 90.0 != 0.0 {
            self.fail(format!("{label} : angle {} (multiples de 90 seulement, pour rester sur la grille)", g(angle)));
        }
        self.sur_grille(&origine, GRID_DETAIL, label);
        let nom = texte(e, "nom").map(str::to_string).unwrap_or_else(|| format!("inst_{}", self.elements.len()));
        self.entite("func_instance", vec![("targetname", nom.clone()), ("file", format!("modules/{nom_fichier}")), ("angles", format!("0 {} 0", g(angle))), ("origin", triplet(origine)), ("fixup_style", "0".to_string())]);
        // l'emprise du module : les points de ses plans, tournés et posés
        let mut pts: Vec<V3> = Vec::new();
        if let Ok(vmf) = hm_vmf::Vmf::open(&src) {
            let solides = vmf.world().into_iter().flat_map(|w| w.solids()).chain(vmf.entities().flat_map(|e| e.solids()));
            for s in solides {
                for side in s.children_named("side") {
                    if let Some(pl) = side.get("plane").and_then(hm_vmf::parse_plane) {
                        pts.extend(pl);
                    }
                }
            }
        }
        if !pts.is_empty() {
            let (s, c) = angle.to_radians().sin_cos();
            let tournes: Vec<V3> = pts.iter().map(|v| plus([v[0] * c - v[1] * s, v[0] * s + v[1] * c, v[2]], origine)).collect();
            let (lo, hi) = bornes(&tournes);
            self.dedans(zi, lo, hi, label);
            self.elements.push(El { typ: "instance".into(), nom: Some(nom), lo, hi, zone: zi, ..Default::default() });
        }
    }

    // --- finitions : cadres, vitres, portes, plinthes
    fn habiller_ouvertures(&mut self) {
        for k in 0..self.ouvertures.len() {
            let o = self.ouvertures[k].clone();
            let (spec, zi, ax) = (&o.spec, o.zone, o.axis);
            if ax == 2 {
                continue;
            }
            let w = 1 - ax;
            let (lo, hi) = (o.lo, o.hi);
            let (mut clo, mut chi) = (lo, hi);
            let cadre = get(spec, "cadre").map_or(o.typ == "porte", |v| vrai(Some(v)));
            if cadre {
                let t = 8.0;
                let mat = self.mat("cadre", Some(zi), texte(spec, "materiau_cadre").map(str::to_string));
                let ent = self.ent_detail("cadre", "func_detail");
                let linteau = if o.typ == "porte" { hi[2] - lo[2] - 112.0 } else { num(spec, "linteau", 0.0) };
                for (s0, s1) in [(lo[w], lo[w] + t), (hi[w] - t, hi[w])] {
                    let (mut blo, mut bhi) = (lo, hi);
                    (blo[w], bhi[w]) = (s0, s1);
                    blo[ax] -= t;
                    bhi[ax] += t;
                    self.boite(ent, blo, bhi, &mat, &mat, None, 16, GRID_DETAIL);
                }
                if linteau > 0.0 {
                    let (mut blo, mut bhi) = (lo, hi);
                    (blo[w], bhi[w]) = (lo[w] + t, hi[w] - t);
                    blo[2] = hi[2] - linteau;
                    blo[ax] -= t;
                    bhi[ax] += t;
                    self.boite(ent, blo, bhi, &mat, &mat, None, 16, GRID_DETAIL);
                }
                clo[w] += t;
                chi[w] -= t;
                chi[2] -= linteau.max(0.0);
                let (mut flo, mut fhi) = (lo, hi);
                flo[ax] -= t;
                fhi[ax] += t;
                self.elements.push(El { typ: "cadre".into(), lo: flo, hi: fhi, zone: Some(zi), ouverture: Some(o.nom.clone()), ..Default::default() });
            }
            self.ouvertures[k].clear = Some((clo, chi));
            if o.typ == "fenetre" && get(spec, "vitre").map_or(true, |v| vrai(Some(v))) {
                let (mut gl, mut gh) = (clo, chi);
                if lo[ax] >= self.zones[zi].hi[ax] - 1e-6 {
                    // l'ouverture est du côté + de la zone porteuse : vitre côté +
                    gl[ax] = hi[ax] - 8.0;
                } else {
                    gh[ax] = lo[ax] + 8.0;
                }
                let verre = self.mat("verre", Some(zi), texte(spec, "materiau_verre").map(str::to_string));
                let ent = self.ent_detail("vitre", "func_detail");
                let mut mats: HashMap<&'static str, String> = ["+x", "-x", "+y", "-y", "+z", "-z"].into_iter().map(|d| (d, NODRAW.to_string())).collect();
                mats.insert(dirkey((ax, 1)), verre.clone());
                mats.insert(dirkey((ax, -1)), verre);
                self.sur_grille(&[gl[0], gl[1], gl[2], gh[0], gh[1], gh[2]], GRID_DETAIL, &format!("vitre {}", o.nom));
                let echelles = mats.iter().map(|(d, m)| (*d, self.echelle(m))).collect();
                let lms = mats.keys().map(|d| (*d, 16)).collect();
                let sol = self.solide(faces_boite(gl, gh, &mats, &echelles, &lms));
                self.entites[ent].solides.push(sol);
            }
            if o.typ == "porte" {
                match texte(spec, "entite").unwrap_or("prop_door_rotating") {
                    "prop_door_rotating" => self.porte_modele(&o, clo, chi),
                    "func_door" => self.porte_solide(&o, clo, chi),
                    "aucune" => {}
                    autre => self.fail(format!("porte {} : entite '{autre}' inconnue (prop_door_rotating, func_door, aucune)", o.nom)),
                }
            }
        }
    }

    fn porte_modele(&mut self, o: &Ouv, clo: V3, chi: V3) {
        let ax = o.axis;
        let w = 1 - ax;
        let modele = texte(&o.spec, "modele").unwrap_or(DOOR_MODEL).to_string();
        if (self.opt.modele)(&modele.to_lowercase()).is_none() {
            self.fail(format!("porte {} : modèle {modele} absent", o.nom));
        }
        let largeur = chi[w] - clo[w];
        if !(48.0..=52.0).contains(&largeur) {
            self.warn.push(format!("porte {} : passage libre de {} pour un battant de 48 ({}) : cadre: true sur une baie de 64", o.nom, g(largeur), if largeur > 52.0 { "jour visible" } else { "le battant frotte" }));
        }
        let mut origine = [0.0; 3];
        origine[ax] = (o.lo[ax] + o.hi[ax]) / 2.0;
        origine[w] = clo[w];
        origine[2] = clo[2] + DOOR_HALF_H + 0.8;
        let lacet = if ax == 0 { 0 } else { 270 };
        let nom = texte(&o.spec, "nom").unwrap_or(&o.nom).to_string();
        self.entite(
            "prop_door_rotating",
            vec![
                ("targetname", nom),
                ("model", modele),
                ("origin", triplet(origine)),
                ("angles", format!("0 {lacet} 0")),
                ("spawnflags", "8192".into()),
                ("distance", "90".into()),
                ("speed", "100".into()),
                ("returndelay", "-1".into()),
                ("hardware", "1".into()),
                ("opendir", "0".into()),
                ("skin", "0".into()),
                ("spawnpos", "0".into()),
                ("health", "0".into()),
            ],
        );
        self.elements.push(El { typ: "porte".into(), nom: Some(o.nom.clone()), lo: clo, hi: chi, zone: Some(o.zone), ..Default::default() });
    }

    fn porte_solide(&mut self, o: &Ouv, clo: V3, chi: V3) {
        let ax = o.axis;
        let w = 1 - ax;
        let (mut lo, mut hi) = (clo, chi);
        // 8 d'épaisseur, sur la grille de 8
        lo[ax] = o.lo[ax] + ((o.hi[ax] - o.lo[ax] - 8.0) / 16.0).floor() * 8.0;
        hi[ax] = lo[ax] + 8.0;
        let nom = texte(&o.spec, "nom").unwrap_or(&o.nom).to_string();
        let ent = self.entite("func_door", vec![("targetname", nom), ("speed", "100".into()), ("wait", "-1".into()), ("lip", "4".into()), ("spawnflags", "256".into()), ("movedir", format!("0 {} 0", if w == 1 { 90 } else { 0 }))]);
        let mat = self.mat("porte", Some(o.zone), texte(&o.spec, "materiau_porte").map(str::to_string));
        self.boite(ent, lo, hi, &mat, &mat, None, 16, GRID_DETAIL);
    }

    /// Rien dans les 64 unités devant une porte ou un passage, des deux côtés (débattement et approche).
    fn controler_degagement_portes(&mut self) {
        for o in self.ouvertures.clone() {
            if !(o.typ == "porte" || o.typ == "passage") || o.axis == 2 {
                continue;
            }
            let (ax, w) = (o.axis, 1 - o.axis);
            let (clo, chi) = o.clear.unwrap_or((o.lo, o.hi));
            let (mut lo, mut hi) = (clo, chi);
            (lo[ax], hi[ax]) = (o.lo[ax] - 64.0, o.hi[ax] + 64.0);
            hi[2] = lo[2] + 72.0;
            for el in &self.elements {
                if ["cadre", "porte", "garde_corps", "dalle"].contains(&el.typ.as_str()) || el.ouverture.as_deref() == Some(&o.nom) {
                    continue;
                }
                if (0..3).all(|k| hi[k].min(el.hi[k]) - lo[k].max(el.lo[k]) > 0.0) {
                    self.warn.push(format!("{} {} : {} {} dans les 64 unités devant le passage ({}..{})", o.typ, o.nom, el.typ, el.nom.clone().unwrap_or_default(), g(lo[w]), g(hi[w])));
                }
            }
            for e in &self.entites {
                if !e.classe.starts_with("prop_") || e.classe == "prop_door_rotating" {
                    continue;
                }
                let val = |k: &str| e.cles.iter().find(|(c, _)| c == k).map(|(_, v)| v.clone());
                let Some(p) = val("origin").map(|v| v.split_whitespace().filter_map(|x| x.parse::<f64>().ok()).collect::<Vec<_>>()).filter(|p| p.len() == 3) else { continue };
                if (0..2).all(|k| p[k] >= lo[k] - 16.0 && p[k] <= hi[k] + 16.0) && p[2] >= lo[2] && p[2] <= hi[2] && p[2] < lo[2] + 64.0 {
                    self.warn.push(format!("{} {} : {} posé devant le passage", o.typ, o.nom, val("model").unwrap_or_default()));
                }
            }
        }
    }

    fn poser_plinthes(&mut self) {
        for zi in 0..self.zones.len() {
            let z = self.zones[zi].clone();
            let spec = get(&z.spec, "plinthe");
            if !vrai(spec) || z.exterieur() {
                continue;
            }
            let spec = spec.filter(|v| v.is_mapping()).cloned().unwrap_or(Value::Null);
            let (h, t) = (num(&spec, "hauteur", 16.0), num(&spec, "epaisseur", 8.0));
            let mat = self.mat("plinthe", Some(zi), texte(&spec, "materiau").map(str::to_string));
            let ent = self.ent_detail("plinthe", "func_detail");
            for c in ["sud", "nord", "ouest", "est"] {
                let (ax, sg) = cote(c).unwrap();
                let w = 1 - ax;
                let (mut a0, mut a1) = (z.lo[w], z.hi[w]);
                if w == 1 {
                    (a0, a1) = (a0 + t, a1 - t);
                }
                let face = if sg > 0 { z.hi[ax] } else { z.lo[ax] };
                let (mut blo, mut bhi) = (z.lo, z.hi);
                (blo[ax], bhi[ax]) = if sg > 0 { (face - t, face) } else { (face, face + t) };
                bhi[2] = z.lo[2] + h;
                let mut spans = vec![(a0, a1)];
                for o in &self.ouvertures {
                    if o.axis == ax && o.lo[2] <= z.lo[2] + h && ((o.lo[ax] - face).abs() < 1e-6 || (o.hi[ax] - face).abs() < 1e-6) {
                        spans = soustraire(spans, o.lo[w], o.hi[w]);
                    }
                }
                for el in &self.elements {
                    if el.lo[2] < bhi[2] && el.hi[2] > blo[2] && el.lo[ax] < bhi[ax] && el.hi[ax] > blo[ax] {
                        spans = soustraire(spans, el.lo[w], el.hi[w]);
                    }
                }
                for (s0, s1) in spans {
                    if s1 - s0 < 8.0 {
                        continue;
                    }
                    let (mut lo, mut hi) = (blo, bhi);
                    (lo[w], hi[w]) = (s0, s1);
                    self.boite(ent, lo, hi, &mat, &mat, None, 16, GRID_DETAIL);
                }
            }
        }
    }

    /// Pilastres, corniche et soubassement le long des murs d'une zone (base, champ, haut), puis alerte sur les
    /// murs extérieurs de plus de 512 sans rupture.
    fn poser_decor(&mut self) {
        for zi in 0..self.zones.len() {
            let z = self.zones[zi].clone();
            let brut = get(&z.spec, "decor").filter(|v| vrai(Some(*v))).cloned();
            let haut = if z.exterieur() { z.lo[2] + z.hm } else { z.hi[2] };
            let mut spec: Option<Value> = brut;
            if spec.is_some() && z.exterieur() && (z.englobe || z.hm <= 0.0) {
                self.warn.push(format!("zone {} : decor ignoré (pas de murs sous le ciel)", z.nom));
                spec = None;
            }
            let spec = spec.map(|s| if s.is_mapping() { s } else { Value::Mapping(Default::default()) });
            // (espacement, largeur, saillie) ; (hauteur, saillie)
            let (mut pil, mut cor, mut sou): (Option<[f64; 3]>, Option<[f64; 2]>, Option<[f64; 2]>) = (None, None, None);
            let (mut m_pil, mut m_cor, mut m_sou, mut ent) = (String::new(), String::new(), String::new(), 0usize);
            if let Some(s) = &spec {
                let partie = |k: &str| -> Option<Value> {
                    match s.as_mapping().and_then(|m| m.get(k)) {
                        Some(Value::Bool(false)) => None,
                        Some(v) if v.is_mapping() => Some(v.clone()),
                        _ => Some(Value::Null),
                    }
                };
                pil = partie("pilastres").map(|p| [num(&p, "espacement", 256.0), num(&p, "largeur", 32.0), num(&p, "saillie", 16.0)]);
                cor = partie("corniche").map(|p| [num(&p, "hauteur", 16.0), num(&p, "saillie", 24.0)]);
                sou = partie("soubassement").map(|p| [num(&p, "hauteur", 32.0), num(&p, "saillie", 8.0)]);
                if sou.is_some() && vrai(get(&z.spec, "plinthe")) {
                    self.fail(format!("zone {} : plinthe et soubassement se chevaucheraient : garder l'un des deux", z.nom));
                    continue;
                }
                let mats = materiaux(get(s, "materiaux"));
                m_pil = self.mat("pilastre", Some(zi), mats.get("pilastre").cloned());
                m_cor = self.mat("corniche", Some(zi), mats.get("corniche").cloned());
                m_sou = self.mat("soubassement", Some(zi), mats.get("soubassement").cloned());
                ent = self.ent_detail("decor", "func_detail");
            }
            for c in ["sud", "nord", "ouest", "est"] {
                let (ax, sg) = cote(c).unwrap();
                let w = 1 - ax;
                let face = if sg > 0 { z.hi[ax] } else { z.lo[ax] };
                let (a0, a1) = (z.lo[w], z.hi[w]);
                // (début, fin, bas, haut) le long du mur
                let trous: Vec<[f64; 4]> = self.ouvertures.iter().filter(|o| o.axis == ax && ((o.lo[ax] - face).abs() < 1e-6 || (o.hi[ax] - face).abs() < 1e-6)).map(|o| [o.lo[w], o.hi[w], o.lo[2], o.hi[2]]).collect();
                let pres: Vec<[f64; 4]> = self.elements.iter().filter(|el| el.zone == Some(zi) && el.lo[ax] < face + 32.0 && el.hi[ax] > face - 32.0).map(|el| [el.lo[w], el.hi[w], el.lo[2], el.hi[2]]).collect();
                let genes: Vec<[f64; 4]> = trous.iter().chain(&pres).copied().collect();
                let bande = |prof: f64, z0: f64, z1: f64, s0: f64, s1: f64| -> (V3, V3) {
                    let (mut lo, mut hi) = (z.lo, z.hi);
                    (lo[ax], hi[ax]) = if sg > 0 { (face - prof, face) } else { (face, face + prof) };
                    (lo[w], hi[w]) = (s0, s1);
                    (lo[2], hi[2]) = (z0, z1);
                    (lo, hi)
                };
                let mut pil_spans: Vec<(f64, f64)> = Vec::new();
                if spec.is_some() {
                    if let Some([esp, larg, saillie]) = pil {
                        let n = (arrondi((a1 - a0) / esp) as i64).max(1);
                        let demi = larg / 2.0;
                        for k in 1..n {
                            let centre = ((a0 + k as f64 * (a1 - a0) / n as f64) / 8.0).floor() * 8.0;
                            let (s0, s1) = (centre - demi, centre + demi);
                            if genes.iter().any(|h| s1 > h[0] - 8.0 && s0 < h[1] + 8.0) {
                                continue;
                            }
                            let ztop = haut - cor.map_or(0.0, |c| c[0]);
                            let (lo, hi) = bande(saillie, z.lo[2], ztop, s0, s1);
                            self.boite(ent, lo, hi, &m_pil, &m_pil, None, 16, GRID_DETAIL);
                            pil_spans.push((s0, s1));
                        }
                    }
                    for (est_sou, partie, m) in [(true, sou, m_sou.clone()), (false, cor, m_cor.clone())] {
                        let Some([hauteur, saillie]) = partie else { continue };
                        let (b0, b1) = if w == 1 { (a0 + saillie, a1 - saillie) } else { (a0, a1) };
                        let (z0, z1) = if est_sou { (z.lo[2], z.lo[2] + hauteur) } else { (haut - hauteur, haut) };
                        let mut spans = vec![(b0, b1)];
                        for h in &genes {
                            if h[2] < z1 && h[3] > z0 {
                                spans = soustraire(spans, h[0], h[1]);
                            }
                        }
                        if est_sou {
                            for (s0, s1) in &pil_spans {
                                spans = soustraire(spans, *s0, *s1);
                            }
                        }
                        for (s0, s1) in spans {
                            if s1 - s0 >= 8.0 {
                                let (lo, hi) = bande(saillie, z0, z1, s0, s1);
                                self.boite(ent, lo, hi, &m, &m, None, 16, GRID_DETAIL);
                            }
                        }
                    }
                }
                if z.exterieur() && !z.englobe && z.hm > 0.0 {
                    let mut spans = vec![(a0, a1)];
                    for h in &trous {
                        spans = soustraire(spans, h[0], h[1]);
                    }
                    for (s0, s1) in &pil_spans {
                        spans = soustraire(spans, *s0, *s1);
                    }
                    let plus_long = spans.iter().map(|(s0, s1)| s1 - s0).fold(0.0, f64::max);
                    if plus_long > 512.0 {
                        self.warn.push(format!("zone {}, mur {c} : {} unités sans rupture (> 512) : decor: (pilastres) ou ouvertures", z.nom, g(plus_long)));
                    }
                }
            }
        }
    }

    // --- lumières et entités
    fn lumieres_auto(&self, zi: usize) -> bool {
        match get(&self.zones[zi].spec, "lumiere") {
            None => true,
            Some(v) => v.as_str() == Some("auto") || v.is_mapping(),
        }
    }

    fn grille_lumieres(&mut self, zi: usize, lo: V3, hi: V3, z_lumiere: f64, z_fixation: f64, eclat: f64) {
        let cfg = get(&self.zones[zi].spec, "lumiere").filter(|v| v.is_mapping()).cloned().unwrap_or(Value::Null);
        let pas = num(&cfg, "espacement", 256.0);
        let couleur: Vec<String> = match get(&cfg, "couleur").and_then(Value::as_sequence) {
            Some(c) => c.iter().map(py_str).collect(),
            None => vec!["255".into(), "236".into(), "214".into()],
        };
        let eclat = num(&cfg, "intensite", eclat);
        let mut modele = texte(&cfg, "modele").unwrap_or(LIGHT_MODEL).to_string();
        if !modele.is_empty() && modele != "aucun" && (self.opt.modele)(&modele.to_lowercase()).and_then(|m| m.statique) != Some(true) {
            let nom = self.zones[zi].nom.clone();
            self.fail(format!("zone {nom} : luminaire {modele} absent ou refusé en prop_static"));
            modele = String::new();
        }
        let (nx, ny) = ((arrondi((hi[0] - lo[0]) / pas) as i64).max(1), (arrondi((hi[1] - lo[1]) / pas) as i64).max(1));
        for i in 0..nx {
            for j in 0..ny {
                let x = lo[0] + (i as f64 + 0.5) * (hi[0] - lo[0]) / nx as f64;
                let y = lo[1] + (j as f64 + 0.5) * (hi[1] - lo[1]) / ny as f64;
                let teinte = format!("{} {} {}", couleur.first().cloned().unwrap_or_default(), couleur.get(1).cloned().unwrap_or_default(), couleur.get(2).cloned().unwrap_or_default());
                if texte(&cfg, "type").unwrap_or("spot") == "spot" {
                    // demi-sphère vers le bas : le plafond ne reçoit que du rebond, pas de tache brûlée. `intensite`
                    // reste en équivalent omni : sans le rebond du plafond, il faut × 2,5 pour le même niveau au sol
                    self.entite(
                        "light_spot",
                        vec![
                            ("origin", format!("{} {} {}", g(x), g(y), g(z_fixation - 16.0))),
                            ("angles", "-90 0 0".into()),
                            ("pitch", "-90".into()),
                            ("_light", format!("{teinte} {}", g(arrondi(eclat * 2.5)))),
                            ("_lightHDR", "-1 -1 -1 1".into()),
                            ("_lightscaleHDR", "1".into()),
                            ("_inner_cone", "60".into()),
                            ("_cone", "85".into()),
                            ("_exponent", "1".into()),
                            ("_quadratic_attn", "1".into()),
                            ("style", "0".into()),
                        ],
                    );
                } else {
                    self.entite("light", vec![("origin", format!("{} {} {}", g(x), g(y), g(z_lumiere))), ("_light", format!("{teinte} {}", g(eclat))), ("_lightHDR", "-1 -1 -1 1".into()), ("_lightscaleHDR", "1".into()), ("_quadratic_attn", "1".into()), ("style", "0".into())]);
                }
                if !modele.is_empty() && modele != "aucun" {
                    self.entite("prop_static", vec![("model", modele.clone()), ("origin", format!("{} {} {}", g(x), g(y), g(z_fixation))), ("angles", "0 0 0".into()), ("solid", "0".into()), ("disableshadows", "1".into()), ("skin", "0".into())]);
                }
            }
        }
    }

    fn poser_lumieres(&mut self) {
        let mut ciel = false;
        for zi in 0..self.zones.len() {
            let z = self.zones[zi].clone();
            if z.exterieur() {
                ciel = true;
                continue;
            }
            if z.plein {
                continue;
            }
            if self.lumieres_auto(zi) {
                // 32 sous le plafond : à 16, chaque plafonnier brûle une tache blanche au plafond
                self.grille_lumieres(zi, z.lo, z.hi, z.hi[2] - 32.0, z.hi[2], arrondi(120.0 * z.taille()[2] / 128.0));
            }
            self.cubemap(zi);
        }
        let deja = get(self.spec, "entites").and_then(Value::as_sequence).map_or(false, |l| l.iter().any(|e| texte(e, "classe") == Some("light_environment")));
        if ciel && !deja {
            let zi = self.zones.iter().position(Zone::exterieur).unwrap();
            let z = self.zones[zi].clone();
            let soleil = get(self.spec, "soleil").cloned().unwrap_or(Value::Null);
            let brut = |k: &str, d: &str| get(&soleil, k).map(py_str).unwrap_or_else(|| d.to_string());
            self.entite(
                "light_environment",
                vec![
                    ("origin", format!("{} {} {}", g((z.lo[0] + z.hi[0]) / 2.0), g((z.lo[1] + z.hi[1]) / 2.0), g(z.hi[2] - 32.0))),
                    ("angles", format!("0 {} 0", brut("cap", "35"))),
                    ("pitch", match get(&soleil, "hauteur") {
                        Some(Value::Number(n)) => match n.as_i64() {
                            Some(i) => (-i).to_string(),
                            None => py_str(&Value::Number(serde_yaml::Number::from(-n.as_f64().unwrap_or(50.0)))),
                        },
                        _ => "-50".to_string(),
                    }),
                    ("_light", brut("lumiere", "255 244 226 280")),
                    ("_ambient", brut("ambiante", "140 160 190 70")),
                    ("_lightHDR", "-1 -1 -1 1".into()),
                    ("_ambientHDR", "-1 -1 -1 1".into()),
                    ("_lightscaleHDR", "1".into()),
                    ("_AmbientScaleHDR", "1".into()),
                    ("SunSpreadAngle", brut("etalement", "5")),
                ],
            );
            self.cubemap(zi);
        }
    }

    /// Cubemap de la zone : au centre, 64 au-dessus du sol ; `cubemap: [x, y, z]` la déplace, `cubemap: false`
    /// la retire.
    fn cubemap(&mut self, zi: usize) {
        let z = &self.zones[zi];
        let p = match get(&z.spec, "cubemap") {
            Some(Value::Bool(false)) => return,
            Some(v) if v.is_sequence() => {
                let c = nombres(Some(v));
                plus(z.lo, [c.first().copied().unwrap_or(0.0), c.get(1).copied().unwrap_or(0.0), c.get(2).copied().unwrap_or(0.0)])
            }
            _ => [(z.lo[0] + z.hi[0]) / 2.0, (z.lo[1] + z.hi[1]) / 2.0, z.lo[2] + 64.0],
        };
        self.entite("env_cubemap", vec![("origin", format!("{} {} {}", g(p[0]), g(p[1]), g(p[2]))), ("cubemapsize", "0".into())]);
    }

    fn poser_entites(&mut self) {
        let mut apparitions = 0;
        let liste: Vec<Value> = get(self.spec, "entites").and_then(Value::as_sequence).cloned().unwrap_or_default();
        for (i, e) in liste.iter().enumerate() {
            if !self.retenu(e) {
                continue;
            }
            let classe = texte(e, "classe").unwrap_or("None").to_string();
            let label = format!("entité {i} ({classe})");
            let zi = self.zone_de(e, &label);
            let o = self.origine_de(zi);
            let pos = get(e, "position");
            let au_centre = pos.map_or(true, |p| p.as_str() == Some("centre"));
            let liste_pos = nombres(pos);
            let mut p = match (au_centre, zi) {
                (true, Some(i)) => {
                    let z = &self.zones[i];
                    [(z.lo[0] + z.hi[0]) / 2.0 - o[0], (z.lo[1] + z.hi[1]) / 2.0 - o[1], 0.0]
                }
                _ => [liste_pos.first().copied().unwrap_or(0.0), liste_pos.get(1).copied().unwrap_or(0.0), liste_pos.get(2).copied().unwrap_or(0.0)],
            };
            let modele = texte(e, "modele").map(str::to_string);
            let connu = modele.as_ref().map(|m| (self.opt.modele)(&m.to_lowercase()));
            if modele.is_some() && (au_centre || liste_pos.len() < 3) {
                // posé au sol : le bas du modèle touche le sol
                p[2] += connu.flatten().map_or(0.0, |m| (-m.bas).max(0.0));
            }
            let n = num(e, "nombre", 1.0) as i64;
            // rangées de 64 le long de la zone, tant qu'un hull de 32 tient avant le mur est
            let colonnes = match zi {
                None => n,
                Some(i) => n.min(((self.zones[i].hi[0] - 16.0 - (o[0] + p[0])) / 64.0).floor() as i64 + 1).max(1),
            };
            for k in 0..n {
                let rang = [64.0 * (k % colonnes) as f64, 64.0 * (k / colonnes) as f64, 0.0];
                let q = if zi.is_none() { self.vers_monde(plus(p, rang)) } else { plus(plus(o, p), rang) };
                if let Some(i) = zi {
                    let z = &self.zones[i];
                    if (0..2).any(|a| q[a] - 16.0 < z.lo[a] || q[a] + 16.0 > z.hi[a]) || !(z.lo[2] <= q[2] && q[2] < z.hi[2]) {
                        let nom = z.nom.clone();
                        self.fail(format!("{label} : n° {} en {} sort de la zone {nom} (réduire nombre ou déplacer position)", k + 1, liste_py(q)));
                        break;
                    }
                }
                let mut kv: Vec<(String, String)> = get(e, "cles").and_then(Value::as_mapping).map(|m| m.iter().filter_map(|(c, v)| Some((c.as_str()?.to_string(), py_str(v)))).collect()).unwrap_or_default();
                let fixer = |kv: &mut Vec<(String, String)>, c: &str, v: String, force: bool| match kv.iter_mut().find(|(k, _)| k == c) {
                    Some(x) if force => x.1 = v,
                    Some(_) => {}
                    None => kv.push((c.to_string(), v)),
                };
                fixer(&mut kv, "origin", triplet(q), true);
                let angle = match (zi, self.repere.1) {
                    // dans un repère tourné, l'angle de l'entité tourne avec lui
                    (None, a) if a != 0 => g((num(e, "angle", 0.0) + a as f64).rem_euclid(360.0)),
                    _ => get(e, "angle").map(py_str).unwrap_or_else(|| "0".into()),
                };
                // `angle: aucun` : une entité sans orientation (une lumière, un son) ne porte pas la clé
                if texte(e, "angle") != Some("aucun") {
                    fixer(&mut kv, "angles", format!("0 {angle} 0"), false);
                }
                if let Some(m) = &modele {
                    if connu == Some(None) {
                        self.fail(format!("{label} : modèle {m} absent (voir data/modeles_disponibles.tsv)"));
                    } else if classe == "prop_static" && connu.flatten().and_then(|x| x.statique) == Some(false) && k == 0 {
                        self.fail(format!("{label} : vbsp refuse {m} en prop_static (data/modeles_statiques.tsv) : prendre prop_dynamic ou un autre modèle"));
                    }
                    fixer(&mut kv, "model", m.clone(), true);
                    if classe == "prop_static" {
                        fixer(&mut kv, "solid", "6".into(), false);
                    }
                }
                self.entites.push(Entite { classe: classe.clone(), cles: kv, solides: Vec::new() });
                if classe == "info_player_start" {
                    apparitions += 1;
                }
            }
        }
        if apparitions == 0 && !vrai(get(self.spec, "module")) {
            self.fail("aucun info_player_start dans entites:".to_string());
        }
    }

    // --- tout
    fn lancer(&mut self) -> bool {
        let ciel = texte(self.spec, "ciel").unwrap_or("sky_day01_05").to_string();
        // {ciel : vrai si vbsp peut lui faire une cubemap par défaut}
        let connus: HashMap<&str, bool> = CIELS.lines().filter(|l| !l.is_empty() && !l.starts_with('#')).map(|l| l.split('\t').collect::<Vec<_>>()).map(|c| (c[0], c.get(2) == Some(&"oui"))).collect();
        match connus.get(ciel.as_str()) {
            None => self.fail(format!("ciel '{ciel}' absent de data/ciels_disponibles.tsv")),
            Some(false) => self.warn.push(format!("ciel '{ciel}' : pas de cubemap par défaut ; faire buildcubemaps en jeu (sinon reflets faux sur les sols)")),
            Some(true) => {}
        }
        self.cles_monde = vec![("skyname".into(), ciel), ("detailmaterial".into(), "detail/detailsprites".into()), ("detailvbsp".into(), "detail.vbsp".into()), ("maxpropscreenwidth".into(), "-1".into())];
        for k in ["murs", "dalles"] {
            self.sur_grille(&[num(self.spec, k, 16.0)], GRID_WORLD, k);
        }
        self.reglages();
        self.resoudre_zones();
        self.controler_paires();
        self.resoudre_ouvertures();
        if !self.err.is_empty() {
            return false;
        }
        if !self.zones.is_empty() {
            self.creuser();
            self.ecrire_coque();
        }
        self.construire_elements();
        self.finir_garde_corps();
        self.habiller_ouvertures();
        self.poser_plinthes();
        self.poser_decor();
        self.poser_lumieres();
        self.poser_entites();
        self.controler_degagement_portes();
        self.err.is_empty()
    }

    fn plan(&self) -> serde_json::Value {
        use serde_json::json;
        let mut niveaux: Vec<f64> = self.zones.iter().map(|z| z.lo[2]).chain(self.elements.iter().filter(|el| el.typ == "dalle").map(|el| el.hi[2])).collect();
        niveaux.sort_by(f64::total_cmp);
        niveaux.dedup();
        let brut = |k: &str| get(self.spec, k).and_then(|v| serde_json::to_value(v).ok()).unwrap_or_else(|| json!([]));
        json!({
            "carte": texte(self.spec, "carte"),
            "phase": self.phase,
            "murs": self.t,
            "dalles": self.s,
            "niveaux": niveaux,
            "zones": self.zones.iter().map(|z| json!({"nom": z.nom, "type": z.typ, "englobe": z.englobe, "lo": z.lo, "hi": z.hi, "hauteur_murs": if z.exterieur() { json!(z.hm) } else { json!(null) }, "role": z.role})).collect::<Vec<_>>(),
            "ouvertures": self.ouvertures.iter().map(|o| json!({"nom": o.nom, "type": o.typ, "entre": o.entre, "lo": o.lo, "hi": o.hi})).collect::<Vec<_>>(),
            "elements": self.elements.iter().map(|el| json!({"type": el.typ, "nom": el.nom, "lo": el.lo, "hi": el.hi, "direction": el.direction})).collect::<Vec<_>>(),
            "contenu": brut("contenu"),
            "kit": brut("kit"),
        })
    }

    /// Le modèle sémantique de ce qui vient d'être construit : chaque pièce et chaque couloir, avec son rôle.
    fn sem(&self) -> crate::Modele {
        let mut m = crate::Modele { version: crate::VERSION, carte: texte(self.spec, "carte").unwrap_or("").to_string(), source: "hm construire".to_string(), ..Default::default() };
        for z in self.zones.iter().filter(|z| !z.exterieur() && !z.plein) {
            let role = match (&z.role, z.typ.as_str()) {
                (Some(r), _) => r.clone(),
                (None, "couloir") => "circulation".to_string(),
                (None, _) => "vide".to_string(),
            };
            m.pieces.push(crate::Piece {
                id: z.nom.replace(char::is_whitespace, "_"),
                role,
                role_source: if z.role.is_some() { "spec".into() } else { format!("type {}", z.typ) },
                emprise: [z.lo[0] as f32, z.lo[1] as f32, z.hi[0] as f32, z.hi[1] as f32],
                sol: z.lo[2] as f32,
                hauteur: Some(z.taille()[2] as f32),
                batiment: String::new(),
                vide_expres: z.role.as_deref() == Some("vide"),
                amenagement: String::new(),
            });
        }
        // les toits que la spec déclare, avec ce qui les porte : la règle « toit sans appui » les jugera
        let prefixe = self.variante.clone().or_else(|| texte(self.spec, "carte").map(str::to_string)).unwrap_or_default();
        for t in get(self.spec, "toits").and_then(Value::as_sequence).into_iter().flatten() {
            let b = nombres(get(t, "boite"));
            if b.len() != 6 {
                continue;
            }
            let (lo, hi) = bornes(&[self.vers_monde([b[0], b[1], b[2]]), self.vers_monde([b[3], b[4], b[5]])]);
            let appuis = get(t, "appuis").and_then(Value::as_sequence).into_iter().flatten().map(|a| nombres(Some(a))).filter(|a| a.len() == 3).map(|a| self.vers_monde([a[0], a[1], a[2]]).map(|x| x as f32)).collect();
            m.toits.push(crate::Toit { id: format!("{prefixe}.{}", texte(t, "nom").unwrap_or("toit")), boite: [lo[0] as f32, lo[1] as f32, lo[2] as f32, hi[0] as f32, hi[1] as f32, hi[2] as f32], appuis });
        }
        // les blocs nommés de la spec : dans le VMF ce sont des func_detail sans nom, et une règle de rôle qui
        // cherche « un comptoir » ne les verrait pas
        for el in self.elements.iter().filter(|el| el.typ == "bloc") {
            if let Some(nom) = &el.nom {
                let c = [(el.lo[0] + el.hi[0]) / 2.0, (el.lo[1] + el.hi[1]) / 2.0, (el.lo[2] + el.hi[2]) / 2.0];
                m.objets.push(crate::ObjetDeclare { nom: nom.clone(), pos: [c[0] as f32, c[1] as f32, c[2] as f32] });
            }
        }
        m
    }

    // --- écriture du VMF, dans la forme de Hammer
    fn ecrire(&self) -> String {
        let mut s = String::with_capacity(1 << 18);
        s += "versioninfo\n{\n\t\"editorversion\" \"400\"\n\t\"editorbuild\" \"5304\"\n\t\"mapversion\" \"1\"\n\t\"formatversion\" \"100\"\n\t\"prefab\" \"0\"\n}\n";
        s += "visgroups\n{\n}\nviewsettings\n{\n\t\"bSnapToGrid\" \"1\"\n\t\"bShowGrid\" \"1\"\n\t\"bShowLogicalGrid\" \"0\"\n\t\"nGridSpacing\" \"64\"\n\t\"bShow3DGrid\" \"0\"\n}\n";
        let solide = |s: &mut String, sol: &Solide, t: &str| {
            let (id_solide, mut id_face, faces) = (sol.id, sol.face0, &sol.faces);
            *s += &format!("{t}solid\n{t}{{\n{t}\t\"id\" \"{id_solide}\"\n");
            for f in faces {
                id_face += 1;
                let p = points_de_face(&f.poly, f.n);
                let pt = |v: V3| format!("({} {} {})", coord(v[0]), coord(v[1]), coord(v[2]));
                let (ua, va) = match f.uv {
                    Some((u, du, eu, v, dv, ev)) => {
                        // décalage et échelle écrits en entier, pas sur six chiffres : 219.4286, 0.164062
                        let n = |x: f64| if x == 0.0 { "0".to_string() } else { format!("{x}") };
                        (format!("[{} {} {} {}] {}", g(u[0]), g(u[1]), g(u[2]), n(du), n(eu)), format!("[{} {} {} {}] {}", g(v[0]), g(v[1]), g(v[2]), n(dv), n(ev)))
                    }
                    None => {
                        let (u, v) = axes_uv(f.n);
                        (format!("[{} {} {} 0] {}", u[0], u[1], u[2], g(f.echelle)), format!("[{} {} {} 0] {}", v[0], v[1], v[2], g(f.echelle)))
                    }
                };
                *s += &format!(
                    "{t}\tside\n{t}\t{{\n{t}\t\t\"id\" \"{id_face}\"\n{t}\t\t\"plane\" \"{} {} {}\"\n{t}\t\t\"material\" \"{}\"\n{t}\t\t\"uaxis\" \"{ua}\"\n{t}\t\t\"vaxis\" \"{va}\"\n{t}\t\t\"rotation\" \"0\"\n{t}\t\t\"lightmapscale\" \"{}\"\n{t}\t\t\"smoothing_groups\" \"0\"\n{t}\t}}\n",
                    pt(p[0]), pt(p[1]), pt(p[2]), f.mat, f.lm
                );
            }
            *s += &format!("{t}\teditor\n{t}\t{{\n{t}\t\t\"color\" \"255 255 255\"\n{t}\t\t\"visgroupshown\" \"1\"\n{t}\t\t\"visgroupautoshown\" \"1\"\n{t}\t}}\n{t}}}\n");
        };
        let mut cles: BTreeMap<&str, &str> = self.cles_monde.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        cles.insert("classname", "worldspawn");
        cles.insert("mapversion", "1");
        s += "world\n{\n\t\"id\" \"1\"\n";
        for (k, v) in &cles {
            s += &format!("\t\"{k}\" \"{v}\"\n");
        }
        for faces in &self.monde {
            solide(&mut s, faces, "\t");
        }
        s += "\teditor\n\t{\n\t\t\"color\" \"255 255 255\"\n\t}\n}\n";
        for (i, e) in self.entites.iter().enumerate() {
            let id = i + 2;
            let mut cles: BTreeMap<&str, &str> = e.cles.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
            cles.insert("classname", &e.classe);
            s += &format!("entity\n{{\n\t\"id\" \"{id}\"\n");
            for (k, v) in &cles {
                s += &format!("\t\"{k}\" \"{v}\"\n");
            }
            for faces in &e.solides {
                solide(&mut s, faces, "\t");
            }
            s += &format!("\teditor\n\t{{\n\t\t\"color\" \"255 255 255\"\n\t\t\"visgroupshown\" \"1\"\n\t\t\"visgroupautoshown\" \"1\"\n\t\t\"logicalpos\" \"[0 {id}]\"\n\t}}\n}}\n");
        }
        s += "cameras\n{\n\t\"activecamera\" \"-1\"\n}\ncordons\n{\n\t\"active\" \"0\"\n}\n";
        s
    }
}

fn bornes(pts: &[V3]) -> (V3, V3) {
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for p in pts {
        for a in 0..3 {
            lo[a] = lo[a].min(p[a]);
            hi[a] = hi[a].max(p[a]);
        }
    }
    (lo, hi)
}

fn triplet(v: V3) -> String {
    format!("{} {} {}", g(v[0]), g(v[1]), g(v[2]))
}

/// Une coordonnée de plan : arrondie au millième, entière si elle l'est.
fn coord(c: f64) -> String {
    let r = (c * 1000.0).round() / 1000.0;
    g(if r == 0.0 { 0.0 } else { r })
}

/// Motif à jokers `*` et `?`, sensible à la casse.
fn motif(m: &str, nom: &str) -> bool {
    fn va(m: &[u8], n: &[u8]) -> bool {
        match (m.first(), n.first()) {
            (None, None) => true,
            (Some(b'*'), _) => va(&m[1..], n) || (!n.is_empty() && va(m, &n[1..])),
            (Some(b'?'), Some(_)) => va(&m[1..], &n[1..]),
            (Some(a), Some(b)) if a == b => va(&m[1..], &n[1..]),
            _ => false,
        }
    }
    va(m.as_bytes(), nom.as_bytes())
}

/// Lit une spec YAML.
pub fn lire_spec(chemin: &Path) -> anyhow::Result<Value> {
    use anyhow::Context;
    let texte = std::fs::read_to_string(chemin).with_context(|| format!("lecture de {}", chemin.display()))?;
    serde_yaml::from_str(&texte).with_context(|| format!("{}", chemin.display()))
}

/// Construit la map décrite par la spec. Avec une erreur, rien n'est rendu à écrire.
pub fn construire(spec: &Value, opt: &Options) -> Construction {
    let mut gen = Gen::new(spec, opt);
    let ok = gen.lancer();
    let mut c = Construction { erreurs: gen.err.clone(), avertissements: gen.warn.clone(), phase: gen.phase.clone(), zones: gen.zones.len(), ouvertures: gen.ouvertures.len(), ..Default::default() };
    if ok {
        c.brosses = gen.monde.len() + gen.entites.iter().map(|e| e.solides.len()).sum::<usize>();
        c.entites = gen.entites.len();
        c.vmf = gen.ecrire();
        c.plan = gen.plan();
        c.sem = gen.sem();
        c.modules = gen.modules.clone();
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Les nombres s'écrivent comme Python les écrit : c'est ce qui rend le VMF identique à celui du skill.
    #[test]
    fn les_nombres_s_ecrivent_comme_en_python() {
        // f"{x:g}"
        for (x, attendu) in [(0.0, "0"), (72.0, "72"), (-8.0, "-8"), (0.107169, "0.107169"), (55.0, "55"), (0.25, "0.25"), (1234.5, "1234.5"), (123456.7, "123457"), (1000000.0, "1e+06"), (0.0001, "0.0001"), (0.00001, "1e-05"), (-0.5, "-0.5"), (16384.0, "16384")] {
            assert_eq!(g(x), attendu, "g({x})");
        }
        // round() : le demi va au pair
        for (x, attendu) in [(0.5, 0.0), (1.5, 2.0), (2.5, 2.0), (3.5, 4.0), (-0.5, 0.0), (-1.5, -2.0), (2.4, 2.0), (2.6, 3.0), (150.0, 150.0)] {
            assert_eq!(arrondi(x), attendu, "arrondi({x})");
        }
        // str() d'une valeur de la spec
        let v: Value = serde_yaml::from_str("[255, 1.5, 2.0, true, texte]").unwrap();
        let ecrits: Vec<String> = v.as_sequence().unwrap().iter().map(py_str).collect();
        assert_eq!(ecrits, ["255", "1.5", "2.0", "True", "texte"]);
    }

    #[test]
    fn l_enveloppe_convexe_d_un_coin_a_cinq_faces() {
        // une rampe : six points, deux triangles et trois quadrilatères, toutes les normales vers le dehors
        let pts = [[0.0, 0.0, 0.0], [128.0, 0.0, 0.0], [128.0, 0.0, 64.0], [0.0, 96.0, 0.0], [128.0, 96.0, 0.0], [128.0, 96.0, 64.0]];
        let faces = faces_enveloppe(&pts);
        assert_eq!(faces.len(), 5);
        let mut tailles: Vec<usize> = faces.iter().map(|(_, p)| p.len()).collect();
        tailles.sort();
        assert_eq!(tailles, [3, 3, 4, 4, 4]);
        let centre = [85.0, 48.0, 21.0];
        for (n, poly) in &faces {
            assert!(scal(*n, moins(poly[0], centre)) > 0.0, "normale {n:?} tournée vers le dedans");
            // les trois points du plan tournent dans le sens du VMF : leur produit vectoriel regarde vers le dehors
            let p = points_de_face(poly, *n);
            assert!(scal(croix(moins(p[2], p[0]), moins(p[1], p[0])), *n) > 0.0);
        }
    }
}
