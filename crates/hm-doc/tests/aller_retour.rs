//! Aller-retour sans perte : relire puis réécrire un VMF sans le modifier.
//!
//! - les fichiers de `fixtures/` (écrits par Hammer++) doivent ressortir **à l'octet** ;
//! - ceux du corpus de ce PC (`corpus_pc.txt`, absents ailleurs) doivent ressortir à l'octet s'ils ont la mise en
//!   page de Hammer, et sinon avec le même sens : relus, ils redonnent le même document.
use std::path::Path;

use hm_doc::Document;

fn relu_pareil(src: &[u8]) -> Result<bool, String> {
    let (d, _) = Document::lire(src).map_err(|e| e.to_string())?;
    let out = d.ecrire();
    if out == src {
        return Ok(true);
    }
    let (d2, _) = Document::lire(&out).map_err(|e| e.to_string())?;
    // même sens : la réécriture de la réécriture est stable, et les objets sont les mêmes
    assert_eq!(d2.ecrire(), out, "la réécriture n'est pas stable");
    assert_eq!(d.nombre_objets(), d2.nombre_objets());
    for (a, b) in d.tous_les_solides().zip(d2.tous_les_solides()) {
        assert_eq!(a.1.faces.len(), b.1.faces.len());
        for (fa, fb) in a.1.faces.iter().zip(&b.1.faces) {
            assert_eq!((fa.plan, &fa.materiau, fa.u, fa.v), (fb.plan, &fb.materiau, fb.u, fb.v));
        }
    }
    for (a, b) in d.entites().zip(d2.entites()) {
        assert_eq!(a.cles, b.cles);
        assert_eq!(a.sorties, b.sorties);
    }
    Ok(false)
}

#[test]
fn fixtures_a_l_octet() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut n = 0;
    for f in std::fs::read_dir(&dir).unwrap() {
        let p = f.unwrap().path();
        if p.extension().map_or(false, |e| e == "vmf") {
            let src = std::fs::read(&p).unwrap();
            assert!(relu_pareil(&src).unwrap(), "{} ne ressort pas à l'octet", p.display());
            n += 1;
        }
    }
    assert!(n >= 3);
}

#[test]
fn corpus_du_pc() {
    let liste = include_str!("corpus_pc.txt");
    let (mut octet, mut sens, mut absents) = (Vec::new(), Vec::new(), 0);
    for l in liste.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let Ok(src) = std::fs::read(l) else {
            absents += 1;
            continue;
        };
        match relu_pareil(&src) {
            Ok(true) => octet.push(l),
            Ok(false) => sens.push(l),
            Err(e) => panic!("{l} : {e}"),
        }
    }
    eprintln!("corpus : {} à l'octet, {} même sens, {} absents", octet.len(), sens.len(), absents);
    for s in &sens {
        eprintln!("  même sens seulement : {s}");
    }
}
