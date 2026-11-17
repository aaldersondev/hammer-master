"""ORACLE DE TEST. C'était `scripts/generer.py` du skill gmod-map-maker jusqu'au 14/11/2026 ; `hm construire` l'a
remplacé. Ce fichier ne sert plus qu'à `tests/construire_differentiel.py`, qui compare les deux sur les mêmes specs.
Ne pas s'en servir pour faire une map.

Génère un .vmf et son <carte>.plan.json à partir d'une spec YAML. On n'écrit jamais de coordonnée dans le .vmf.

    python generer.py <spec.yaml> [--phase blockout|texture] [-o sortie.vmf]

La spec décrit des volumes d'air (zones et ouvertures) ; les murs, sols et plafonds sont déduits :
une enveloppe d'épaisseur `murs` / `dalles` autour de chaque volume, découpée en boîtes sur la grille
(un mur = un brush, aucun chevauchement, map scellée par construction). Format : references/spec.md.
Règles d'échelle (references/echelle.md) : une violation est une ERREUR, sauf si l'élément porte
`exception: "raison"` (elle devient un avertissement).
"""
import argparse
import filecmp
import fnmatch
import json
import math
import shutil
import sys
from pathlib import Path

import numpy as np
import yaml

# oracle : les briques communes (mapkit, tables) restent celles du skill installé
sys.path.insert(0, str(Path.home() / ".claude" / "skills" / "gmod-map-maker" / "scripts"))
from mapkit.config import (DATA, GRID_DETAIL, GRID_WORLD, MODULES, load_available, load_skies, load_static,  # noqa: E402
                           scan_content_dir)
from mapkit.vmfio import box_faces, find_module, hull_faces, make_solid, read_map  # noqa: E402

NODRAW, SKY, SKY2D = "tools/toolsnodraw", "tools/toolsskybox", "tools/toolsskybox2d"
SIDES = {"est": (0, 1), "ouest": (0, -1), "nord": (1, 1), "sud": (1, -1), "dessus": (2, 1), "dessous": (2, -1)}
DIRKEY = {(0, 1): "+x", (0, -1): "-x", (1, 1): "+y", (1, -1): "-y", (2, 1): "+z", (2, -1): "-z"}
YAW = {"est": 0, "nord": 90, "ouest": 180, "sud": 270}
DOOR_MODEL = "models/props_c17/door01_left.mdl"  # battant 48.4 x 108.4, charnière à l'origine, le long de +Y
DOOR_HALF_H = 54.2
LIGHT_MODEL = "models/props_c17/light_domelight02_on.mdl"  # plafonnier, origine = point de fixation
FINISH_ROLES = {"plinthe", "cadre", "marche", "garde_corps", "porte", "pilastre", "corniche", "soubassement"}


class Zone:
    def __init__(self, nom, typ, lo, hi, spec):
        self.nom, self.type, self.spec = nom, typ, spec
        self.lo, self.hi = np.array(lo, float), np.array(hi, float)
        self.englobe = bool(spec.get("englobe"))
        self.cibles = None   # zone englobante : noms (motifs fnmatch) des zones qu'elle entoure ; None = toutes
        # ciel_2d : faces de ciel en toolsskybox2d (une 2e couche sous la ville : un seul sky_camera par map)
        self.ciel2d = bool(spec.get("ciel_2d"))
        # plein : bâtiment fermé sans intérieur (masse d'une ville) : volume entièrement en brosses du monde, aucune
        # face intérieure. Une pièce creuse jamais visitée coûtait sa lightmap (48 Mo sur 16 pour une ville, 06/11)
        self.plein = bool(spec.get("plein"))
        self.lm = spec.get("lightmap", 32 if typ == "exterieur" else 16)
        # role : ce que la pièce est (wc, cuisine, bureau…, ou vide) ; valider.py contrôle qu'elle en a le contenu
        self.role = spec.get("role")

    @property
    def size(self):
        return self.hi - self.lo


class Gen:
    def __init__(self, spec, base, phase=None):
        from srctools.vmf import VMF

        self.spec, self.base = spec, Path(base)
        self.phase = phase or spec.get("phase", "texture")
        self.T = spec.get("murs", 16)
        self.S = spec.get("dalles", 16)
        self.err, self.warn = [], []
        self.vmf = VMF()
        self.zones = {}
        self.openings = []   # dict : type, entre, lo, hi, axis, zone (zone porteuse des matériaux), spec
        self.elements = []   # dict : type, nom, lo, hi (AABB), zone
        self.lights = []
        wl = yaml.safe_load((DATA / "whitelist.yaml").read_text(encoding="utf8"))
        self.blockout = wl.get("blockout", {})
        self.scales = {e["nom"]: e.get("echelle", wl.get("echelle_defaut", 0.25))
                       for lst in (wl.get("usages") or {}).values() for e in lst}
        self.default_scale = wl.get("echelle_defaut", 0.25)
        # kit: une entrée peut être {nom, echelle} : une falaise de 3 500 à l'échelle 0,25 répète ses strates tous
        # les 256 (moiré avec le contour d'encre, vu en jeu le 07/11)
        for k in spec.get("kit", []) or []:
            if isinstance(k, dict) and k.get("nom"):
                self.scales[k["nom"].lower()] = k.get("echelle", self.default_scale)
        self.models = load_available("modeles")
        for d in spec.get("contenu", []):
            self.models.update(scan_content_dir(self.base / d, "modeles"))
        self.static_ok = load_static()
        self.defaults = {k: v.lower() for k, v in (spec.get("materiaux") or {}).items()}

    # --- messages -----------------------------------------------------------
    def fail(self, msg):
        self.err.append(msg)

    def rule(self, ok, msg, item):
        """Règle d'échelle : erreur, ou avertissement si l'élément déclare une exception."""
        if ok:
            return
        if isinstance(item, dict) and item.get("exception"):
            self.warn.append(f"{msg} (exception : {item['exception']})")
        else:
            self.err.append(msg + "  [ajouter exception: \"raison\" si c'est voulu]")

    def on_grid(self, values, grid, what):
        for v in np.ravel(values):
            if abs(v / grid - round(v / grid)) > 1e-6:
                self.fail(f"{what} : {v:g} n'est pas un multiple de {grid}")
                return False
        return True

    # --- matériaux ----------------------------------------------------------
    def mat(self, role, zone=None, override=None):
        if self.phase == "blockout":
            key = {"toit": "sol", "plinthe": "detail", "cadre": "detail", "marche": "detail", "garde_corps": "detail",
                   "porte": "detail", "pilastre": "detail", "corniche": "detail", "soubassement": "detail"}.get(role, role)
            if role == "mur" and zone is not None and zone.type == "exterieur":
                key = "facade"
            return self.blockout.get(key, self.blockout.get("detail", NODRAW))
        if override:
            return override.lower()
        zm = {k: v.lower() for k, v in ((zone.spec.get("materiaux") or {}) if zone else {}).items()}
        if role in zm:
            return zm[role]
        if zone is not None and zone.type == "exterieur":
            chain = {"sol": ["sol_exterieur", "sol"], "mur": ["facade", "mur"], "toit": ["toit", "facade"]}.get(role, [role])
        else:
            chain = [role]
        if role in FINISH_ROLES:  # finitions : repli sur `detail`
            chain = chain + ["detail"]
        for k in chain:
            if k in self.defaults:
                return self.defaults[k]
        self.fail(f"aucun matériau pour le rôle '{role}'" + (f" (zone {zone.nom})" if zone else "") +
                  " : le déclarer dans materiaux: de la spec ou de la zone")
        return NODRAW

    def scale(self, mat):
        return self.scales.get(mat, self.default_scale)

    # --- zones --------------------------------------------------------------
    def resolve_zones(self):
        pending = list(self.spec.get("zones") or [])
        englobes = []
        while pending:
            progress = False
            for z in list(pending):
                if z.get("englobe"):
                    englobes.append(z)
                    pending.remove(z)
                    progress = True
                    continue
                size = np.array(z.get("taille", [0, 0, 0]), float)
                if "origine" in z:
                    lo = np.array(z["origine"], float)
                elif "contre" in z:
                    ref = self.zones.get(z["contre"].get("zone"))
                    if ref is None:
                        continue
                    lo = self.place_against(z, ref, size)
                else:
                    self.fail(f"zone {z.get('nom')} : ni 'origine' ni 'contre'")
                    pending.remove(z)
                    continue
                self.add_zone(z, lo, lo + size)
                pending.remove(z)
                progress = True
            if not progress:
                self.fail("zones non placées (référence inconnue ou circulaire) : " + ", ".join(z.get("nom", "?") for z in pending))
                break
        for z in englobes:
            others = list(self.zones.values())
            motifs = z.get("englobe_zones")   # ex. ["W*", "gare"] : les autres zones restent hors de l'enveloppe
            if motifs:
                others = [o for o in others if any(fnmatch.fnmatchcase(o.nom, m) for m in motifs)]
            if not others:
                self.fail(f"zone englobante {z.get('nom')} sans rien à englober")
                continue
            m = z.get("marge", 512)
            # marge par côté possible : {sud: 0} colle l'enceinte à la zone la plus au sud (porte dans le mur)
            mg = {c: (m.get(c, 512) if isinstance(m, dict) else m) for c in ("ouest", "est", "sud", "nord")}
            lo = np.min([o.lo for o in others], axis=0) - [self.T + mg["ouest"], self.T + mg["sud"], 0]
            hi = np.max([o.hi for o in others], axis=0) + [self.T + mg["est"], self.T + mg["nord"], 0]
            lo[2] = z.get("sol", min(o.lo[2] for o in others))
            hi[2] = lo[2] + z.get("hauteur", max(512, hi[2] - lo[2] + self.S + 256))
            self.add_zone(z, lo, hi)
            if z.get("nom") in self.zones:
                self.zones[z["nom"]].cibles = {o.nom for o in others}

    def place_against(self, z, ref, size):
        c = z["contre"]
        side = c.get("cote")
        if side not in SIDES:
            self.fail(f"zone {z.get('nom')} : contre.cote '{side}' inconnu ({', '.join(SIDES)})")
            return ref.lo.copy()
        ax, sg = SIDES[side]
        gap = c.get("mur", self.S if ax == 2 else self.T)
        lo = ref.lo.copy()
        dec = c.get("decalage", 0)
        dec = np.array(dec if isinstance(dec, list) else [dec, dec], float)
        align = c.get("aligne", "centre")
        others = [a for a in (0, 1) if a != ax]
        for k, a in enumerate(others):
            if align == "debut":
                lo[a] = ref.lo[a]
            elif align == "fin":
                lo[a] = ref.hi[a] - size[a]
            else:
                lo[a] = math.floor((ref.lo[a] + (ref.size[a] - size[a]) / 2) / GRID_WORLD) * GRID_WORLD
            lo[a] += dec[k] if ax == 2 else dec[0]
        if ax != 2:
            lo[2] = ref.lo[2] + c.get("dz", 0)
        lo[ax] = ref.hi[ax] + gap if sg > 0 else ref.lo[ax] - gap - size[ax]
        return lo

    def add_zone(self, z, lo, hi):
        nom, typ = z.get("nom"), z.get("type", "piece")
        if not nom or nom in self.zones:
            self.fail(f"zone sans nom ou nom en double : {nom}")
            return
        if typ not in ("piece", "couloir", "exterieur"):
            self.fail(f"zone {nom} : type '{typ}' inconnu (piece, couloir, exterieur)")
        Z = Zone(nom, typ, lo, hi, z)
        if Z.role is not None:
            from mapkit.config import load_roles
            if Z.role not in load_roles():
                self.fail(f"zone {nom} : role '{Z.role}' inconnu (data/roles.yaml : {', '.join(sorted(load_roles()))})")
        self.on_grid(np.r_[Z.lo, Z.hi], GRID_WORLD, f"zone {nom}")
        sx, sy, h = Z.size
        if typ == "piece":
            self.rule(h >= 128, f"zone {nom} : plafond à {h:g} (< 128)", z)
            self.rule(min(sx, sy) >= 64, f"zone {nom} : pièce de {min(sx, sy):g} de large (< 64)", z)
        elif typ == "couloir":
            self.rule(min(sx, sy) >= 96, f"zone {nom} : couloir de {min(sx, sy):g} de large (< 96)", z)
            self.rule(h >= 128, f"zone {nom} : plafond à {h:g} (< 128)", z)
        else:
            hm = z.get("hauteur_murs", min(192, h - 64))
            Z.hm = hm
            self.on_grid([hm], GRID_WORLD, f"zone {nom} hauteur_murs")
            self.rule(h - hm >= 64, f"zone {nom} : moins de 64 de ciel au-dessus des murs", z)
        self.zones[nom] = Z

    def check_zone_pairs(self):
        zs = [z for z in self.zones.values() if not z.englobe]
        for i, A in enumerate(zs):
            for B in zs[i + 1:]:
                ext = np.minimum(A.hi, B.hi) - np.maximum(A.lo, B.lo)
                if np.all(ext > 0):
                    self.fail(f"zones {A.nom} et {B.nom} se chevauchent")
                    continue
                sep = [a for a in range(3) if ext[a] <= 0]
                if len(sep) == 1:
                    a = sep[0]
                    others = [k for k in range(3) if k != a]
                    if all(ext[k] > 0 for k in others):
                        need = self.S if a == 2 else self.T
                        gap = -ext[a]
                        if gap < need:
                            self.fail(f"zones {A.nom} et {B.nom} : {gap:g} entre elles, moins que l'épaisseur {need} "
                                      f"({'zones collées sans mur : utiliser une ouverture passage' if gap == 0 else 'mur trop mince'})")
        for E in (z for z in self.zones.values() if z.englobe):
            if E.type != "exterieur":
                self.fail(f"zone {E.nom} : englobe: true n'existe que pour type: exterieur")
            for Z in zs:
                if E.cibles is not None and Z.nom not in E.cibles:
                    # hors de l'enveloppe : elle ne doit pas la toucher (couche à part, ex. des badlands dessous)
                    ext = np.minimum(Z.hi + [self.T, self.T, self.S], E.hi) - np.maximum(Z.lo - [self.T, self.T, self.S], E.lo)
                    if np.all(ext > 0):
                        self.fail(f"zone {Z.nom} : hors de englobe_zones mais dans l'enveloppe de {E.nom}")
                    continue
                if (np.any(Z.lo[:2] - self.T < E.lo[:2]) or np.any(Z.hi[:2] + self.T > E.hi[:2])
                        or Z.hi[2] + self.S > E.hi[2]):
                    self.fail(f"zone {Z.nom} déborde de la zone englobante {E.nom}")

    # --- ouvertures -----------------------------------------------------------
    def resolve_openings(self):
        for i, o in enumerate(self.spec.get("ouvertures") or []):
            typ = o.get("type", "passage")
            try:
                a, b = o["entre"]
                A, B = self.zones[a], self.zones[b]
            except (KeyError, ValueError, TypeError):
                self.fail(f"ouverture {i} : 'entre' doit nommer deux zones existantes ({o.get('entre')})")
                continue
            label = f"{typ} {a}/{b}"
            if A.englobe or B.englobe:
                Z, E = (B, A) if A.englobe else (A, B)
                side = o.get("cote")
                # dessus : trémie dans la dalle haute de Z vers l'air de l'enveloppe (cour ou jardin en contrebas)
                if side not in ("est", "ouest", "nord", "sud") and not (side == "dessus" and typ == "tremie"):
                    self.fail(f"{label} : vers une zone englobante, préciser cote: (nord/sud/est/ouest de {Z.nom}, "
                              "ou dessus pour une trémie)")
                    continue
                ax, sg = SIDES[side]
                ep = self.S if ax == 2 else self.T
                s0, s1 = (Z.hi[ax], Z.hi[ax] + ep) if sg > 0 else (Z.lo[ax] - ep, Z.lo[ax])
                lo_rng, hi_rng = Z.lo.copy(), Z.hi.copy()
                carrier = Z
            else:
                sep = None
                for ax in range(3):
                    if A.hi[ax] <= B.lo[ax]:
                        sep = (ax, A.hi[ax], B.lo[ax])
                    elif B.hi[ax] <= A.lo[ax]:
                        sep = (ax, B.hi[ax], A.lo[ax])
                    else:
                        continue
                    break
                if sep is None:
                    self.fail(f"{label} : les zones ne sont pas séparées par un mur")
                    continue
                ax, s0, s1 = sep
                need = self.S if ax == 2 else self.T
                if s1 - s0 > 2 * need:
                    self.fail(f"{label} : {s1 - s0:g} entre les zones (> {2 * need}) : ajouter une zone couloir")
                    continue
                lo_rng, hi_rng = np.maximum(A.lo, B.lo), np.minimum(A.hi, B.hi)
                carrier = A if not A.type == "exterieur" else B
            if ax == 2:
                if typ != "tremie":
                    self.fail(f"{label} : zones superposées : seule une ouverture 'tremie' est possible")
                    continue
                low = A if A.lo[2] < B.lo[2] else B
                r = o.get("rect")
                if not r:
                    self.fail(f"{label} : rect: [x, y, largeur, profondeur] requis (relatif à {low.nom})")
                    continue
                lo = np.array([low.lo[0] + r[0], low.lo[1] + r[1], s0])
                hi = np.array([lo[0] + r[2], lo[1] + r[3], s1])
                if np.any(lo[:2] < lo_rng[:2]) or np.any(hi[:2] > hi_rng[:2]):
                    self.fail(f"{label} : la trémie sort de l'emprise commune des deux zones")
            else:
                w = 1 - ax
                avail = hi_rng[w] - lo_rng[w]
                defaults = {"porte": (64, 128), "fenetre": (128, 64), "passage": (avail, None)}
                dw, dh = defaults.get(typ, (64, 128))
                width = o.get("largeur", dw)
                floor = carrier.lo[2]
                if typ in ("porte", "passage") and not (A.englobe or B.englobe):
                    if A.lo[2] != B.lo[2] and "z" not in o:
                        self.fail(f"{label} : sols à des hauteurs différentes ({A.lo[2]:g} / {B.lo[2]:g}) : "
                                  "ajouter un escalier ou préciser z:")
                bottom = floor + o.get("z", o.get("allege", 48) if typ == "fenetre" else 0)
                height = o.get("hauteur", dh if dh else hi_rng[2] - bottom)
                dec = o.get("decalage", "centre")
                if dec == "centre":
                    start = lo_rng[w] + math.floor((avail - width) / 2 / GRID_WORLD) * GRID_WORLD
                else:
                    start = lo_rng[w] + dec
                lo, hi = np.zeros(3), np.zeros(3)
                lo[ax], hi[ax] = s0, s1
                lo[w], hi[w] = start, start + width
                lo[2], hi[2] = bottom, bottom + height
                if start < lo_rng[w] or start + width > hi_rng[w]:
                    self.fail(f"{label} : l'ouverture ({start:g}..{start + width:g}) sort du mur commun "
                              f"({lo_rng[w]:g}..{hi_rng[w]:g})")
                if bottom < lo_rng[2] or bottom + height > hi_rng[2]:
                    self.fail(f"{label} : l'ouverture ({bottom:g}..{bottom + height:g} en z) dépasse la hauteur commune "
                              f"({lo_rng[2]:g}..{hi_rng[2]:g})")
                if typ == "porte":
                    self.rule(56 <= width <= 64, f"{label} : baie de porte {width:g} de large (56 à 64)", o)
                    self.rule(112 <= height <= 128, f"{label} : baie de porte {height:g} de haut (112 à 128)", o)
                elif typ == "passage":
                    self.rule(width >= 56, f"{label} : passage de {width:g} de large (< 56)", o)
                    self.rule(height >= 112, f"{label} : passage de {height:g} de haut (< 112)", o)
            self.on_grid(np.r_[lo, hi], GRID_WORLD, f"{label} (une ouverture découpe des murs du monde)")
            self.openings.append({"type": typ, "entre": [a, b], "lo": lo, "hi": hi, "axis": ax, "zone": carrier,
                                  "spec": o, "nom": o.get("nom", f"{typ}_{i}"), "ext": B if carrier is A else A})

    # --- enveloppe : air -> boîtes de murs ------------------------------------
    def carve(self):
        T, S = self.T, self.S
        dil = np.array([T, T, S], float)
        airs = []  # (lo, hi, clé)
        shells_e, shells = [], []
        for Z in self.zones.values():
            parts = [("int", Z.lo, Z.hi)]
            if Z.type == "exterieur":
                cut = Z.lo[2] + Z.hm
                parts = [("bas", Z.lo, np.r_[Z.hi[:2], cut]), ("ciel", np.r_[Z.lo[:2], cut], Z.hi)]
                if Z.hm <= 0:  # ciel dès le sol : pas de tranche « bas » de hauteur nulle
                    parts = [("ciel", Z.lo, Z.hi)]
            for part, lo, hi in parts:
                if Z.plein:
                    continue
                airs.append((lo, hi, ("zone", Z.nom, part), Z.englobe))
            (shells_e if Z.englobe else shells).append((Z.lo - dil, Z.hi + dil, Z if Z.type != "exterieur" else None))
        for k, o in enumerate(self.openings):
            airs.append((o["lo"], o["hi"], ("ouv", k), False))
        boxes = [(lo, hi) for lo, hi, _, _ in airs] + [(lo, hi) for lo, hi, _ in shells + shells_e]
        coords = [np.unique(np.concatenate([[b[0][a], b[1][a]] for b in boxes])) for a in range(3)]
        n = [len(c) - 1 for c in coords]
        keys = [None]
        state = np.zeros(n, np.int32)      # 0 vide, -1 plein, k > 0 air n° k
        # cellule pleine qui appartient à un bâtiment (coque d'une pièce ou d'un couloir) : vue depuis le ciel
        # d'un extérieur, elle est façade ou toit ; sinon c'est l'enceinte de l'extérieur, donc du ciel.
        # inner = n° du bâtiment propriétaire (self.owners), 0 = aucun : chaque bâtiment porte sa façade et son toit
        inner = np.zeros(n, np.int32)
        self.owners = [None]

        def rng(lo, hi):
            return tuple(slice(np.searchsorted(coords[a], lo[a]), np.searchsorted(coords[a], hi[a])) for a in range(3))

        for lo, hi, _ in shells_e:
            state[rng(lo, hi)] = -1
        for lo, hi, key, englobe in airs:
            if englobe:
                keys.append(key)
                state[rng(lo, hi)] = len(keys) - 1
        for lo, hi, building in shells:
            r = rng(lo, hi)
            state[r] = -1
            if building is not None:
                self.owners.append(building)
                inner[r] = len(self.owners) - 1
        for lo, hi, key, englobe in airs:
            if not englobe:
                keys.append(key)
                state[rng(lo, hi)] = len(keys) - 1
        solid = state == -1
        air = np.where(state > 0, state, 0)
        contact = {}
        for a in range(3):
            for sg in (1, -1):
                c = np.zeros(n, np.int32)
                src = [slice(None)] * 3
                dst = [slice(None)] * 3
                if sg > 0:
                    dst[a], src[a] = slice(0, n[a] - 1), slice(1, n[a])
                else:
                    dst[a], src[a] = slice(1, n[a]), slice(0, n[a] - 1)
                c[tuple(dst)] = air[tuple(src)]
                contact[(a, sg)] = c
        used = np.zeros(n, bool)
        out = []

        def faces_ok(i0, i1):
            sets = {}
            for (a, sg), c in contact.items():
                sl = [slice(i0[k], i1[k]) for k in range(3)]
                sl[a] = slice(i1[a] - 1, i1[a]) if sg > 0 else slice(i0[a], i0[a] + 1)
                v = np.unique(c[tuple(sl)])
                v = v[v > 0]
                if len(v) > 1:
                    return None
                sets[(a, sg)] = int(v[0]) if len(v) else 0
            return sets

        # graines dans l'ordre z, y, x : les sols d'abord (ils passent sous les murs), puis les murs pleine hauteur
        for k, j, i in np.argwhere(solid.transpose(2, 1, 0)):
            if used[i, j, k]:
                continue
            i0, i1 = [i, j, k], [i + 1, j + 1, k + 1]
            flag = inner[i, j, k]
            for a in range(3):
                while i1[a] < n[a]:
                    t1 = list(i1)
                    t1[a] += 1
                    sl = tuple(slice(i0[q], t1[q]) for q in range(3))
                    if not (solid[sl].all() and not used[sl].any() and (inner[sl] == flag).all()):
                        break
                    if faces_ok(i0, t1) is None:
                        break
                    i1 = t1
            sl = tuple(slice(i0[q], i1[q]) for q in range(3))
            used[sl] = True
            lo = np.array([coords[q][i0[q]] for q in range(3)])
            hi = np.array([coords[q][i1[q]] for q in range(3)])
            out.append((lo, hi, {d: keys[v] if v else None for d, v in faces_ok(i0, i1).items()}, flag))
        self.shell_boxes = out

    def mat_proprio(self, role, owner, Z):
        """Face d'un bâtiment vue depuis un extérieur : la façade (ou le toit) déclarée par le bâtiment lui-même
        dans ses materiaux, sinon celle de l'extérieur. Sans cela, toute une ville prenait la façade de l'enveloppe."""
        O = self.owners[owner] if owner else None
        want = {"toit": "toit", "dessous": "sous_face"}.get(role, "facade")
        if O is not None and self.phase != "blockout":
            om = O.spec.get("materiaux") or {}
            if want in om:
                return self.mat(want, O)
            if want == "sous_face" and "facade" in om:
                return self.mat("facade", O)
        return self.mat("mur" if role == "dessous" else role, Z)

    def shell_material(self, d, key, inner, lo, hi):
        if key is None:
            return NODRAW, 16
        if key[0] == "ouv":
            o = self.openings[key[1]]
            Z = o["zone"]
            if d == (2, 1) and o["type"] in ("porte", "passage"):
                return self.mat("sol", Z), Z.lm
            return self.mat("mur", Z), Z.lm
        Z = self.zones[key[1]]
        if key[2] == "ciel":
            if d == (2, 1) and hi[2] == Z.lo[2]:  # le sol d'un extérieur reste un sol, même sous le ciel
                return self.mat("sol", Z), Z.lm
            if not inner:
                return (SKY2D if Z.ciel2d else SKY), 16
            role = {(2, 1): "toit", (2, -1): "dessous"}.get(d, "mur")
            return self.mat_proprio(role, inner, Z), Z.lm
        if Z.type == "exterieur":
            if d == (2, 1):
                return (self.mat("sol", Z) if hi[2] == Z.lo[2] else self.mat_proprio("toit", inner, Z)), Z.lm
            if d == (2, -1):   # sous-face d'un volume en saillie (arcade, porte-à-faux) vue d'en dessous
                return self.mat_proprio("dessous", inner, Z), Z.lm
            return self.mat_proprio("mur", inner, Z), Z.lm
        if d == (2, 1):
            return self.mat("sol", Z), Z.lm
        if d == (2, -1):
            return self.mat("plafond", Z), Z.lm
        return self.mat("mur", Z), Z.lm

    def write_shell(self):
        for lo, hi, contacts, inner in self.shell_boxes:
            mats, scales, lms = {}, {}, {}
            for d, key in contacts.items():
                m, lm = self.shell_material(d, key, inner, lo, hi)
                k = DIRKEY[d]
                mats[k], scales[k], lms[k] = m, self.scale(m), lm
            self.vmf.add_brush(make_solid(self.vmf, box_faces(lo, hi, mats, scales, lms)))

    # --- éléments positifs (func_detail) ---------------------------------------
    def detail_ent(self, name, cls="func_detail"):
        ent = self.vmf.create_ent(cls)
        if cls != "func_detail":
            ent["targetname"] = name
        return ent

    def add_box(self, ent, lo, hi, top, sides, bottom=None, lm=16, grid=None):
        lo, hi = np.asarray(lo, float), np.asarray(hi, float)
        self.on_grid(np.r_[lo, hi], grid or GRID_DETAIL, f"élément ({ent['classname']})")
        bottom = bottom or sides
        mats = {"+z": top, "-z": bottom, "+x": sides, "-x": sides, "+y": sides, "-y": sides}
        ent.solids.append(make_solid(self.vmf, box_faces(lo, hi, mats, {k: self.scale(v) for k, v in mats.items()},
                                                         {k: lm for k in mats})))

    def zone_of(self, e, label):
        if "zone" not in e:
            return None
        Z = self.zones.get(e["zone"])
        if Z is None:
            self.fail(f"{label} : zone '{e['zone']}' inconnue")
        return Z

    def origin_of(self, Z):
        return Z.lo if Z is not None else np.zeros(3)

    def inside(self, Z, lo, hi, label):
        if Z is not None and (np.any(lo < Z.lo - 1e-6) or np.any(hi > Z.hi + 1e-6)):
            self.fail(f"{label} : sort de la zone {Z.nom} ({lo.tolist()}..{hi.tolist()})")

    def build_elements(self):
        specs = self.spec.get("elements") or []
        # les dalles d'abord : les escaliers vérifient qu'ils y arrivent
        order = sorted(range(len(specs)), key=lambda i: specs[i].get("type") != "dalle")
        for i in order:
            e = specs[i]
            typ = e.get("type")
            label = f"élément {e.get('nom', i)} ({typ})"
            Z = self.zone_of(e, label)
            O = self.origin_of(Z)
            fn = getattr(self, f"el_{typ}", None)
            if fn is None:
                self.fail(f"{label} : type inconnu (dalle, escalier, rampe, bloc, instance)")
                continue
            fn(e, Z, O, label)

    def mats3(self, e, Z, top_role, side_role="detail"):
        m = e.get("materiaux") or {}
        top = self.mat(top_role, Z, m.get("dessus"))
        sides = self.mat(side_role, Z, m.get("cotes"))
        bottom = self.mat(side_role, Z, m.get("dessous"))
        return top, sides, bottom

    def el_dalle(self, e, Z, O, label):
        x, y, w, d = e["rect"]
        ep = e.get("epaisseur", 16)
        top_z = O[2] + e["z"]
        lo = np.array([O[0] + x, O[1] + y, top_z - ep])
        hi = np.array([lo[0] + w, lo[1] + d, top_z])
        self.inside(Z, lo, hi, label)
        if Z is not None:
            self.rule(top_z - Z.lo[2] - ep >= 128, f"{label} : {top_z - Z.lo[2] - ep:g} sous la dalle (< 128)", e)
            self.rule(Z.hi[2] - top_z >= 128, f"{label} : {Z.hi[2] - top_z:g} au-dessus de la dalle (< 128)", e)
        m = e.get("materiaux") or {}
        ent = self.detail_ent(e.get("nom", "dalle"))
        self.add_box(ent, lo, hi, self.mat("sol", Z, m.get("dessus")), self.mat("detail", Z, m.get("cotes")),
                     self.mat("plafond", Z, m.get("dessous")), Z.lm if Z else 16)
        el = {"type": "dalle", "nom": e.get("nom"), "lo": lo, "hi": hi, "zone": Z, "spec": e, "ent": ent}
        self.elements.append(el)
        if Z is not None and e.get("lumiere", "auto") == "auto" and self.lights_auto(Z):
            self.grid_lights(Z, lo, hi, lo[2] - 8, lo[2], 150)

    def finish_rails(self):
        """Garde-corps des dalles, une fois les escaliers connus (on laisse leur arrivée libre)."""
        stairs = [el for el in self.elements if el["type"] == "escalier"]
        for el in [el for el in self.elements if el["type"] == "dalle"]:
            e, lo, hi, Z = el["spec"], el["lo"], el["hi"], el["zone"]
            sides = e.get("garde_corps") or []
            if not sides:
                continue
            h, t = e.get("hauteur_garde_corps", 48), 8
            ent = self.detail_ent("garde_corps")
            mat = self.mat("garde_corps", Z, (e.get("materiaux") or {}).get("garde_corps"))
            for side in sides:
                ax, sg = SIDES[side]
                w = 1 - ax
                a0, a1 = lo[w], hi[w]
                if w == 1:  # côtés est/ouest : raccourcis aux angles tenus par nord/sud
                    if "sud" in sides:
                        a0 += t
                    if "nord" in sides:
                        a1 -= t
                spans = [(a0, a1)]
                edge = hi[ax] if sg > 0 else lo[ax]
                for st in stairs:
                    if st["arrive_axis"] == ax and abs(st["arrive_at"] - edge) < 1e-6:
                        spans = subtract(spans, st["lo"][w], st["hi"][w])
                for s0, s1 in spans:
                    if s1 - s0 < 8:
                        continue
                    blo, bhi = lo.copy(), hi.copy()
                    blo[2], bhi[2] = hi[2], hi[2] + h
                    blo[w], bhi[w] = s0, s1
                    if sg > 0:
                        blo[ax] = hi[ax] - t
                    else:
                        bhi[ax] = lo[ax] + t
                    self.add_box(ent, blo, bhi, mat, mat)
                    self.elements.append({"type": "garde_corps", "lo": blo, "hi": bhi, "zone": Z})

    def el_escalier(self, e, Z, O, label):
        side = e.get("direction")
        if side not in YAW:
            self.fail(f"{label} : direction '{side}' (nord, sud, est, ouest)")
            return
        ax, sg = SIDES[side]
        w = 1 - ax
        rise, run = e.get("marche", 8), e.get("giron", 16)
        H, width = e["hauteur"], e.get("largeur", 96)
        z0 = O[2] + e.get("z", 0)
        self.rule(rise <= 18, f"{label} : marche de {rise} (> 18 : infranchissable sans sauter)", e)
        self.rule(run >= 12, f"{label} : giron de {run} (< 12)", e)
        self.rule(width >= 64, f"{label} : escalier de {width} de large (< 64)", e)
        if H % rise:
            self.fail(f"{label} : hauteur {H} non divisible par la marche {rise}")
            return
        nsteps = H // rise
        L = (nsteps - 1) * run
        p = np.array([O[0] + e["depart"][0], O[1] + e["depart"][1], z0])
        top, sides, _ = self.mats3(e, Z, "marche")
        ent = self.detail_ent("escalier")
        style = e.get("style", "plein")  # plein : marches pleines jusqu'au sol ; limon : marches de 8, dessous ouvert
        if style not in ("plein", "limon"):
            self.fail(f"{label} : style '{style}' (plein, limon)")
            return
        for k in range(1, nsteps):
            lo, hi = p.copy(), p.copy()
            lo[w], hi[w] = p[w], p[w] + width
            a = p[ax] + sg * (k - 1) * run
            b = a + sg * run
            lo[ax], hi[ax] = min(a, b), max(a, b)
            lo[2], hi[2] = (z0 if style == "plein" else z0 + k * rise - 8), z0 + k * rise
            self.add_box(ent, lo, hi, top, sides)
        # garde-corps rampant sur les côtés libres : 48 au-dessus du nez des marches, 8 d'épaisseur,
        # il rejoint à l'arrivée le garde-corps d'une dalle (même hauteur)
        end_s = p[ax] + sg * L
        for gside in e.get("garde_corps") or []:
            if gside not in SIDES or SIDES[gside][0] != w:
                self.fail(f"{label} : garde_corps '{gside}' doit être un côté de l'escalier, pas son sens")
                continue
            if rise % 8:
                self.fail(f"{label} : garde-corps rampant impossible avec une marche de {rise} (multiple de 8)")
                continue
            gs = SIDES[gside][1]
            q0, q1 = (p[w] + width, p[w] + width + 8) if gs > 0 else (p[w] - 8, p[w])
            pts = []
            for q in (q0, q1):
                for s, zz in ((p[ax], z0), (p[ax], z0 + rise + 48), (end_s, z0 + H + 48), (end_s, z0 + H - 16)):
                    v = np.zeros(3)
                    v[w], v[ax], v[2] = q, s, zz
                    pts.append(v)
            self.on_grid(np.ravel(pts), GRID_DETAIL, f"{label} garde-corps")
            m = self.mat("garde_corps", Z, (e.get("materiaux") or {}).get("garde_corps"))
            gent = self.detail_ent("garde_corps_escalier")
            gent.solids.append(make_solid(self.vmf, [(n, poly, m, self.scale(m), 16) for n, poly in hull_faces(pts)]))
            P = np.array(pts)
            self.inside(Z, P.min(axis=0), P.max(axis=0), f"{label} garde-corps")
            self.elements.append({"type": "garde_corps", "lo": P.min(axis=0), "hi": P.max(axis=0), "zone": Z})
        lo, hi = p.copy(), p.copy()
        lo[w], hi[w] = p[w], p[w] + width
        end = p[ax] + sg * L
        lo[ax], hi[ax] = min(p[ax], end), max(p[ax], end)
        hi[2] = z0 + H - rise
        self.inside(Z, lo, hi, label)
        landing = [el for el in self.elements if el["type"] == "dalle" and abs(el["hi"][2] - (z0 + H)) < 1e-6
                   and abs((el["lo"][ax] if sg > 0 else el["hi"][ax]) - end) < 1e-6
                   and el["lo"][w] <= lo[w] and el["hi"][w] >= hi[w]]
        if not landing:
            self.warn.append(f"{label} : la dernière marche n'arrive sur aucune dalle à z={z0 + H:g}")
        if e.get("clip"):
            c = self.vmf.create_ent("func_detail")
            pts = []
            for q in (lo[w], hi[w]):
                for (s, zz) in ((p[ax], z0), (end, z0 + H - rise), (end, z0)):
                    v = np.zeros(3)
                    v[w], v[ax], v[2] = q, s, zz
                    pts.append(v)
            c.solids.append(make_solid(self.vmf, [(n, poly, "tools/toolsplayerclip", 0.25, 16) for n, poly in hull_faces(pts)]))
        self.elements.append({"type": "escalier", "lo": lo, "hi": hi, "zone": Z, "arrive_axis": ax, "arrive_at": end,
                              "direction": side})

    def el_rampe(self, e, Z, O, label):
        side = e.get("direction")
        if side not in YAW:
            self.fail(f"{label} : direction '{side}' (nord, sud, est, ouest)")
            return
        ax, sg = SIDES[side]
        w = 1 - ax
        L, H, width = e["longueur"], e["hauteur"], e.get("largeur", 128)
        ang = math.degrees(math.atan2(H, L))
        self.rule(ang <= 45, f"{label} : pente de {ang:.1f}° (> 45 : on glisse)", e)
        p = np.array([O[0] + e["depart"][0], O[1] + e["depart"][1], O[2] + e.get("z", 0)])
        end = p[ax] + sg * L
        pts = []
        for q in (p[w], p[w] + width):
            for (s, zz) in ((p[ax], p[2]), (end, p[2]), (end, p[2] + H)):
                v = np.zeros(3)
                v[w], v[ax], v[2] = q, s, zz
                pts.append(v)
        self.on_grid(np.ravel(pts), GRID_DETAIL, label)
        top, sides, _ = self.mats3(e, Z, "sol")
        ent = self.detail_ent("rampe")
        faces = []
        for n, poly in hull_faces(pts):
            m = top if n[2] > 0.01 else sides
            faces.append((n, poly, m, self.scale(m), Z.lm if Z else 16))
        ent.solids.append(make_solid(self.vmf, faces))
        P = np.array(pts)
        self.inside(Z, P.min(axis=0), P.max(axis=0), label)
        self.elements.append({"type": "rampe", "lo": P.min(axis=0), "hi": P.max(axis=0), "zone": Z})

    def el_bloc(self, e, Z, O, label):
        x, y, w, d = e["rect"]
        lo = np.array([O[0] + x, O[1] + y, O[2] + e.get("z", 0)])
        hi = lo + [w, d, e["hauteur"]]
        self.inside(Z, lo, hi, label)
        top, sides, bottom = self.mats3(e, Z, "detail")
        cls = e.get("classe", "func_detail")
        ent = self.vmf.spawn if cls == "monde" else self.detail_ent(e.get("nom", "bloc"), cls)
        grid = GRID_WORLD if cls == "monde" else GRID_DETAIL
        lm = e.get("lightmap", Z.lm if Z else 16)   # un relief de désert en blocs se met à 128-256
        if cls == "monde":
            mats = {"+z": top, "-z": bottom, "+x": sides, "-x": sides, "+y": sides, "-y": sides}
            self.on_grid(np.r_[lo, hi], grid, label)
            self.vmf.add_brush(make_solid(self.vmf, box_faces(lo, hi, mats, {k: self.scale(v) for k, v in mats.items()},
                                                              {k: lm for k in mats})))
        else:
            self.add_box(ent, lo, hi, top, sides, bottom, lm, grid=grid)
        self.elements.append({"type": "bloc", "nom": e.get("nom"), "lo": lo, "hi": hi, "zone": Z})

    def el_instance(self, e, Z, O, label):
        fname = e.get("fichier", "")
        src = find_module(fname, self.base)
        if src is None:
            self.fail(f"{label} : module '{fname}' introuvable (modules/ du projet ou du skill)")
            return
        dest = self.out.parent / "modules" / src.name
        # Une copie d'un module du skill dans le projet ne doit pas masquer une version plus récente du skill
        # (vu le 02/11/2026 : l'arbuste corrigé dans le skill restait l'ancien dans la map).
        skill_src = MODULES / src.name
        if skill_src.exists() and src.resolve() != skill_src.resolve() and not filecmp.cmp(src, skill_src, shallow=False):
            if skill_src.stat().st_mtime > src.stat().st_mtime:
                self.warn.append(f"{label} : module {src.name} mis à jour depuis le skill (copie du projet remplacée)")
                src = skill_src
            else:
                self.warn.append(f"{label} : la copie de {src.name} du projet diffère du skill et elle est plus "
                                 "récente : gardée (la renommer si c'est un module propre au projet)")
        if src.resolve() != dest.resolve():
            dest.parent.mkdir(exist_ok=True)
            shutil.copy2(src, dest)
        pos = list(e.get("position", [0, 0])) + [0]
        origin = O + np.array(pos[:3], float)
        ang = e.get("angle", 0)
        if ang % 90:
            self.fail(f"{label} : angle {ang} (multiples de 90 seulement, pour rester sur la grille)")
        self.on_grid(origin, GRID_DETAIL, label)
        ent = self.vmf.create_ent("func_instance", targetname=e.get("nom", f"inst_{len(self.elements)}"),
                                  file=f"modules/{src.name}", angles=f"0 {ang} 0",
                                  origin=" ".join(f"{c:g}" for c in origin), fixup_style="0")
        _, brushes, _ = read_map(src)
        if brushes:
            V = np.array([v for b in brushes for v in b.vertices])
            th = math.radians(ang)
            R = np.array([[math.cos(th), -math.sin(th), 0], [math.sin(th), math.cos(th), 0], [0, 0, 1]])
            V = V @ R.T + origin
            lo, hi = V.min(axis=0), V.max(axis=0)
            self.inside(Z, lo, hi, label)
            self.elements.append({"type": "instance", "lo": lo, "hi": hi, "zone": Z, "nom": ent["targetname"],
                                  "fichier": src.name})

    # --- finitions : cadres, vitres, portes, plinthes -------------------------------
    def build_openings_dressing(self):
        for o in self.openings:
            spec, Z, ax = o["spec"], o["zone"], o["axis"]
            if ax == 2:
                continue
            w = 1 - ax
            lo, hi = o["lo"], o["hi"]
            clear_lo, clear_hi = lo.copy(), hi.copy()
            frame = spec.get("cadre", o["type"] == "porte")
            if frame:
                t = 8
                mat = self.mat("cadre", Z, spec.get("materiau_cadre"))
                ent = self.detail_ent("cadre")
                head = hi[2] - lo[2] - 112 if o["type"] == "porte" else spec.get("linteau", 0)
                for s0, s1 in ((lo[w], lo[w] + t), (hi[w] - t, hi[w])):
                    blo, bhi = lo.copy(), hi.copy()
                    blo[w], bhi[w] = s0, s1
                    blo[ax] -= t
                    bhi[ax] += t
                    self.add_box(ent, blo, bhi, mat, mat)
                if head > 0:
                    blo, bhi = lo.copy(), hi.copy()
                    blo[w], bhi[w] = lo[w] + t, hi[w] - t
                    blo[2] = hi[2] - head
                    blo[ax] -= t
                    bhi[ax] += t
                    self.add_box(ent, blo, bhi, mat, mat)
                clear_lo[w] += t
                clear_hi[w] -= t
                clear_hi[2] -= max(head, 0)
                flo, fhi = lo.copy(), hi.copy()
                flo[ax] -= t
                fhi[ax] += t
                self.elements.append({"type": "cadre", "lo": flo, "hi": fhi, "zone": Z, "ouverture": o["nom"]})
            o["clear"] = (clear_lo, clear_hi)
            if o["type"] == "fenetre" and spec.get("vitre", True):
                gl, gh = clear_lo.copy(), clear_hi.copy()
                if lo[ax] >= Z.hi[ax] - 1e-6:  # l'ouverture est du côté + de la zone porteuse : vitre côté +
                    gl[ax] = hi[ax] - 8
                else:
                    gh[ax] = lo[ax] + 8
                verre = self.mat("verre", Z, spec.get("materiau_verre"))
                ent = self.detail_ent("vitre")
                mats = {k: NODRAW for k in ("+x", "-x", "+y", "-y", "+z", "-z")}
                mats[DIRKEY[(ax, 1)]] = mats[DIRKEY[(ax, -1)]] = verre
                self.on_grid(np.r_[gl, gh], GRID_DETAIL, f"vitre {o['nom']}")
                ent.solids.append(make_solid(self.vmf, box_faces(gl, gh, mats, {k: self.scale(v) for k, v in mats.items()},
                                                                 {k: 16 for k in mats})))
            if o["type"] == "porte":
                kind = spec.get("entite", "prop_door_rotating")
                if kind == "prop_door_rotating":
                    self.door_prop(o, clear_lo, clear_hi)
                elif kind == "func_door":
                    self.door_brush(o, clear_lo, clear_hi)
                elif kind not in ("aucune", None):
                    self.fail(f"porte {o['nom']} : entite '{kind}' inconnue (prop_door_rotating, func_door, aucune)")

    def door_prop(self, o, clo, chi):
        ax = o["axis"]
        w = 1 - ax
        model = o["spec"].get("modele", DOOR_MODEL)
        if model.lower() not in self.models:
            self.fail(f"porte {o['nom']} : modèle {model} absent")
        width = chi[w] - clo[w]
        if width < 48 or width > 52:
            self.warn.append(f"porte {o['nom']} : passage libre de {width:g} pour un battant de 48 "
                             f"({'jour visible' if width > 52 else 'le battant frotte'}) : cadre: true sur une baie de 64")
        origin = np.zeros(3)
        origin[ax] = (o["lo"][ax] + o["hi"][ax]) / 2
        origin[w] = clo[w]
        origin[2] = clo[2] + DOOR_HALF_H + 0.8
        yaw = 0 if ax == 0 else 270
        self.vmf.create_ent("prop_door_rotating", targetname=o["spec"].get("nom", o["nom"]), model=model,
                            origin=" ".join(f"{c:g}" for c in origin), angles=f"0 {yaw} 0", spawnflags="8192",
                            distance="90", speed="100", returndelay="-1", hardware="1", opendir="0", skin="0",
                            spawnpos="0", health="0")
        self.elements.append({"type": "porte", "lo": clo, "hi": chi, "zone": o["zone"], "nom": o["nom"]})

    def door_brush(self, o, clo, chi):
        ax = o["axis"]
        w = 1 - ax
        lo, hi = clo.copy(), chi.copy()
        lo[ax] = o["lo"][ax] + (o["hi"][ax] - o["lo"][ax] - 8) // 16 * 8  # 8 d'épaisseur, sur la grille de 8
        hi[ax] = lo[ax] + 8
        ent = self.vmf.create_ent("func_door", targetname=o["spec"].get("nom", o["nom"]), speed="100", wait="-1",
                                  lip="4", spawnflags="256", movedir=f"0 {90 if w == 1 else 0} 0")
        mat = self.mat("porte", o["zone"], o["spec"].get("materiau_porte"))
        self.add_box(ent, lo, hi, mat, mat)

    def check_door_clearance(self):
        """Rien dans les 64 unités devant une porte ou un passage, des deux côtés (débattement + approche)."""
        for o in self.openings:
            if o["type"] not in ("porte", "passage") or o["axis"] == 2:
                continue
            ax = o["axis"]
            w = 1 - ax
            clo, chi = o.get("clear", (o["lo"], o["hi"]))
            lo, hi = clo.copy(), chi.copy()
            lo[ax], hi[ax] = o["lo"][ax] - 64, o["hi"][ax] + 64
            hi[2] = lo[2] + 72
            for el in self.elements:
                if el["type"] in ("cadre", "porte", "garde_corps", "dalle") or el.get("ouverture") == o["nom"]:
                    continue
                if np.all(np.minimum(hi, el["hi"]) - np.maximum(lo, el["lo"]) > 0):
                    self.warn.append(f"{o['type']} {o['nom']} : {el['type']} {el.get('nom') or ''} dans les 64 unités "
                                     f"devant le passage ({lo[w]:g}..{hi[w]:g})")
            for e in self.vmf.entities:
                if e["classname"].startswith("prop_") and e["classname"] != "prop_door_rotating" and e["origin"]:
                    p = np.array(tuple(e.get_origin()))
                    if np.all(p >= lo - [16, 16, 0]) and np.all(p <= hi + [16, 16, 0]) and p[2] < lo[2] + 64:
                        self.warn.append(f"{o['type']} {o['nom']} : {e['model']} posé devant le passage")

    def build_skirting(self):
        for Z in self.zones.values():
            spec = Z.spec.get("plinthe")
            if not spec or Z.type == "exterieur":
                continue
            spec = spec if isinstance(spec, dict) else {}
            h, t = spec.get("hauteur", 16), spec.get("epaisseur", 8)
            mat = self.mat("plinthe", Z, spec.get("materiau"))
            ent = self.detail_ent("plinthe")
            for side in ("sud", "nord", "ouest", "est"):
                ax, sg = SIDES[side]
                w = 1 - ax
                a0, a1 = Z.lo[w], Z.hi[w]
                if w == 1:
                    a0, a1 = a0 + t, a1 - t
                face = Z.hi[ax] if sg > 0 else Z.lo[ax]
                strip_lo, strip_hi = Z.lo.copy(), Z.hi.copy()
                strip_lo[ax], strip_hi[ax] = (face - t, face) if sg > 0 else (face, face + t)
                strip_hi[2] = Z.lo[2] + h
                spans = [(a0, a1)]
                for o in self.openings:
                    if o["axis"] == ax and o["lo"][2] <= Z.lo[2] + h and \
                            (abs(o["lo"][ax] - face) < 1e-6 or abs(o["hi"][ax] - face) < 1e-6):
                        spans = subtract(spans, o["lo"][w], o["hi"][w])
                for el in self.elements:
                    lo, hi = el["lo"], el["hi"]
                    if lo[2] < strip_hi[2] and hi[2] > strip_lo[2] and lo[ax] < strip_hi[ax] and hi[ax] > strip_lo[ax]:
                        spans = subtract(spans, lo[w], hi[w])
                for s0, s1 in spans:
                    if s1 - s0 < 8:
                        continue
                    blo, bhi = strip_lo.copy(), strip_hi.copy()
                    blo[w], bhi[w] = s0, s1
                    self.add_box(ent, blo, bhi, mat, mat)

    def build_decor(self):
        """Pilastres, corniche et soubassement le long des murs d'une zone (base, champ, haut), puis alerte
        sur les murs extérieurs de plus de 512 sans rupture (règle de la direction artistique)."""
        for Z in self.zones.values():
            spec = Z.spec.get("decor")
            top = (Z.lo[2] + Z.hm) if Z.type == "exterieur" else Z.hi[2]
            if spec and Z.type == "exterieur" and (Z.englobe or Z.hm <= 0):
                self.warn.append(f"zone {Z.nom} : decor ignoré (pas de murs sous le ciel)")
                spec = None
            spec = ({} if spec is True else spec) if spec else None
            pil = cor = sou = None
            if spec is not None:
                pil = spec.get("pilastres", {}) if spec.get("pilastres", True) is not False else None
                cor = spec.get("corniche", {}) if spec.get("corniche", True) is not False else None
                sou = spec.get("soubassement", {}) if spec.get("soubassement", True) is not False else None
                pil = None if pil is None else {"espacement": 256, "largeur": 32, "saillie": 16, **(pil or {})}
                cor = None if cor is None else {"hauteur": 16, "saillie": 24, **(cor or {})}
                sou = None if sou is None else {"hauteur": 32, "saillie": 8, **(sou or {})}
                if sou and Z.spec.get("plinthe"):
                    self.fail(f"zone {Z.nom} : plinthe et soubassement se chevaucheraient : garder l'un des deux")
                    continue
                mats = spec.get("materiaux") or {}
                m_pil = self.mat("pilastre", Z, mats.get("pilastre"))
                m_cor = self.mat("corniche", Z, mats.get("corniche"))
                m_sou = self.mat("soubassement", Z, mats.get("soubassement"))
                ent = self.detail_ent("decor")
            for side in ("sud", "nord", "ouest", "est"):
                ax, sg = SIDES[side]
                w = 1 - ax
                face = Z.hi[ax] if sg > 0 else Z.lo[ax]
                a0, a1 = Z.lo[w], Z.hi[w]
                holes = [(o["lo"][w], o["hi"][w], o["lo"][2], o["hi"][2]) for o in self.openings
                         if o["axis"] == ax and (abs(o["lo"][ax] - face) < 1e-6 or abs(o["hi"][ax] - face) < 1e-6)]
                near = [(el["lo"][w], el["hi"][w], el["lo"][2], el["hi"][2]) for el in self.elements
                        if el.get("zone") is Z and el["lo"][ax] < face + 32 and el["hi"][ax] > face - 32]

                def strip(depth, z0, z1, s0, s1):
                    lo, hi = Z.lo.copy(), Z.hi.copy()
                    lo[ax], hi[ax] = (face - depth, face) if sg > 0 else (face, face + depth)
                    lo[w], hi[w] = s0, s1
                    lo[2], hi[2] = z0, z1
                    return lo, hi

                pil_spans = []
                if spec is not None:
                    if pil:
                        n = max(1, round((a1 - a0) / pil["espacement"]))
                        half = pil["largeur"] / 2
                        for k in range(1, n):
                            c = math.floor((a0 + k * (a1 - a0) / n) / 8) * 8
                            s0, s1 = c - half, c + half
                            if any(s1 > h0 - 8 and s0 < h1 + 8 for h0, h1, _, _ in holes + near):
                                continue
                            ztop = top - (cor["hauteur"] if cor else 0)
                            self.add_box(ent, *strip(pil["saillie"], Z.lo[2], ztop, s0, s1), m_pil, m_pil)
                            pil_spans.append((s0, s1))
                    for part, m in ((sou, m_sou), (cor, m_cor)):
                        if not part:
                            continue
                        b0, b1 = (a0 + part["saillie"], a1 - part["saillie"]) if w == 1 else (a0, a1)
                        z0, z1 = (Z.lo[2], Z.lo[2] + part["hauteur"]) if part is sou else (top - part["hauteur"], top)
                        spans = [(b0, b1)]
                        for h0, h1, hz0, hz1 in holes + near:
                            if hz0 < z1 and hz1 > z0:
                                spans = subtract(spans, h0, h1)
                        if part is sou:
                            for s0, s1 in pil_spans:
                                spans = subtract(spans, s0, s1)
                        for s0, s1 in spans:
                            if s1 - s0 >= 8:
                                self.add_box(ent, *strip(part["saillie"], z0, z1, s0, s1), m, m)
                if Z.type == "exterieur" and not Z.englobe and Z.hm > 0:
                    spans = [(a0, a1)]
                    for h0, h1, _, _ in holes:
                        spans = subtract(spans, h0, h1)
                    for s0, s1 in pil_spans:
                        spans = subtract(spans, s0, s1)
                    longest = max((s1 - s0 for s0, s1 in spans), default=0)
                    if longest > 512:
                        self.warn.append(f"zone {Z.nom}, mur {side} : {longest:g} unités sans rupture (> 512) : "
                                         "decor: (pilastres) ou ouvertures")

    # --- lumières et entités ---------------------------------------------------------
    def lights_auto(self, Z):
        v = Z.spec.get("lumiere", "auto")
        return v == "auto" or isinstance(v, dict)

    def grid_lights(self, Z, lo, hi, z_light, z_fixture, bright):
        cfg = Z.spec.get("lumiere") if isinstance(Z.spec.get("lumiere"), dict) else {}
        step = cfg.get("espacement", 256)
        col = cfg.get("couleur", [255, 236, 214])
        bright = cfg.get("intensite", bright)
        model = cfg.get("modele", LIGHT_MODEL)
        if model and model != "aucun" and self.static_ok.get(model.lower()) is not True:
            self.fail(f"zone {Z.nom} : luminaire {model} absent ou refusé en prop_static")
            model = None
        nx = max(1, round((hi[0] - lo[0]) / step))
        ny = max(1, round((hi[1] - lo[1]) / step))
        for i in range(nx):
            for j in range(ny):
                x = lo[0] + (i + 0.5) * (hi[0] - lo[0]) / nx
                y = lo[1] + (j + 0.5) * (hi[1] - lo[1]) / ny
                if cfg.get("type", "spot") == "spot":
                    # demi-sphère vers le bas : le plafond ne reçoit que du rebond, pas de tache brûlée (vu en jeu).
                    # `intensite` reste en équivalent omni : sans le rebond du plafond, il faut ×2,5 pour garder le
                    # même niveau au sol (mesuré sur la démo : 0,64 en omni, 0,21 en spot à intensité égale)
                    self.vmf.create_ent("light_spot", origin=f"{x:g} {y:g} {z_fixture - 16:g}", angles="-90 0 0",
                                        pitch="-90", _light=f"{col[0]} {col[1]} {col[2]} {round(bright * 2.5):g}",
                                        _lightHDR="-1 -1 -1 1", _lightscaleHDR="1", _inner_cone="60", _cone="85",
                                        _exponent="1", _quadratic_attn="1", style="0")
                else:
                    self.vmf.create_ent("light", origin=f"{x:g} {y:g} {z_light:g}",
                                        _light=f"{col[0]} {col[1]} {col[2]} {bright:g}", _lightHDR="-1 -1 -1 1",
                                        _lightscaleHDR="1", _quadratic_attn="1", style="0")
                if model and model != "aucun":
                    self.vmf.create_ent("prop_static", model=model, origin=f"{x:g} {y:g} {z_fixture:g}",
                                        angles="0 0 0", solid="0", disableshadows="1", skin="0")

    def build_lights(self):
        any_sky = False
        for Z in self.zones.values():
            if Z.type == "exterieur":
                any_sky = True
                continue
            if Z.plein:
                continue
            if self.lights_auto(Z):
                h = Z.size[2]
                # 32 sous le plafond : à 16, chaque plafonnier brûle une tache blanche au plafond (vu en jeu)
                self.grid_lights(Z, Z.lo, Z.hi, Z.hi[2] - 32, Z.hi[2], round(120 * h / 128))
            self.zone_cubemap(Z)
        has_env = any(e.get("classe") == "light_environment" for e in self.spec.get("entites") or [])
        if any_sky and not has_env:
            Z = next(z for z in self.zones.values() if z.type == "exterieur")
            sun = self.spec.get("soleil", {})
            self.vmf.create_ent("light_environment", origin=f"{(Z.lo[0] + Z.hi[0]) / 2:g} {(Z.lo[1] + Z.hi[1]) / 2:g} {Z.hi[2] - 32:g}",
                                angles=f"0 {sun.get('cap', 35)} 0", pitch=str(-sun.get('hauteur', 50)),
                                _light=sun.get("lumiere", "255 244 226 280"), _ambient=sun.get("ambiante", "140 160 190 70"),
                                _lightHDR="-1 -1 -1 1", _ambientHDR="-1 -1 -1 1", _lightscaleHDR="1",
                                _AmbientScaleHDR="1", SunSpreadAngle=str(sun.get("etalement", 5)))
            self.zone_cubemap(Z)

    def zone_cubemap(self, Z):
        """Cubemap de la zone : au centre, 64 au-dessus du sol ; `cubemap: [x, y, z]` (relatif à l'origine de la
        zone) la déplace, `cubemap: false` la retire (sol occupé par du relief : la poser en entité)."""
        cm = Z.spec.get("cubemap", True)
        if cm is False:
            return
        p = Z.lo + np.array(cm, float) if isinstance(cm, (list, tuple)) else np.r_[(Z.lo[:2] + Z.hi[:2]) / 2,
                                                                                    Z.lo[2] + 64]
        self.vmf.create_ent("env_cubemap", origin=f"{p[0]:g} {p[1]:g} {p[2]:g}", cubemapsize="0")

    def ground_offset(self, model):
        """Hauteur à ajouter à l'origine pour que le bas de la boîte du modèle touche le sol (0 si illisible)."""
        if not hasattr(self, "_fs"):
            from mapkit.config import game_filesystem

            self._fs = game_filesystem([self.base / d for d in self.spec.get("contenu", [])])
        try:
            from srctools.mdl import Model

            return max(0.0, -float(Model(self._fs, self._fs[model.lower()]).hull_min.z))
        except Exception:
            return 0.0

    def build_entities(self):
        spawns = 0
        for i, e in enumerate(self.spec.get("entites") or []):
            cls = e.get("classe")
            label = f"entité {i} ({cls})"
            Z = self.zone_of(e, label)
            O = self.origin_of(Z)
            pos = e.get("position", "centre")
            if pos == "centre" and Z is not None:
                p = np.r_[(Z.lo[:2] + Z.hi[:2]) / 2 - O[:2], 0]
            else:
                p = np.array((list(pos) + [0, 0, 0])[:3], float)
            if "modele" in e and (pos == "centre" or len(pos) < 3):
                p[2] += self.ground_offset(e["modele"])  # posé au sol : le bas du modèle touche le sol
            n = e.get("nombre", 1)
            # rangées de 64 le long de la zone, tant qu'un hull de 32 tient avant le mur est
            cols = n if Z is None else max(1, min(n, int((Z.hi[0] - 16 - (O[0] + p[0])) // 64) + 1))
            for k in range(n):
                q = O + p + [64 * (k % cols), 64 * (k // cols), 0]
                if Z is not None and (np.any(q[:2] - 16 < Z.lo[:2]) or np.any(q[:2] + 16 > Z.hi[:2])
                                      or not Z.lo[2] <= q[2] < Z.hi[2]):
                    self.fail(f"{label} : n° {k + 1} en {q.tolist()} sort de la zone {Z.nom} "
                              "(réduire nombre ou déplacer position)")
                    break
                kv = {k2: str(v) for k2, v in (e.get("cles") or {}).items()}
                kv["origin"] = " ".join(f"{c:g}" for c in q)
                kv.setdefault("angles", f"0 {e.get('angle', 0)} 0")
                if "modele" in e:
                    mdl = e["modele"].lower()
                    if mdl not in self.models:
                        self.fail(f"{label} : modèle {e['modele']} absent (voir data/modeles_disponibles.tsv)")
                    elif cls == "prop_static" and self.static_ok.get(mdl) is False and k == 0:
                        self.fail(f"{label} : vbsp refuse {e['modele']} en prop_static (data/modeles_statiques.tsv) : "
                                  "prendre prop_dynamic ou un autre modèle")
                    kv["model"] = e["modele"]
                    if cls == "prop_static":
                        kv.setdefault("solid", "6")
                self.vmf.create_ent(cls, **kv)
                spawns += cls == "info_player_start"
        if not spawns and not self.spec.get("module"):
            self.fail("aucun info_player_start dans entites:")

    # --- tout ----------------------------------------------------------------------
    def run(self, out):
        self.out = Path(out)
        sky = self.spec.get("ciel", "sky_day01_05")
        skies = load_skies()
        if sky not in skies:
            self.fail(f"ciel '{sky}' absent de data/ciels_disponibles.tsv")
        elif not skies[sky]:
            self.warn.append(f"ciel '{sky}' : pas de cubemap par défaut ; faire buildcubemaps en jeu "
                             "(sinon reflets faux sur les sols)")
        self.vmf.spawn["skyname"] = sky
        self.vmf.spawn["detailmaterial"] = "detail/detailsprites"
        self.vmf.spawn["detailvbsp"] = "detail.vbsp"
        self.vmf.spawn["maxpropscreenwidth"] = "-1"
        for k in ("murs", "dalles"):
            self.on_grid([self.spec.get(k, 16)], GRID_WORLD, k)
        self.resolve_zones()
        self.check_zone_pairs()
        self.resolve_openings()
        if self.err:
            return
        if self.zones:
            self.carve()
            self.write_shell()
        self.build_elements()
        self.finish_rails()
        self.build_openings_dressing()
        self.build_skirting()
        self.build_decor()
        self.build_lights()
        self.build_entities()
        self.check_door_clearance()
        if self.err:
            return
        self.out.parent.mkdir(parents=True, exist_ok=True)
        with open(self.out, "w", encoding="utf8") as fh:
            self.vmf.export(fh)
        levels = sorted({float(z.lo[2]) for z in self.zones.values()} |
                        {float(el["hi"][2]) for el in self.elements if el["type"] == "dalle"})
        plan = {
            "carte": self.spec.get("carte"), "phase": self.phase, "murs": self.T, "dalles": self.S, "niveaux": levels,
            "zones": [{"nom": z.nom, "type": z.type, "englobe": z.englobe, "lo": z.lo.tolist(), "hi": z.hi.tolist(),
                       "hauteur_murs": getattr(z, "hm", None), "role": z.role} for z in self.zones.values()],
            "ouvertures": [{"nom": o["nom"], "type": o["type"], "entre": o["entre"], "lo": o["lo"].tolist(),
                            "hi": o["hi"].tolist()} for o in self.openings],
            "elements": [{"type": el["type"], "nom": el.get("nom"), "lo": np.asarray(el["lo"]).tolist(),
                          "hi": np.asarray(el["hi"]).tolist(), "direction": el.get("direction")}
                         for el in self.elements],
            "contenu": self.spec.get("contenu", []), "kit": self.spec.get("kit", []),
        }
        self.out.with_suffix(".plan.json").write_text(json.dumps(plan, indent=1, ensure_ascii=False), encoding="utf8")


def subtract(spans, a, b):
    out = []
    for s0, s1 in spans:
        if b <= s0 or a >= s1:
            out.append((s0, s1))
            continue
        if a > s0:
            out.append((s0, a))
        if b < s1:
            out.append((b, s1))
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("spec")
    ap.add_argument("--phase", choices=["blockout", "texture"])
    ap.add_argument("-o", "--out")
    a = ap.parse_args()
    spec_path = Path(a.spec).resolve()
    spec = yaml.safe_load(spec_path.read_text(encoding="utf8"))
    name = spec.get("carte") or spec_path.stem
    out = Path(a.out) if a.out else spec_path.with_name(f"{name}.vmf")
    g = Gen(spec, spec_path.parent, a.phase)
    g.run(out)
    for w in g.warn:
        print(f"AVERTISSEMENT  {w}")
    for e in g.err:
        print(f"ERREUR         {e}")
    if g.err:
        print(f"\n{len(g.err)} erreur(s) : rien n'a été écrit.")
        sys.exit(1)
    nb = len(g.vmf.brushes) + sum(len(e.solids) for e in g.vmf.entities)
    print(f"OK  {out}  ({g.phase}) : {len(g.zones)} zones, {len(g.openings)} ouvertures, {nb} brushes, "
          f"{len(g.vmf.entities)} entités ; plan : {out.with_suffix('.plan.json').name}")


if __name__ == "__main__":
    main()
