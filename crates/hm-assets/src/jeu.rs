//! Où est Garry's Mod sur ce PC : variable `HM_GMOD`, sinon Steam (registre, puis ses bibliothèques).
use std::path::{Path, PathBuf};

/// Numéro de Garry's Mod sur Steam.
const APPID_GMOD: &str = "4000";

/// Dossier `…/steamapps/common/GarrysMod`, s'il est trouvé.
pub fn garrysmod() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("HM_GMOD") {
        let p = PathBuf::from(p);
        if p.join("garrysmod/gameinfo.txt").is_file() {
            return Some(p);
        }
    }
    for steam in dossiers_steam() {
        for lib in bibliotheques(&steam) {
            let p = lib.join("steamapps/common/GarrysMod");
            if p.join("garrysmod/gameinfo.txt").is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// `garrysmod/gameinfo.txt` de Garry's Mod.
pub fn gameinfo_gmod() -> Option<PathBuf> {
    garrysmod().map(|g| g.join("garrysmod/gameinfo.txt"))
}

/// `bin/win64` de Garry's Mod (vbsp, vvis, vrad).
pub fn bin_gmod() -> Option<PathBuf> {
    let g = garrysmod()?;
    [g.join("bin/win64"), g.join("bin")].into_iter().find(|d| d.join("vbsp.exe").is_file())
}

/// Jeux que Garry's Mod monte (`cfg/mountdepots.txt`, valeur « 1 ») : dossier d'installation sous `steamapps/common`,
/// sous-dossiers montés, contenu de repli livré avec Garry's Mod (`sourceengine/content_<nom>_dir.vpk`). Sa console le
/// dit à chaque démarrage : « Mounting game 'Half-Life 2 & Episodes' (hl2, 220) », et pour un jeu non installé
/// « Mounting game 'Counter-Strike: Source' as fallback (cstrike, 240) ».
const DEPOTS: [(&str, &str, &[&str]); 6] = [
    ("hl2", "Half-Life 2", &["hl2", "episodic", "ep2"]),
    ("cstrike", "Counter-Strike Source", &["cstrike"]),
    ("tf", "Team Fortress 2", &["tf"]),
    ("hl2mp", "Half-Life 2 Deathmatch", &["hl2mp"]),
    ("dod", "Day of Defeat Source", &["dod"]),
    ("portal", "Portal", &["portal"]),
];

/// Ce qu'il faut monter après le contenu de Garry's Mod (`gmod` : son dossier `GarrysMod`), dans l'ordre de
/// `mountdepots.txt` : d'abord le contenu livré avec Garry's Mod (`content_<nom>`, chez tous les joueurs depuis juillet
/// 2025, ce qui garde juste la provenance d'un fichier présent dans les deux), puis, pour un jeu installé dans un des
/// dossiers `communs` (`…/steamapps/common`), les `*_dir.vpk` puis le dossier de chacun de ses sous-dossiers. Un nom
/// inconnu est ignoré.
pub fn jeux_montes(gmod: &Path, communs: &[PathBuf]) -> Vec<PathBuf> {
    let Ok(t) = std::fs::read_to_string(gmod.join("garrysmod/cfg/mountdepots.txt")) else { return Vec::new() };
    let mut out = Vec::new();
    for l in t.lines() {
        let m: Vec<&str> = l.split('"').filter(|x| !x.trim().is_empty()).collect();
        let (Some(nom), Some(&"1")) = (m.first(), m.get(1)) else { continue };
        let Some((_, dossier, sous)) = DEPOTS.iter().find(|d| d.0.eq_ignore_ascii_case(nom)) else { continue };
        let livre = gmod.join(format!("sourceengine/content_{}_dir.vpk", nom.to_ascii_lowercase()));
        if livre.is_file() {
            out.push(livre);
        }
        if let Some(install) = communs.iter().map(|c| c.join(dossier)).find(|d| d.is_dir()) {
            for s in sous.iter().map(|s| install.join(s)).filter(|s| s.is_dir()) {
                let mut vpks: Vec<PathBuf> = std::fs::read_dir(&s).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.to_string_lossy().ends_with("_dir.vpk")).collect();
                vpks.sort();
                out.extend(vpks);
                out.push(s);
            }
        }
    }
    out
}

/// Les dossiers `steamapps/common` où chercher les jeux montés : celui de Garry's Mod d'abord, puis ceux de Steam.
pub fn dossiers_communs(gmod: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = gmod.parent().map(|c| vec![c.to_path_buf()]).unwrap_or_default();
    for steam in dossiers_steam() {
        for lib in bibliotheques(&steam) {
            let c = lib.join("steamapps/common");
            if !out.contains(&c) {
                out.push(c);
            }
        }
    }
    out
}

fn dossiers_steam() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(p) = steam_du_registre() {
        out.push(p);
    }
    for d in [r"C:\Program Files (x86)\Steam", r"C:\Program Files\Steam"] {
        let p = PathBuf::from(d);
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out.into_iter().filter(|p| p.is_dir()).collect()
}

#[cfg(windows)]
fn steam_du_registre() -> Option<PathBuf> {
    let sortie = std::process::Command::new("reg").args(["query", r"HKCU\Software\Valve\Steam", "/v", "SteamPath"]).output().ok()?;
    let t = String::from_utf8_lossy(&sortie.stdout);
    let ligne = t.lines().find(|l| l.contains("SteamPath"))?;
    let v = ligne.split("REG_SZ").nth(1)?.trim();
    Some(PathBuf::from(v.replace('/', "\\")))
}

#[cfg(not(windows))]
fn steam_du_registre() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".steam/steam"))
}

/// Les bibliothèques Steam : le dossier de Steam, puis celles de `libraryfolders.vdf` (celle qui déclare Garry's
/// Mod en premier).
fn bibliotheques(steam: &Path) -> Vec<PathBuf> {
    let mut out = vec![steam.to_path_buf()];
    let Ok(t) = std::fs::read_to_string(steam.join("steamapps/libraryfolders.vdf")) else { return out };
    let mut avec_gmod = Vec::new();
    let mut courant: Option<PathBuf> = None;
    for l in t.lines() {
        let morceaux: Vec<&str> = l.split('"').filter(|m| !m.trim().is_empty()).collect();
        if morceaux.len() >= 2 && morceaux[0] == "path" {
            let p = PathBuf::from(morceaux[1].replace("\\\\", "\\"));
            courant = Some(p.clone());
            if !out.contains(&p) {
                out.push(p);
            }
        }
        if morceaux.first() == Some(&APPID_GMOD) {
            if let Some(c) = &courant {
                avec_gmod.push(c.clone());
            }
        }
    }
    avec_gmod.extend(out);
    avec_gmod
}

#[cfg(test)]
mod tests {
    #[test]
    fn trouve_gmod_sur_ce_pc() {
        // sur un PC sans Garry's Mod, rien à vérifier
        if let Some(g) = super::garrysmod() {
            assert!(g.join("garrysmod/gameinfo.txt").is_file());
            assert!(super::bin_gmod().is_some());
        }
    }
}
