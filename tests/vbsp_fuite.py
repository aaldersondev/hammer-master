"""La fuite est trouvée par vbsp lancé à part (`hm vbsp`), règle "geo.fuite".

    python tests/vbsp_fuite.py

Deux maps d'essai : la salle fermée de tests/carte_essai.py (pas de fuite, code 0), et la même sans son mur est
(fuite, code 2, avec le chemin du fichier de points de vbsp). Demande le vbsp de Garry's Mod.
"""
import os
import subprocess
import sys
import tempfile

ICI = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, ICI)
import carte_essai  # noqa: E402

HM = os.path.join(ICI, "..", "bin", "hm.exe")
VBSP = r"C:\Program Files (x86)\Steam\steamapps\common\GarrysMod\bin\win64\vbsp.exe"
GAMEINFO = r"C:\Program Files (x86)\Steam\steamapps\common\GarrysMod\garrysmod\gameinfo.txt"


def lancer(vmf):
    r = subprocess.run([HM, "vbsp", vmf, "--gameinfo", GAMEINFO], capture_output=True)
    return r.returncode, r.stdout.decode("utf8", "replace")


def main():
    if not os.path.exists(VBSP):
        print("vbsp de Garry's Mod absent : essai sauté")
        return 0
    echecs = []

    def verifier(cond, quoi):
        print(("  ok   " if cond else "  ÉCHEC ") + quoi)
        if not cond:
            echecs.append(quoi)

    d = tempfile.mkdtemp(prefix="hm_fuite_")
    ferme = os.path.join(d, "ferme.vmf")
    open(ferme, "w", encoding="utf8", newline="\r\n").write(carte_essai.carte())
    code, sortie = lancer(ferme)
    verifier(code == 0 and "pas de fuite" in sortie, "salle fermée : vbsp ne trouve pas de fuite, code 0")
    verifier("brosses" in sortie and "6 " in sortie, "les budgets du vbsp à part sont rendus")

    # la même salle, sans son quatrième solide : le mur est (x de 256 à 272)
    texte = carte_essai.carte()
    blocs = [k for k in range(len(texte)) if texte.startswith("\tsolid\n", k)]
    debut, fin = blocs[3], blocs[4]
    troue = os.path.join(d, "troue.vmf")
    open(troue, "w", encoding="utf8", newline="\r\n").write(texte[:debut] + texte[fin:])
    code, sortie = lancer(troue)
    verifier(code == 2 and "FUITE" in sortie, "salle sans mur est : vbsp trouve la fuite, code 2")
    verifier("geo.fuite" in sortie and "sort vers" in sortie, "le constat geo.fuite dit où la fuite sort")
    print("fuite par vbsp : tous les contrôles passent" if not echecs else f"fuite par vbsp : {len(echecs)} échec(s)")
    return 1 if echecs else 0


if __name__ == "__main__":
    sys.exit(main())
