"""La révision, de la map au signalement et retour : une map d'essai est estampillée, compilée par vbsp, « jouée »
par l'addon (hors jeu), puis modifiée. L'outil doit dire que le signalement date d'avant la modification.

    python tests/revision_bout_en_bout.py

Ce que ce test prouve : l'estampille ne fait pas de fuite à la compilation, elle se retrouve dans le .bsp, le code
Lua de l'addon la lit dans ce .bsp, et `hm signalements` distingue « fait sur la révision ouverte » de « fait sur
une ancienne map ». Ce qu'il ne prouve pas : la lecture par un vrai client du jeu.
"""
import importlib.util
import os
import shutil
import subprocess
import sys
import tempfile

ICI = os.path.dirname(os.path.abspath(__file__))
HM = os.path.join(os.environ.get("CARGO_TARGET_DIR", os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "target")), r"release\hm.exe")
GMOD = r"C:\Program Files (x86)\Steam\steamapps\common\GarrysMod"
VBSP = os.path.join(GMOD, "bin", "win64", "vbsp.exe")


def charger(nom):
    spec = importlib.util.spec_from_file_location(nom, os.path.join(ICI, nom + ".py"))
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


def hm(*args):
    r = subprocess.run([HM, *args], capture_output=True, text=True, encoding="utf8", errors="replace")
    return r.stdout + r.stderr


def main():
    if not os.path.exists(VBSP):
        sys.exit("vbsp.exe introuvable : essai impossible sur cette machine")
    d = tempfile.mkdtemp(prefix="hm_rev_")
    vmf = os.path.join(d, "hm_essai.vmf")
    open(vmf, "w", encoding="utf8", newline="\r\n").write(charger("carte_essai").carte())
    echecs = []

    def verifier(cond, quoi):
        print(("  ok   " if cond else "  ÉCHEC ") + quoi)
        if not cond:
            echecs.append(quoi)

    rev = hm("revision", vmf).split()[1]
    verifier("estampillée : révision " + rev in hm("estampiller", vmf), f"map estampillée, révision {rev}")
    verifier(rev in hm("revision", vmf) and "estampille à jour" in hm("revision", vmf), "la révision n'a pas changé en estampillant")

    log = subprocess.run([VBSP, "-game", os.path.join(GMOD, "garrysmod"), "hm_essai"], cwd=d, capture_output=True, text=True, errors="replace").stdout
    bsp = os.path.join(d, "hm_essai.bsp")
    verifier(os.path.exists(bsp) and "leaked" not in log.lower(), "vbsp compile la map estampillée sans fuite")
    verifier(os.path.exists(os.path.join(d, "hm_essai.prt")), "fichier .prt écrit : la map est bien fermée")

    addon = charger("addon_hors_jeu")
    lua, jeu, racine = addon.session("hm_essai", bsp)
    g = lua.globals()
    g.commandes["hm_defaut"](None, None, None, "essai de révision")
    addon.image(lua)
    dossier = os.path.join(racine, "data", "hammer_master", "hm_essai")
    ligne = open(os.path.join(dossier, "signalements.txt"), encoding="utf8").read()
    verifier(f'"revision": "{rev}"' in ligne, "l'addon lit la révision dans le .bsp compilé")

    s = hm("signalements", vmf, "--dossier", dossier)
    verifier("fait sur la révision ouverte" in s, "map inchangée : « fait sur la révision ouverte »")

    # la map change après coup (la lampe monte de 8 unités) : le signalement date maintenant d'une ancienne version
    texte = open(vmf, encoding="utf8", newline="").read()
    assert '"origin" "0 0 200"' in texte
    open(vmf, "w", encoding="utf8", newline="").write(texte.replace('"origin" "0 0 200"', '"origin" "0 0 208"'))
    s = hm("signalements", vmf, "--dossier", dossier)
    verifier("ATTENTION : fait sur la révision " + rev in s, "map modifiée : le signalement est dit fait sur une ancienne révision")
    verifier("porte l'estampille " + rev in hm("revision", vmf), "et la map elle-même dit que son estampille est périmée")
    print(s.strip().splitlines()[-1].strip())

    shutil.rmtree(d, ignore_errors=True)
    if echecs:
        sys.exit(f"{len(echecs)} contrôle(s) en échec")
    print("révision de bout en bout : tous les contrôles passent")


if __name__ == "__main__":
    main()
