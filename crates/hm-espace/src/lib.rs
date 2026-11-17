//! L'espace libre d'une map, en colonnes de 16 × 16 unités.
//!
//! Chaque colonne garde ses intervalles pleins (solides, collisions des modèles, terrain) et, entre eux, ses
//! intervalles libres assez hauts pour un joueur debout. De cette seule structure sortent les **pièces** (régions
//! d'espace praticable séparées par les étranglements de la largeur d'une porte) et les **façades** (bords de
//! l'espace extérieur le long d'un mur).
//!
//! Les pleins sont élargis d'une demi-cellule : un mur mince est toujours vu, au prix d'un espace libre réduit de
//! 8 unités de chaque côté. C'est voulu pour trouver les pièces ; le joueur simulé de la phase 3 demandera un
//! calcul exact.
use std::collections::VecDeque;
use std::time::Instant;

use glam::{Mat3, Vec3};
use hm_scene::Scene;
use rayon::prelude::*;

pub const CELL: f32 = 16.0;
/// Hauteur du joueur debout.
pub const PLAYER_H: i32 = 72;
/// Marche franchissable.
pub const STEP: i32 = 18;
/// Dénivelé qu'un joueur franchit en sautant accroupi (68 dans Garry's Mod). Deux sols séparés par moins que cela
/// sont dans la même pièce : une table, un comptoir ou une estrade ne coupent pas une salle en deux.
pub const SAUT: i32 = 64;
/// Distance au mur (en cellules) à partir de laquelle une cellule fait partie du cœur d'une pièce.
const CORE: u8 = 3;
const ZOFF: i64 = 20000;

#[derive(Clone, Copy, Debug)]
pub struct SolidSpan {
    pub z0: i16,
    pub z1: i16,
    /// Le dessous du bloc est du ciel (vu d'en bas, ce n'est pas un plafond).
    pub sky_bottom: bool,
    /// Le dessus du bloc est du ciel (ce n'est pas un sol).
    pub sky_top: bool,
}

/// Un intervalle libre où un joueur tient debout.
#[derive(Clone, Copy, Debug)]
pub struct Free {
    pub floor: i16,
    pub ceil: i16,
    /// Rien au-dessus, ou le ciel.
    pub open_sky: bool,
    pub col: u32,
}

impl Free {
    /// Couvert par un plafond proche : un intérieur.
    pub fn interior(&self) -> bool {
        !self.open_sky && (self.ceil as i32 - self.floor as i32) <= 900
    }
}

#[derive(Clone, Debug)]
pub struct Region {
    pub id: u32,
    pub cells: u32,
    pub min: Vec3,
    pub max: Vec3,
    pub floor: f32,
    pub ceil: f32,
    /// Part des cellules couvertes par un plafond proche.
    pub interior: f32,
    /// Région sans cœur (plus étroite que 96) : placard, cabine, recoin.
    pub small: bool,
    /// Point le plus dégagé, puis deux points éloignés l'un de l'autre, tous au sol.
    pub centre: Vec3,
    pub far_a: Vec3,
    pub far_b: Vec3,
    /// Plus grande distance au mur, en unités.
    pub clearance: f32,
    /// Part du bord qui donne sur le vide plutôt que sur un mur. Près de 1 : un plateau (dessus de meuble, de
    /// conteneur, toit sans acrotère, quai) et non une pièce.
    pub drop_edges: f32,
}

impl Region {
    /// Surface libre en unités carrées.
    pub fn area(&self) -> f32 {
        self.cells as f32 * CELL * CELL
    }
    /// Une pièce : couverte, bordée de murs, assez grande pour s'y tenir.
    pub fn is_room(&self) -> bool {
        if self.interior <= 0.6 {
            return false;
        }
        if self.small {
            // sans cœur : seulement une vraie petite pièce fermée de murs, pas un recoin ni un dessus de caisse
            self.cells >= 40 && self.drop_edges < 0.3
        } else {
            self.cells >= 20 && !(self.drop_edges > 0.6 && self.cells < 300)
        }
    }
}

/// Un tronçon droit du bord de l'espace extérieur, le long d'un mur.
#[derive(Clone, Debug)]
pub struct Facade {
    /// De l'espace libre vers le mur.
    pub dir: Vec3,
    pub a: Vec3,
    pub b: Vec3,
    pub wall_top: f32,
    /// Recul libre devant le milieu du tronçon.
    pub back: f32,
    pub region: u32,
}

pub struct Field {
    pub x0: f32,
    pub y0: f32,
    pub nx: usize,
    pub ny: usize,
    solid_start: Vec<u32>,
    solids: Vec<SolidSpan>,
    free_start: Vec<u32>,
    pub free: Vec<Free>,
    /// Distance au bord de l'espace praticable, en cellules (1 = contre un mur).
    pub dist: Vec<u8>,
    /// Région de chaque intervalle libre (0 = aucune).
    pub region: Vec<u32>,
    pub regions: Vec<Region>,
    pub facades: Vec<Facade>,
    pub timings: Vec<(String, f32)>,
}

const DIRS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

struct Convex {
    planes: Vec<[f32; 4]>,
    min: Vec3,
    max: Vec3,
    sky: bool,
    boxy: bool,
}

/// Ce que le champ retient de la scène.
pub struct Options<'a> {
    /// Les objets (rang dans la scène) qui comptent comme pleins. Le joueur simulé en retire les portes.
    pub garder: &'a (dyn Fn(u32) -> bool + Sync),
    /// Tous les modèles solides, et non les seuls qui font mur ou plancher.
    pub tous_les_modeles: bool,
    /// Découper en pièces et chercher les façades (inutile pour le joueur simulé).
    pub pieces: bool,
}

impl Field {
    pub fn build(scene: &Scene) -> Field {
        Field::build_avec(scene, &Options { garder: &|_| true, tous_les_modeles: false, pieces: true })
    }

    /// Intervalles libres d'une colonne.
    pub fn intervalles(&self, col: usize) -> std::ops::Range<usize> {
        self.free_range(col)
    }

    pub fn build_avec(scene: &Scene, opts: &Options) -> Field {
        let mut timings = Vec::new();
        let mut t = Instant::now();
        let mut lap = |timings: &mut Vec<(String, f32)>, what: &str| {
            timings.push((what.to_string(), t.elapsed().as_secs_f32()));
            t = Instant::now();
        };
        // ---------------------------------------------------------------- les convexes
        let mut convexes: Vec<Convex> = scene.solids.iter().filter(|s| (opts.garder)(s.object)).map(|s| Convex { planes: s.planes.clone(), min: s.min, max: s.max, sky: s.sky, boxy: s.boxy }).collect();
        for (lo, hi) in &scene.disp_cells {
            convexes.push(Convex { planes: Vec::new(), min: Vec3::new(lo.x, lo.y, lo.z - 16.0), max: *hi, sky: false, boxy: true });
        }
        let prop_convexes: Vec<Convex> = scene
            .props
            .par_iter()
            .filter(|p| {
                // Seuls les modèles qui font mur (au moins 96 de haut) ou plancher (au moins 160 dans les deux sens)
                // découpent l'espace. Le mobilier bas est ignoré : sinon chaque bureau coupe sa salle en poches et
                // chaque dessus de table devient un « sol ».
                let o = &scene.objects[p.object as usize];
                let e = o.max - o.min;
                p.solid && (opts.tous_les_modeles || e.z >= 96.0 || e.x.min(e.y) >= 160.0) && (opts.garder)(p.object)
            })
            .flat_map_iter(|p| {
                let m = p.transform;
                let lin = Mat3::from_mat4(m);
                let scale = lin.x_axis.length().max(1e-6);
                let rot = lin * (1.0 / scale);
                let tr = m.w_axis.truncate();
                scene.models[p.model as usize].collision.iter().map(move |h| {
                    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
                    for pt in &h.points {
                        let w = m.transform_point3(Vec3::from(*pt));
                        lo = lo.min(w);
                        hi = hi.max(w);
                    }
                    let planes: Vec<[f32; 4]> = h
                        .planes
                        .iter()
                        .map(|pl| {
                            let n = rot * Vec3::new(pl[0], pl[1], pl[2]);
                            [n.x, n.y, n.z, pl[3] * scale + n.dot(tr)]
                        })
                        .collect();
                    let boxy = planes.iter().all(|p| p[0].abs().max(p[1].abs()).max(p[2].abs()) > 0.9999);
                    Convex { planes, min: lo, max: hi, sky: false, boxy }
                })
            })
            .collect();
        convexes.extend(prop_convexes);
        let lo = convexes.iter().fold(Vec3::splat(f32::MAX), |a, c| a.min(c.min)).max(Vec3::splat(-16400.0));
        let hi = convexes.iter().fold(Vec3::splat(f32::MIN), |a, c| a.max(c.max)).min(Vec3::splat(16400.0));
        let (x0, y0) = ((lo.x / CELL).floor() * CELL - CELL, (lo.y / CELL).floor() * CELL - CELL);
        let nx = (((hi.x - x0) / CELL).ceil() as usize + 2).max(1);
        let ny = (((hi.y - y0) / CELL).ceil() as usize + 2).max(1);
        lap(&mut timings, "convexes");

        // ---------------------------------------------------------------- pleins par colonne
        let half = CELL / 2.0;
        let mut keys: Vec<u64> = convexes
            .par_iter()
            .flat_map_iter(|c| {
                let mut out = Vec::new();
                let ix0 = (((c.min.x - half - x0) / CELL - 0.5).ceil().max(0.0)) as usize;
                let ix1 = (((c.max.x + half - x0) / CELL - 0.5).floor().min(nx as f32 - 1.0)).max(-1.0) as isize;
                let iy0 = (((c.min.y - half - y0) / CELL - 0.5).ceil().max(0.0)) as usize;
                let iy1 = (((c.max.y + half - y0) / CELL - 0.5).floor().min(ny as f32 - 1.0)).max(-1.0) as isize;
                let key = |col: usize, z0: f32, z1: f32| {
                    let (a, b) = ((z0.floor() as i64).clamp(-16400, 16400) + ZOFF, (z1.ceil() as i64).clamp(-16400, 16400) + ZOFF);
                    ((col as u64) << 34) | ((a as u64) << 18) | ((b as u64) << 2) | c.sky as u64
                };
                for iy in iy0 as isize..=iy1 {
                    let cy = y0 + (iy as f32 + 0.5) * CELL;
                    for ix in ix0 as isize..=ix1 {
                        let col = iy as usize * nx + ix as usize;
                        if c.boxy {
                            out.push(key(col, c.min.z, c.max.z));
                            continue;
                        }
                        let cx = x0 + (ix as f32 + 0.5) * CELL;
                        let (mut zmin, mut zmax) = (c.min.z, c.max.z);
                        let mut ok = true;
                        for p in &c.planes {
                            // plan écarté d'une demi-cellule : la colonne entière est testée, pas seulement son axe
                            let a = p[0] * cx + p[1] * cy - p[3] - half * (p[0].abs() + p[1].abs());
                            if p[2].abs() < 1e-5 {
                                if a > 0.0 {
                                    ok = false;
                                    break;
                                }
                            } else if p[2] > 0.0 {
                                zmax = zmax.min(-a / p[2]);
                            } else {
                                zmin = zmin.max(-a / p[2]);
                            }
                        }
                        if ok && zmax > zmin {
                            out.push(key(col, zmin, zmax));
                        }
                    }
                }
                out
            })
            .collect();
        keys.par_sort_unstable();
        lap(&mut timings, "pleins par colonne");

        // ---------------------------------------------------------------- fusion, intervalles libres
        let ncol = nx * ny;
        let mut solid_start = vec![0u32; ncol + 1];
        let mut solids: Vec<SolidSpan> = Vec::with_capacity(keys.len() / 2);
        let mut free_start = vec![0u32; ncol + 1];
        let mut free: Vec<Free> = Vec::new();
        let mut k = 0usize;
        for col in 0..ncol {
            solid_start[col] = solids.len() as u32;
            free_start[col] = free.len() as u32;
            let first = solids.len();
            while k < keys.len() && (keys[k] >> 34) as usize == col {
                let z0 = (((keys[k] >> 18) & 0xFFFF) as i64 - ZOFF) as i16;
                let z1 = (((keys[k] >> 2) & 0xFFFF) as i64 - ZOFF) as i16;
                let sky = keys[k] & 1 == 1;
                // fusion avec le plein précédent de la même colonne s'ils se touchent
                // Un bloc fusionné garde la nature de son dessous et celle de son dessus : le sol de la ville peut
                // toucher le plafond de ciel d'une zone située dessous sans devenir « du ciel ».
                if solids.len() > first && z0 <= solids[solids.len() - 1].z1 {
                    let last = solids.last_mut().unwrap();
                    if z1 > last.z1 {
                        last.z1 = z1;
                        last.sky_top = sky;
                    }
                } else {
                    solids.push(SolidSpan { z0, z1, sky_bottom: sky, sky_top: sky });
                }
                k += 1;
            }
            let n = solids.len();
            for i in first..n {
                let below = solids[i];
                if below.sky_top {
                    continue;
                }
                let (ceil, open_sky) = if i + 1 < n { (solids[i + 1].z0, solids[i + 1].sky_bottom) } else { (i16::MAX, true) };
                if ceil as i32 - below.z1 as i32 >= PLAYER_H {
                    free.push(Free { floor: below.z1, ceil, open_sky, col: col as u32 });
                }
            }
        }
        solid_start[ncol] = solids.len() as u32;
        free_start[ncol] = free.len() as u32;
        drop(keys);
        lap(&mut timings, "intervalles libres");

        let mut f = Field { x0, y0, nx, ny, solid_start, solids, free_start, free, dist: Vec::new(), region: Vec::new(), regions: Vec::new(), facades: Vec::new(), timings };
        if opts.pieces {
            f.segment();
            f.timings.push(("pièces".to_string(), t.elapsed().as_secs_f32()));
            t = Instant::now();
            f.find_facades();
            f.timings.push(("façades".to_string(), t.elapsed().as_secs_f32()));
        }
        f
    }

    pub fn solids_of(&self, col: usize) -> &[SolidSpan] {
        &self.solids[self.solid_start[col] as usize..self.solid_start[col + 1] as usize]
    }

    fn free_range(&self, col: usize) -> std::ops::Range<usize> {
        self.free_start[col] as usize..self.free_start[col + 1] as usize
    }

    /// Position au sol du centre de la cellule d'un intervalle libre.
    pub fn pos(&self, i: usize) -> Vec3 {
        let s = self.free[i];
        let (ix, iy) = (s.col as usize % self.nx, s.col as usize / self.nx);
        Vec3::new(self.x0 + (ix as f32 + 0.5) * CELL, self.y0 + (iy as f32 + 0.5) * CELL, s.floor as f32)
    }

    /// L'intervalle voisin atteignable à pied dans la direction `d` (marche de 18 au plus, 72 de haut en commun).
    pub fn link(&self, i: usize, d: usize) -> Option<usize> {
        self.lien(i, d, STEP)
    }

    /// Le voisin de la même pièce : atteignable à pied ou en sautant.
    fn lien_piece(&self, i: usize, d: usize) -> Option<usize> {
        self.lien(i, d, SAUT)
    }

    fn lien(&self, i: usize, d: usize, marche: i32) -> Option<usize> {
        let s = self.free[i];
        let (ix, iy) = ((s.col as usize % self.nx) as i32 + DIRS[d].0, (s.col as usize / self.nx) as i32 + DIRS[d].1);
        if ix < 0 || iy < 0 || ix >= self.nx as i32 || iy >= self.ny as i32 {
            return None;
        }
        let col = iy as usize * self.nx + ix as usize;
        self.free_range(col).find(|&j| {
            let o = self.free[j];
            (o.floor as i32 - s.floor as i32).abs() <= marche && (s.ceil.min(o.ceil) as i32 - s.floor.max(o.floor) as i32) >= PLAYER_H
        })
    }

    /// Vrai si le point est dans un intervalle libre de sa colonne (ni dans un plein, ni hors de la map).
    pub fn libre(&self, p: Vec3) -> bool {
        let (ix, iy) = (((p.x - self.x0) / CELL).floor() as i64, ((p.y - self.y0) / CELL).floor() as i64);
        if ix < 0 || iy < 0 || ix >= self.nx as i64 || iy >= self.ny as i64 {
            return false;
        }
        let col = iy as usize * self.nx + ix as usize;
        let sol = self.solids_of(col);
        // libre s'il existe un plein dessous et aucun plein ne contient le point
        let z = p.z;
        sol.iter().any(|w| (w.z1 as f32) <= z && !w.sky_top) && !sol.iter().any(|w| (w.z0 as f32) < z && z < (w.z1 as f32))
    }

    /// Part du segment `a → b` qui passe dans l'espace libre (1 = vue dégagée).
    pub fn degage(&self, a: Vec3, b: Vec3) -> f32 {
        let n = ((a.distance(b) / 12.0).ceil() as usize).max(1);
        let ok = (0..=n).filter(|&i| self.libre(a.lerp(b, i as f32 / n as f32))).count();
        ok as f32 / (n + 1) as f32
    }

    /// La région (pièce, rue, plateau) où se trouve un point, s'il est dans l'espace praticable.
    pub fn region_at(&self, p: Vec3) -> Option<&Region> {
        let j = self.free_at(p)?;
        // l'intervalle doit contenir le point : un point pris dans un mur ne doit pas tomber à l'étage du dessous
        let f = &self.free[j];
        if (f.ceil as f32) < p.z {
            return None;
        }
        let id = *self.region.get(j)?;
        self.regions.iter().find(|r| r.id == id && id != 0)
    }

    /// Où se trouve ce qu'on regarde : le point visé est sur une surface, donc dans un plein élargi ; on recule
    /// vers l'œil, par pas d'une cellule, jusqu'au premier point de l'espace praticable.
    pub fn decrire_lieu_vise(&self, vise: Vec3, oeil: Vec3) -> String {
        let vers_oeil = (oeil - vise).normalize_or_zero();
        let portee = oeil.distance(vise);
        (0..12).map(|k| 8.0 + CELL * k as f32).filter(|d| *d < portee).map(|d| vise + vers_oeil * d).find(|p| self.region_at(*p).is_some()).map_or_else(|| "hors de l'espace praticable".to_string(), |p| self.decrire_lieu(p))
    }

    /// Une phrase qui dit où se trouve un point : dans quelle pièce, ou dehors.
    pub fn decrire_lieu(&self, p: Vec3) -> String {
        match self.region_at(p) {
            Some(r) if r.is_room() => format!("pièce n° {} ({:.0} × {:.0}, sol à {:.0}, plafond à {:.0})", r.id, r.max.x - r.min.x, r.max.y - r.min.y, r.floor, r.ceil),
            // un espace ouvert s'étend sur plusieurs niveaux (place, jardin en contrebas, escaliers) : son sol moyen
            // ne dit rien, c'est le sol sous le point qui compte
            Some(r) => {
                let ici = self.free_at(p).map_or(r.floor, |j| self.free[j].floor as f32);
                format!("espace ouvert n° {} (rue, toit ou plateau ; sol à {:.0} à cet endroit)", r.id, ici)
            }
            None => "hors de l'espace praticable".to_string(),
        }
    }

    /// L'intervalle libre qui contient le point, ou celui dont le sol est juste dessous.
    pub fn free_at(&self, p: Vec3) -> Option<usize> {
        let (ix, iy) = (((p.x - self.x0) / CELL).floor() as i64, ((p.y - self.y0) / CELL).floor() as i64);
        if ix < 0 || iy < 0 || ix >= self.nx as i64 || iy >= self.ny as i64 {
            return None;
        }
        let col = iy as usize * self.nx + ix as usize;
        self.free_range(col).filter(|&j| (self.free[j].floor as f32) <= p.z + 1.0).max_by_key(|&j| self.free[j].floor)
    }

    fn segment(&mut self) {
        let n = self.free.len();
        // distance au bord
        let mut dist = vec![0u8; n];
        let mut queue: VecDeque<u32> = VecDeque::new();
        let border: Vec<bool> = (0..n).into_par_iter().map(|i| (0..4).any(|d| self.lien_piece(i, d).is_none())).collect();
        for i in 0..n {
            if border[i] {
                dist[i] = 1;
                queue.push_back(i as u32);
            }
        }
        while let Some(i) = queue.pop_front() {
            let di = dist[i as usize];
            if di >= 60 {
                continue;
            }
            for d in 0..4 {
                if let Some(j) = self.lien_piece(i as usize, d) {
                    if dist[j] == 0 {
                        dist[j] = di + 1;
                        queue.push_back(j as u32);
                    }
                }
            }
        }
        // cœurs : composantes des cellules à CORE du mur ou plus
        let mut region = vec![0u32; n];
        let mut next = 1u32;
        let mut small = vec![false];
        let mut grow: VecDeque<u32> = VecDeque::new();
        for seed in 0..n {
            if dist[seed] < CORE || region[seed] != 0 {
                continue;
            }
            region[seed] = next;
            queue.push_back(seed as u32);
            while let Some(i) = queue.pop_front() {
                grow.push_back(i);
                for d in 0..4 {
                    if let Some(j) = self.lien_piece(i as usize, d) {
                        if region[j] == 0 && dist[j] >= CORE {
                            region[j] = next;
                            queue.push_back(j as u32);
                        }
                    }
                }
            }
            small.push(false);
            next += 1;
        }
        // chaque cœur regagne ses bords, tous en même temps : ils se rejoignent au milieu des portes
        while let Some(i) = grow.pop_front() {
            for d in 0..4 {
                if let Some(j) = self.lien_piece(i as usize, d) {
                    if region[j] == 0 {
                        region[j] = region[i as usize];
                        grow.push_back(j as u32);
                    }
                }
            }
        }
        // ce qui reste n'a pas de cœur : recoins et petites pièces
        for seed in 0..n {
            if region[seed] != 0 {
                continue;
            }
            let mut members = vec![seed as u32];
            region[seed] = next;
            let mut head = 0;
            while head < members.len() {
                let i = members[head] as usize;
                head += 1;
                for d in 0..4 {
                    if let Some(j) = self.lien_piece(i, d) {
                        if region[j] == 0 {
                            region[j] = next;
                            members.push(j as u32);
                        }
                    }
                }
            }
            if members.len() >= 12 {
                small.push(true);
                next += 1;
            } else {
                for m in members {
                    region[m as usize] = u32::MAX; // trop petit pour compter
                }
            }
        }
        for r in region.iter_mut() {
            if *r == u32::MAX {
                *r = 0;
            }
        }
        // mesures par région
        let nr = next as usize;
        let mut cells = vec![0u32; nr];
        let mut sum = vec![[0f64; 4]; nr]; // x, y, sol, plafond
        let mut inter = vec![0u32; nr];
        let mut lo = vec![Vec3::splat(f32::MAX); nr];
        let mut hi = vec![Vec3::splat(f32::MIN); nr];
        let mut best = vec![(0u8, 0usize); nr];
        let mut edge = vec![(0u32, 0u32); nr]; // bords sur le vide, bords sur un mur
        for i in 0..n {
            let r = region[i] as usize;
            if r == 0 {
                continue;
            }
            let (p, s) = (self.pos(i), self.free[i]);
            if dist[i] == 1 {
                let (ix, iy) = ((s.col as usize % self.nx) as i32, (s.col as usize / self.nx) as i32);
                for d in 0..4 {
                    if self.lien_piece(i, d).is_some() {
                        continue;
                    }
                    let (jx, jy) = (ix + DIRS[d].0, iy + DIRS[d].1);
                    let drop = jx >= 0
                        && jy >= 0
                        && jx < self.nx as i32
                        && jy < self.ny as i32
                        && self.free_range(jy as usize * self.nx + jx as usize).any(|j| (self.free[j].floor as i32) < s.floor as i32 - SAUT && (self.free[j].ceil as i32) >= s.floor as i32 + 36);
                    if drop {
                        edge[r].0 += 1;
                    } else {
                        edge[r].1 += 1;
                    }
                }
            }
            let ceil = (s.ceil as i32).min(s.floor as i32 + 1024) as f32;
            cells[r] += 1;
            sum[r][0] += p.x as f64;
            sum[r][1] += p.y as f64;
            sum[r][2] += p.z as f64;
            sum[r][3] += ceil as f64;
            inter[r] += s.interior() as u32;
            lo[r] = lo[r].min(p - Vec3::new(8.0, 8.0, 0.0));
            hi[r] = hi[r].max(Vec3::new(p.x + 8.0, p.y + 8.0, ceil));
            if dist[i] > best[r].0 {
                best[r] = (dist[i], i);
            }
        }
        let centroid: Vec<Vec3> = (0..nr).map(|r| if cells[r] == 0 { Vec3::ZERO } else { Vec3::new((sum[r][0] / cells[r] as f64) as f32, (sum[r][1] / cells[r] as f64) as f32, (sum[r][2] / cells[r] as f64) as f32) }).collect();
        // centre : parmi les plus dégagées, la plus proche du barycentre
        let mut centre = vec![(f32::MAX, 0usize); nr];
        for i in 0..n {
            let r = region[i] as usize;
            if r != 0 && dist[i] + 1 >= best[r].0 {
                let d = self.pos(i).distance_squared(centroid[r]);
                if d < centre[r].0 {
                    centre[r] = (d, i);
                }
            }
        }
        let far = |from: &Vec<Vec3>| {
            let mut out = vec![(-1f32, 0usize); nr];
            for i in 0..n {
                let r = region[i] as usize;
                if r != 0 && dist[i] >= 2.min(best[r].0) {
                    let p = self.pos(i);
                    // les angles d'abord : distance en plan, la hauteur ne compte pas
                    let d = (p.truncate() - from[r].truncate()).length_squared();
                    if d > out[r].0 {
                        out[r] = (d, i);
                    }
                }
            }
            out
        };
        let centre_pos: Vec<Vec3> = (0..nr).map(|r| if cells[r] == 0 { Vec3::ZERO } else { self.pos(centre[r].1) }).collect();
        let fa = far(&centre_pos);
        let fa_pos: Vec<Vec3> = (0..nr).map(|r| if cells[r] == 0 { Vec3::ZERO } else { self.pos(fa[r].1) }).collect();
        let fb = far(&fa_pos);
        let mut regions = Vec::with_capacity(nr);
        for r in 1..nr {
            if cells[r] == 0 {
                continue;
            }
            regions.push(Region {
                id: r as u32,
                cells: cells[r],
                min: lo[r],
                max: hi[r],
                floor: (sum[r][2] / cells[r] as f64) as f32,
                ceil: (sum[r][3] / cells[r] as f64) as f32,
                interior: inter[r] as f32 / cells[r] as f32,
                small: small[r],
                centre: centre_pos[r],
                far_a: fa_pos[r],
                far_b: self.pos(fb[r].1),
                clearance: best[r].0 as f32 * CELL,
                drop_edges: edge[r].0 as f32 / (edge[r].0 + edge[r].1).max(1) as f32,
            });
        }
        self.dist = dist;
        self.region = region;
        self.regions = regions;
    }

    fn find_facades(&mut self) {
        // bords de l'espace extérieur contre un mur d'au moins 96 de haut
        let n = self.free.len();
        let mut outdoor = vec![false; self.regions.iter().map(|r| r.id as usize + 1).max().unwrap_or(1)];
        for r in &self.regions {
            outdoor[r.id as usize] = !r.is_room() && r.cells >= 2000;
        }
        let edges: Vec<(u8, i32, i32, i32, u32)> = (0..n)
            .into_par_iter()
            .flat_map_iter(|i| {
                let s = self.free[i];
                let mut out = Vec::new();
                if self.region[i] == 0 || !outdoor[self.region[i] as usize] {
                    return out;
                }
                let (ix, iy) = ((s.col as usize % self.nx) as i32, (s.col as usize / self.nx) as i32);
                for d in 0..4 {
                    if self.link(i, d).is_some() {
                        continue;
                    }
                    let (jx, jy) = (ix + DIRS[d].0, iy + DIRS[d].1);
                    if jx < 0 || jy < 0 || jx >= self.nx as i32 || jy >= self.ny as i32 {
                        continue;
                    }
                    let wall = self.solids_of(jy as usize * self.nx + jx as usize).iter().any(|w| !(w.sky_bottom && w.sky_top) && (w.z0 as i32) <= s.floor as i32 + 24 && (w.z1 as i32) >= s.floor as i32 + 120);
                    if wall {
                        let (line, along) = if d < 2 { (ix, iy) } else { (iy, ix) };
                        out.push((d as u8, line, (s.floor as i32).div_euclid(96), along, i as u32));
                    }
                }
                out
            })
            .collect();
        let mut edges = edges;
        edges.par_sort_unstable();
        let mut facades = Vec::new();
        let mut k = 0;
        while k < edges.len() {
            let mut e = k;
            while e + 1 < edges.len() && edges[e + 1].0 == edges[k].0 && edges[e + 1].1 == edges[k].1 && edges[e + 1].2 == edges[k].2 && edges[e + 1].3 - edges[e].3 <= 24 {
                e += 1;
            }
            if edges[e].3 - edges[k].3 >= 24 {
                let d = edges[k].0 as usize;
                let mid = edges[(k + e) / 2].4 as usize;
                let (a, b) = (self.pos(edges[k].4 as usize), self.pos(edges[e].4 as usize));
                // recul libre : on s'éloigne du mur à hauteur d'yeux, en passant les petits obstacles (poteaux, bornes)
                let s = self.free[mid];
                let dirv = Vec3::new(DIRS[d].0 as f32, DIRS[d].1 as f32, 0.0);
                let eye = self.pos(mid) + Vec3::Z * 72.0;
                let (mut steps, mut blocked) = (0, 0);
                for k in 1..=60 {
                    if self.libre(eye - dirv * (k as f32 * CELL)) {
                        steps = k;
                        blocked = 0;
                    } else {
                        blocked += 1;
                        if blocked > 4 {
                            break;
                        }
                    }
                }
                let (ix, iy) = ((s.col as usize % self.nx) as i32 + DIRS[d].0, (s.col as usize / self.nx) as i32 + DIRS[d].1);
                let wall_top = self.solids_of(iy as usize * self.nx + ix as usize).iter().filter(|w| !(w.sky_bottom && w.sky_top) && (w.z0 as i32) <= s.floor as i32 + 24).map(|w| w.z1 as f32).fold(s.floor as f32 + 120.0, f32::max);
                facades.push(Facade { dir: Vec3::new(DIRS[d].0 as f32, DIRS[d].1 as f32, 0.0), a, b, wall_top, back: steps as f32 * CELL, region: self.region[mid] });
            }
            k = e + 1;
        }
        self.facades = facades;
    }

    pub fn summary(&self) -> String {
        let rooms = self.regions.iter().filter(|r| r.is_room()).count();
        format!(
            "espace : {} × {} colonnes de {CELL:.0}, {} intervalles pleins, {} libres ; {} régions dont {} pièces ; {} tronçons de façade ; {}",
            self.nx,
            self.ny,
            self.solids.len(),
            self.free.len(),
            self.regions.len(),
            rooms,
            self.facades.len(),
            self.timings.iter().map(|(w, t)| format!("{w} {t:.2} s")).collect::<Vec<_>>().join(", ")
        )
    }
}
