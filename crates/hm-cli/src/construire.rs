//! `hm construire <spec.yaml>` : la spec du skill gmod-map-maker → un VMF, son plan et son modèle sémantique.
use std::path::{Path, PathBuf};

use anyhow::Result;
use hm_edit::construction::{bilan, construire_sur_disque};

use crate::Args;

pub fn lancer(chemin: &Path, args: &Args) -> Result<()> {
    // les fichiers du jeu : pour savoir si un modèle existe, où est son bas, et si vbsp l'accepte en prop_static
    let mut vfs = crate::monter(chemin, args)?;
    let sortie = args.get("-o").map(PathBuf::from);
    let (c, sortie) = construire_sur_disque(chemin, sortie.as_deref(), args.get("--phase"), args.get("--modules").map(PathBuf::from), &mut vfs)?;
    println!("{}", bilan(&c, &sortie));
    if !c.erreurs.is_empty() {
        std::process::exit(1);
    }
    Ok(())
}
