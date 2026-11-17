//! Joueur simulé : ce qui est accessible à pied depuis les points d'apparition, et d'où l'on ne revient pas.
//!
//! Le calcul se fait sur le champ de colonnes de 16 de `hm-espace`, portes retirées et tout le mobilier solide
//! compté. Les pleins y sont élargis de 8 : un passage de moins de 48 de large peut y paraître fermé, et les
//! espaces de moins de 72 de haut (où l'on passe accroupi) n'y sont pas. Les constats de cette famille disent donc
//! « à vérifier » plutôt que « faux ».
//!
//! Mouvements, d'après docs/limites_et_dimensions.md : marche de 18 au plus, saut accroupi de 64 (68 mesurés par
//! le wiki, pris à 64 pour rester du côté sûr), chute de toute hauteur à sens unique. Liaisons : échelles
//! (`func_ladder`, `func_useableladder`), téléporteurs (`trigger_teleport`), plateformes et ascenseurs
//! (`func_movelinear`, `func_door` verticale), et celles que le projet déclare (`[[liaison]]`).
use std::collections::VecDeque;

use glam::Vec3;
use hm_espace::{Field, Options, CELL, PLAYER_H, SAUT, STEP};
use hm_scene::Scene;
use serde::Serialize;

use crate::{Config, Constat, Niveau, Regle};

pub const REGLES: &[Regle] = &[
    Regle { id: "joueur.inaccessible", niveau: Niveau::Information, titre: "Pièce inaccessible", explication: "Une pièce où un joueur tiendrait debout n'est reliée à aucun point d'apparition : ni à pied, ni par une échelle, un ascenseur ou un téléporteur connus.", source: "joueur simulé, maille de 16 ; à vérifier en jeu (passages de moins de 48, portes de map faites en Lua)" },
    Regle { id: "sem.modele_invalide", niveau: Niveau::Erreur, titre: "Modèle sémantique illisible", explication: "Le fichier <carte>.sem.json existe mais ne se lit pas ou se contredit : identifiant en double, volume à l'envers, clé inconnue.", source: "hm-model, contrôle du fichier" },
    Regle { id: "sem.piece_inaccessible", niveau: Niveau::Avertissement, titre: "Pièce déclarée inaccessible", explication: "Une pièce du modèle sémantique n'est reliée à aucun point d'apparition : ni à pied, ni par une liaison connue ou déclarée.", source: "joueur simulé sur les pièces du modèle (<carte>.sem.json) ; maille de 16, à vérifier en jeu pour une porte de moins de 48" },
    Regle { id: "sem.piece_sans_sol", niveau: Niveau::Information, titre: "Pièce déclarée sans sol libre", explication: "Aucun endroit où tenir debout n'a été trouvé dans la boîte d'une pièce du modèle : pièce pleine, ou boîte mal déclarée.", source: "joueur simulé sur les pièces du modèle" },
    Regle { id: "geo.nodraw_visible", niveau: Niveau::Information, titre: "Face sans texture visible", explication: "Une face en nodraw donne sur l'air d'un endroit où un joueur peut se tenir : en jeu, on voit à travers. À confirmer en jeu : aucun cas n'a encore été vérifié.", source: "contrôle propre à l'outil, écrit pour les signalements L02 et L18 ; ceux-là venaient en fait d'un modèle, pas d'une face de solide" },
    Regle { id: "joueur.piege", niveau: Niveau::Avertissement, titre: "Piège", explication: "On peut y tomber depuis un point d'apparition, mais aucun chemin n'en revient.", source: "joueur simulé : les chutes sont à sens unique" },
    Regle { id: "joueur.sans_apparition", niveau: Niveau::Erreur, titre: "Aucun point d'apparition", explication: "La map n'a aucune entité info_player_* posée dans l'espace libre.", source: "contrôle propre à l'outil" },
];

#[derive(Clone, Debug, Default, Serialize)]
pub struct Bilan {
    pub apparitions: usize,
    /// Surfaces en unités carrées.
    pub surface_praticable: f64,
    pub surface_accessible: f64,
    pub echelles: usize,
    pub teleporteurs: usize,
    pub plateformes: usize,
    pub liaisons_declarees: usize,
    pub pieces: usize,
    pub pieces_inaccessibles: usize,
    pub pieges: usize,
    /// Creux sans retour que la simulation ne sait pas juger : (gaines d'ascenseur, bassins d'eau, relief).
    pub non_juges: (usize, usize, usize),
    /// Pièces déclarées par le modèle où aucun sol libre n'a été trouvé.
    pub pieces_sans_sol: usize,
    /// Pièces sans chemin mais posées sur le relief, que la simulation ne sait pas juger.
    pub pieces_sur_relief: usize,
    /// Liaisons déclarées par le projet dont un bout ne tombe sur aucun sol.
    pub liaisons_sans_sol: Vec<String>,
    pub duree: f32,
}

pub fn resume(b: &Bilan) -> String {
    hm_langue::tr!("joueur simulé : {} point(s) d'apparition ; {:.0} % de la surface praticable est accessible ({:.2} sur {:.2} millions d'unités carrées) ; liaisons : {} échelle(s), {} téléporteur(s), {} plateforme(s) ou ascenseur(s), {} déclarée(s) ; {} pièce(s) sur {} inaccessible(s), {} piège(s) ; non jugés : {} gaine(s) d'ascenseur, {} bassin(s) d'eau, {} creux du relief, {} pièce(s) sur le relief{} ; {:.1} s", "simulated player: {} spawn point(s); {:.0} % of the walkable surface is reachable ({:.2} of {:.2} million square units); links: {} ladder(s), {} teleporter(s), {} platform(s) or elevator(s), {} declared; {} room(s) of {} unreachable, {} trap(s); not judged: {} elevator shaft(s), {} water pool(s), {} terrain hollow(s), {} room(s) on terrain{}; {:.1} s",
        b.apparitions,
        if b.surface_praticable > 0.0 { b.surface_accessible / b.surface_praticable * 100.0 } else { 0.0 },
        b.surface_accessible / 1e6,
        b.surface_praticable / 1e6,
        b.echelles,
        b.teleporteurs,
        b.plateformes,
        b.liaisons_declarees,
        b.pieces_inaccessibles,
        b.pieces,
        b.pieges,
        b.non_juges.0,
        b.non_juges.1,
        b.non_juges.2,
        b.pieces_sur_relief,
        if b.liaisons_sans_sol.is_empty() { String::new() } else { hm_langue::tr!(" ; {} liaison(s) déclarée(s) sans sol à un bout : {}", "; {} declared link(s) without floor at one end: {}", b.liaisons_sans_sol.len(), b.liaisons_sans_sol.iter().take(6).cloned().collect::<Vec<_>>().join(", ")) },
        b.duree
    )
}

/// Classes de portes : ouvertes pour le joueur simulé.
fn est_porte(class: &str) -> bool {
    class.starts_with("prop_door") || class == "func_door" || class == "func_door_rotating" || class == "func_movelinear" || class == "func_breakable" || class == "func_breakable_surf" || class == "func_tracktrain" || class == "func_rotating" || class == "func_physbox" || class.starts_with("prop_physics")
}

struct Graphe {
    /// Arêtes sortantes : pour chaque nœud, ses successeurs.
    debut: Vec<u32>,
    vers: Vec<u32>,
}

impl Graphe {
    fn de(n: usize, mut aretes: Vec<(u32, u32)>) -> Graphe {
        aretes.sort_unstable();
        aretes.dedup();
        let mut debut = vec![0u32; n + 1];
        for (a, _) in &aretes {
            debut[*a as usize + 1] += 1;
        }
        for i in 0..n {
            debut[i + 1] += debut[i];
        }
        Graphe { debut, vers: aretes.into_iter().map(|a| a.1).collect() }
    }
    fn suivants(&self, i: u32) -> &[u32] {
        &self.vers[self.debut[i as usize] as usize..self.debut[i as usize + 1] as usize]
    }
    fn parcourir(&self, departs: &[u32]) -> Vec<bool> {
        let mut vu = vec![false; self.debut.len() - 1];
        let mut file: VecDeque<u32> = VecDeque::new();
        for &d in departs {
            if !vu[d as usize] {
                vu[d as usize] = true;
                file.push_back(d);
            }
        }
        while let Some(i) = file.pop_front() {
            for &j in self.suivants(i) {
                if !vu[j as usize] {
                    vu[j as usize] = true;
                    file.push_back(j);
                }
            }
        }
        vu
    }
}

/// L'intervalle libre d'une colonne dont le sol est le plus proche de `z` (à `tol` près).
fn noeud_pres(f: &Field, p: Vec3, tol: f32) -> Option<u32> {
    let (ix, iy) = (((p.x - f.x0) / CELL).floor() as i64, ((p.y - f.y0) / CELL).floor() as i64);
    let mut best: Option<(f32, u32)> = None;
    for dy in -2..=2i64 {
        for dx in -2..=2i64 {
            let (x, y) = (ix + dx, iy + dy);
            if x < 0 || y < 0 || x >= f.nx as i64 || y >= f.ny as i64 {
                continue;
            }
            for j in f.intervalles(y as usize * f.nx + x as usize) {
                let d = (f.free[j].floor as f32 - p.z).abs() + (dx.abs() + dy.abs()) as f32 * 4.0;
                if (f.free[j].floor as f32 - p.z).abs() <= tol && best.map_or(true, |b| d < b.0) {
                    best = Some((d, j as u32));
                }
            }
        }
    }
    best.map(|b| b.1)
}

/// Le résultat brut de la simulation : le champ du joueur et, pour chaque intervalle libre, s'il est atteint depuis
/// un point d'apparition.
pub struct Simulation {
    pub champ: Field,
    pub atteint: Vec<bool>,
}

impl Simulation {
    /// Vrai si un joueur parti d'un point d'apparition peut se tenir à cet endroit (sol à `tol` près, dans la
    /// colonne du point ou une voisine).
    pub fn accessible(&self, p: Vec3, tol: f32) -> Option<bool> {
        noeud_pres(&self.champ, p, tol).map(|j| self.atteint[j as usize])
    }

    /// Vrai si le point est dans l'air d'un endroit où un joueur parti d'un point d'apparition peut se tenir :
    /// entre le sol et le plafond d'un intervalle atteint de sa colonne.
    pub fn dans_espace_atteint(&self, p: Vec3) -> bool {
        let f = &self.champ;
        let (ix, iy) = (((p.x - f.x0) / CELL).floor() as i64, ((p.y - f.y0) / CELL).floor() as i64);
        if ix < 0 || iy < 0 || ix >= f.nx as i64 || iy >= f.ny as i64 {
            return false;
        }
        f.intervalles(iy as usize * f.nx + ix as usize).any(|j| self.atteint[j] && (f.free[j].floor as f32) <= p.z && p.z <= (f.free[j].ceil as f32).min(f.free[j].floor as f32 + 512.0))
    }
}

pub fn simuler(scene: &Scene, config: &Config, modele: Option<&hm_model::Modele>) -> (Bilan, Vec<Constat>, Option<Simulation>) {
    let t = std::time::Instant::now();
    let mut bilan = Bilan::default();
    let mut out = Vec::new();
    // ---- le champ du joueur : portes ouvertes, tout le mobilier solide
    let garder = |o: u32| !est_porte(&scene.objects[o as usize].class);
    let f = Field::build_avec(scene, &Options { garder: &garder, tous_les_modeles: true, pieces: false });
    let n = f.free.len();
    let dirs: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
    // ---- arêtes : marche, saut, chute
    let mut aretes: Vec<(u32, u32)> = Vec::with_capacity(n * 4);
    for i in 0..n {
        let s = f.free[i];
        let (cx, cy) = ((s.col as usize % f.nx) as i32, (s.col as usize / f.nx) as i32);
        for (dx, dy) in dirs {
            let (x, y) = (cx + dx, cy + dy);
            if x < 0 || y < 0 || x >= f.nx as i32 || y >= f.ny as i32 {
                continue;
            }
            for j in f.intervalles(y as usize * f.nx + x as usize) {
                let o = f.free[j];
                let d = o.floor as i32 - s.floor as i32;
                let passe = if d.abs() <= STEP {
                    // marche : 72 de haut en commun
                    (s.ceil.min(o.ceil) as i32 - s.floor.max(o.floor) as i32) >= PLAYER_H
                } else if d > 0 {
                    // saut : il faut la place de monter dans sa propre colonne
                    d <= SAUT && s.ceil as i32 >= o.floor as i32 + PLAYER_H
                } else {
                    // chute : la colonne voisine est ouverte à la hauteur d'où l'on vient
                    o.ceil as i32 >= s.floor as i32 + PLAYER_H
                };
                if passe {
                    aretes.push((i as u32, j as u32));
                }
            }
        }
    }
    // ---- liaisons
    let lier = |a: Option<u32>, b: Option<u32>, deux: bool, aretes: &mut Vec<(u32, u32)>| -> bool {
        match (a, b) {
            (Some(a), Some(b)) => {
                aretes.push((a, b));
                if deux {
                    aretes.push((b, a));
                }
                true
            }
            _ => false,
        }
    };
    let boite_de = |e: &hm_vmf::Node| -> Option<(Vec3, Vec3)> {
        let id = e.id()?;
        let mut it = scene.brushes.iter().filter(|b| b.entity_id == id && b.object != 0).map(|b| (scene.objects[b.object as usize].min, scene.objects[b.object as usize].max));
        let premier = it.next()?;
        Some(it.fold(premier, |(lo, hi), (a, b)| (lo.min(a), hi.max(b))))
    };
    // gaines d'ascenseur : le volume balayé par une cabine. Ce qui s'y passe (on y « tombe », on n'en « sort » pas
    // quand la porte palière fait moins de 48) n'est pas jugé.
    let mut gaines: Vec<(Vec3, Vec3)> = Vec::new();
    let destinations: std::collections::HashMap<String, Vec3> = scene.points.iter().filter(|p| !p.name.is_empty()).map(|p| (p.name.to_ascii_lowercase(), p.pos)).collect();
    for e in scene.vmf.entities() {
        let c = e.classname().to_ascii_lowercase();
        match c.as_str() {
            "func_ladder" | "func_useableladder" => {
                // l'échelle relie le sol à son pied et le sol à son sommet, autour de son volume
                let (lo, hi) = match (c.as_str(), boite_de(e)) {
                    (_, Some(b)) => b,
                    ("func_useableladder", None) => {
                        let (Some(a), Some(b)) = (e.vec3("point0"), e.vec3("point1")) else { continue };
                        let (a, b) = (Vec3::new(a[0] as f32, a[1] as f32, a[2] as f32), Vec3::new(b[0] as f32, b[1] as f32, b[2] as f32));
                        (a.min(b), a.max(b))
                    }
                    _ => continue,
                };
                let centre = (lo + hi) * 0.5;
                let bas = noeud_pres(&f, Vec3::new(centre.x, centre.y, lo.z), 40.0);
                let haut = noeud_pres(&f, Vec3::new(centre.x, centre.y, hi.z), 40.0);
                if lier(bas, haut, true, &mut aretes) {
                    bilan.echelles += 1;
                }
            }
            "trigger_teleport" => {
                let Some((lo, hi)) = boite_de(e) else { continue };
                let Some(&dest) = e.get("target").and_then(|t| destinations.get(&t.to_ascii_lowercase())) else { continue };
                let centre = (lo + hi) * 0.5;
                let a = noeud_pres(&f, Vec3::new(centre.x, centre.y, lo.z), (hi.z - lo.z).max(64.0));
                if lier(a, noeud_pres(&f, dest, 64.0), false, &mut aretes) {
                    bilan.teleporteurs += 1;
                }
            }
            "func_movelinear" | "func_door" => {
                // une plateforme qui monte ou descend : son plancher relie ses deux arrêts
                let Some((lo, hi)) = boite_de(e) else { continue };
                let a = e.vec3("movedir").unwrap_or([0.0; 3]);
                let dir = hm_geom::angles_forward(a[0], a[1]).as_vec3();
                let dist = if c == "func_movelinear" {
                    e.f64("movedistance").unwrap_or(0.0) as f32
                } else {
                    // func_door : sa course est sa taille dans la direction du mouvement, moins `lip`
                    ((hi - lo) * dir.abs()).length() - e.f64("lip").unwrap_or(0.0) as f32
                };
                if dir.z.abs() < 0.9 || dist < 48.0 || (hi.x - lo.x).min(hi.y - lo.y) < 40.0 {
                    continue; // une porte coulissante, pas un ascenseur
                }
                let centre = (lo + hi) * 0.5;
                let depart = noeud_pres(&f, Vec3::new(centre.x, centre.y, lo.z), 48.0).or_else(|| noeud_pres(&f, Vec3::new(centre.x, centre.y, hi.z), 48.0));
                let z_arrivee = lo.z + dir.z * dist;
                let arrivee = noeud_pres(&f, Vec3::new(centre.x, centre.y, z_arrivee), 48.0).or_else(|| noeud_pres(&f, Vec3::new(centre.x, centre.y, z_arrivee + (hi.z - lo.z)), 48.0));
                if lier(depart, arrivee, true, &mut aretes) {
                    bilan.plateformes += 1;
                }
                let v = dir * dist;
                gaines.push((lo.min(lo + v) - Vec3::splat(48.0), hi.max(hi + v) + Vec3::splat(48.0)));
            }
            _ => {}
        }
    }
    // les liaisons du projet : celles de ses réglages et celles de son modèle sémantique
    let du_modele: Vec<crate::Liaison> = modele.map_or(Vec::new(), |m| m.liaisons.iter().map(|l| crate::Liaison { de: l.de, vers: l.vers, nom: l.id.clone(), aller_retour: l.aller_retour }).collect());
    for l in config.liaison.iter().chain(&du_modele) {
        if lier(noeud_pres(&f, Vec3::from(l.de), 64.0), noeud_pres(&f, Vec3::from(l.vers), 64.0), l.aller_retour, &mut aretes) {
            bilan.liaisons_declarees += 1;
        } else {
            // une liaison du projet dont un bout ne tombe sur aucun sol : à dire, sinon tout ce qu'elle dessert
            // sort « inaccessible » sans raison apparente
            bilan.liaisons_sans_sol.push(if l.nom.is_empty() { format!("{:?} → {:?}", l.de, l.vers) } else { l.nom.clone() });
        }
        // une liaison presque verticale est une gaine d'ascenseur
        let (a, b) = (Vec3::from(l.de), Vec3::from(l.vers));
        if (a - b).truncate().length() < 96.0 {
            gaines.push((a.min(b) - Vec3::new(96.0, 96.0, 32.0), a.max(b) + Vec3::new(96.0, 96.0, 160.0)));
        }
    }
    // ---- départs
    let departs: Vec<u32> = scene.points.iter().filter(|p| p.class.starts_with("info_player_")).filter_map(|p| noeud_pres(&f, p.pos, 96.0)).collect();
    bilan.apparitions = departs.len();
    if departs.is_empty() {
        out.push(Constat::lieu("joueur.sans_apparition", scene.min, scene.max, hm_langue::t("aucun point d'apparition (info_player_*) posé dans l'espace libre").to_string()));
        bilan.duree = t.elapsed().as_secs_f32();
        return (bilan, out, None);
    }
    let inverse = Graphe::de(n, aretes.iter().map(|(a, b)| (*b, *a)).collect());
    let direct = Graphe::de(n, aretes);
    let atteint = direct.parcourir(&departs);
    let revient = inverse.parcourir(&departs);
    let aire = (CELL * CELL) as f64;
    bilan.surface_praticable = n as f64 * aire;
    bilan.surface_accessible = atteint.iter().filter(|b| **b).count() as f64 * aire;

    // ---- pièces déclarées par le modèle sémantique : jugées une à une, sur leur propre emprise
    if let Some(m) = modele {
        for piece in &m.pieces {
            let e = piece.emprise;
            let (mut cellules, mut atteintes) = (0u32, 0u32);
            let (ix0, ix1) = ((((e[0] + 16.0 - f.x0) / CELL).floor() as i64).max(0), (((e[2] - 16.0 - f.x0) / CELL).floor() as i64).min(f.nx as i64 - 1));
            let (iy0, iy1) = ((((e[1] + 16.0 - f.y0) / CELL).floor() as i64).max(0), (((e[3] - 16.0 - f.y0) / CELL).floor() as i64).min(f.ny as i64 - 1));
            for iy in iy0..=iy1 {
                for ix in ix0..=ix1 {
                    for j in f.intervalles(iy as usize * f.nx + ix as usize) {
                        let sol = f.free[j].floor as f32;
                        if sol >= piece.sol - 24.0 && sol <= piece.sol + 40.0 {
                            cellules += 1;
                            atteintes += atteint[j] as u32;
                        }
                    }
                }
            }
            bilan.pieces += 1;
            let (lo, hi) = (Vec3::new(e[0], e[1], piece.sol), Vec3::new(e[2], e[3], piece.sol + piece.hauteur.unwrap_or(128.0)));
            let quoi = hm_langue::tr!("pièce « {} » ({}, {:.0} × {:.0}, sol à {:.0})", "room \"{}\" ({}, {:.0} × {:.0}, floor at {:.0})", piece.id, piece.role, piece.largeur(), piece.profondeur(), piece.sol);
            if cellules == 0 {
                bilan.pieces_sans_sol += 1;
                out.push(Constat::lieu("sem.piece_sans_sol", lo, hi, hm_langue::tr!("{quoi} : aucun sol libre trouvé à cette altitude — la pièce est pleine, ou le modèle ne dit pas la bonne boîte", "{quoi}: no free floor found at this height — the room is full, or the model gives the wrong box")));
            } else if atteintes == 0 {
                bilan.pieces_inaccessibles += 1;
                out.push(Constat::lieu("sem.piece_inaccessible", lo, hi, hm_langue::tr!("{quoi} : aucun chemin depuis un point d'apparition", "{quoi}: no path from a spawn point")).mesure(piece.surface()));
            }
        }
    }
    // ---- sans modèle : les pièces du champ ordinaire (celui du tour), jugées sur le champ du joueur
    let pieces = Field::build(scene);
    let mut par_region: std::collections::HashMap<u32, (u32, u32)> = std::collections::HashMap::new(); // région → (cellules, atteintes)
    for i in 0..pieces.free.len() {
        let reg = pieces.region[i];
        if reg == 0 {
            continue;
        }
        let p = pieces.pos(i);
        let e = par_region.entry(reg).or_default();
        e.0 += 1;
        if noeud_colonne(&f, p, 24.0).map_or(false, |j| atteint[j as usize]) {
            e.1 += 1;
        }
    }
    for r in pieces.regions.iter().filter(|r| r.is_room() && modele.is_none()) {
        bilan.pieces += 1;
        let (cellules, atteintes) = par_region.get(&r.id).copied().unwrap_or((0, 0));
        if cellules >= 24 && atteintes == 0 {
            // sur le relief (displacement), le sol n'est connu que par des boîtes : des marches qui n'existent pas y
            // coupent les chemins. Ces pièces-là ne sont pas jugées.
            let c = r.centre;
            if scene.disp_cells.iter().any(|(a, b)| c.x >= a.x && c.x <= b.x && c.y >= a.y && c.y <= b.y && c.z >= a.z - 64.0 && c.z <= b.z + 64.0) {
                bilan.pieces_sur_relief += 1;
                continue;
            }
            bilan.pieces_inaccessibles += 1;
            out.push(
                Constat::lieu("joueur.inaccessible", r.min, Vec3::new(r.max.x, r.max.y, r.floor + 96.0), hm_langue::tr!("pièce n° {} ({:.0} × {:.0}, sol à {:.0}) : aucun chemin depuis un point d'apparition — à vérifier", "room #{} ({:.0} × {:.0}, floor at {:.0}): no path from a spawn point — to check", r.id, r.max.x - r.min.x, r.max.y - r.min.y, r.floor))
                    .mesure(r.area()),
            );
        }
    }
    // ---- l'eau : la boîte de chaque solide d'eau
    let eau: std::collections::HashSet<&str> = scene.materials.iter().filter(|m| m.shader.starts_with("water")).map(|m| m.name.as_str()).collect();
    let eaux: Vec<(Vec3, Vec3)> = scene.brushes.iter().filter(|b| b.object != 0 && b.brush.faces.iter().any(|f| eau.contains(f.material.as_str()))).map(|b| (scene.objects[b.object as usize].min, scene.objects[b.object as usize].max)).collect();
    // ---- pièges : atteints, sans retour ; regroupés par voisinage
    let mut vu = vec![false; n];
    for i in 0..n {
        if !(atteint[i] && !revient[i]) || vu[i] {
            continue;
        }
        let mut groupe = vec![i as u32];
        vu[i] = true;
        let mut k = 0;
        while k < groupe.len() {
            let a = groupe[k];
            k += 1;
            for &j in direct.suivants(a).iter().chain(inverse.suivants(a)) {
                if atteint[j as usize] && !revient[j as usize] && !vu[j as usize] {
                    vu[j as usize] = true;
                    groupe.push(j);
                }
            }
        }
        if groupe.len() < 16 {
            continue; // moins de 64 × 64 : un dessus de meuble, un rebord
        }
        let lo = groupe.iter().fold(Vec3::splat(f32::MAX), |a, &j| a.min(f.pos(j as usize)));
        let hi = groupe.iter().fold(Vec3::splat(f32::MIN), |a, &j| a.max(f.pos(j as usize)));
        // Trois cas que la simulation ne sait pas juger : une gaine d'ascenseur, un bassin (on y nage, et la nage
        // n'est pas simulée), un creux du relief (le terrain en displacement n'est connu que par des boîtes).
        let c = (lo + hi) * 0.5;
        let dans = |b: &(Vec3, Vec3)| c.x >= b.0.x && c.x <= b.1.x && c.y >= b.0.y && c.y <= b.1.y && c.z >= b.0.z - 16.0 && c.z <= b.1.z;
        if gaines.iter().any(dans) {
            bilan.non_juges.0 += 1;
            continue;
        }
        if eaux.iter().any(dans) {
            bilan.non_juges.1 += 1;
            continue;
        }
        if scene.disp_cells.iter().any(|(a, b)| c.x >= a.x && c.x <= b.x && c.y >= a.y && c.y <= b.y && lo.z <= b.z + 48.0 && lo.z >= a.z - 48.0) {
            bilan.non_juges.2 += 1;
            continue;
        }
        bilan.pieges += 1;
        out.push(
            Constat::lieu("joueur.piege", lo - Vec3::splat(8.0), hi + Vec3::new(8.0, 8.0, 96.0), hm_langue::tr!("on peut tomber ici ({:.0} × {:.0}, sol vers {:.0}) et aucun chemin n'en revient — à vérifier", "you can fall here ({:.0} × {:.0}, floor near {:.0}) and no path leads back — to check", hi.x - lo.x + 16.0, hi.y - lo.y + 16.0, lo.z))
                .mesure(groupe.len() as f32 * CELL * CELL),
        );
    }
    bilan.duree = t.elapsed().as_secs_f32();
    (bilan, out, Some(Simulation { champ: f, atteint }))
}

/// Une aire du maillage de navigation du jeu : son identifiant et son centre.
pub struct AireNav {
    pub id: u32,
    pub centre: Vec3,
    pub taille: Vec3,
}

/// Lit les aires d'un fichier `.nav` (versions 9 à 16 ; celui de Garry's Mod est en version 16).
/// Disposition lue dans le SDK 2013, `game/server/nav_file.cpp` et `nav_area.cpp` (`CNavArea::Load`) ; la lecture
/// se contrôle elle-même : elle doit consommer exactement le nombre d'aires annoncé sans sortir du fichier, et les
/// coins des aires doivent être dans les limites de la map.
pub fn lire_nav(octets: &[u8]) -> anyhow::Result<Vec<AireNav>> {
    struct L<'a>(&'a [u8], usize);
    impl L<'_> {
        fn n(&mut self, k: usize) -> anyhow::Result<&[u8]> {
            anyhow::ensure!(self.1 + k <= self.0.len(), hm_langue::tr!("fichier .nav trop court à l'octet {}", ".nav file too short at byte {}", self.1));
            self.1 += k;
            Ok(&self.0[self.1 - k..self.1])
        }
        fn u8(&mut self) -> anyhow::Result<u32> {
            Ok(self.n(1)?[0] as u32)
        }
        fn u16(&mut self) -> anyhow::Result<u32> {
            Ok(u16::from_le_bytes(self.n(2)?.try_into().unwrap()) as u32)
        }
        fn u32(&mut self) -> anyhow::Result<u32> {
            Ok(u32::from_le_bytes(self.n(4)?.try_into().unwrap()))
        }
        fn f32(&mut self) -> anyhow::Result<f32> {
            Ok(f32::from_le_bytes(self.n(4)?.try_into().unwrap()))
        }
    }
    let mut l = L(octets, 0);
    anyhow::ensure!(l.u32()? == 0xFEED_FACE, hm_langue::tr!("ce n'est pas un fichier .nav", "this is not a .nav file"));
    let version = l.u32()?;
    anyhow::ensure!((9..=16).contains(&version), hm_langue::tr!("version {version} du .nav non prise en charge", ".nav version {version} not supported"));
    if version >= 10 {
        l.u32()?; // sous-version
    }
    l.u32()?; // taille du BSP
    if version >= 14 {
        l.u8()?; // analysé
    }
    for _ in 0..l.u16()? {
        let k = l.u16()? as usize;
        l.n(k)?;
    }
    if version > 11 {
        l.u8()?; // aires sans nom de lieu
    }
    let n = l.u32()? as usize;
    anyhow::ensure!(n < 2_000_000, hm_langue::tr!("nombre d'aires invraisemblable : {n}", "implausible number of areas: {n}"));
    let mut aires = Vec::with_capacity(n);
    for _ in 0..n {
        let id = l.u32()?;
        if version <= 8 {
            l.u8()?;
        } else if version < 13 {
            l.u16()?;
        } else {
            l.u32()?;
        }
        let nw = Vec3::new(l.f32()?, l.f32()?, l.f32()?);
        let se = Vec3::new(l.f32()?, l.f32()?, l.f32()?);
        l.f32()?; // z du coin nord-est
        l.f32()?; // z du coin sud-ouest
        anyhow::ensure!(nw.abs().max_element() <= 17000.0 && se.abs().max_element() <= 17000.0, hm_langue::tr!("aire {id} hors de la map : la disposition lue n'est pas la bonne", "area {id} outside the map: the layout read is wrong"));
        for _ in 0..4 {
            let k = l.u32()? as usize;
            l.n(k * 4)?;
        }
        let caches = l.u8()? as usize;
        l.n(caches * 17)?; // identifiant, position, drapeaux
        if version < 15 {
            let k = l.u8()? as usize;
            l.n(k * 14)?;
        }
        for _ in 0..l.u32()? {
            l.n(10)?; // de, direction, vers, direction
            let k = l.u8()? as usize;
            l.n(k * 5)?;
        }
        l.u16()?; // lieu
        for _ in 0..2 {
            let k = l.u32()? as usize;
            l.n(k * 4)?;
        }
        l.n(8)?; // premiers temps d'occupation
        if version >= 11 {
            l.n(16)?; // lumière aux quatre coins
        }
        if version >= 16 {
            let k = l.u32()? as usize;
            l.n(k * 5)?; // aires visibles
            l.u32()?; // héritage de visibilité
        }
        aires.push(AireNav { id, centre: (nw + se) * 0.5, taille: (se - nw).abs() });
    }
    Ok(aires)
}

/// Accord entre la simulation et le maillage de navigation du jeu : part des aires du `.nav` dont le centre est
/// accessible au joueur simulé. Rend (aires, accessibles, inaccessibles, sans sol trouvé, exemples de désaccord).
pub fn accord_nav(sim: &Simulation, aires: &[AireNav]) -> (usize, usize, usize, usize, Vec<Vec3>) {
    let (mut oui, mut non, mut rien) = (0, 0, 0);
    let mut exemples = Vec::new();
    for a in aires {
        match sim.accessible(a.centre, 40.0) {
            Some(true) => oui += 1,
            Some(false) => {
                non += 1;
                if exemples.len() < 12 {
                    exemples.push(a.centre);
                }
            }
            None => rien += 1,
        }
    }
    (aires.len(), oui, non, rien, exemples)
}

/// L'intervalle de la colonne du point dont le sol est à `tol` près de son altitude.
fn noeud_colonne(f: &Field, p: Vec3, tol: f32) -> Option<u32> {
    let (ix, iy) = (((p.x - f.x0) / CELL).floor() as i64, ((p.y - f.y0) / CELL).floor() as i64);
    if ix < 0 || iy < 0 || ix >= f.nx as i64 || iy >= f.ny as i64 {
        return None;
    }
    f.intervalles(iy as usize * f.nx + ix as usize).find(|&j| (f.free[j].floor as f32 - p.z).abs() <= tol).map(|j| j as u32)
}

/// Faces en nodraw qui donnent sur l'air d'un endroit accessible : le joueur voit à travers.
pub fn nodraw_visible(scene: &Scene, monde: &crate::Monde, sim: &Simulation, out: &mut Vec<Constat>) {
    use rayon::prelude::*;
    let nodraw: std::collections::HashSet<&str> = scene.materials.iter().filter(|m| m.tool == Some(hm_assets::Tool::Nodraw)).map(|m| m.name.as_str()).collect();
    let res: Vec<Constat> = scene
        .brushes
        .par_iter()
        .filter(|b| b.object != 0 && !scene.objects[b.object as usize].hidden)
        .flat_map_iter(|b| {
            let mut v = Vec::new();
            let moi = monde.de_objet[b.object as usize];
            for f in &b.brush.faces {
                if f.poly.len() < 3 || f.disp.is_some() || !nodraw.contains(f.material.as_str()) || hm_geom::aire(&f.poly) < 256.0 {
                    continue;
                }
                let c = (f.poly.iter().fold(glam::DVec3::ZERO, |a, p| a + *p) / f.poly.len() as f64).as_vec3();
                let n = f.plane.n.as_vec3();
                // l'air devant la face, à 24 puis à 40 : rien de plein, rien qui cache la face, et un joueur y passe
                let vue = [24.0f32, 40.0].iter().any(|d| {
                    let p = c + n * *d;
                    monde.dans(p, |k, i| i != moi && (k.visible || k.ciel)).is_none()
                        && monde.rayon(p, c + n * 0.5, |k, i| i != moi && k.visible).is_none()
                        && sim.dans_espace_atteint(p)
                });
                // Et il faut qu'un joueur debout la voie : un œil à 64 du sol, dans une case accessible à moins de 320,
                // devant la face, sans rien entre les deux. Le dessus d'une armoire haute n'est vu de personne.
                let vue = vue && {
                    let ch = &sim.champ;
                    let (cx, cy) = (((c.x - ch.x0) / CELL).floor() as i64, ((c.y - ch.y0) / CELL).floor() as i64);
                    let mut trouve = false;
                    'yeux: for dy in (-20..=20i64).step_by(2) {
                        for dx in (-20..=20i64).step_by(2) {
                            let (x, y) = (cx + dx, cy + dy);
                            if x < 0 || y < 0 || x >= ch.nx as i64 || y >= ch.ny as i64 {
                                continue;
                            }
                            for j in ch.intervalles(y as usize * ch.nx + x as usize) {
                                if !sim.atteint[j] {
                                    continue;
                                }
                                let oeil = ch.pos(j) + Vec3::Z * 64.0;
                                let vers = c - oeil;
                                if vers.length() > 420.0 || vers.dot(n) > -8.0 {
                                    continue;
                                }
                                if monde.rayon(oeil, c + n * 0.5, |k, i| i != moi && k.visible).is_none() {
                                    trouve = true;
                                    break 'yeux;
                                }
                            }
                        }
                    }
                    trouve
                };
                if vue {
                    let lo = f.poly.iter().fold(f.poly[0], |a, p| a.min(*p)).as_vec3();
                    let hi = f.poly.iter().fold(f.poly[0], |a, p| a.max(*p)).as_vec3();
                    v.push(
                        Constat::objet(scene, "geo.nodraw_visible", b.object, hm_langue::tr!("{} : sa face {} est en nodraw et un joueur debout la voit ({:.0} unités carrées)", "{}: its face {} is nodraw and a standing player sees it ({:.0} square units)", monde.nommer(b.object), f.side + 1, hm_geom::aire(&f.poly)))
                            .boite(lo - Vec3::splat(24.0), hi + Vec3::splat(24.0))
                            .mesure(hm_geom::aire(&f.poly) as f32),
                    );
                }
            }
            v
        })
        .collect();
    out.extend(res);
}
