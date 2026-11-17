"""Rassemble les lectures d'un tour : la liste des défauts, par gravité et par zone.

    python outils/agreger_lectures.py <dossier du tour> [sortie.md] [--puis <tour de relecture> ...]

Écrit <tour>/defauts.csv (tous les défauts) et un fichier Markdown des défauts de gravité 1 et 2, avec pour chacun
la vue, la planche, la position de l'œil et la confiance du lecteur. Sert de liste de contrôle pour la passe en jeu.

`--puis` ajoute un tour différentiel rendu plus tard (mêmes vues, images changées) : pour chaque vue qu'il a relue,
sa lecture remplace celle du tour précédent ; les autres vues gardent la leur.
"""
import collections
import csv
import glob
import json
import os
import sys

args = sys.argv[1:]
relectures = []
while "--puis" in args:
    i = args.index("--puis")
    relectures.append(args[i + 1])
    del args[i:i + 2]
tour = args[0]
sortie = args[1] if len(args) > 1 else os.path.join(tour, "DEFAUTS.md")


def lire_tour(dossier):
    """Rend (tour.json, {vue: [défauts]}, nombre de planches lues) pour un dossier de tour."""
    T = json.load(open(os.path.join(dossier, "tour.json"), encoding="utf8"))
    par_planche = collections.defaultdict(dict)
    for v in T["vues"]:
        if v["planche"]:
            par_planche[v["planche"]][v["case"]] = v
    par_vue = {}
    planches = 0
    for f in sorted(glob.glob(os.path.join(dossier, "lectures", "*.json"))):
        try:
            d = json.load(open(f, encoding="utf8"))
        except Exception as e:  # une lecture illisible se signale, elle ne se devine pas
            print("lecture illisible :", f, e)
            continue
        planches += 1
        for l in d.get("vues", []):
            v = par_planche.get(d["planche"], {}).get(l.get("case"))
            if v is None:
                continue
            par_vue[v["nom"]] = [{
                "gravite": int(x.get("gravite", 3)),
                "confiance": (x.get("confiance") or "").strip(),
                "zone": v["zone"],
                "groupe": v["groupe"],
                "vue": v["nom"],
                "planche": d["planche"],
                "case": v["case"],
                "oeil": " ".join(str(round(c)) for c in v["oeil"]),
                "sujet": v["legende"][0],
                "vu": l.get("vu", ""),
                "quoi": x.get("quoi", "").strip(),
            } for x in l.get("defauts", [])]
    return T, par_vue, planches


T, par_vue, planches = lire_tour(tour)
relues = 0
for r in relectures:
    _, pv, _ = lire_tour(r)
    relues += len(pv)
    par_vue.update(pv)
lues = len(par_vue)
lignes = [x for rows in par_vue.values() for x in rows]

lignes.sort(key=lambda r: (r["gravite"], r["zone"], r["groupe"], r["vue"]))
with open(os.path.join(relectures[-1] if relectures else tour, "defauts.csv"), "w", encoding="utf-8-sig", newline="") as fh:
    w = csv.DictWriter(fh, fieldnames=list(lignes[0].keys()), delimiter=";")
    w.writeheader()
    w.writerows(lignes)

n = collections.Counter(r["gravite"] for r in lignes)
with open(sortie, "w", encoding="utf8", newline="\n") as fh:
    fh.write("# Défauts relevés sur les planches du tour\n\n")
    fh.write(f"Map : `{T['carte']}` (révision {T['revision']}).\n\n")
    fh.write(f"{planches} planches lues sur {len(T['planches'])}, {lues} vues sur {len(T['vues'])}. "
             f"Défauts : {n[1]} de gravité 1 (saute aux yeux), {n[2]} de gravité 2 (gênant), {n[3]} de gravité 3 (détail, "
             f"dans `defauts.csv`).\n\n")
    if relectures:
        fh.write(f"{relues} de ces vues ont été relues après une correction du rendu : leur seconde lecture remplace la "
                 f"première (planches du tour de relecture).\n\n")
    fh.write("Chaque ligne vient d'une lecture d'image, pas du jeu : « sûr » veut dire que le lecteur l'a vu nettement sur "
             "le rendu, pas que c'est confirmé en jeu. La position est celle de l'œil de la vue (`setpos x y z` en jeu).\n")
    for g, titre in ((1, "Gravité 1 : saute aux yeux"), (2, "Gravité 2 : gênant")):
        fh.write(f"\n## {titre}\n")
        zone = None
        for r in (r for r in lignes if r["gravite"] == g):
            if r["zone"] != zone:
                zone = r["zone"]
                fh.write(f"\n### Zone {zone}\n\n")
            fh.write(f"- **{r['vue']}** ({r['planche']}, case {r['case']}) · œil `{r['oeil']}` · {r['confiance'] or 'confiance non dite'}  \n"
                     f"  {r['quoi']}\n")
print(f"{planches} planches, {lues} vues ({relues} relues), défauts par gravité : {dict(n)} ; {sortie}")
