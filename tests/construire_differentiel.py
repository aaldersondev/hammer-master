"""Constructeur Rust contre l'ancien générateur Python du skill gmod-map-maker (tests/oracle) : mêmes specs, mêmes sorties ?

    python tests/construire_differentiel.py [spec.yaml ...]

Pour chaque spec, lance l'oracle Python et `hm construire`, chacun dans son dossier de travail (jamais à côté
de la spec), puis compare : les messages (avertissements, erreurs), le VMF octet pour octet, le plan. Sans argument :
la démo du skill en texture et en blockout, les specs d'essai de `tests/specs/`, et les specs réelles de la v2 de
rp_megalopole si elles sont là.
"""
import glob
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time

ICI = os.path.dirname(os.path.abspath(__file__))
HM = os.path.join(ICI, "..", "bin", "hm.exe")
SKILL = os.path.join(os.path.expanduser("~"), ".claude", "skills", "gmod-map-maker")
# l'ancien générateur Python du skill, gardé ici comme oracle depuis que `hm construire` l'a remplacé
GENERER = os.path.join(ICI, "oracle", "generer_skill.py")


def lancer(cmd):
    t = time.time()
    r = subprocess.run(cmd, capture_output=True, text=True, encoding="utf8", errors="replace", env={**os.environ, "PYTHONIOENCODING": "utf-8"})
    return r.returncode, (r.stdout + r.stderr), time.time() - t


def messages(sortie):
    """Les lignes d'avertissement et d'erreur, sans les chemins ni la ligne de bilan."""
    return sorted(l.strip() for l in sortie.splitlines() if l.startswith(("AVERTISSEMENT", "ERREUR")))


def comparer(spec, phase, travail):
    nom = os.path.splitext(os.path.basename(spec))[0] + ("_" + phase if phase else "")
    k = 2
    while os.path.exists(os.path.join(travail, nom)):   # deux specs du même nom dans deux dossiers
        nom = f"{nom.rsplit('#', 1)[0]}#{k}"
        k += 1
    ref, rust = os.path.join(travail, nom, "python"), os.path.join(travail, nom, "rust")
    os.makedirs(ref)
    os.makedirs(rust)
    opt = ["--phase", phase] if phase else []
    cp, sp, tp = lancer([sys.executable, GENERER, spec, "-o", os.path.join(ref, "carte.vmf")] + opt)
    cr, sr, tr = lancer([HM, "construire", spec, "-o", os.path.join(rust, "carte.vmf")] + opt)
    ecarts = []
    if (cp == 0) != (cr == 0):
        ecarts.append(f"codes de sortie : python {cp}, rust {cr}")
    mp, mr = messages(sp), messages(sr)
    if mp != mr:
        seuls_p = [m for m in mp if m not in mr]
        seuls_r = [m for m in mr if m not in mp]
        ecarts.append(f"messages : {len(seuls_p)} propres à python, {len(seuls_r)} propres à rust"
                      + "".join(f"\n      python : {m[:200]}" for m in seuls_p[:3]) + "".join(f"\n      rust   : {m[:200]}" for m in seuls_r[:3]))
    quoi = "refusée par les deux" if cp != 0 and cr != 0 else ""
    geometrie = ""
    if cp == 0 and cr == 0:
        a, b = open(os.path.join(ref, "carte.vmf"), "rb").read(), open(os.path.join(rust, "carte.vmf"), "rb").read()
        if a != b:
            la, lb = a.splitlines(), b.splitlines()
            autres = [i for i, (x, y) in enumerate(zip(la, lb)) if x != y]
            k = autres[0] if autres else min(len(la), len(lb))
            # le même plan dit par trois autres points n'est pas une différence : `hm difference` compare les plans
            _, d, _ = lancer([HM, "difference", os.path.join(ref, "carte.vmf"), os.path.join(rust, "carte.vmf")])
            if "aucune différence" in d and len(la) == len(lb):
                plans = sum(1 for i in autres if b'"plane"' in la[i])
                geometrie = f" ; mêmes solides et mêmes entités, {len(autres)} ligne(s) écrite(s) autrement ({plans} plan(s) dit(s) par trois autres points, le reste : faces d'un solide rangées dans un autre ordre)"
            else:
                ecarts.append(f"VMF différents ({len(a)} et {len(b)} octets), première ligne différente : {k + 1}"
                              f"\n      python : {la[k][:160] if k < len(la) else b'(fin)'}\n      rust   : {lb[k][:160] if k < len(lb) else b'(fin)'}")
        pa = json.load(open(os.path.join(ref, "carte.plan.json"), encoding="utf8"))
        pb = json.load(open(os.path.join(rust, "carte.plan.json"), encoding="utf8"))
        if pa != pb:
            ecarts.append("plans différents : " + ", ".join(k for k in pa if pa[k] != pb.get(k)))
        quoi = f"VMF de {len(a)} octets" + (geometrie if a != b and not ecarts else " identique octet pour octet" if a == b else "")
    print(f"{'OK    ' if not ecarts else 'ÉCART '} {nom:40s} {quoi} ; {len(mp)} message(s) ; python {tp:.1f} s, rust {tr:.1f} s")
    for e in ecarts:
        print("    " + e)
    return not ecarts


def main():
    specs = [(s, None) for s in sys.argv[1:]]
    if not specs:
        demo = os.path.join(SKILL, "exemples", "demo", "demo_skill.yaml")
        specs = [(demo, None), (demo, "blockout")]
        specs += [(s, None) for s in sorted(glob.glob(os.path.join(ICI, "specs", "*.yaml")))]
        megalopole = os.path.expanduser(r"~\Desktop\rp_megalopole\v2")
        for s in ("p0/p0_anime.yaml", "blockout/essai_vis/rp_megalopole_v2.yaml", "blockout/rp_megalopole_v2.yaml", "plan_masse/rp_megalopole_v2.yaml"):
            if os.path.isfile(os.path.join(megalopole, s)):
                specs.append((os.path.join(megalopole, s), None))
    travail = tempfile.mkdtemp(prefix="hm_construire_")
    bons = 0
    try:
        for spec, phase in specs:
            bons += comparer(spec, phase, travail)
    finally:
        shutil.rmtree(travail, ignore_errors=True)
    print(f"{bons} spec(s) identiques sur {len(specs)}")
    sys.exit(0 if bons == len(specs) else 1)


if __name__ == "__main__":
    main()
