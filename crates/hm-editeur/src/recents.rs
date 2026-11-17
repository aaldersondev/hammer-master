//! Fichiers récents (menu Fichier) : un chemin par ligne dans `%LOCALAPPDATA%\HammerMaster\recents.txt`, le plus
//! récent d'abord.
use std::path::{Path, PathBuf};

const NOMBRE: usize = 10;

fn fichier() -> PathBuf {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("HammerMaster").join("recents.txt")
}

pub fn lire() -> Vec<PathBuf> {
    std::fs::read_to_string(fichier()).unwrap_or_default().lines().filter(|l| !l.trim().is_empty()).map(PathBuf::from).collect()
}

/// Met `p` en tête de la liste (ouvert ou enregistré à l'instant).
pub fn ajouter(p: &Path) {
    let f = fichier();
    let liste = avec(lire(), p);
    let _ = std::fs::create_dir_all(f.parent().unwrap_or(Path::new(".")));
    let _ = std::fs::write(f, liste.iter().map(|p| p.to_string_lossy() + "\n").collect::<String>());
}

/// La liste avec `p` en tête, sans doublon (casse ignorée : chemins Windows), `NOMBRE` au plus.
fn avec(liste: Vec<PathBuf>, p: &Path) -> Vec<PathBuf> {
    let p = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    let cle = |q: &Path| q.to_string_lossy().to_lowercase();
    let mut v = vec![p.clone()];
    v.extend(liste.into_iter().filter(|q| cle(q) != cle(&p)));
    v.truncate(NOMBRE);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_dernier_en_tete_sans_doublon() {
        let l = avec(vec![PathBuf::from("C:\\a.vmf"), PathBuf::from("C:\\b.vmf")], Path::new("C:\\B.VMF"));
        assert_eq!(l, vec![PathBuf::from("C:\\B.VMF"), PathBuf::from("C:\\a.vmf")]);
        let mut l = Vec::new();
        for i in 0..15 {
            l = avec(l, Path::new(&format!("C:\\m{i}.vmf")));
        }
        assert_eq!(l.len(), NOMBRE);
        assert_eq!(l[0], PathBuf::from("C:\\m14.vmf"));
    }
}
