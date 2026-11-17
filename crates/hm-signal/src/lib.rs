//! Les signalements faits en jeu (`!defaut …`) : lecture des deux formats, état de chacun, et vue comparée — la
//! capture du jeu à côté du rendu de l'outil, même œil, même angle, même champ.
//!
//! Deux formats sont lus :
//! - format 2, écrit par l'addon `hammer_master_signalement` : `data/hammer_master/<map>/signalements.txt` ;
//! - l'ancien format de rp_megalopole (11/11/2026) : `data/megalopole_defauts/defauts.txt`, sans champ de vision,
//!   sans révision, angles sans roulis.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use glam::Vec3;
use hm_render::planche::{Image, Police};
use hm_render::{Camera, GpuScene, Lighting, Projection, Renderer, View};
use hm_scene::Scene;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Dossier `garrysmod/data` du jeu installé (ou celui que désigne `HM_GMOD_DATA`).
pub fn data_du_jeu() -> PathBuf {
    std::env::var_os("HM_GMOD_DATA").map(PathBuf::from).unwrap_or_else(|| hm_assets::jeu::garrysmod().map(|g| g.join("garrysmod/data")).unwrap_or_default())
}

/// Champ de vision réglé dans le jeu installé (`fov_desired` de `cfg/config.cfg`, base 4/3). Sert d'hypothèse pour
/// les signalements de l'ancien format, qui ne relevaient pas le champ.
pub fn fov_du_jeu() -> Option<f32> {
    let cfg = std::fs::read_to_string(data_du_jeu().parent()?.join("cfg").join("config.cfg")).ok()?;
    cfg.lines().find_map(|l| l.trim().strip_prefix("fov_desired")).and_then(|v| v.trim().trim_matches('"').parse().ok())
}

/// Dossier où l'addon range les signalements d'une map (`data/hammer_master/<map>`). Les signalements de l'ancien
/// format se lisent encore en donnant leur dossier.
pub fn dossiers_par_defaut(carte: &str) -> Vec<PathBuf> {
    vec![data_du_jeu().join("hammer_master").join(carte.to_ascii_lowercase())]
}

#[derive(Clone, Debug, PartialEq)]
pub struct Vue {
    pub origine: [f32; 3],
    /// tangage, lacet, roulis
    pub angles: [f32; 3],
    /// Champ horizontal ajusté au format de l'écran, s'il a été relevé.
    pub fov: Option<f32>,
    /// Champ tel que réglé (base 4/3), s'il a été relevé.
    pub fov_reglage: Option<f32>,
    pub aspect: f32,
    pub ecran: [u32; 2],
}

#[derive(Clone, Debug, PartialEq)]
pub struct ObjetVise {
    pub classe: String,
    pub modele: String,
    pub pos: [f32; 3],
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bsp {
    pub fichier: String,
    pub taille: i64,
    /// Secondes depuis 1970 ; 0 si le jeu ne l'a pas trouvé, 1 pour un contenu du Workshop.
    pub date: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Signalement {
    /// `L07` pour l'ancien format, `007` pour le format 2.
    pub id: String,
    pub format: u8,
    pub n: u32,
    pub quand: String,
    pub carte: String,
    pub texte: String,
    pub pos: [f32; 3],
    pub oeil: [f32; 3],
    pub angles: [f32; 3],
    pub vue: Option<Vue>,
    pub vise: [f32; 3],
    pub normale: [f32; 3],
    pub texture: String,
    pub monde: bool,
    pub objet: Option<ObjetVise>,
    /// Révision de la map lue en jeu ; vide si la map n'en portait pas (ou ancien format).
    pub revision: String,
    pub bsp: Option<Bsp>,
    /// noclip, accroupi, véhicule, au sol, vue externe — tels que relevés.
    pub joueur: BTreeMap<String, bool>,
    /// Chemin complet de la capture, si le fichier existe.
    pub capture: Option<PathBuf>,
}

fn v3(v: &Value) -> Option<[f32; 3]> {
    let a = v.as_array()?;
    Some([a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32, a.get(2)?.as_f64()? as f32])
}

fn angles(v: &Value) -> Option<[f32; 3]> {
    let a = v.as_array()?;
    Some([a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32, a.get(2).and_then(|x| x.as_f64()).unwrap_or(0.0) as f32])
}

impl Signalement {
    /// Lit une ligne JSON de l'un ou l'autre format. `dossier` sert à retrouver la capture.
    pub fn de_json(ligne: &str, dossier: &Path) -> Result<Signalement> {
        let v: Value = serde_json::from_str(ligne).context("ligne JSON illisible")?;
        let champ = |k: &str| v.get(k).ok_or_else(|| anyhow!("champ `{k}` manquant"));
        let format = v.get("format").and_then(|f| f.as_u64()).unwrap_or(1) as u8;
        let n = champ("n")?.as_u64().ok_or_else(|| anyhow!("`n` n'est pas un entier"))? as u32;
        let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
        let vue = v.get("vue").and_then(|u| {
            let ecran = u.get("ecran")?.as_array()?;
            let nombre = |k: &str| u.get(k).and_then(|x| x.as_f64()).filter(|x| *x > 0.0).map(|x| x as f32);
            Some(Vue {
                origine: v3(u.get("origine")?)?,
                angles: angles(u.get("angles")?)?,
                fov: nombre("fov"),
                fov_reglage: nombre("fov_reglage"),
                aspect: nombre("aspect")?,
                ecran: [ecran.first()?.as_u64()? as u32, ecran.get(1)?.as_u64()? as u32],
            })
        });
        let objet = v.get("objet").and_then(|o| {
            Some(ObjetVise { classe: o.get("classe")?.as_str()?.to_string(), modele: o.get("modele").and_then(|m| m.as_str()).unwrap_or("").to_string(), pos: v3(o.get("pos")?)? })
        });
        let bsp = v.get("bsp").and_then(|b| Some(Bsp { fichier: b.get("fichier")?.as_str()?.to_string(), taille: b.get("taille")?.as_i64()?, date: b.get("date")?.as_i64()? }));
        let joueur = v.get("joueur").and_then(|j| j.as_object()).map(|j| j.iter().filter_map(|(k, x)| Some((k.clone(), x.as_bool()?))).collect()).unwrap_or_default();
        let capture = v.get("capture").and_then(|c| c.as_str()).map(|c| dossier.join(c)).filter(|p| p.is_file());
        Ok(Signalement {
            id: if format >= 2 { format!("{n:03}") } else { format!("L{n:02}") },
            format,
            n,
            quand: s("quand"),
            carte: s("carte"),
            texte: s("texte"),
            pos: v3(champ("pos")?).ok_or_else(|| anyhow!("`pos` mal formé"))?,
            oeil: v3(champ("oeil")?).ok_or_else(|| anyhow!("`oeil` mal formé"))?,
            angles: angles(champ("angles")?).ok_or_else(|| anyhow!("`angles` mal formé"))?,
            vue,
            vise: v3(champ("vise")?).ok_or_else(|| anyhow!("`vise` mal formé"))?,
            normale: v.get("normale").and_then(v3).unwrap_or([0.0; 3]),
            texture: s("texture"),
            monde: v.get("monde").and_then(|m| m.as_bool()).unwrap_or(false),
            objet,
            revision: s("revision"),
            bsp,
            joueur,
            capture,
        })
    }

    /// Œil, angles (tangage, lacet, roulis) et champ vertical de la vue à rendre. Le booléen dit si le champ est
    /// supposé (ancien format : `fov_defaut`, en base 4/3) et non relevé.
    pub fn vue_a_rendre(&self, fov_defaut: f32) -> (Vec3, [f32; 3], f32, bool) {
        match &self.vue {
            Some(u) => {
                let (fov_y, suppose) = match (u.fov, u.fov_reglage) {
                    // champ horizontal réel et format de l'écran : tan(v/2) = tan(h/2) / format
                    (Some(h), _) => (2.0 * ((h.to_radians() / 2.0).tan() / u.aspect).atan().to_degrees(), false),
                    (None, Some(r)) => (hm_render::fov_y_from_game(r), false),
                    (None, None) => (hm_render::fov_y_from_game(fov_defaut), true),
                };
                (Vec3::from(u.origine), u.angles, fov_y, suppose)
            }
            None => (Vec3::from(self.oeil), self.angles, hm_render::fov_y_from_game(fov_defaut), true),
        }
    }
}

/// Lit tous les signalements d'un dossier (les deux fichiers s'ils existent), triés par date.
pub fn lire_dossier(dossier: &Path) -> Result<Vec<Signalement>> {
    let mut out = Vec::new();
    for fichier in ["defauts.txt", "signalements.txt"] {
        let p = dossier.join(fichier);
        let Ok(texte) = std::fs::read_to_string(&p) else { continue };
        for (i, ligne) in texte.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()) {
            out.push(Signalement::de_json(ligne, dossier).with_context(|| format!("{} ligne {}", p.display(), i + 1))?);
        }
    }
    Ok(out)
}

/// Lit plusieurs dossiers ; rend les signalements triés par date et les dossiers effectivement lus.
pub fn lire(dossiers: &[PathBuf]) -> Result<Vec<Signalement>> {
    let mut out = Vec::new();
    for d in dossiers.iter().filter(|d| d.is_dir()) {
        out.extend(lire_dossier(d)?);
    }
    out.sort_by(|a, b| (&a.quand, a.n).cmp(&(&b.quand, b.n)));
    Ok(out)
}

// ------------------------------------------------------------------------------------------------ états

/// Date et heure en temps universel, au format des signalements (sans dépendance de plus).
pub fn maintenant() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (j, r) = (secs / 86400, secs % 86400);
    // jours depuis 1970 → date civile (algorithme de Howard Hinnant)
    let z = j as i64 + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (d, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC", r / 3600, r % 3600 / 60, r % 60)
}


#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Etat {
    /// `ouvert`, `corrige`, `regle` (une règle le surveille désormais) ou `ecarte`.
    pub etat: String,
    pub note: String,
    pub quand: String,
}

/// États des signalements d'un projet, gardés près du VMF (`.hammer-master/signalements.json`), pas dans le jeu.
#[derive(Default, Serialize, Deserialize)]
pub struct Etats {
    pub etats: BTreeMap<String, Etat>,
}

pub const ETATS: [&str; 4] = ["ouvert", "corrige", "regle", "ecarte"];

impl Etats {
    pub fn charger(suivi: &Path) -> Self {
        std::fs::read(suivi.join("signalements.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn enregistrer(&self, suivi: &Path) -> Result<()> {
        std::fs::create_dir_all(suivi)?;
        std::fs::write(suivi.join("signalements.json"), serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }

    fn cle(s: &Signalement) -> String {
        format!("{}/{}", s.carte.to_ascii_lowercase(), s.id)
    }

    pub fn de(&self, s: &Signalement) -> Etat {
        self.etats.get(&Self::cle(s)).cloned().unwrap_or(Etat { etat: "ouvert".into(), ..Default::default() })
    }

    pub fn fixer(&mut self, s: &Signalement, etat: &str, note: &str, quand: &str) -> Result<()> {
        if !ETATS.contains(&etat) {
            return Err(anyhow!("état inconnu « {etat} » (attendu : {})", ETATS.join(", ")));
        }
        self.etats.insert(Self::cle(s), Etat { etat: etat.to_string(), note: note.to_string(), quand: quand.to_string() });
        Ok(())
    }
}

// ------------------------------------------------------------------------------------------------ révision

/// Ce que l'on peut dire de la map sur laquelle le signalement a été fait, face à la révision ouverte.
pub fn jugement_revision(s: &Signalement, revision_ouverte: &str) -> String {
    if s.revision.is_empty() {
        let bsp = s.bsp.as_ref().map(|b| format!(" ; .bsp de {} octets", b.taille)).unwrap_or_default();
        return format!("révision inconnue (map compilée sans estampille{}{bsp}) : rien ne dit que c'est la version ouverte ({revision_ouverte})", if s.format < 2 { ", ancien format" } else { "" });
    }
    if s.revision == revision_ouverte {
        format!("fait sur la révision ouverte ({revision_ouverte})")
    } else {
        format!("ATTENTION : fait sur la révision {}, pas sur celle qui est ouverte ({revision_ouverte}) — la map a changé depuis, ou le jeu avait chargé une ancienne map", s.revision)
    }
}

// ------------------------------------------------------------------------------------------------ vue comparée

pub struct Comparaison {
    /// Capture du jeu à gauche (ou un cartouche gris si elle manque), rendu de l'outil à droite.
    pub image: Image,
    /// Rang de l'objet sous le centre de la vue rendue (0 = rien), c'est-à-dire ce que le joueur visait.
    pub objet: u32,
    /// Distance entre l'œil et le point d'impact du rayon de l'outil, s'il touche quelque chose.
    pub distance_outil: Option<f32>,
    /// Distance entre l'œil et le point visé relevé en jeu.
    pub distance_jeu: f32,
    pub fov_suppose: bool,
}

/// Caméra de l'outil pour un signalement : même œil, mêmes angles (roulis compris), même champ vertical.
pub fn camera(s: &Signalement, fov_defaut: f32) -> (Camera, bool) {
    let (oeil, a, fov_y, suppose) = s.vue_a_rendre(fov_defaut);
    let (sp, cp) = a[0].to_radians().sin_cos();
    let (sy, cy) = a[1].to_radians().sin_cos();
    let (sr, cr) = a[2].to_radians().sin_cos();
    // axes du moteur Source pour des angles (tangage, lacet, roulis) : mathlib, AngleVectors
    let dir = Vec3::new(cp * cy, cp * sy, -sp);
    let up = Vec3::new(cr * sp * cy + sr * sy, cr * sp * sy - sr * cy, cr * cp);
    (Camera { eye: oeil, dir, up, proj: Projection::Perspective { fov_y }, near: 2.0, far: 80000.0 }, suppose)
}

fn charger_capture(p: &Path) -> Option<image::RgbImage> {
    image::load_from_memory(&std::fs::read(p).ok()?).ok().map(|i| i.to_rgb8())
}

/// Rend la vue du signalement et la pose à côté de la capture du jeu.
pub fn comparer(renderer: &Renderer, gs: &GpuScene, scene: &Scene, s: &Signalement, police: &Police, legende: &[String], fov_defaut: f32) -> Comparaison {
    let capture = s.capture.as_deref().and_then(charger_capture);
    let aspect = capture.as_ref().map(|c| c.width() as f32 / c.height() as f32).or(s.vue.as_ref().map(|u| u.aspect)).unwrap_or(16.0 / 9.0);
    let (w, h) = (960u32, (960.0 / aspect).round() as u32);
    let (cam, fov_suppose) = camera(s, fov_defaut);
    let frame = renderer.render(gs, &View { camera: cam.clone(), width: w, height: h, show_tools: false, clip: None, light: Lighting::neutral(scene) });
    let objet = frame.id_at(w / 2, h / 2);
    let distance_outil = scene.rayon(cam.eye, cam.dir).map(|(d, _)| d);
    let distance_jeu = Vec3::from(s.vise).distance(Vec3::from(s.oeil));

    let bandeau = 30 + 26 * legende.len() as u32;
    let mut img = Image::new(w * 2 + 4, h + bandeau, [18, 18, 20]);
    match capture {
        Some(c) => {
            let r = image::imageops::resize(&c, w, h, image::imageops::FilterType::Triangle);
            img.blit(&Image::from_rgb(w, h, r.into_raw()), 0, bandeau);
        }
        None => {
            img.fill_rect(0, bandeau as i32, w as i32, h as i32, [60, 60, 64], 1.0);
            police.ecrire(&mut img, 24.0, bandeau as f32 + 24.0, 22.0, "pas de capture du jeu", [220, 220, 220]);
        }
    }
    img.blit(&Image::from_rgb(w, h, frame.rgb()), w + 4, bandeau);
    // le centre de chaque vue : ce que le joueur visait
    for x0 in [w as i32 / 2, w as i32 + 4 + w as i32 / 2] {
        let y0 = bandeau as i32 + h as i32 / 2;
        img.fill_rect(x0 - 9, y0 - 1, 18, 2, [255, 230, 60], 0.9);
        img.fill_rect(x0 - 1, y0 - 9, 2, 18, [255, 230, 60], 0.9);
    }
    police.etiquette(&mut img, 8, bandeau as i32 + 6, 16.0, "en jeu", [255, 255, 255]);
    police.etiquette(&mut img, w as i32 + 12, bandeau as i32 + 6, 16.0, "Hammer Master, map ouverte", [255, 255, 255]);
    for (i, l) in legende.iter().enumerate() {
        police.ecrire(&mut img, 10.0, 6.0 + 26.0 * i as f32, if i == 0 { 20.0 } else { 17.0 }, l, if i == 0 { [245, 245, 245] } else { [190, 190, 195] });
    }
    Comparaison { image: img, objet, distance_outil, distance_jeu, fov_suppose }
}

/// Une ligne qui dit quel objet de la map est sous le centre de la vue.
pub fn decrire_objet(scene: &Scene, rang: u32) -> String {
    match scene.objects.get(rang as usize).filter(|_| rang != 0) {
        None => "rien sous le centre de la vue (ciel ou vide)".to_string(),
        Some(o) => {
            let quoi = if !o.model.is_empty() { format!("modèle {}", o.model) } else { format!("matériau {}", o.material) };
            let nom = if o.name.is_empty() { String::new() } else { format!(" « {} »", o.name) };
            let c = (o.min + o.max) / 2.0;
            format!("objet {rang} · {}{nom} · {quoi} · id VMF {} · centre ({:.0} {:.0} {:.0})", o.class, o.vmf_id, c.x, c.y, c.z)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angles_et_champ() {
        let l = r#"{"format":2,"n":3,"quand":"2026-11-13 10:00:00","carte":"x","texte":"t","pos":[0,0,0],"oeil":[0,0,64],"angles":[0,90,0],"vise":[0,100,64],"vue":{"origine":[0,0,64],"angles":[0,90,0],"fov":106.26,"fov_reglage":90,"aspect":1.7778,"ecran":[1920,1080]},"revision":"","revision_source":"","monde":true}"#;
        let s = Signalement::de_json(l, Path::new(".")).unwrap();
        let (cam, suppose) = camera(&s, 75.0);
        assert!(!suppose);
        // lacet 90 : on regarde vers +Y, le haut reste +Z
        assert!((cam.dir - Vec3::Y).length() < 1e-5 && (cam.up - Vec3::Z).length() < 1e-5);
        // 106,26° en 16/9 et 90° en 4/3 sont le même champ vertical (73,74°)
        let Projection::Perspective { fov_y } = cam.proj else { panic!() };
        assert!((fov_y - hm_render::fov_y_from_game(90.0)).abs() < 0.02, "{fov_y}");
        assert!((fov_y - 73.74).abs() < 0.02, "{fov_y}");
    }

    #[test]
    fn tangage_positif_regarde_vers_le_bas() {
        let l = r#"{"n":1,"quand":"","carte":"x","texte":"t","pos":[0,0,0],"oeil":[0,0,64],"angles":[30,0],"vise":[10,0,0]}"#;
        let s = Signalement::de_json(l, Path::new(".")).unwrap();
        let (cam, suppose) = camera(&s, 90.0);
        assert!(suppose && s.format == 1 && s.id == "L01");
        assert!(cam.dir.z < -0.49 && cam.dir.x > 0.86 && cam.up.z > 0.86);
        assert!(cam.dir.dot(cam.up).abs() < 1e-5);
    }
}
