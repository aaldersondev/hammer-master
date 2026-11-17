//! Bodygroups : la valeur `body` choisit un sous-modèle par bodypart, `(body / base) % n` ; un autre choix donne
//! une autre géométrie. Essayé sur les modèles du jeu (sauté sans Garry's Mod).
use hm_assets::model::{body_de, bodygroups, load_model, load_model_body, Partie};
use hm_assets::Vfs;

#[test]
fn valeur_body_et_choix() {
    let p = vec![
        Partie { nom: "a".into(), base: 1, sous_modeles: vec!["a0".into(), "a1".into(), "a2".into()] },
        Partie { nom: "b".into(), base: 3, sous_modeles: vec!["b0".into(), "b1".into()] },
    ];
    assert_eq!(bodygroups(&p, 0), vec![0, 0]);
    assert_eq!(bodygroups(&p, 5), vec![2, 1]);
    for a in 0..3 {
        for b in 0..2 {
            assert_eq!(bodygroups(&p, body_de(&p, &[a, b])), vec![a, b]);
        }
    }
}

#[test]
fn modele_du_jeu_avec_bodygroups() {
    let Some(gi) = hm_assets::jeu::gameinfo_gmod() else { return };
    let mut vfs = Vfs::new();
    vfs.add_gameinfo(&gi).unwrap();
    // le premier modèle des VPK du jeu dont un bodypart a plusieurs sous-modèles non vides
    let mut essais = 0;
    for (nom, _) in vfs.list_dans("models", "mdl") {
        essais += 1;
        if essais > 4000 {
            break;
        }
        let Ok(m) = load_model(&vfs, &nom) else { continue };
        let Some((i, p)) = m.parties.iter().enumerate().find(|(_, p)| p.sous_modeles.len() >= 2 && p.base > 0) else { continue };
        let mut choix = vec![0; m.parties.len()];
        choix[i] = 1;
        let body = body_de(&m.parties, &choix);
        let autre = load_model_body(&vfs, &nom, body).unwrap();
        if autre.triangles == m.triangles {
            continue; // sous-modèle vide ou identique : on cherche un cas qui se voit
        }
        println!("{nom} : bodypart « {} », {} sous-modèles ; body 0 → {} triangles, body {body} → {}", p.nom, p.sous_modeles.len(), m.triangles, autre.triangles);
        assert_eq!(bodygroups(&autre.parties, body)[i], 1);
        return;
    }
    panic!("aucun modèle à bodygroups trouvé dans {essais} essais");
}

#[test]
fn bandes_vtx_au_drapeau_zero() {
    // des portages de L4D (addon « extra » de Garry's Mod, vu sur rp_rockford) : bandes VTX au drapeau 0, que le jeu
    // et Hammer++ dessinent ; sautés sans ces fichiers
    let Some(gi) = hm_assets::jeu::gameinfo_gmod() else { return };
    let mut vfs = Vfs::new();
    vfs.add_gameinfo(&gi).unwrap();
    let mut vus = 0;
    for nom in ["models/props_interiors/chairs_airport.mdl", "models/props_equipment/snack_machine.mdl"] {
        let (Some(mdl), Some(vtx)) = (vfs.read(nom), vfs.read(&nom.replace(".mdl", ".dx90.vtx"))) else { continue };
        let version = i32::from_le_bytes(mdl.bytes[4..8].try_into().unwrap());
        let mut v = vtx.bytes.to_vec();
        let n = hm_assets::model::vtx_bandes_en_listes(&mut v, version);
        assert!(n > 0, "{nom} : aucune bande au drapeau 0");
        assert_eq!(hm_assets::model::vtx_bandes_en_listes(&mut v, version), 0, "déjà corrigées");
        let m = load_model(&vfs, nom).unwrap_or_else(|e| panic!("{e}"));
        println!("{nom} : {n} bande(s) corrigée(s), {} triangles", m.triangles);
        assert!(m.triangles > 100);
        vus += 1;
    }
    if vus == 0 {
        println!("modèles absents : test sauté");
    }
}

#[test]
fn pose_de_la_sequence_zero() {
    // le point d'apparition de l'éditeur : dans sa pose de référence, un personnage tourné d'un quart de tour ; Hammer
    // le montre dans la pose de sa séquence 0, de face vers +X comme tout modèle à l'angle 0 (vu côte à côte le 15/11
    // sur rp_rockford : nos 20 points d'apparition à 270° regardaient -X au lieu de -Y)
    let Some(gi) = hm_assets::jeu::gameinfo_gmod() else { return };
    let mut vfs = Vfs::new();
    vfs.add_gameinfo(&gi).unwrap();
    let nom = "models/editor/playerstart.mdl";
    let Ok(m) = load_model(&vfs, nom) else {
        println!("playerstart.mdl absent : test sauté");
        return;
    };
    let pieds = |pts: &mut dyn Iterator<Item = [f32; 3]>| pts.filter(|p| p[2] < 3.0).fold([f32::MAX, f32::MIN, f32::MAX, f32::MIN], |a, p| [a[0].min(p[0]), a[1].max(p[0]), a[2].min(p[1]), a[3].max(p[1])]);
    let pose = pieds(&mut m.vertices.iter().map(|v| v.pos));
    // la pose de référence, pour le contre-essai
    let (mdl, vvd, vtx) = (vfs.read(nom).unwrap(), vfs.read("models/editor/playerstart.vvd").unwrap(), vfs.read("models/editor/playerstart.dx90.vtx").unwrap());
    let sm = source_mdl::StudioModel::from_bytes(&mdl.bytes, &vvd.bytes, &vtx.bytes).unwrap();
    let geo = sm.geometry(&source_mdl::GeometryOptions::default()).unwrap();
    let repos = pieds(&mut geo.vertices.iter().map(|v| v.position));
    println!("pieds (x min, x max, y min, y max) : pose {pose:?}, référence {repos:?}");
    // pieds côte à côte selon Y, orteils vers +X
    assert!(pose[3] - pose[2] > pose[1] - pose[0], "épaules et pieds selon Y");
    assert!(pose[1] > -pose[0], "orteils vers +X");
    assert!(repos[1] - repos[0] > repos[3] - repos[2], "la référence est bien tournée (sinon le test ne prouve rien)");
}
