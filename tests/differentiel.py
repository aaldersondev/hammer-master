"""Mesure du tour différentiel sur rp_megalopole.

    python tests/differentiel.py <dossier de travail>

1. tour de la map (A), sans suivi ; toutes ses vues sont ensuite déclarées lues dans un registre d'essai ;
2. tour de la même map : toutes les vues doivent être reconnues (empreintes stables d'un rendu à l'autre) ;
3. tour d'une copie où un seul modèle posé a été déplacé de 64 unités (B) : seules les vues qui le montrent, ou qui
   montraient ce qu'il cachait, doivent être à relire.
Le registre d'essai est dans le dossier de travail : il ne touche pas au suivi réel de la map.
"""
import json
import os
import re
import shutil
import subprocess
import sys

HM = os.path.join(os.environ.get("CARGO_TARGET_DIR", os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "target")), r"release\hm.exe")
SRC = os.path.expanduser(r"~\Desktop\rp_megalopole\out\rp_megalopole_v1.vmf")
GI = os.path.expanduser(r"~\Desktop\rp_megalopole\compile\gameinfo.txt")
d = sys.argv[1]
os.makedirs(d, exist_ok=True)
a, b = os.path.join(d, "a.vmf"), os.path.join(d, "b.vmf")
shutil.copyfile(SRC, a)
txt = open(SRC, encoding="utf8", errors="surrogateescape").read()
# le premier distributeur ou meuble venu ferait l'affaire : on prend un modèle posé au niveau de la rue
m = None
for m in re.finditer(r'"classname" "prop_static"[^}]*?"model" "models/megalopole/kit/banc[^"]*"[^}]*?"origin" "([-\d.]+) ([-\d.]+) ([-\d.]+)"', txt):
    break
if m is None:
    sys.exit("aucun banc trouvé dans la map")
x, y, z = (float(v) for v in m.groups())
nouveau = '"origin" "%g %g %g"' % (x + 64, y, z)
b_txt = txt[:m.start()] + re.sub(r'"origin" "[^"]*"', nouveau, m.group(0)) + txt[m.end():]
open(b, "w", encoding="utf8", errors="surrogateescape", newline="").write(b_txt)
print("objet déplacé : un banc en (%g %g %g), de 64 unités vers +x" % (x, y, z))


def tour(vmf, dossier, suivi):
    args = [HM, "tour", vmf, "--gameinfo", GI, "--dossier", os.path.join(d, dossier)]
    if not suivi:
        args.append("--sans-suivi")
    out = subprocess.run(args, capture_output=True, text=True, encoding="utf8", errors="replace").stdout
    ligne = [l for l in out.splitlines() if l.startswith("tour :")]
    print(dossier, "|", ligne[-1] if ligne else out[-400:])
    return json.load(open(os.path.join(d, dossier, "tour.json"), encoding="utf8"))


ta = tour(a, "tour_a", False)
suivi = os.path.join(d, ".hammer-master")
os.makedirs(suivi, exist_ok=True)
registre = {"lectures": {v["empreinte"]: {"nom": v["nom"], "vu": "lecture d'essai pour la mesure du différentiel", "defauts": [], "revision": ta["revision"], "planche": v["planche"]} for v in ta["vues"]}}
json.dump(registre, open(os.path.join(suivi, "lectures.json"), "w", encoding="utf8"))
ta2 = tour(a, "tour_a2", True)
tb = tour(b, "tour_b", True)
relire = [v for v in tb["vues"] if not v["deja_lue"]]
print("même map : %d vues, %d reconnues, %d à relire" % (len(ta2["vues"]), sum(v["deja_lue"] for v in ta2["vues"]), sum(not v["deja_lue"] for v in ta2["vues"])))
print("un objet déplacé : %d vues, %d reconnues, %d à relire sur %d planche(s)" % (len(tb["vues"]), sum(v["deja_lue"] for v in tb["vues"]), len(relire), len(tb["planches"])))
for v in relire[:12]:
    print("   à relire :", v["nom"], "|", v["legende"][0][:90])
