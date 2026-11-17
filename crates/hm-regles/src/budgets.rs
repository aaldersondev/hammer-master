//! Budgets de la map : ce qui se compte dans le VMF avant toute compilation, les limites du compilateur
//! installé (lues dans la table de son dernier log), et une estimation de la taille de la lumière.
//!
//! Les plafonds ne sont pas écrits en dur : le vbsp de Garry's Mod n'a pas ceux du SDK 2013, et sa table
//! « Objects/Maxobjs » fait foi pour ce PC (docs/limites_et_dimensions.md).
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use glam::{DVec3, Vec3};
use hm_assets::Tool;
use hm_scene::Scene;
use serde::Serialize;

use crate::{Constat, Niveau, Regle};

pub const REGLES: &[Regle] = &[
    Regle { id: "budget.depasse", niveau: Niveau::Erreur, titre: "Limite du compilateur dépassée", explication: "Un comptage exact du VMF dépasse la limite que le compilateur installé annonce : la compilation échouera.", source: "table « Objects/Maxobjs » du dernier log de compilation" },
    Regle { id: "budget.serre", niveau: Niveau::Avertissement, titre: "Budget presque plein", explication: "Un budget dépasse 90 % de sa limite.", source: "table « Objects/Maxobjs » du dernier log ; pour les areaportals, 192 (SDK public/const.h l. 431, plantage vu en jeu le 31/10/2026)" },
    Regle { id: "budget.lumieres_nommees", niveau: Niveau::Avertissement, titre: "Trop de lumières nommées", explication: "Plus de 32 noms de lumières distincts : vbsp refuse (« Too many switched lights »).", source: "SDK utils/vbsp/writebsp.cpp l. 984-1022 (MAX_SWITCHED_LIGHTS) ; valeur du vbsp de Garry's Mod non vérifiée" },
];

#[derive(Clone, Debug, Serialize)]
pub struct Ligne {
    /// Nom dans la table du compilateur quand il existe (« brushes », « static prop count »…).
    pub nom: String,
    pub libelle: String,
    /// Valeur comptée dans le VMF.
    pub valeur: u64,
    /// Vrai si la valeur comptée doit être celle du compilateur à l'unité près ; faux pour une estimation.
    pub exact: bool,
    /// Valeur du dernier log, si la table la donne.
    pub log: Option<u64>,
    pub limite: Option<u64>,
    pub source_limite: String,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Lumiere {
    /// D'où vient le facteur : « défaut », ou la révision de la compilation qui l'a donné.
    pub calage: String,
    /// Luxels comptés sur les faces éclairées (avant toute découpe par le compilateur).
    pub luxels: u64,
    /// Octets estimés : luxels × 4 × facteur de calage.
    pub octets: u64,
    pub facteur: f64,
    /// Part de chaque zone (carreaux de 4 096), en octets estimés, les plus lourdes d'abord.
    pub zones: Vec<(String, u64)>,
    /// Les matériaux les plus coûteux.
    pub materiaux: Vec<(String, u64)>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Budgets {
    pub lignes: Vec<Ligne>,
    pub lumiere: Lumiere,
    /// Log de compilation lu pour les limites.
    pub log: Option<String>,
}

/// Facteur de calage de la lumière par défaut : octets du bloc de lumière par luxel compté dans le VMF. Il dépend du
/// projet (lumières nommées, faces cachées que le comptage ne voit pas) : mesuré de 2,4 à 6,0 sur 12 couples VMF / BSP
/// le 13/11/2026 (`tests/calibration_budgets.py`), mais stable à 5 % près d'une compilation à la suivante d'un même
/// projet. Dès qu'une map compilée et estampillée de la révision ouverte existe, son facteur exact le remplace
/// (`caler_lumiere`).
pub const FACTEUR_LUMIERE: f64 = 5.4;

/// Classes d'entités à solides qui ne deviennent pas un modèle de brosses dans le BSP.
const SANS_MODELE: &[&str] = &["func_detail", "func_areaportal", "func_areaportalwindow", "func_viscluster", "func_occluder", "func_ladder", "func_instance"];

fn zone_de(p: DVec3) -> String {
    let lettre = |v: f64| (b'A' + (((v + 16384.0) / 4096.0).floor().clamp(0.0, 7.0) as u8)) as char;
    let etage = if p.z < -2048.0 { " bas" } else if p.z > 4096.0 { " haut" } else { "" };
    format!("{}{}{}", lettre(p.x), ((p.y + 16384.0) / 4096.0).floor().clamp(0.0, 7.0) as u8 + 1, etage)
}

pub fn compter(scene: &Scene, monde: &crate::Monde) -> Budgets {
    let mut b = Budgets::default();
    let outil: HashMap<&str, Option<Tool>> = scene.materials.iter().map(|m| (m.name.as_str(), m.tool)).collect();
    let translucide: HashMap<&str, bool> = scene.materials.iter().map(|m| (m.name.as_str(), m.unlit || m.shader.starts_with("unlit"))).collect();
    let relief: HashMap<&str, bool> = scene.materials.iter().map(|m| (m.name.as_str(), m.bump)).collect();
    let mut ligne = |nom: &str, libelle: &str, valeur: u64, exact: bool| b.lignes.push(Ligne { nom: nom.into(), libelle: libelle.into(), valeur, exact, log: None, limite: None, source_limite: String::new() });

    // Un solide qui porte un displacement ne devient pas une brosse du BSP : vbsp le retire de la liste (mesuré le
    // 13/11/2026 : 8 103 solides dans le VMF, dont 320 à displacement, pour 7 783 brosses au log).
    let a_disp = |s: &hm_vmf::Node| s.children_named("side").any(|c| c.child("dispinfo").is_some());
    let tous: Vec<&hm_vmf::Node> = scene.vmf.world().into_iter().flat_map(|w| w.solids()).chain(scene.vmf.entities().flat_map(|e| e.solids())).collect();
    let solides = tous.iter().filter(|s| !a_disp(s)).count() as u64;
    let cotes: u64 = tous.iter().filter(|s| !a_disp(s)).map(|s| s.children_named("side").count() as u64).sum();
    ligne("brushes", "solides (brosses, hors displacements)", solides, true);
    // vbsp ajoute des faces de biseau aux solides non droits (AddBrushBevels) : le compte du VMF est un plancher
    ligne("brushsides", "faces de solides (plancher : vbsp ajoute des biseaux)", cotes, false);
    let mut modeles = 1u64; // le monde
    let (mut statiques, mut areaportals, mut cubemaps, mut overlays, mut entites, mut entdata) = (0u64, 0u64, 0u64, 0u64, 0u64, 0u64);
    let mut noms_lumieres: HashSet<String> = HashSet::new();
    let mut lumieres = 0u64;
    for e in scene.vmf.entities() {
        let c = e.classname().to_ascii_lowercase();
        entites += 1;
        match c.as_str() {
            "prop_static" => statiques += 1,
            "func_areaportal" | "func_areaportalwindow" => areaportals += 1,
            "env_cubemap" => cubemaps += 1,
            "info_overlay" => overlays += 1,
            _ => {}
        }
        if c.starts_with("light") && c != "light_dynamic" {
            lumieres += 1;
            if let Some(n) = e.get("targetname").filter(|n| !n.is_empty()) {
                noms_lumieres.insert(n.to_ascii_lowercase());
            }
        }
        if e.solids().next().is_some() && !SANS_MODELE.contains(&c.as_str()) {
            modeles += 1;
        }
        // ce qui reste dans le bloc d'entités du BSP : tout sauf les props statiques, le détail et les entités
        // que vbsp consomme ; taille ≈ clés et valeurs avec leurs guillemets
        if !matches!(c.as_str(), "prop_static" | "func_detail" | "env_cubemap" | "info_overlay" | "func_viscluster" | "prop_detail" | "info_no_dynamic_shadow" | "func_instance") {
            entdata += 4 + e.pairs.iter().filter(|p| p.key != "id").map(|p| p.key.len() as u64 + p.value.len() as u64 + 6).sum::<u64>();
        }
    }
    ligne("models", "modèles de brosses (monde + entités à solides)", modeles, true);
    ligne("static prop count", "props statiques", statiques, true);
    ligne("cubemapsamples", "cubemaps", cubemaps, true);
    ligne("overlays", "overlays", overlays, true);
    ligne("dispinfos", "displacements", scene.report.displacements as u64, true);
    ligne("areaportals", "areaportals", areaportals, true);
    ligne("lumieres_nommees", "noms de lumières distincts", noms_lumieres.len() as u64, true);
    ligne("lumieres", "entités de lumière", lumieres, true);
    ligne("entites", "entités dans le VMF", entites, true);
    ligne("entdata", "données d'entités (octets, estimation)", entdata, false);

    // ---- lumière : luxels des faces éclairées
    let mut par_zone: BTreeMap<String, u64> = BTreeMap::new();
    let mut par_mat: HashMap<&str, u64> = HashMap::new();
    let mut luxels = 0u64;
    for sb in &scene.brushes {
        if sb.object == 0 || scene.objects[sb.object as usize].hidden {
            continue;
        }
        for f in &sb.brush.faces {
            if outil.get(f.material.as_str()).copied().flatten().is_some() || translucide.get(f.material.as_str()).copied().unwrap_or(false) {
                continue;
            }
            let pas = f.lightmap_scale.max(1.0);
            let l = if let Some(d) = &f.disp {
                // un displacement : (2^puissance × côté ÷ pas… ) le moteur lui donne une grille propre ; compté sur sa base
                let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
                for p in &d.flat {
                    let (u, v) = (p.dot(DVec3::from(f.uaxis.dir)), p.dot(DVec3::from(f.vaxis.dir)));
                    lo = [lo[0].min(u), lo[1].min(v)];
                    hi = [hi[0].max(u), hi[1].max(v)];
                }
                (((hi[0] - lo[0]) / pas).ceil() + 1.0) * (((hi[1] - lo[1]) / pas).ceil() + 1.0)
            } else {
                if f.poly.len() < 3 {
                    continue;
                }
                let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
                for p in &f.poly {
                    let (u, v) = (p.dot(DVec3::from(f.uaxis.dir)), p.dot(DVec3::from(f.vaxis.dir)));
                    lo = [lo[0].min(u), lo[1].min(v)];
                    hi = [hi[0].max(u), hi[1].max(v)];
                }
                // étendue en luxels comme dans CalcFaceExtents : bornes arrondies au pas, plus un
                let n = |a: f64, b: f64| ((b / pas).ceil() - (a / pas).floor() + 1.0).max(1.0);
                n(lo[0], hi[0]) * n(lo[1], hi[1])
            };
            // Une face du monde collée contre un autre solide du monde disparaît à la compilation : sa part cachée
            // ne coûte rien. Mesurée sur le centre et des points à mi-chemin des coins, 1 unité devant la face.
            let mut visible = 1.0;
            if sb.world && f.disp.is_none() {
                let c = f.poly.iter().fold(DVec3::ZERO, |a, p| a + *p) / f.poly.len() as f64;
                let mut pts = vec![c];
                pts.extend(f.poly.iter().map(|p| c.lerp(*p, 0.6)));
                let vus = pts.iter().filter(|p| monde.dans((**p + f.plane.n).as_vec3(), |k, _| k.genre == crate::monde::Genre::Monde).is_none()).count();
                visible = vus as f64 / pts.len() as f64;
                // Tournée vers le vide : aucun solide du monde devant elle jusqu'au bout des limites. vbsp retire ces
                // faces après avoir rempli l'intérieur depuis les entités (map fermée) ; elles ne coûtent rien.
                if visible > 0.0 && vers_le_vide(monde, (c + f.plane.n).as_vec3(), f.plane.n.as_vec3(), (scene.min, scene.max)) {
                    visible = 0.0;
                }
            }
            // une carte de relief fait stocker la lumière quatre fois (une par direction, plus la moyenne)
            let fois = if relief.get(f.material.as_str()).copied().unwrap_or(false) { 4.0 } else { 1.0 };
            let l = (l * visible * fois) as u64;
            luxels += l;
            let centre = f.poly.iter().chain(f.disp.iter().flat_map(|d| d.points.iter())).fold(DVec3::ZERO, |a, p| a + *p) / (f.poly.len() + f.disp.as_ref().map_or(0, |d| d.points.len())).max(1) as f64;
            *par_zone.entry(zone_de(centre)).or_default() += l;
            *par_mat.entry(f.material.as_str()).or_default() += l;
        }
    }
    let octets = |l: u64| (l as f64 * FACTEUR_LUMIERE) as u64;
    let mut zones: Vec<(String, u64)> = par_zone.into_iter().map(|(z, l)| (z, octets(l))).collect();
    zones.sort_by_key(|z| std::cmp::Reverse(z.1));
    let mut materiaux: Vec<(String, u64)> = par_mat.into_iter().map(|(m, l)| (m.to_string(), octets(l))).collect();
    materiaux.sort_by_key(|z| std::cmp::Reverse(z.1));
    materiaux.truncate(12);
    b.lumiere = Lumiere { calage: "défaut (aucune compilation estampillée de ce projet)".into(), luxels, octets: octets(luxels), facteur: FACTEUR_LUMIERE, zones, materiaux };
    b
}

/// La dernière table « Object names  Objects/Maxobjs » d'un log de compilation : nom → (objets, limite).
/// Une limite de 0 ou « [variable] » veut dire que le compilateur n'en annonce pas.
pub fn lire_log(texte: &str) -> BTreeMap<String, (u64, Option<u64>)> {
    let mut table = BTreeMap::new();
    let Some(debut) = texte.rfind("Object names") else { return table };
    for l in texte[debut..].lines().skip(2) {
        let l = l.trim_end();
        if l.trim().is_empty() || l.starts_with("Level flags") || l.starts_with("Total") {
            break;
        }
        // nom (peut contenir des espaces), puis « a/b » ou « [variable] », puis « mémoire/max »
        let Some(pos) = l.find(|c: char| c == '/' || c == '[') else { continue };
        let avant = &l[..pos];
        let nom_fin = avant.rfind(|c: char| !c.is_ascii_digit()).map_or(0, |i| i + 1);
        let nom = if l[pos..].starts_with('[') { avant.trim() } else { l[..nom_fin].trim() };
        let reste: Vec<&str> = l[nom_fin.min(pos)..].split_whitespace().collect();
        let paire = |s: &str| s.split_once('/').and_then(|(a, b)| Some((a.parse::<u64>().ok()?, b.parse::<u64>().ok()?)));
        if l[pos..].starts_with('[') {
            // [variable]   octets/max
            if let Some((o, m)) = reste.iter().find_map(|s| paire(s)) {
                table.insert(nom.to_string(), (o, (m > 0).then_some(m)));
            }
        } else if let Some((o, m)) = reste.first().and_then(|s| paire(s)) {
            table.insert(nom.to_string(), (o, (m > 0).then_some(m)));
        }
    }
    table
}

/// Complète les lignes avec le log du vbsp lancé sur cette révision (`.hammer-master/vbsp/<révision>.log`), ou à
/// défaut avec le dernier log de compilation trouvé près du VMF (`<carte>.log`), qui peut dater d'une autre révision.
pub fn avec_log(b: &mut Budgets, vmf: &Path, revision: &str) {
    let de_revision = dossier_vbsp(vmf).join(format!("{revision}.log"));
    let log = if de_revision.is_file() { de_revision } else { vmf.with_extension("log") };
    let Ok(octets) = std::fs::read(&log) else { return };
    let table = lire_log(&String::from_utf8_lossy(&octets));
    if table.is_empty() {
        return;
    }
    b.log = Some(log.display().to_string());
    for l in &mut b.lignes {
        if let Some((o, m)) = table.get(&l.nom) {
            l.log = Some(*o);
            l.limite = *m;
            l.source_limite = "table du dernier log".into();
        }
    }
    // budgets que seul le compilateur connaît : repris du log tels quels, pour le tableau
    for (nom, libelle) in [("vertexes", "sommets"), ("faces", "faces"), ("planes", "plans"), ("nodes", "nœuds"), ("leaves", "feuilles"), ("leaffaces", "faces par feuille"), ("texinfos", "texinfos"), ("edges", "arêtes"), ("waterindices", "indices d'eau"), ("LDR worldlights", "lumières (worldlights)"), ("areas", "areas"), ("visdata", "visibilité (octets)"), ("physics", "collision (octets)")] {
        if let Some((o, m)) = table.get(nom) {
            b.lignes.push(Ligne { nom: nom.into(), libelle: format!("{libelle} — connu après vbsp seulement"), valeur: *o, exact: false, log: Some(*o), limite: *m, source_limite: "table du dernier log".into() });
        }
    }
    let complet = std::fs::read(vmf.with_extension("log")).map(|o| lire_log(&String::from_utf8_lossy(&o))).unwrap_or_default();
    b.lignes.retain(|l| !(l.nom == "visdata" && l.valeur == 0));
    if let Some((o, _)) = complet.get("LDR lightdata").or(table.get("LDR lightdata")) {
        b.lignes.push(Ligne { nom: "LDR lightdata".into(), libelle: "lumière (octets, estimation)".into(), valeur: b.lumiere.octets, exact: false, log: Some(*o), limite: None, source_limite: "pas de plafond annoncé ; 16 777 216 dans le SDK 2013 (bspfile.h MAX_MAP_LIGHTING)".into() });
    }
    if let Some((o, m)) = table.get("entdata") {
        if let Some(l) = b.lignes.iter_mut().find(|l| l.nom == "entdata") {
            l.log = Some(*o);
            l.limite = *m;
            l.source_limite = "table du dernier log".into();
        }
    }
}

pub fn verifier(b: &Budgets, out: &mut Vec<Constat>) {
    for l in &b.lignes {
        // la limite de 192 areaportals n'est dans aucune table : état envoyé au client sur 24 octets
        let limite = if l.nom == "areaportals" { Some(192) } else { l.limite };
        let Some(limite) = limite else { continue };
        if !l.exact && l.log.is_none() {
            continue;
        }
        let plein = l.valeur as f64 / limite as f64;
        let quoi = format!("{} : {} sur {} ({:.1} %)", l.libelle, l.valeur, limite, plein * 100.0);
        if l.exact && l.valeur > limite {
            out.push(Constat::lieu("budget.depasse", glam::Vec3::ZERO, glam::Vec3::ZERO, quoi).mesure(plein as f32 * 100.0));
        } else if plein >= 0.9 {
            out.push(Constat::lieu("budget.serre", glam::Vec3::ZERO, glam::Vec3::ZERO, format!("{quoi}{}", if l.exact { "" } else { " — valeur du dernier log" })).mesure(plein as f32 * 100.0));
        }
    }
    if let Some(l) = b.lignes.iter().find(|l| l.nom == "lumieres_nommees") {
        if l.valeur > 32 {
            out.push(Constat::lieu("budget.lumieres_nommees", glam::Vec3::ZERO, glam::Vec3::ZERO, format!("{} noms de lumières distincts, pour 32 admis par le vbsp du SDK", l.valeur)).mesure(l.valeur as f32));
        }
    }
}

pub fn tableau(b: &Budgets) -> String {
    let mut s = String::from("budgets (compté dans le VMF · dernier log · limite) :\n");
    for l in &b.lignes {
        let log = l.log.map_or("—".to_string(), |v| v.to_string());
        let lim = l.limite.map_or("—".to_string(), |v| v.to_string());
        let ecart = match (l.exact, l.log) {
            (true, Some(v)) if v != l.valeur => format!("  ≠ log ({:+})", l.valeur as i64 - v as i64),
            (true, Some(_)) => "  = log".to_string(),
            _ => String::new(),
        };
        let plein = l.limite.map_or(String::new(), |m| format!("  {:.1} %", l.valeur as f64 / m as f64 * 100.0));
        s += &format!("  {:<58} {:>10} · {:>10} · {:>8}{plein}{ecart}\n", l.libelle, l.valeur, log, lim);
    }
    s += &format!("lumière estimée : {} luxels × {:.3} = {:.2} Mo ; facteur : {}\n", b.lumiere.luxels, b.lumiere.facteur, b.lumiere.octets as f64 / 1048576.0, b.lumiere.calage);
    s += &format!("  par zone : {}\n", b.lumiere.zones.iter().take(10).map(|(z, o)| format!("{z} {:.2} Mo", *o as f64 / 1048576.0)).collect::<Vec<_>>().join(", "));
    s += &format!("  par matériau : {}", b.lumiere.materiaux.iter().take(8).map(|(z, o)| format!("{z} {:.2} Mo", *o as f64 / 1048576.0)).collect::<Vec<_>>().join(", "));
    if let Some(l) = &b.log {
        s += &format!("\nlimites lues dans {l}");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_du_log() {
        let log = "blabla\nObject names       Objects/Maxobjs  Memory / Maxmem  Fullness \n------------       ---------------  ---------------  -------- \nmodels                 201/4096         9648/196608   ( 4.9%) \nbrushes               7783/65535       93396/786420   (11.9%) \ndispinfos              320/0           56320/0        ( 0.0%) \nLDR worldlights       3128/8192       275264/720896   (38.2%) \nLDR lightdata         [variable]    17310028/0        ( 0.0%) \nvisdata               [variable]     2731723/16777216 (16.3%) \nstatic prop count    15801/16384           0/0        (96.4%) SIZE DANGER!!\n\nLevel flags = 0\n";
        let t = lire_log(log);
        assert_eq!(t["models"], (201, Some(4096)));
        assert_eq!(t["brushes"], (7783, Some(65535)));
        assert_eq!(t["dispinfos"], (320, None));
        assert_eq!(t["LDR worldlights"], (3128, Some(8192)));
        assert_eq!(t["LDR lightdata"], (17310028, None));
        assert_eq!(t["visdata"], (2731723, Some(16777216)));
        assert_eq!(t["static prop count"], (15801, Some(16384)));
    }
}

/// Dossier où vbsp est lancé à part : `.hammer-master/vbsp` près du VMF.
pub fn dossier_vbsp(vmf: &Path) -> std::path::PathBuf {
    vmf.parent().unwrap_or(Path::new(".")).join(".hammer-master").join("vbsp")
}

/// Résultat d'un vbsp lancé à part sur une révision.
#[derive(Clone, Debug, Serialize)]
pub struct Vbsp {
    pub revision: String,
    pub code: i32,
    pub duree: f32,
    pub log: String,
    /// La table « Objects/Maxobjs » de ce vbsp : nom → (objets, limite).
    pub table: BTreeMap<String, (u64, Option<u64>)>,
    /// Chemin de la fuite si vbsp en a trouvé une : de l'entité de départ jusqu'au vide.
    pub fuite: Option<Vec<[f32; 3]>>,
    /// Lignes d'erreur et d'avertissement de vbsp.
    pub messages: Vec<String>,
}

/// Lance vbsp seul sur une copie du VMF, dans un dossier à part : budgets exacts (sommets, faces, jonctions…) et
/// fuite, sans toucher à la map compilée. `jeu` est le dossier du gameinfo.txt, `exe` le vbsp de Garry's Mod.
pub fn lancer_vbsp(vmf: &Path, revision: &str, jeu: &Path, exe: &Path) -> anyhow::Result<Vbsp> {
    lancer_vbsp_sur(vmf, None, revision, jeu, exe)
}

/// Comme [`lancer_vbsp`], sur `contenu` s'il est donné (la map en cours d'édition) plutôt que sur le fichier.
pub fn lancer_vbsp_sur(vmf: &Path, contenu: Option<&[u8]>, revision: &str, jeu: &Path, exe: &Path) -> anyhow::Result<Vbsp> {
    let dossier = dossier_vbsp(vmf);
    std::fs::create_dir_all(&dossier)?;
    let nom = "hm_budget";
    let copie = dossier.join(format!("{nom}.vmf"));
    match contenu {
        Some(c) => std::fs::write(&copie, c)?,
        None => {
            std::fs::copy(vmf, &copie)?;
        }
    }
    for ext in ["bsp", "prt", "lin", "log"] {
        let _ = std::fs::remove_file(dossier.join(format!("{nom}.{ext}")));
    }
    let t = std::time::Instant::now();
    let sortie = std::process::Command::new(exe).args(["-game", &jeu.display().to_string(), nom]).current_dir(&dossier).output().map_err(|e| anyhow::anyhow!("{} : {e}", exe.display()))?;
    let duree = t.elapsed().as_secs_f32();
    let mut log = String::from_utf8_lossy(&sortie.stdout).to_string() + &String::from_utf8_lossy(&sortie.stderr);
    // vbsp seul n'écrit pas la table des budgets : les comptes se lisent dans le BSP qu'il vient d'écrire, les
    // limites dans le dernier log de compilation complet (elles ne dépendent que du compilateur installé)
    let limites = std::fs::read(vmf.with_extension("log")).map(|o| lire_log(&String::from_utf8_lossy(&o))).unwrap_or_default();
    let mut table: BTreeMap<String, (u64, Option<u64>)> = BTreeMap::new();
    if let Ok(comptes) = lire_bsp(&dossier.join(format!("{nom}.bsp"))) {
        log += "\nObject names       Objects/Maxobjs  Memory / Maxmem  Fullness \n------------       ---------------  ---------------  -------- \n";
        for (n, o) in comptes {
            let m = limites.get(&n).and_then(|x| x.1);
            log += &format!("{n:<20} {o}/{}  0/0  (lu dans le BSP de vbsp)\n", m.unwrap_or(0));
            table.insert(n, (o, m));
        }
        log += "\n";
    }
    let fuite = if log.contains("**** leaked ****") {
        // fichier de points : une ligne par segment, « x y z - x y z » ou six nombres
        let lin = std::fs::read_to_string(dossier.join(format!("{nom}.lin"))).unwrap_or_default();
        let mut pts: Vec<[f32; 3]> = Vec::new();
        for l in lin.lines() {
            let v: Vec<f32> = l.split(|c: char| c.is_whitespace() || c == '-' && false).filter_map(|x| x.parse().ok()).collect();
            if v.len() >= 3 {
                pts.push([v[0], v[1], v[2]]);
            }
            if v.len() >= 6 {
                pts.push([v[3], v[4], v[5]]);
            }
        }
        Some(pts)
    } else {
        None
    };
    let messages: Vec<String> = log.lines().filter(|l| {
        let m = l.to_ascii_lowercase();
        m.contains("error") || m.contains("warning") || m.contains("leak") || m.contains("too many") || m.contains("outside the map") || m.contains("danger")
    }).map(|l| l.trim().to_string()).take(60).collect();
    std::fs::write(dossier.join(format!("{revision}.log")), &log)?;
    // la map compilée à part ne sert plus : elle pèse des dizaines de Mo
    for ext in ["bsp", "prt", "vmf"] {
        let _ = std::fs::remove_file(dossier.join(format!("{nom}.{ext}")));
    }
    Ok(Vbsp { revision: revision.to_string(), code: sortie.status.code().unwrap_or(-1), duree, log: dossier.join(format!("{revision}.log")).display().to_string(), table, fuite, messages })
}

/// Constats tirés d'un vbsp : la fuite, avec l'endroit où elle sort.
pub fn constats_vbsp(v: &Vbsp, out: &mut Vec<Constat>) {
    if let Some(pts) = &v.fuite {
        let (premier, dernier) = (pts.first().copied().unwrap_or([0.0; 3]), pts.last().copied().unwrap_or([0.0; 3]));
        let d = glam::Vec3::from(dernier);
        out.push(
            Constat::lieu("geo.fuite", d - glam::Vec3::splat(160.0), d + glam::Vec3::splat(160.0), format!("vbsp : la map fuit. Le chemin part de ({:.0} {:.0} {:.0}) et sort vers ({:.0} {:.0} {:.0}), en {} points", premier[0], premier[1], premier[2], dernier[0], dernier[1], dernier[2], pts.len()))
                .mesure(pts.len() as f32),
        );
    }
}

/// Comptes d'un BSP, sous les noms de la table du compilateur. Tailles des structures vérifiées sur la table du log
/// du 12/11/2026 (colonne « Memory » ÷ colonne « Objects ») ; numéros des blocs : SDK public/bspfile.h.
pub fn lire_bsp(chemin: &Path) -> anyhow::Result<BTreeMap<String, u64>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(chemin)?;
    let mut tete = vec![0u8; 8 + 64 * 16];
    f.read_exact(&mut tete)?;
    anyhow::ensure!(&tete[0..4] == b"VBSP", "{} n'est pas un BSP Source", chemin.display());
    let bloc = |i: usize| -> (u64, u64) {
        let o = 8 + i * 16;
        (u32::from_le_bytes(tete[o..o + 4].try_into().unwrap()) as u64, u32::from_le_bytes(tete[o + 4..o + 8].try_into().unwrap()) as u64)
    };
    let mut t = BTreeMap::new();
    for (nom, id, taille) in [
        ("models", 14, 48u64), ("brushes", 18, 12), ("brushsides", 19, 8), ("planes", 1, 20), ("vertexes", 3, 12), ("nodes", 5, 32), ("texinfos", 6, 72),
        ("texdata", 2, 32), ("dispinfos", 26, 176), ("faces", 7, 56), ("origfaces", 27, 56), ("leaves", 10, 32), ("leaffaces", 16, 2), ("leafbrushes", 17, 2),
        ("areas", 20, 8), ("surfedges", 13, 4), ("edges", 12, 4), ("waterstrips", 37, 10), ("waterverts", 38, 12), ("waterindices", 39, 2), ("cubemapsamples", 42, 16),
        ("overlays", 45, 352), ("entdata", 0, 1), ("LDR lightdata", 8, 1), ("visdata", 4, 1), ("physics", 29, 1),
    ] {
        t.insert(nom.to_string(), bloc(id).1 / taille);
    }
    // props statiques : bloc de jeu « sprp » = dictionnaire de noms (128 octets), feuilles (2 octets), puis le nombre
    let (pos, long) = bloc(35);
    if long >= 4 {
        f.seek(SeekFrom::Start(pos))?;
        let mut b = vec![0u8; long.min(16 * 64 + 4) as usize];
        f.read_exact(&mut b)?;
        let n = i32::from_le_bytes(b[0..4].try_into().unwrap()) as usize;
        for k in 0..n.min(63) {
            let e = &b[4 + k * 16..4 + k * 16 + 16];
            if &e[0..4] == b"prps" {
                let (o, _) = (i32::from_le_bytes(e[8..12].try_into().unwrap()) as u64, 0);
                f.seek(SeekFrom::Start(o))?;
                let mut q = [0u8; 4];
                f.read_exact(&mut q)?;
                let dict = i32::from_le_bytes(q) as u64;
                f.seek(SeekFrom::Start(o + 4 + dict * 128))?;
                f.read_exact(&mut q)?;
                let feuilles = i32::from_le_bytes(q) as u64;
                f.seek(SeekFrom::Start(o + 8 + dict * 128 + feuilles * 2))?;
                f.read_exact(&mut q)?;
                t.insert("static prop count".to_string(), i32::from_le_bytes(q) as u64);
            }
        }
    }
    Ok(t)
}

/// Cale l'estimation de la lumière sur le projet. Si la map compilée voisine (`<carte>.bsp`) porte l'estampille de la
/// révision ouverte, le facteur est exact et il est gardé dans `.hammer-master/lumiere.json` ; sinon le dernier
/// facteur gardé sert, avec la révision dont il vient.
pub fn caler_lumiere(b: &mut Budgets, vmf: &Path, revision: &str) {
    let fichier = vmf.parent().unwrap_or(Path::new(".")).join(".hammer-master").join("lumiere.json");
    let mut calage: Option<(f64, String)> = None;
    let bsp = vmf.with_extension("bsp");
    if let (Ok(t), true) = (lire_bsp(&bsp), b.lumiere.luxels > 0) {
        let octets = t.get("LDR lightdata").copied().unwrap_or(0);
        if octets > 0 && revision_du_bsp(&bsp).as_deref() == Some(revision) {
            let f = octets as f64 / b.lumiere.luxels as f64;
            let _ = std::fs::create_dir_all(fichier.parent().unwrap());
            let _ = std::fs::write(&fichier, serde_json::json!({ "facteur": f, "revision": revision, "octets": octets, "luxels": b.lumiere.luxels }).to_string());
            calage = Some((f, format!("compilation de cette révision ({revision})")));
        }
    }
    if calage.is_none() {
        if let Some(v) = std::fs::read(&fichier).ok().and_then(|o| serde_json::from_slice::<serde_json::Value>(&o).ok()) {
            if let (Some(f), Some(rev)) = (v["facteur"].as_f64(), v["revision"].as_str()) {
                calage = Some((f, format!("compilation de la révision {rev}")));
            }
        }
    }
    if let Some((f, d)) = calage {
        let k = f / b.lumiere.facteur;
        b.lumiere.facteur = f;
        b.lumiere.calage = d;
        b.lumiere.octets = (b.lumiere.luxels as f64 * f) as u64;
        for z in b.lumiere.zones.iter_mut().chain(b.lumiere.materiaux.iter_mut()) {
            z.1 = (z.1 as f64 * k) as u64;
        }
        if let Some(l) = b.lignes.iter_mut().find(|l| l.nom == "LDR lightdata") {
            l.valeur = b.lumiere.octets;
        }
    }
}

/// Révision inscrite dans le bloc d'entités d'un BSP (entité `hm_rev_…`), s'il est estampillé.
pub fn revision_du_bsp(chemin: &Path) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(chemin).ok()?;
    let mut tete = [0u8; 24];
    f.read_exact(&mut tete).ok()?;
    let (pos, long) = (u32::from_le_bytes(tete[8..12].try_into().unwrap()) as u64, u32::from_le_bytes(tete[12..16].try_into().unwrap()) as usize);
    f.seek(SeekFrom::Start(pos)).ok()?;
    let mut b = vec![0u8; long.min(64 << 20)];
    f.read_exact(&mut b).ok()?;
    let texte = String::from_utf8_lossy(&b);
    let i = texte.find(hm_vmf::PREFIXE_REVISION)? + hm_vmf::PREFIXE_REVISION.len();
    Some(texte[i..].chars().take_while(|c| c.is_ascii_hexdigit()).collect())
}

/// Aucun solide du monde sur la demi-droite depuis `a` le long de `n`, jusqu'à la sortie de la boîte de la map. Par
/// segments de plus en plus longs : une face intérieure touche presque toujours un mur dans les premières centaines
/// d'unités, et seule une face tournée vers le vide paie le trajet entier.
fn vers_le_vide(monde: &crate::Monde, a: Vec3, n: Vec3, (lo, hi): (Vec3, Vec3)) -> bool {
    let (lo, hi) = (lo - Vec3::splat(64.0), hi + Vec3::splat(64.0));
    let sortie = (0..3)
        .filter(|&k| n[k].abs() > 1e-6)
        .map(|k| ((if n[k] > 0.0 { hi[k] } else { lo[k] }) - a[k]) / n[k])
        .fold(f32::MAX, f32::min)
        .max(0.0);
    let mut debut = 0.0;
    for fin in [256.0, 2048.0, sortie] {
        let fin = fin.min(sortie);
        if fin > debut && monde.rayon(a + n * debut, a + n * fin, |k, _| k.genre == crate::monde::Genre::Monde).is_some() {
            return false;
        }
        debut = debut.max(fin);
    }
    true
}
