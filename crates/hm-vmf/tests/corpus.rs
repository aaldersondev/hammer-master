//! Aller-retour sur les VMF réels de ce PC : `écrire(lire(f)) == f`, à l'octet.
//! Les dossiers absents sont sautés ; le test échoue si aucun fichier n'a été trouvé.
use std::path::{Path, PathBuf};
use std::time::Instant;

fn walk(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if depth > 0 {
                walk(&p, out, depth - 1);
            }
        } else if p.extension().map_or(false, |x| x.eq_ignore_ascii_case("vmf")) {
            out.push(p);
        }
    }
}

#[test]
fn aller_retour_sur_le_corpus() {
    let home = std::env::var("USERPROFILE").unwrap_or_default();
    let mut files = Vec::new();
    for (dir, depth) in [
        (format!("{home}/Desktop/rp_megalopole"), 5),
        (format!("{home}/.claude/skills/gmod-map-maker"), 4),
        ("D:/etude_maps".to_string(), 2),
        (format!("{home}/Desktop/hammer-master/tests/vmf"), 3),
    ] {
        walk(Path::new(&dir), &mut files, depth);
    }
    assert!(!files.is_empty(), "aucun VMF trouvé : le corpus a bougé");
    let (mut octets, mut solides, mut entites) = (0u64, 0usize, 0usize);
    let t = Instant::now();
    let mut refus = Vec::new();
    for p in &files {
        let src = std::fs::read(p).unwrap();
        octets += src.len() as u64;
        match hm_vmf::Vmf::parse(src.clone()) {
            Ok(v) => {
                assert!(v.to_bytes() == src, "aller-retour différent : {}", p.display());
                solides += v.world().map_or(0, |w| w.solids().count()) + v.entities().map(|e| e.solids().count()).sum::<usize>();
                entites += v.entities().count();
            }
            Err(e) => refus.push(format!("{} : {e}", p.display())),
        }
    }
    println!(
        "{} VMF, {:.0} Mo, {} solides, {} entités, lus et réécrits à l'identique en {:.1} s ; {} refusé(s)",
        files.len() - refus.len(),
        octets as f64 / 1e6,
        solides,
        entites,
        t.elapsed().as_secs_f32(),
        refus.len()
    );
    for r in &refus {
        println!("  refusé : {r}");
    }
    assert!(refus.len() * 20 <= files.len(), "trop de VMF refusés");
}
