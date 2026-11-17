//! Propper++ : des brosses de la map en un modèle (`prop_static`), comme Propper / Propper++ de Hammer++. Méthode
//! vérifiée en jeu sur des proppers de l'utilisateur (skill gmod-addon-maker, « Proppers ») :
//! - chaque face visible devient des triangles, dans le sens direct vu de dehors, avec la projection de texture de
//!   Hammer (`u = p·uaxis / échelle + décalage`, divisé par la largeur de la texture ; le SMD garde `1 − v`) ;
//! - une face enfouie dans une autre brosse est retirée (son milieu et ses coins, poussés de 0,05 u vers dehors, sont
//!   tous dans une autre brosse), comme « SkipNodraw » ; les faces d'outil (`tools/…`) ne se dessinent pas ;
//! - chaque brosse fait une pièce de collision fermée et convexe, rétrécie de 0,05 u, avec sa propre normale, pour
//!   que studiomdl ne soude pas deux pièces qui se touchent ; `$concave`, `$remove2d`, `$maxconvexpieces` au-dessus
//!   du nombre de pièces ;
//! - `$origin 0 0 0 -90` annule le quart de tour que studiomdl donne aux SMD : les axes du modèle sont ceux de la map ;
//! - l'origine du modèle est le milieu du dessous des brosses, là où le `prop_static` se pose ;
//! - un modèle veut des matériaux `VertexLitGeneric` : une copie de chaque VMT, sous `materials/models/<modèle>/`,
//!   qui reprend la texture du matériau de la map.
use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::{Path, PathBuf};

use glam::DVec3;
use hm_assets::Vfs;
use hm_doc::{Document, Ref};

const TETE_SMD: &str = "version 1\nnodes\n0 \"root\" -1\nend\nskeleton\ntime 0\n0 0 0 0 0 0 0\nend\ntriangles\n";

#[derive(Clone, Debug)]
pub struct OptionsPropper {
    /// Chemin du modèle sous `models/`, sans `.mdl` (« hammer_master/ma_piece »).
    pub modele: String,
    pub surfaceprop: String,
    pub masse: f64,
    pub collision: bool,
}

/// Les fichiers d'un propper, prêts à écrire.
#[derive(Clone, Debug)]
pub struct Propper {
    pub nom: String,
    pub smd: String,
    pub phys: Option<String>,
    pub qc: String,
    /// (chemin sous la racine du jeu, contenu) des VMT du modèle.
    pub vmts: Vec<(String, String)>,
    /// Où poser le `prop_static` pour qu'il prenne la place des brosses.
    pub origine: DVec3,
    pub triangles: usize,
    pub pieces: usize,
    pub faces_enfouies: usize,
    /// Ce qui est passé dans le modèle (solides du monde, entités à brosses) : ce que le prop remplace.
    pub utilises: Vec<Ref>,
}

/// Entités à brosses que le joueur ne voit pas : elles n'entrent pas dans un modèle.
fn invisible(classe: &str) -> bool {
    let c = classe.to_ascii_lowercase();
    c.starts_with("trigger_") || c.starts_with("func_areaportal") || c == "func_occluder" || c == "func_viscluster"
}

fn nombre(x: f64) -> String {
    let s = format!("{x:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() {
        "0".into()
    } else {
        s.into()
    }
}

/// Le nom d'un matériau dans le SMD (un seul dossier, celui de `$cdmaterials`).
fn nom_plat(m: &str) -> String {
    m.to_lowercase().replace(['/', '\\'], "_")
}

/// Largeur et hauteur de la texture de base d'un matériau (en-tête du VTF), 512 si elle ne se trouve pas.
fn taille_texture(vfs: &Vfs, base: &str) -> (f64, f64) {
    vfs.read(&format!("materials/{base}.vtf"))
        .filter(|f| f.bytes.len() >= 20)
        .map(|f| (u16::from_le_bytes([f.bytes[16], f.bytes[17]]).max(1) as f64, u16::from_le_bytes([f.bytes[18], f.bytes[19]]).max(1) as f64))
        .unwrap_or((512.0, 512.0))
}

/// Les plans sortants d'un solide : (normale vers dehors, d), un point p est dedans si n·p − d < 0 pour tous.
fn plans_sortants(s: &hm_doc::Solide) -> Vec<(DVec3, f64)> {
    let sommets = hm_doc::brosses::sommets(s);
    let c = sommets.iter().fold(DVec3::ZERO, |a, p| a + *p) / sommets.len().max(1) as f64;
    s.faces
        .iter()
        .map(|f| {
            let mut n = f.normale();
            if n.dot(c - f.plan[1]) > 0.0 {
                n = -n;
            }
            (n, n.dot(f.plan[1]))
        })
        .collect()
}

fn dedans(plans: &[(DVec3, f64)], p: DVec3) -> bool {
    plans.iter().all(|(n, d)| n.dot(p) - d < -1e-4)
}

/// Prépare le propper des objets (leurs solides, groupes dépliés). `vfs` : les fichiers du jeu, pour les tailles de
/// texture et les VMT d'origine (vide : 512 × 512 et la texture au nom du matériau).
pub fn preparer(doc: &Document, objets: &[Ref], vfs: &Vfs, o: &OptionsPropper) -> Result<Propper, String> {
    let modele = o.modele.trim().trim_end_matches(".mdl").replace('\\', "/").to_lowercase();
    if modele.is_empty() || modele.contains(' ') || modele.starts_with('/') {
        return Err("nom de modèle : un chemin sous models/, en minuscules, sans espace (« dossier/nom »)".into());
    }
    let nom = modele.rsplit('/').next().unwrap_or(&modele).to_string();
    // solides du monde, et ceux des entités à brosses visibles (func_detail, func_brush…)
    let (ids, ents, _) = hm_doc::resoudre(doc, objets)?;
    let mut utilises: Vec<Ref> = Vec::new();
    let mut solides: Vec<&hm_doc::Solide> = Vec::new();
    for i in ids {
        match doc.proprio(i) {
            Some(hm_doc::Proprio::Entite(e)) => {
                if doc.entite(e).map_or(false, |x| !invisible(x.classe())) && !utilises.contains(&Ref::Entite(e)) {
                    utilises.push(Ref::Entite(e));
                }
            }
            _ => {
                if let Some(s) = doc.solide(i) {
                    solides.push(s);
                    utilises.push(Ref::Solide(i));
                }
            }
        }
    }
    for e in ents.iter().filter_map(|e| doc.entite(*e)).filter(|e| !e.solides.is_empty() && !invisible(e.classe())) {
        if !utilises.contains(&Ref::Entite(e.id)) {
            utilises.push(Ref::Entite(e.id));
        }
    }
    for r in &utilises {
        if let Ref::Entite(e) = r {
            solides.extend(doc.entite(*e).map(|x| x.solides.iter()).into_iter().flatten());
        }
    }
    if solides.is_empty() {
        return Err("aucun solide dans la sélection".into());
    }
    let polys: Vec<Vec<Vec<DVec3>>> = solides.iter().map(|s| s.polygones()).collect();
    let (mut lo, mut hi) = (DVec3::splat(f64::MAX), DVec3::splat(f64::MIN));
    for p in polys.iter().flatten().flatten() {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    let origine = DVec3::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0, lo.z);
    let plans: Vec<Vec<(DVec3, f64)>> = solides.iter().map(|s| plans_sortants(s)).collect();
    let mut tailles: BTreeMap<String, (f64, f64)> = BTreeMap::new();
    let mut materiaux: BTreeMap<String, String> = BTreeMap::new();
    let (mut smd, mut triangles, mut enfouies) = (String::from(TETE_SMD), 0, 0);
    for (k, s) in solides.iter().enumerate() {
        for ((f, poly), (n, _)) in s.faces.iter().zip(&polys[k]).zip(&plans[k]) {
            let mat = f.materiau.to_lowercase().replace('\\', "/");
            if poly.len() < 3 || mat.starts_with("tools/") {
                continue;
            }
            let c = poly.iter().fold(DVec3::ZERO, |a, p| a + *p) / poly.len() as f64;
            let essais: Vec<DVec3> = std::iter::once(c).chain(poly.iter().map(|p| c + (*p - c) * 0.92)).map(|p| p + *n * 0.05).collect();
            if essais.iter().all(|p| plans.iter().enumerate().any(|(j, pl)| j != k && dedans(pl, *p))) {
                enfouies += 1;
                continue;
            }
            let base = materiaux.entry(mat.clone()).or_insert_with(|| hm_assets::material::load_material(vfs, &mat).base.unwrap_or_else(|| mat.clone())).clone();
            let (tw, th) = *tailles.entry(mat.clone()).or_insert_with(|| taille_texture(vfs, &base));
            // sens direct vu de dehors
            let mut p = poly.clone();
            if (p[1] - p[0]).cross(p[2] - p[0]).dot(*n) < 0.0 {
                p.reverse();
            }
            let plat = nom_plat(&mat);
            for i in 1..p.len() - 1 {
                smd.push_str(&plat);
                smd.push('\n');
                for q in [p[0], p[i], p[i + 1]] {
                    let u = (q.dot(f.u.dir) / f.u.echelle.max(1e-6) + f.u.decalage) / tw;
                    let v = (q.dot(f.v.dir) / f.v.echelle.max(1e-6) + f.v.decalage) / th;
                    let r = q - origine;
                    let _ = writeln!(smd, "0 {} {} {} {:.5} {:.5} {:.5} {:.5} {:.5}", nombre(r.x), nombre(r.y), nombre(r.z), n.x, n.y, n.z, u, 1.0 - v);
                }
                triangles += 1;
            }
        }
    }
    smd.push_str("end\n");
    if triangles == 0 {
        return Err("aucune face visible (tout est enfoui ou en texture d'outil)".into());
    }
    // collision : chaque brosse, fermée, rétrécie autour de son milieu, avec sa propre normale
    let mut phys = String::from(TETE_SMD);
    for (k, s) in solides.iter().enumerate() {
        let sommets = hm_doc::brosses::sommets(s);
        let c = sommets.iter().fold(DVec3::ZERO, |a, p| a + *p) / sommets.len().max(1) as f64;
        let etendue = sommets.iter().map(|p| (*p - c).length()).fold(0.0, f64::max).max(0.1);
        let r = 1.0 - 0.05 / etendue;
        let h = (k as f64 + 1.0) * 12.9898;
        let nk = DVec3::new(h.sin(), (h * 1.7).cos(), (h * 2.3).sin() + 1.5).normalize();
        for (poly, (n, _)) in polys[k].iter().zip(&plans[k]) {
            if poly.len() < 3 {
                continue;
            }
            let mut p: Vec<DVec3> = poly.iter().map(|q| c + (*q - c) * r).collect();
            if (p[1] - p[0]).cross(p[2] - p[0]).dot(*n) < 0.0 {
                p.reverse();
            }
            for i in 1..p.len() - 1 {
                phys.push_str("phys\n");
                for q in [p[0], p[i], p[i + 1]] {
                    let q = q - origine;
                    let _ = writeln!(phys, "0 {} {} {} {:.5} {:.5} {:.5} 0 0", nombre(q.x), nombre(q.y), nombre(q.z), nk.x, nk.y, nk.z);
                }
            }
        }
    }
    phys.push_str("end\n");
    let pieces = if o.collision { solides.len() } else { 0 };
    let collision = if pieces > 0 {
        format!("$collisionmodel \"{nom}_phys.smd\"\n{{\n\t$concave\n\t$mass {}\n\t$remove2d\n\t$maxconvexpieces {}\n}}\n", nombre(o.masse.max(1.0)), pieces + 8)
    } else {
        String::new()
    };
    let qc = format!(
        "// écrit par Hammer Master (Propper++)\n$modelname \"{modele}.mdl\"\n$staticprop\n$surfaceprop \"{}\"\n$origin 0 0 0 -90\n$cdmaterials \"models/{modele}/\"\n$body \"body\" \"{nom}.smd\"\n$sequence \"idle\" \"{nom}.smd\"\n{collision}",
        o.surfaceprop.trim()
    );
    // un VMT de modèle par matériau, qui reprend la texture de celui de la map
    let vmts = materiaux
        .iter()
        .map(|(mat, base)| {
            let m = hm_assets::material::load_material(vfs, mat);
            let mut t = format!("\"VertexLitGeneric\"\n{{\n\t\"$basetexture\" \"{base}\"\n");
            for (oui, cle) in [(m.alphatest, "$alphatest"), (m.translucent, "$translucent"), (m.nocull, "$nocull"), (m.additive, "$additive")] {
                if oui {
                    let _ = writeln!(t, "\t\"{cle}\" \"1\"");
                }
            }
            t.push_str("}\n");
            (format!("materials/models/{modele}/{}.vmt", nom_plat(mat)), t)
        })
        .collect();
    Ok(Propper { nom, smd, phys: (pieces > 0).then_some(phys), qc, vmts, origine, triangles, pieces, faces_enfouies: enfouies, utilises })
}

/// Écrit les sources (SMD, QC) dans `sources`, les VMT sous `racine` (dossier du jeu ou d'un addon) ; rend le QC.
pub fn ecrire(p: &Propper, sources: &Path, racine: &Path) -> Result<PathBuf, String> {
    let e = |c: &Path, t: &str| std::fs::write(c, t).map_err(|x| format!("{} : {x}", c.display()));
    std::fs::create_dir_all(sources).map_err(|x| format!("{} : {x}", sources.display()))?;
    e(&sources.join(format!("{}.smd", p.nom)), &p.smd)?;
    if let Some(ph) = &p.phys {
        e(&sources.join(format!("{}_phys.smd", p.nom)), ph)?;
    }
    let qc = sources.join(format!("{}.qc", p.nom));
    e(&qc, &p.qc)?;
    for (chemin, contenu) in &p.vmts {
        let c = racine.join(chemin);
        if let Some(d) = c.parent() {
            std::fs::create_dir_all(d).map_err(|x| format!("{} : {x}", d.display()))?;
        }
        e(&c, contenu)?;
    }
    Ok(qc)
}

/// Compile le QC avec le studiomdl 32 bits de Garry's Mod (`bin/studiomdl.exe` : celui de `bin/win64` plante sur les
/// modèles de collision, vu en 10/2026). `jeu` : le dossier `garrysmod` (le modèle sort sous `jeu/models/`).
/// Rend le journal ; une erreur de studiomdl le rend en erreur.
pub fn compiler(qc: &Path, jeu: &Path) -> Result<String, String> {
    let gmod = hm_assets::jeu::garrysmod().ok_or("Garry's Mod introuvable")?;
    let exe = gmod.join("bin").join("studiomdl.exe");
    let mut c = std::process::Command::new(&exe);
    c.arg("-nop4").arg("-game").arg(jeu).arg(qc).current_dir(qc.parent().unwrap_or(Path::new(".")));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // pas de console qui s'ouvre
    }
    let r = c.output().map_err(|e| format!("{} : {e}", exe.display()))?;
    let journal = format!("{}{}", String::from_utf8_lossy(&r.stdout), String::from_utf8_lossy(&r.stderr));
    if !r.status.success() || journal.lines().any(|l| l.contains("ERROR")) {
        let fin: Vec<&str> = journal.lines().rev().take(25).collect();
        return Err(format!("studiomdl a échoué :\n{}", fin.into_iter().rev().collect::<Vec<_>>().join("\n")));
    }
    Ok(journal)
}

/// La boîte de collision d'un `.mdl` compilé (en-tête : `hull_min` à l'octet 104, `hull_max` à 116).
pub fn bornes_mdl(mdl: &Path) -> Option<([f32; 3], [f32; 3])> {
    let o = std::fs::read(mdl).ok()?;
    let f = |k: usize| f32::from_le_bytes([o[k], o[k + 1], o[k + 2], o[k + 3]]);
    (o.len() >= 128).then(|| ([f(104), f(108), f(112)], [f(116), f(120), f(124)]))
}
