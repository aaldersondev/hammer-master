//! Matériaux : ce que le rendu simplifié a besoin de savoir d'un VMT.
use std::collections::HashMap;

use vformats::Limits;

use crate::vfs::Vfs;

/// Rôle d'un matériau d'outil : jamais dessiné en jeu, affichable ici en couleur codée.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Nodraw,
    Sky,
    Clip,
    Trigger,
    Areaportal,
    Hint,
    Invisible,
    Other,
}

#[derive(Clone, Debug)]
pub struct Material {
    /// Chemin sous `materials/`, sans extension, en minuscules.
    pub name: String,
    pub found: bool,
    /// Indice du point de montage du VMT.
    pub mount: Option<usize>,
    pub shader: String,
    pub base: Option<String>,
    pub base2: Option<String>,
    pub translucent: bool,
    pub alphatest: bool,
    pub alpha_ref: f32,
    pub additive: bool,
    pub nocull: bool,
    pub selfillum: bool,
    pub unlit: bool,
    pub color: [f32; 3],
    pub alpha: f32,
    /// Force du reflet d'un vitrage (0 = aucun). En jeu, un verre presque transparent se voit par le reflet de son
    /// `$envmap` ; sans ce voile, le rendu de contrôle le montrerait comme un trou.
    pub reflet: f32,
    /// Porte une carte de relief (`$bumpmap`, `$ssbump`) : sa lumière est stockée quatre fois.
    pub bump: bool,
    pub tool: Option<Tool>,
    /// Échelle d'un décal (`$decalscale`, 1 par défaut) : sa taille en unités est celle de sa texture fois celle-ci.
    pub decalscale: f32,
    /// Reflet de `$envmap` ; l'eau en a toujours un.
    pub envmap: Option<Envmap>,
}

/// Reflet d'une cubemap (wiki Valve, « $envmap », « $envmapmask »).
#[derive(Clone, Debug, PartialEq)]
pub struct Envmap {
    /// `$envmaptint` (`$reflecttint` pour l'eau).
    pub teinte: [f32; 3],
    pub masque: MasqueEnvmap,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MasqueEnvmap {
    Aucun,
    /// `$basealphaenvmapmask` : l'alpha de la texture de base, inversé sur les brosses (transparent = reflet).
    AlphaBase,
    /// `$normalmapalphaenvmapmask` : l'alpha de cette carte de normales.
    AlphaNormales(String),
    /// `$envmapmask` (sans `$bumpmap`, qui l'annule) : cette texture, teinte par texel.
    Texture(String),
}

impl Material {
    pub fn missing(name: &str) -> Self {
        Self {
            name: name.to_string(),
            found: false,
            mount: None,
            shader: String::new(),
            base: None,
            base2: None,
            translucent: false,
            alphatest: false,
            alpha_ref: 0.5,
            additive: false,
            nocull: false,
            selfillum: false,
            unlit: false,
            color: [1.0, 1.0, 1.0],
            alpha: 1.0,
            reflet: 0.0,
            bump: false,
            tool: tool_of(name),
            decalscale: 1.0,
            envmap: None,
        }
    }
}

pub fn normalize(name: &str) -> String {
    let mut s = name.replace('\\', "/").to_ascii_lowercase();
    while s.starts_with('/') {
        s.remove(0);
    }
    if let Some(r) = s.strip_prefix("materials/") {
        s = r.to_string();
    }
    for ext in [".vmt", ".vtf"] {
        if let Some(r) = s.strip_suffix(ext) {
            s = r.to_string();
        }
    }
    s
}

fn tool_of(name: &str) -> Option<Tool> {
    let n = name.strip_prefix("tools/")?;
    if n.starts_with("toolsblack") {
        return None; // un vrai noir, dessiné en jeu
    }
    Some(if n.contains("skybox") {
        Tool::Sky
    } else if n.contains("nodraw") {
        Tool::Nodraw
    } else if n.contains("clip") {
        Tool::Clip
    } else if n.contains("trigger") {
        Tool::Trigger
    } else if n.contains("areaportal") {
        Tool::Areaportal
    } else if n.contains("hint") || n.contains("skip") {
        Tool::Hint
    } else if n.contains("invisible") {
        Tool::Invisible
    } else {
        Tool::Other
    })
}

fn flag(v: Option<&String>) -> bool {
    v.map_or(false, |s| {
        let s = s.trim();
        !(s.is_empty() || s == "0" || s.eq_ignore_ascii_case("false"))
    })
}

fn color(v: &str) -> Option<[f32; 3]> {
    let floats: Vec<f32> = v.split(|c: char| c.is_whitespace() || matches!(c, '[' | ']' | '{' | '}')).filter_map(|p| p.parse().ok()).collect();
    let scale = if v.contains('{') { 1.0 / 255.0 } else { 1.0 };
    match floats.len() {
        1 => Some([floats[0] * scale; 3]),
        n if n >= 3 => Some([floats[0] * scale, floats[1] * scale, floats[2] * scale]),
        _ => None,
    }
}

/// Paramètres d'un VMT, les matériaux `patch` étant suivis jusqu'à leur base.
fn params(vfs: &Vfs, name: &str, depth: usize) -> Option<(String, HashMap<String, String>, usize)> {
    let found = vfs.read(&format!("materials/{name}.vmt"))?;
    let text = String::from_utf8_lossy(&found.bytes);
    let doc = vformats::vmt::parse(&text, &Limits::default()).ok()?;
    if let Some(patch) = doc.patch() {
        if depth < 8 {
            if let Some((shader, mut map, _)) = params(vfs, &normalize(&patch.include), depth + 1) {
                for o in &patch.overrides {
                    map.insert(o.key.to_ascii_lowercase(), o.value.to_string());
                }
                return Some((shader, map, found.mount));
            }
        }
        return None;
    }
    let mut map = HashMap::new();
    for pair in &doc.kv.pairs {
        if let Some(v) = pair.value.as_str() {
            map.entry(pair.key.to_ascii_lowercase()).or_insert_with(|| v.to_string());
        }
    }
    Some((doc.shader.to_ascii_lowercase(), map, found.mount))
}

/// Charge un matériau par son nom (`megalopole/road`, avec ou sans `materials/` et extension).
pub fn load_material(vfs: &Vfs, name: &str) -> Material {
    let name = normalize(name);
    let mut m = Material::missing(&name);
    let Some((shader, p, mount)) = params(vfs, &name, 0) else { return m };
    m.found = true;
    m.mount = Some(mount);
    m.base = p.get("$basetexture").map(|s| normalize(s)).filter(|s| !s.is_empty());
    m.base2 = p.get("$basetexture2").map(|s| normalize(s)).filter(|s| !s.is_empty());
    m.translucent = flag(p.get("$translucent"));
    m.alphatest = flag(p.get("$alphatest"));
    m.alpha_ref = p.get("$alphatestreference").and_then(|v| v.trim().parse().ok()).unwrap_or(0.5);
    m.additive = flag(p.get("$additive"));
    m.nocull = flag(p.get("$nocull"));
    m.selfillum = flag(p.get("$selfillum"));
    m.unlit = shader.starts_with("unlit") || shader == "sprite" || shader == "sky";
    if let Some(c) = p.get("$color").or_else(|| p.get("$color2")).and_then(|v| color(v)) {
        m.color = c;
    }
    m.alpha = p.get("$alpha").and_then(|v| v.trim().parse().ok()).unwrap_or(1.0);
    // un Sprite (icône de l'éditeur, comme `editor/light`) : découpé par l'alpha de sa texture, comme Hammer
    // dessine les `iconsprite`
    if shader == "sprite" && !m.additive && !m.translucent {
        m.alphatest = true;
    }
    if shader.starts_with("water") || shader == "refract" {
        m.translucent = true;
        match p.get("$fogcolor").and_then(|v| color(v)).filter(|_| m.base.is_none() && shader.starts_with("water")) {
            // l'eau comme la montre Hammer++ : opaque et sombre, de la couleur de son brouillard (vu le 15/11 sur
            // rp_rockford : nous la faisions bleu-vert translucide, le fond nodraw au travers)
            Some(c) => {
                m.translucent = false;
                m.color = c.map(|x| if x <= 0.04045 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) });
            }
            None if m.base.is_none() => {
                m.color = [0.10, 0.22, 0.30];
                m.alpha = 0.75;
            }
            None => {}
        }
    }
    m.reflet = reflet_de_vitrage(&name, &p, m.translucent && !m.additive);
    m.bump = p.get("$bumpmap").map_or(false, |s| !s.trim().is_empty());
    m.decalscale = p.get("$decalscale").and_then(|v| v.trim().parse::<f32>().ok()).filter(|v| *v > 0.0).unwrap_or(1.0);
    let a_envmap = p.get("$envmap").map_or(false, |s| !s.trim().is_empty());
    if a_envmap || shader.starts_with("water") {
        let texture = |k: &str| p.get(k).map(|s| normalize(s)).filter(|s| !s.is_empty());
        let bump = texture("$bumpmap").or_else(|| texture("$normalmap"));
        let masque = match (flag(p.get("$normalmapalphaenvmapmask")), &bump, texture("$envmapmask")) {
            (true, Some(b), _) => MasqueEnvmap::AlphaNormales(b.clone()),
            (_, None, Some(t)) => MasqueEnvmap::Texture(t),
            _ if flag(p.get("$basealphaenvmapmask")) => MasqueEnvmap::AlphaBase,
            _ => MasqueEnvmap::Aucun,
        };
        let cle_teinte = if shader.starts_with("water") { "$reflecttint" } else { "$envmaptint" };
        m.envmap = Some(Envmap { teinte: p.get(cle_teinte).and_then(|v| color(v)).unwrap_or([1.0; 3]), masque });
    }
    m.shader = shader;
    m
}

/// Un matériau translucide est un vitrage s'il se déclare en verre (`$surfaceprop`) ou, avec un `$envmap`, s'il en
/// porte le nom. Le reflet suit `$envmaptint` : c'est lui qui rend la vitre visible en jeu.
fn reflet_de_vitrage(name: &str, p: &HashMap<String, String>, translucide: bool) -> f32 {
    if !translucide {
        return 0.0;
    }
    let verre = p.get("$surfaceprop").map_or(false, |s| s.to_ascii_lowercase().contains("glass"));
    let envmap = p.get("$envmap").map_or(false, |s| !s.trim().is_empty());
    let nom = ["glass", "verre", "vitre", "window"].iter().any(|m| name.contains(m));
    if !(verre || (envmap && nom)) {
        return 0.0;
    }
    if !envmap {
        return 0.22;
    }
    let teinte = p.get("$envmaptint").and_then(|v| color(v)).unwrap_or([1.0; 3]);
    let lum = 0.30 * teinte[0] + 0.59 * teinte[1] + 0.11 * teinte[2];
    (0.25 + 0.8 * lum).clamp(0.22, 0.70)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noms() {
        assert_eq!(normalize("Materials\\Megalopole\\Road.VMT"), "megalopole/road");
        assert_eq!(normalize("megalopole/road"), "megalopole/road");
        assert_eq!(tool_of("tools/toolsnodraw"), Some(Tool::Nodraw));
        assert_eq!(tool_of("tools/toolsskybox2d"), Some(Tool::Sky));
        assert_eq!(tool_of("tools/toolsblack"), None);
        assert_eq!(tool_of("megalopole/road"), None);
        assert_eq!(color("[0.5 0.25 1]"), Some([0.5, 0.25, 1.0]));
        assert_eq!(color("{255 0 0}"), Some([1.0, 0.0, 0.0]));
    }

    #[test]
    fn eau_opaque_de_la_couleur_de_son_brouillard() {
        let dir = std::env::temp_dir().join(format!("hm_eau_{}", std::process::id()));
        std::fs::create_dir_all(dir.join("materials/essai")).unwrap();
        std::fs::write(dir.join("materials/essai/eau.vmt"), "\"Water\" { \"$fogcolor\" \"{8 16 19}\" \"$normalmap\" \"x\" }").unwrap();
        std::fs::write(dir.join("materials/essai/eau_bon_marche.vmt"), "\"Water\" { \"$basetexture\" \"essai/b\" \"$fogcolor\" \"{8 16 19}\" }").unwrap();
        let mut vfs = Vfs::new();
        vfs.add_dir(&dir);
        let m = load_material(&vfs, "essai/eau");
        assert!(!m.translucent && m.alpha == 1.0);
        assert!((m.color[0] - 0.0024).abs() < 1e-3 && (m.color[2] - 0.0065).abs() < 1e-3, "{:?}", m.color);
        // avec sa propre texture : inchangée (translucide)
        assert!(load_material(&vfs, "essai/eau_bon_marche").translucent);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn vitrage_reconnu() {
        let p = |kv: &[(&str, &str)]| kv.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<HashMap<_, _>>();
        // verre translucide à reflet : voile selon $envmaptint
        let shop = p(&[("$surfaceprop", "glass"), ("$envmap", "env_cubemap"), ("$envmaptint", "[.35 .35 .42]")]);
        let r = reflet_de_vitrage("megalopole/glass_shop", &shop, true);
        assert!(r > 0.5 && r < 0.6, "{r}");
        // verre sans envmap : voile léger
        assert_eq!(reflet_de_vitrage("megalopole/verre_clair", &p(&[("$surfaceprop", "glass")]), true), 0.22);
        // translucide qui n'est pas un verre (flaque, décalque) : rien
        assert_eq!(reflet_de_vitrage("megalopole/flaque", &p(&[("$envmap", "env_cubemap")]), true), 0.0);
        // verre opaque : rien
        assert_eq!(reflet_de_vitrage("megalopole/glass_shop", &shop, false), 0.0);
    }
}
