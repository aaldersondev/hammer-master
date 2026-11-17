//! Construire une map sur le disque à partir d'une spec : le VMF, son plan, son modèle sémantique, ses modules.
//! Le constructeur lui-même est dans `hm-model` ; ici il reçoit les fichiers du jeu (pour savoir si un modèle
//! existe, où est son bas, et si vbsp l'accepte en `prop_static`) et ses sorties sont écrites.
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use hm_assets::Vfs;
use hm_model::construire::{construire, lire_spec, Construction, InfoModele, Options};
use hm_model::Modele;

/// Le dossier `modules/` du skill gmod-map-maker, s'il est installé.
pub fn modules_du_skill() -> Option<PathBuf> {
    let maison = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    let d = PathBuf::from(maison).join(".claude").join("skills").join("gmod-map-maker").join("modules");
    d.is_dir().then_some(d)
}

/// Ce que le constructeur demande d'un modèle : s'il existe, où est le bas de sa boîte, si vbsp l'accepte en
/// `prop_static`.
pub fn info_modele(vfs: &Vfs, m: &str) -> Option<InfoModele> {
    let f = vfs.read(m)?;
    // `hull_min.z` dans l'en-tête d'un .mdl (studiohdr_t : après id, version, somme, nom[64], taille, œil, éclairage)
    let o = f.bytes.get(112..116)?;
    // accepté en prop_static : les deux contrôles de vbsp, lus comme le font les règles
    let statique = hm_assets::model::load_model(vfs, m).ok().map(|x| x.static_prop && !x.prop_data);
    Some(InfoModele { bas: f32::from_le_bytes([o[0], o[1], o[2], o[3]]) as f64, statique })
}

/// Construit la spec et, s'il n'y a pas d'erreur, écrit `<sortie>.vmf`, `.plan.json`, `.sem.json` et copie les
/// modules `func_instance` dans `modules/` à côté. Sans `sortie` : `<carte>.vmf` à côté de la spec, comme le
/// générateur du skill. Rend la construction et le chemin du VMF ; avec une erreur, rien n'est écrit.
pub fn construire_sur_disque(chemin: &Path, sortie: Option<&Path>, phase: Option<&str>, modules: Option<PathBuf>, vfs: &mut Vfs) -> Result<(Construction, PathBuf)> {
    let spec = lire_spec(chemin)?;
    let complet = std::path::absolute(chemin)?;
    let base = complet.parent().unwrap_or(Path::new(".")).to_path_buf();
    let nom = spec.get("carte").and_then(|v| v.as_str()).map(str::to_string).unwrap_or_else(|| complet.file_stem().unwrap_or_default().to_string_lossy().to_string());
    let sortie = sortie.map(Path::to_path_buf).unwrap_or_else(|| base.join(format!("{nom}.vmf")));
    // le contenu du projet que la spec déclare s'ajoute aux fichiers du jeu
    for d in spec.get("contenu").and_then(|v| v.as_sequence()).into_iter().flatten().filter_map(|v| v.as_str()) {
        vfs.add_dir(base.join(d));
    }
    let vfs = &*vfs;
    let modele = |m: &str| info_modele(vfs, m);
    let c = construire(&spec, &Options { phase: phase.map(str::to_string), base, modules_skill: modules.or_else(modules_du_skill), modele: &modele });
    if !c.erreurs.is_empty() {
        return Ok((c, sortie));
    }
    let dossier = sortie.parent().unwrap_or(Path::new(".")).to_path_buf();
    std::fs::create_dir_all(&dossier)?;
    // comme le générateur du skill, qui écrit en mode texte : fins de ligne de Windows sous Windows
    let texte = if cfg!(windows) { c.vmf.replace('\n', "\r\n") } else { c.vmf.clone() };
    std::fs::write(&sortie, texte).with_context(|| format!("écriture de {}", sortie.display()))?;
    std::fs::write(sortie.with_extension("plan.json"), serde_json::to_string_pretty(&c.plan)? + "\n")?;
    c.sem.ecrire(&Modele::chemin(&sortie))?;
    for (src, nom) in &c.modules {
        let dest = dossier.join("modules").join(nom);
        if std::fs::canonicalize(src).ok() != std::fs::canonicalize(&dest).ok() {
            std::fs::create_dir_all(dossier.join("modules"))?;
            std::fs::copy(src, &dest).with_context(|| format!("copie du module {}", src.display()))?;
        }
    }
    Ok((c, sortie))
}

/// Le bilan d'une construction, dans la forme du générateur du skill.
pub fn bilan(c: &Construction, sortie: &Path) -> String {
    let mut t = String::new();
    for w in &c.avertissements {
        t += &format!("AVERTISSEMENT  {w}\n");
    }
    for e in &c.erreurs {
        t += &format!("ERREUR         {e}\n");
    }
    if !c.erreurs.is_empty() {
        return t + &format!("\n{} erreur(s) : rien n'a été écrit.", c.erreurs.len());
    }
    let fichier = |p: PathBuf| p.file_name().unwrap_or_default().to_string_lossy().to_string();
    t + &format!(
        "OK  {}  ({}) : {} zones, {} ouvertures, {} brushes, {} entités ; plan : {} ; modèle sémantique : {} ({} pièce(s), {} objet(s) déclaré(s))",
        sortie.display(),
        c.phase,
        c.zones,
        c.ouvertures,
        c.brosses,
        c.entites,
        fichier(sortie.with_extension("plan.json")),
        fichier(Modele::chemin(sortie)),
        c.sem.pieces.len(),
        c.sem.objets.len()
    )
}
