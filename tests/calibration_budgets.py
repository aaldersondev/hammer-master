"""Calage des budgets sur les couples VMF / BSP archivés de rp_megalopole.

    python tests/calibration_budgets.py [--json sortie.json]

Pour chaque couple : les comptages exacts du VMF (brosses, modèles de brosses, props statiques, cubemaps) sont
comparés aux blocs du BSP, et la taille du bloc de lumière est rapportée au nombre de luxels compté dans le VMF.
Un couple n'est retenu que si le BSP vient bien de ce VMF : même nombre de brosses et de props statiques.
"""
import json
import os
import struct
import subprocess
import sys
import tempfile

ICI = os.path.dirname(os.path.abspath(__file__))
HM = os.path.join(ICI, "..", "bin", "hm.exe")
MEGA = os.path.join(os.environ.get("USERPROFILE", ""), "Desktop", "rp_megalopole")

COUPLES = [
    ("out/rp_megalopole_v1.vmf", "out/rp_megalopole_v1.bsp"),
    ("megastructures_10-11/avant/avant_megastructures.vmf", "maps/rp_megalopole_v1_10-11_avant_megastructures.bsp.bak"),
    ("revue_09-11/corrections/avant/apres_kiosques.vmf", "maps/rp_megalopole_v1_09-11_avant_corrections.bsp.bak"),
    ("v11/retouches/avant/rp_megalopole_v1.vmf", "maps/rp_megalopole_v1_08-11_vocabulaire.bsp.bak"),
    ("v11/phase4/essai_gen/rp_megalopole_v1_build.vmf", "maps/rp_megalopole_v1_08-11_phase4.bsp.bak"),
    ("v11/phase3/corrections/rp_megalopole_v1_build.vmf", "maps/rp_megalopole_v1_08-11_phase3.bsp.bak"),
    ("v11/phase2/avant_phase2.vmf", "maps/rp_megalopole_v1_07-11_phase1.bsp.bak"),
    ("v11/phase1/avant.vmf", "maps/rp_megalopole_v1_07-11_avant_fusion.bsp.bak"),
    ("out/base_volets/avant.vmf", "maps/rp_megalopole_v1_06-11.bsp.bak"),
    ("out/hop_04-11/avant.vmf", "out/hop_04-11/avant.bsp"),
    ("out/base_v10/avant.vmf", "out/base_v10/rp_megalopole_v1_avant_v10.bsp"),
    ("v2/blockout/essai_vis/_compile/rp_megalopole_v2.vmf", "v2/blockout/essai_vis/_compile/rp_megalopole_v2.bsp"),
    ("megastructures_10-11/maquette/h7_maquette.vmf", "megastructures_10-11/maquette/h7_maquette.bsp"),
]


def blocs(chemin):
    with open(chemin, "rb") as f:
        tete = f.read(8 + 64 * 16)
        assert tete[:4] == b"VBSP", chemin
        b = [struct.unpack_from("<ii", tete, 8 + i * 16) for i in range(64)]
        t = {"brushes": b[18][1] // 12, "models": b[14][1] // 48, "cubemapsamples": b[42][1] // 16, "lumiere": b[8][1], "lumiere_hdr": b[53][1]}
        pos, long = b[35]
        f.seek(pos)
        n = struct.unpack("<i", f.read(4))[0]
        for _ in range(n):
            ident, _f, _v, o, _l = struct.unpack("<4sHHii", f.read(16))
            if ident == b"prps":
                ici = f.tell()
                f.seek(o)
                dico = struct.unpack("<i", f.read(4))[0]
                f.seek(o + 4 + dico * 128)
                feuilles = struct.unpack("<i", f.read(4))[0]
                f.seek(o + 8 + dico * 128 + feuilles * 2)
                t["static prop count"] = struct.unpack("<i", f.read(4))[0]
                f.seek(ici)
        return t


def main():
    lignes, facteurs = [], []
    for vmf, bsp in COUPLES:
        vmf, bsp = os.path.join(MEGA, vmf), os.path.join(MEGA, bsp)
        if not (os.path.exists(vmf) and os.path.exists(bsp)):
            continue
        sortie = os.path.join(tempfile.gettempdir(), "hm_calibration.json")
        r = subprocess.run([HM, "valider", vmf, "--familles", "budgets", "--json", sortie], capture_output=True)
        if r.returncode != 0:
            print("échec de hm sur", vmf)
            continue
        b = json.load(open(sortie, encoding="utf8"))["budgets"]
        compte = {l["nom"]: l["valeur"] for l in b["lignes"]}
        reel = blocs(bsp)
        meme = all(compte.get(k) == reel.get(k) for k in ("brushes", "static prop count"))
        exacts = {k: (compte.get(k), reel.get(k)) for k in ("brushes", "models", "static prop count", "cubemapsamples")}
        f = reel["lumiere"] / b["lumiere"]["luxels"] if b["lumiere"]["luxels"] and reel["lumiere"] else None
        lignes.append({"vmf": os.path.relpath(vmf, MEGA), "bsp": os.path.relpath(bsp, MEGA), "couple": meme, "exacts": exacts, "luxels": b["lumiere"]["luxels"], "lumiere": reel["lumiere"], "octets_par_luxel": f})
        etat = "couple sûr" if meme else "PAS LE MÊME ÉTAT (écarté)"
        print(f"{os.path.relpath(vmf, MEGA)}\n   {etat} ; comptes VMF/BSP : " + ", ".join(f"{k} {a}/{c}" for k, (a, c) in exacts.items()))
        if f:
            print(f"   lumière : {reel['lumiere']} octets pour {b['lumiere']['luxels']} luxels = {f:.3f} octet(s) par luxel")
            if meme:
                facteurs.append(f)
    if facteurs:
        m = sum(facteurs) / len(facteurs)
        print(f"\n{len(facteurs)} couple(s) sûr(s) avec lumière : facteur moyen {m:.3f}, de {min(facteurs):.3f} à {max(facteurs):.3f}")
        print("erreur de l'estimation avec le facteur moyen : " + ", ".join(f"{(m / f - 1) * 100:+.1f} %" for f in facteurs))
    if "--json" in sys.argv:
        json.dump(lignes, open(sys.argv[sys.argv.index("--json") + 1], "w", encoding="utf8"), ensure_ascii=False, indent=1)
    faux = [l for l in lignes if l["couple"] and any(a != c for a, c in l["exacts"].values())]
    sys.exit(1 if faux else 0)


if __name__ == "__main__":
    main()
