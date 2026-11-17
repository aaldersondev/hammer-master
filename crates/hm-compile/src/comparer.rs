//! Hammer++ et Hammer Master au même point de vue, côte à côte : pour comparer les deux rendus (et voir une
//! régression). Hammer++ installé est lancé sur une copie de la map dont le bloc `cameras` place sa caméra, sa vue 3D
//! capturée, puis il est refermé et ses réglages remis tels qu'avant ; la vue 3D de la fenêtre de Hammer Master est
//! capturée par son mode script (`camera`, `capture3d`). Méthode essayée à la main le 15/11 sur rp_rockford.
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

/// Le texte du VMF avec un bloc `cameras` dont la caméra active est en `oeil`, tournée vers `vise` ; le bloc
/// d'origine est remplacé, ou le nouveau ajouté à la fin.
pub fn avec_camera(vmf: &str, oeil: [f64; 3], vise: [f64; 3]) -> String {
    let nl = if vmf.contains("\r\n") { "\r\n" } else { "\n" };
    let bloc = [
        "cameras".to_string(),
        "{".into(),
        "\t\"activecamera\" \"0\"".into(),
        "\tcamera".into(),
        "\t{".into(),
        format!("\t\t\"position\" \"[{} {} {}]\"", oeil[0], oeil[1], oeil[2]),
        format!("\t\t\"look\" \"[{} {} {}]\"", vise[0], vise[1], vise[2]),
        "\t}".into(),
        "}".into(),
    ]
    .join(nl)
        + nl;
    // le bloc d'origine : de la ligne `cameras` (en début de ligne) à la première `}` en début de ligne
    let mut debut = 0;
    for l in vmf.split_inclusive('\n') {
        if l.trim_end() == "cameras" {
            let reste = &vmf[debut..];
            if let Some(f) = reste.find(&format!("{nl}}}")) {
                let fin = debut + f + nl.len() + 1;
                let fin = fin + vmf[fin..].find('\n').map_or(vmf.len() - fin, |k| k + 1);
                return format!("{}{bloc}{}", &vmf[..debut], &vmf[fin..]);
            }
        }
        debut += l.len();
    }
    format!("{vmf}{}{bloc}", if vmf.is_empty() || vmf.ends_with('\n') { "" } else { nl })
}

/// Hammer++ tourne-t-il déjà ?
fn hammerpp_ouvert() -> bool {
    Command::new("tasklist").args(["/FI", "IMAGENAME eq hammerplusplus.exe", "/NH"]).output().map_or(false, |o| String::from_utf8_lossy(&o.stdout).to_ascii_lowercase().contains("hammerplusplus.exe"))
}

/// Attend la fin d'un processus, au plus `delai` ; le tue au-delà. Rend son code de sortie.
fn attendre(mut enfant: std::process::Child, delai: Duration) -> Result<Option<i32>, String> {
    let t0 = Instant::now();
    loop {
        if let Some(s) = enfant.try_wait().map_err(|e| e.to_string())? {
            return Ok(s.code());
        }
        if t0.elapsed() > delai {
            let _ = enfant.kill();
            return Err(format!("délai de {} s dépassé", delai.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Ce qu'a donné une comparaison.
pub struct Comparaison {
    /// L'image côte à côte : Hammer++ à gauche, Hammer Master à droite.
    pub image: PathBuf,
    /// Ce qui s'est passé (ouverture, boîtes de dialogue de Hammer++, tailles).
    pub journal: Vec<String>,
}

/// Capture les deux vues 3D depuis `oeil` vers `vise` et les met côte à côte dans `sortie` (PNG ou JPEG). `editeur` :
/// l'exécutable de Hammer Master (`hammer-master.exe`). `attente` : secondes laissées à Hammer++ après l'ouverture,
/// le temps que ses modèles et textures arrivent.
pub fn comparer(vmf: &Path, oeil: [f64; 3], vise: [f64; 3], editeur: &Path, sortie: &Path, attente: u32) -> Result<Comparaison, String> {
    let gmod = hm_assets::jeu::garrysmod().ok_or("Garry's Mod introuvable")?;
    let hpp = gmod.join("bin/win64/hammerplusplus.exe");
    if !hpp.is_file() {
        return Err(format!("Hammer++ n'est pas installé ({})", hpp.display()));
    }
    if !editeur.is_file() {
        return Err(format!("Hammer Master introuvable ({})", editeur.display()));
    }
    if hammerpp_ouvert() {
        return Err("Hammer++ est ouvert : le fermer d'abord (ses réglages sont remis à l'identique après la comparaison)".into());
    }
    let vmf = std::path::absolute(vmf).map_err(|e| e.to_string())?;
    let texte = String::from_utf8_lossy(&std::fs::read(&vmf).map_err(|e| format!("{} : {e}", vmf.display()))?).into_owned();
    let tmp = std::env::temp_dir().join(format!("hm_comparer_{}", std::process::id()));
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    // la copie à côté de l'original : ses instances (chemins relatifs) restent valables
    let nom = vmf.file_stem().and_then(|s| s.to_str()).unwrap_or("carte");
    let copie = vmf.with_file_name(format!("{nom}.hm_comparer.vmf"));
    let ini = gmod.join("bin/win64/hammerplusplus/hammerplusplus_settings.ini");
    let reglages = std::fs::read(&ini).ok();
    let r = (|| -> Result<Comparaison, String> {
        let mut journal = Vec::new();
        // ---- Hammer++
        std::fs::write(&copie, avec_camera(&texte, oeil, vise)).map_err(|e| format!("{} : {e}", copie.display()))?;
        let ps1 = tmp.join("hammerpp_vue3d.ps1");
        std::fs::write(&ps1, include_str!("hammerpp_vue3d.ps1")).map_err(|e| e.to_string())?;
        let png_hpp = tmp.join("hammerpp.png");
        let sortie_ps = Command::new("powershell")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&ps1)
            .arg("-Exe")
            .arg(&hpp)
            .arg("-Vmf")
            .arg(&copie)
            .arg("-Out")
            .arg(&png_hpp)
            .arg("-Attente")
            .arg(attente.to_string())
            .output()
            .map_err(|e| format!("powershell : {e}"))?;
        journal.extend(String::from_utf8_lossy(&sortie_ps.stdout).lines().filter(|l| !l.trim().is_empty()).map(|l| format!("Hammer++ : {l}")));
        if !png_hpp.is_file() {
            return Err(format!("pas de capture de Hammer++ :\n{}", journal.join("\n")));
        }
        // ---- Hammer Master, par son mode script, avec ses propres réglages (rien de ceux de l'utilisateur)
        let png_hm = tmp.join("hammer_master.png");
        let barre = |p: &Path| p.display().to_string().replace('\\', "/");
        let script = tmp.join("script.txt");
        let lignes = [
            "attendre 10".to_string(),
            format!("ouvrir {}", barre(&vmf)),
            "attendre 60".into(),
            "pause 15000".into(),
            format!("camera {} {} {} {} {} {}", oeil[0], oeil[1], oeil[2], vise[0], vise[1], vise[2]),
            "attendre 30".into(),
            "pause 5000".into(),
            "attendre 10".into(),
            format!("capture3d {}", barre(&png_hm)),
            "quitter".into(),
        ];
        std::fs::write(&script, lignes.join("\n")).map_err(|e| e.to_string())?;
        let enfant = Command::new(editeur).arg("--script").arg(&script).env("LOCALAPPDATA", tmp.join("lad")).spawn().map_err(|e| format!("{} : {e}", editeur.display()))?;
        attendre(enfant, Duration::from_secs(300)).map_err(|e| format!("Hammer Master : {e}"))?;
        if !png_hm.is_file() {
            return Err("pas de capture de Hammer Master".into());
        }
        // ---- côte à côte, à la même hauteur
        let a = image::open(&png_hpp).map_err(|e| e.to_string())?.to_rgba8();
        let b = image::open(&png_hm).map_err(|e| e.to_string())?.to_rgba8();
        let h = a.height().max(b.height());
        let a = image::imageops::resize(&a, a.width() * h / a.height(), h, image::imageops::FilterType::Triangle);
        let b = image::imageops::resize(&b, b.width() * h / b.height(), h, image::imageops::FilterType::Triangle);
        let mut img = image::RgbaImage::from_pixel(a.width() + 8 + b.width(), h, image::Rgba([40, 40, 40, 255]));
        image::imageops::overlay(&mut img, &a, 0, 0);
        image::imageops::overlay(&mut img, &b, (a.width() + 8) as i64, 0);
        image::DynamicImage::ImageRgba8(img).to_rgb8().save(sortie).map_err(|e| format!("{} : {e}", sortie.display()))?;
        journal.push(format!("Hammer++ {}×{} à gauche, Hammer Master {}×{} à droite", a.width(), a.height(), b.width(), b.height()));
        Ok(Comparaison { image: sortie.to_path_buf(), journal })
    })();
    // toujours : la copie retirée, les réglages de Hammer++ remis (sa fermeture les réécrit)
    let _ = std::fs::remove_file(&copie);
    if let Some(octets) = reglages {
        let _ = std::fs::write(&ini, octets);
    }
    let _ = std::fs::remove_dir_all(&tmp);
    r
}

#[cfg(test)]
mod tests {
    use super::avec_camera;

    #[test]
    fn camera_placee() {
        let vmf = "world\r\n{\r\n\t\"id\" \"1\"\r\n}\r\ncameras\r\n{\r\n\t\"activecamera\" \"-1\"\r\n}\r\ncordon\r\n{\r\n\t\"active\" \"0\"\r\n}\r\n";
        let t = avec_camera(vmf, [1.0, 2.0, 3.0], [4.0, 5.5, -6.0]);
        assert!(t.starts_with("world\r\n{\r\n\t\"id\" \"1\"\r\n}\r\ncameras\r\n{\r\n\t\"activecamera\" \"0\"\r\n"), "{t}");
        assert!(t.contains("\t\t\"position\" \"[1 2 3]\"\r\n\t\t\"look\" \"[4 5.5 -6]\"\r\n\t}\r\n}\r\ncordon\r\n"), "{t}");
        assert_eq!(t.matches("cameras").count(), 1);
        assert!(!t.contains("-1"));
        // sans bloc cameras : ajouté à la fin, avec les fins de ligne du fichier
        let t = avec_camera("world\n{\n}\n", [0.0; 3], [1.0, 0.0, 0.0]);
        assert!(t.starts_with("world\n{\n}\ncameras\n{\n") && t.ends_with("\t}\n}\n") && !t.contains('\r'), "{t}");
    }
}
