//! Sur les fichiers du jeu installé : les noms des systèmes de particules lus dans tous les `.pcf`, les noms des
//! soundscripts déclarés par le manifeste.
use hm_assets::{particules, sons, Vfs};

fn vfs() -> Option<Vfs> {
    let gi = hm_assets::jeu::gameinfo_gmod()?;
    let mut v = Vfs::new();
    v.add_gameinfo(gi).ok()?;
    Some(v)
}

#[test]
fn particules_du_jeu() {
    let Some(v) = vfs() else { return };
    let pcf = v.list_dans("particles/", "pcf");
    assert!(!pcf.is_empty());
    let (mut lus, mut noms, mut erreurs) = (0, Vec::new(), Vec::new());
    for (f, _) in &pcf {
        match particules::systemes(&v.read(f).unwrap().bytes) {
            Ok(n) => {
                lus += 1;
                noms.extend(n);
            }
            Err(e) => erreurs.push(format!("{f} : {e}")),
        }
    }
    println!("{lus} fichiers lus sur {}, {} systèmes ; erreurs : {erreurs:?}", pcf.len(), noms.len());
    println!("exemples : {:?}", &noms[..noms.len().min(12)]);
    assert!(erreurs.is_empty());
    // des systèmes que les maps de HL2 et GMod emploient
    for n in ["explosion_huge", "fire_small_01", "smoke_burning_engine_01"] {
        assert!(noms.iter().any(|x| x.eq_ignore_ascii_case(n)), "{n} manque");
    }
}

#[test]
fn soundscripts_du_jeu() {
    let Some(v) = vfs() else { return };
    let manifeste = String::from_utf8_lossy(&v.read("scripts/game_sounds_manifest.txt").expect("manifeste").bytes).into_owned();
    let fichiers = sons::fichiers_du_manifeste(&manifeste);
    let noms: Vec<String> = fichiers.iter().filter_map(|f| v.read(f)).flat_map(|f| sons::noms_soundscript(&String::from_utf8_lossy(&f.bytes))).collect();
    println!("{} fichiers, {} sons", fichiers.len(), noms.len());
    assert!(noms.iter().any(|n| n.eq_ignore_ascii_case("Weapon_Pistol.Single")));
    assert!(noms.len() > 1000);
}
