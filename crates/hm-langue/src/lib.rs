//! Langue des textes montrés à l'utilisateur : le français écrit dans le source, ou l'anglais de `en.rs`.
//! L'éditeur la choisit (Outils → Options) ; le CLI et le MCP ne la changent pas et restent en français.
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

include!("en.rs");

static ANGLAIS: AtomicBool = AtomicBool::new(false);

pub fn choisir(anglais: bool) {
    ANGLAIS.store(anglais, Ordering::Relaxed)
}

pub fn anglais() -> bool {
    ANGLAIS.load(Ordering::Relaxed)
}

/// Le texte `fr` dans la langue choisie (tel quel s'il n'a pas de traduction).
pub fn t(fr: &'static str) -> &'static str {
    traduire(fr, anglais())
}

fn traduire(fr: &'static str, anglais: bool) -> &'static str {
    static TABLE: OnceLock<HashMap<&str, &str>> = OnceLock::new();
    if !anglais {
        return fr;
    }
    TABLE.get_or_init(|| PAIRES.iter().copied().collect()).get(fr).copied().unwrap_or(fr)
}

/// `format!` en deux langues : `tr!("français {x}", "english {x}", args…)` ; les deux gabarits sont vérifiés à la
/// compilation.
#[macro_export]
macro_rules! tr {
    ($fr:literal, $en:literal $(, $($a:tt)*)?) => {
        if $crate::anglais() { format!($en $(, $($a)*)?) } else { format!($fr $(, $($a)*)?) }
    };
}

/// Au premier lancement de l'éditeur : français si le système l'est, anglais sinon.
#[cfg(windows)]
pub fn systeme_anglais() -> bool {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetUserDefaultUILanguage() -> u16;
    }
    // langue primaire 0x0C : français (toutes variantes)
    (unsafe { GetUserDefaultUILanguage() } & 0x3ff) != 0x0c
}

#[cfg(not(windows))]
pub fn systeme_anglais() -> bool {
    !std::env::var("LANG").is_ok_and(|l| l.starts_with("fr"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traduit_ou_garde_le_francais() {
        assert_eq!(traduire("Fichier", true), "File");
        assert_eq!(traduire("Fichier", false), "Fichier");
        assert_eq!(traduire("Matériau introuvable", true), "Material not found");
        assert_eq!(traduire("pas dans la table", true), "pas dans la table");
        assert!(PAIRES.iter().all(|(fr, en)| !fr.trim().is_empty() && !en.trim().is_empty()));
        // une seule traduction par texte français
        let mut fr: Vec<&str> = PAIRES.iter().map(|(f, _)| *f).collect();
        fr.sort_unstable();
        fr.dedup();
        assert_eq!(fr.len(), PAIRES.len());
    }
}
