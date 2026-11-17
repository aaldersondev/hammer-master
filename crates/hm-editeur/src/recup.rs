//! Sauvegarde automatique et récupération après un plantage.
//!
//! Tant que la map a des modifications non enregistrées, elle est écrite toutes les minutes dans
//! `%LOCALAPPDATA%\HammerMaster\recuperation\` (et aussitôt après une erreur interne rattrapée). Une fermeture
//! normale efface ces fichiers ; au démarrage, ceux d'une session qui ne tourne plus sont proposés à la récupération.
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Ce qu'on sait d'une sauvegarde automatique.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Fiche {
    /// La copie de la map.
    pub vmf: PathBuf,
    /// Le fichier d'où venait la map (aucun pour une map jamais enregistrée).
    pub origine: Option<PathBuf>,
    /// Date de la copie (secondes depuis 1970).
    pub quand: u64,
    pub pid: u32,
    /// Nombre de modifications depuis l'ouverture.
    pub version: u64,
}

pub struct Recup {
    pub dossier: PathBuf,
    id: String,
    derniere: Option<Instant>,
    version: Option<u64>,
    pub intervalle: Duration,
}

pub fn dossier_par_defaut() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("HammerMaster").join("recuperation")
}

fn maintenant() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl Recup {
    pub fn new(dossier: PathBuf) -> Recup {
        let id = format!("{}_{}", std::process::id(), maintenant());
        Recup { dossier, id, derniere: None, version: None, intervalle: Duration::from_secs(60) }
    }

    fn chemins(&self) -> (PathBuf, PathBuf) {
        (self.dossier.join(format!("{}.vmf", self.id)), self.dossier.join(format!("{}.json", self.id)))
    }

    /// Écrit la copie si la map a changé depuis la dernière et que l'intervalle est passé (ou tout de suite si
    /// `maintenant`). Rend le chemin écrit.
    pub fn sauver(&mut self, octets: impl FnOnce() -> Vec<u8>, origine: Option<&Path>, version: u64, modifiee: bool, tout_de_suite: bool) -> Result<Option<PathBuf>, String> {
        if !modifiee {
            // tout est enregistré : la copie ne sert plus
            if self.version.is_some() {
                self.effacer();
            }
            return Ok(None);
        }
        if self.version == Some(version) {
            return Ok(None);
        }
        if !tout_de_suite && self.derniere.map_or(false, |t| t.elapsed() < self.intervalle) {
            return Ok(None);
        }
        std::fs::create_dir_all(&self.dossier).map_err(|e| format!("{} : {e}", self.dossier.display()))?;
        let (vmf, json) = self.chemins();
        hm_doc::session::ecrire_sur(&vmf, &octets())?;
        let fiche = Fiche { vmf: vmf.clone(), origine: origine.map(Path::to_path_buf), quand: maintenant(), pid: std::process::id(), version };
        std::fs::write(&json, serde_json::to_string_pretty(&fiche).unwrap_or_default()).map_err(|e| format!("{} : {e}", json.display()))?;
        self.derniere = Some(Instant::now());
        self.version = Some(version);
        Ok(Some(vmf))
    }

    /// La copie à jour de cette session, s'il y en a une.
    pub fn copie(&self) -> Option<PathBuf> {
        self.version.map(|_| self.chemins().0)
    }

    /// Fermeture normale : la copie de cette session disparaît.
    pub fn effacer(&mut self) {
        let (vmf, json) = self.chemins();
        let _ = std::fs::remove_file(vmf);
        let _ = std::fs::remove_file(json);
        self.version = None;
    }
}

/// Les sauvegardes laissées par des sessions qui ne tournent plus, de la plus récente à la plus ancienne.
pub fn a_recuperer(dossier: &Path) -> Vec<Fiche> {
    let Ok(lecture) = std::fs::read_dir(dossier) else { return Vec::new() };
    let mut out: Vec<Fiche> = lecture
        .flatten()
        .filter(|e| e.path().extension().map_or(false, |x| x == "json"))
        .filter_map(|e| serde_json::from_slice::<Fiche>(&std::fs::read(e.path()).ok()?).ok())
        .filter(|f| f.pid != std::process::id() && !processus_vivant(f.pid) && f.vmf.is_file())
        .collect();
    out.sort_by_key(|f| std::cmp::Reverse(f.quand));
    out
}

/// Abandonne une sauvegarde (après récupération ou refus).
pub fn oublier(f: &Fiche) {
    let _ = std::fs::remove_file(&f.vmf);
    let _ = std::fs::remove_file(f.vmf.with_extension("json"));
}

#[cfg(windows)]
fn processus_vivant(pid: u32) -> bool {
    std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(&format!("\"{pid}\"")))
        .unwrap_or(false)
}

#[cfg(not(windows))]
fn processus_vivant(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

/// Date lisible d'une fiche.
pub fn depuis(quand: u64) -> String {
    let s = maintenant().saturating_sub(quand);
    if s < 120 {
        tr!("il y a {s} s", "{s} s ago")
    } else if s < 7200 {
        tr!("il y a {} min", "{} min ago", s / 60)
    } else {
        tr!("il y a {} h", "{} h ago", s / 3600)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sauver_puis_retrouver() {
        let d = std::env::temp_dir().join(format!("hm_recup_essai_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let mut r = Recup::new(d.clone());
        // rien tant que la map n'est pas modifiée
        assert_eq!(r.sauver(|| b"x".to_vec(), None, 0, false, true).unwrap(), None);
        let p = r.sauver(|| b"versioninfo\n{\n}\n".to_vec(), Some(Path::new("C:/a/b.vmf")), 3, true, false).unwrap().unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"versioninfo\n{\n}\n");
        // même version : pas de réécriture ; intervalle pas écoulé : pas de réécriture
        assert_eq!(r.sauver(|| b"y".to_vec(), None, 3, true, false).unwrap(), None);
        assert_eq!(r.sauver(|| b"y".to_vec(), None, 4, true, false).unwrap(), None);
        assert!(r.sauver(|| b"y".to_vec(), None, 4, true, true).unwrap().is_some());
        // une session vivante (celle-ci) n'est pas proposée ; une fiche d'un processus mort l'est
        assert!(a_recuperer(&d).is_empty());
        let mort = Fiche { vmf: p.clone(), origine: None, quand: maintenant(), pid: 4_000_000_000, version: 4 };
        std::fs::write(d.join("autre.json"), serde_json::to_string(&mort).unwrap()).unwrap();
        let l = a_recuperer(&d);
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].pid, 4_000_000_000);
        // enregistrée : la copie s'efface
        r.sauver(|| Vec::new(), None, 4, false, false).unwrap();
        assert!(!p.exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
