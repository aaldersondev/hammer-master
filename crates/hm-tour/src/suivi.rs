//! Suivi des lectures : quelle vue a été lue, par qui, avec quel constat ; et le feu vert d'une révision.
//!
//! Une vue est reconnue d'une révision à l'autre par son **empreinte** : celle de l'image rendue (couleurs
//! arrondies et objet présent sous chaque pixel). Si rien de ce qu'elle montre n'a changé, l'empreinte est la même
//! et la lecture précédente reste valable ; sinon la vue est à relire. C'est la version la plus simple du tour
//! différentiel : elle ne dit pas *ce qui* a changé, seulement *qu'il faut regarder de nouveau*.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use hm_render::{Camera, Frame, GpuScene, Lighting, Renderer, View};
use hm_scene::Scene;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Defaut {
    /// 1 = saute aux yeux, 2 = gênant, 3 = détail.
    pub gravite: u8,
    #[serde(default)]
    pub confiance: String,
    pub quoi: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Lecture {
    pub nom: String,
    /// Ce qu'on voit sur la vue.
    pub vu: String,
    #[serde(default)]
    pub defauts: Vec<Defaut>,
    #[serde(default)]
    pub revision: String,
    #[serde(default)]
    pub planche: String,
}

/// Ce qu'un lecteur rend pour une planche.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LecturePlanche {
    pub planche: String,
    pub code: String,
    pub vues: Vec<LectureCase>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LectureCase {
    pub case: usize,
    #[serde(default)]
    pub nom: String,
    pub vu: String,
    #[serde(default)]
    pub defauts: Vec<Defaut>,
}

/// Les lectures connues d'un projet, rangées par empreinte de vue.
#[derive(Serialize, Deserialize, Default)]
pub struct Registre {
    pub lectures: BTreeMap<String, Lecture>,
}

/// Dossier de suivi d'une map : `.hammer-master` à côté du VMF.
pub fn dossier_suivi(vmf: &Path) -> PathBuf {
    vmf.parent().unwrap_or(Path::new(".")).join(".hammer-master")
}

impl Registre {
    pub fn charger(dossier: &Path) -> Self {
        std::fs::read(dossier.join("lectures.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn enregistrer(&self, dossier: &Path) -> Result<()> {
        std::fs::create_dir_all(dossier)?;
        std::fs::write(dossier.join("lectures.json"), serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }
}

/// Identité d'un objet indépendante de son rang : ce qu'il est et où il est.
fn identite(scene: &Scene, rang: u32) -> u64 {
    let o = &scene.objects[rang as usize];
    let mut h = blake3::Hasher::new();
    h.update(o.class.as_bytes());
    h.update(o.model.as_bytes());
    h.update(o.material.as_bytes());
    for v in [o.min, o.max] {
        for c in v.to_array() {
            h.update(&((c * 4.0).round() as i32).to_le_bytes());
        }
    }
    u64::from_le_bytes(h.finalize().as_bytes()[..8].try_into().unwrap())
}

/// Empreinte d'une vue rendue.
pub fn empreinte(scene: &Scene, frame: &Frame, identites: &mut BTreeMap<u32, u64>) -> String {
    let mut h = blake3::Hasher::new();
    for (px, &id) in frame.rgba.chunks_exact(4).zip(&frame.ids) {
        let ident = if id == 0 { 0 } else { *identites.entry(id).or_insert_with(|| identite(scene, id)) };
        h.update(&[px[0] >> 4, px[1] >> 4, px[2] >> 4]);
        h.update(&ident.to_le_bytes());
    }
    h.finalize().to_hex()[..20].to_string()
}

/// Empreinte d'une vue à partir de sa caméra (image de contrôle de 448 × 252).
pub fn empreinte_camera(renderer: &Renderer, gs: &GpuScene, scene: &Scene, camera: Camera, identites: &mut BTreeMap<u32, u64>) -> String {
    let frame = renderer.render(gs, &View { camera, width: 448, height: 252, show_tools: false, clip: None, light: Lighting::neutral(scene) });
    empreinte(scene, &frame, identites)
}

/// Vérifie et enregistre la lecture d'une planche d'un tour. Refuse si le code ne correspond pas à l'image, si une
/// vue n'a pas de lecture, ou si une lecture ne dit pas ce qu'on voit.
pub fn enregistrer_lecture(dossier_tour: &Path, registre: &mut Registre, lecture: &LecturePlanche) -> Result<usize> {
    let tour: serde_json::Value = serde_json::from_slice(&std::fs::read(dossier_tour.join("tour.json")).context("tour.json")?)?;
    let revision = tour["revision"].as_str().unwrap_or("").to_string();
    let info = tour["infos"].as_array().and_then(|a| a.iter().find(|i| i["fichier"] == lecture.planche.as_str())).ok_or_else(|| anyhow!("planche {} absente de ce tour", lecture.planche))?;
    if crate::empreinte_code(&revision, &lecture.planche, &lecture.code) != info["empreinte_code"].as_str().unwrap_or("") {
        bail!("planche {} : le code de lecture « {} » n'est pas celui de l'image", lecture.planche, lecture.code);
    }
    let vues: Vec<&serde_json::Value> = tour["vues"].as_array().map(|a| a.iter().filter(|v| v["planche"] == lecture.planche.as_str()).collect()).unwrap_or_default();
    let mut n = 0;
    for v in &vues {
        let case = v["case"].as_u64().unwrap_or(0) as usize;
        let l = lecture.vues.iter().find(|l| l.case == case).ok_or_else(|| anyhow!("planche {} : la case {case} n'a pas de lecture", lecture.planche))?;
        let texte = l.vu.trim();
        let vide = ["rien à signaler", "ras", "r.a.s.", "ok", "rien"].iter().any(|m| texte.eq_ignore_ascii_case(m));
        if texte.chars().count() < 15 || vide {
            bail!("planche {} case {case} : la lecture doit dire ce qu'on voit sur la vue, pas seulement « {texte} »", lecture.planche);
        }
        let empreinte = v["empreinte"].as_str().filter(|e| !e.is_empty()).ok_or_else(|| anyhow!("ce tour n'a pas d'empreintes de vues : le relancer"))?;
        registre.lectures.insert(empreinte.to_string(), Lecture { nom: v["nom"].as_str().unwrap_or("").to_string(), vu: l.vu.clone(), defauts: l.defauts.clone(), revision: revision.clone(), planche: lecture.planche.clone() });
        n += 1;
    }
    Ok(n)
}

#[derive(Serialize, Deserialize, Debug)]
pub struct EtatVerrou {
    pub revision: String,
    pub vues: usize,
    pub lues: usize,
    pub planches_a_lire: Vec<String>,
    pub ouvert: bool,
    pub raison: String,
}

/// État du verrou « tour lu » pour une révision : ouvert si un tour de cette révision existe et que chacune de ses
/// vues a une lecture enregistrée.
pub fn etat_verrou(vmf: &Path, revision: &str) -> EtatVerrou {
    etat_verrou_dans(vmf, revision, None)
}

/// Enregistre un passage en force pour une révision. N'est appelé que par la commande que l'utilisateur lance
/// lui-même dans un terminal.
pub fn forcer(vmf: &Path, revision: &str, raison: &str) -> Result<()> {
    let suivi = dossier_suivi(vmf);
    std::fs::create_dir_all(&suivi)?;
    let quand = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let ligne = serde_json::json!({"revision": revision, "raison": raison, "quand_unix": quand, "carte": vmf.display().to_string()});
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(suivi.join("passages_en_force.jsonl"))?;
    writeln!(f, "{ligne}")?;
    Ok(())
}

fn force(suivi: &Path, revision: &str) -> Option<String> {
    let txt = std::fs::read_to_string(suivi.join("passages_en_force.jsonl")).ok()?;
    txt.lines().filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok()).filter(|v| v["revision"] == revision).last().map(|v| v["raison"].as_str().unwrap_or("").to_string())
}

/// Comme [`etat_verrou`], avec un dossier de tour donné (par défaut `.hammer-master/tour/<révision>`).
pub fn etat_verrou_dans(vmf: &Path, revision: &str, tour: Option<&Path>) -> EtatVerrou {
    let suivi = dossier_suivi(vmf);
    if let Some(raison) = force(&suivi, revision) {
        return EtatVerrou { revision: revision.to_string(), vues: 0, lues: 0, planches_a_lire: Vec::new(), ouvert: true, raison: format!("PASSAGE EN FORCE demandé par l'utilisateur (« {raison} ») : cette révision n'a pas été vérifiée") };
    }
    let dossier = tour.map(|t| t.to_path_buf()).unwrap_or_else(|| suivi.join("tour").join(revision));
    let Some(tour) = std::fs::read(dossier.join("tour.json")).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()) else {
        return EtatVerrou { revision: revision.to_string(), vues: 0, lues: 0, planches_a_lire: Vec::new(), ouvert: false, raison: format!("aucun tour rendu pour la révision {revision} (attendu dans {})", dossier.display()) };
    };
    let registre = Registre::charger(&suivi);
    let vues = tour["vues"].as_array().cloned().unwrap_or_default();
    let mut a_lire: Vec<String> = Vec::new();
    let mut lues = 0;
    for v in &vues {
        if registre.lectures.contains_key(v["empreinte"].as_str().unwrap_or("")) {
            lues += 1;
        } else if let Some(p) = v["planche"].as_str() {
            if !p.is_empty() && !a_lire.iter().any(|x| x == p) {
                a_lire.push(p.to_string());
            }
        }
    }
    let ouvert = !vues.is_empty() && lues == vues.len();
    let raison = if ouvert { "toutes les vues du tour ont été lues".to_string() } else { format!("{} vue(s) sur {} restent à lire, sur {} planche(s)", vues.len() - lues, vues.len(), a_lire.len()) };
    EtatVerrou { revision: revision.to_string(), vues: vues.len(), lues, planches_a_lire: a_lire, ouvert, raison }
}
