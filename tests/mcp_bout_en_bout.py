"""Essai de bout en bout du serveur MCP, par le protocole (stdio, une ligne JSON par message).

    python tests/mcp_bout_en_bout.py [carte.vmf] [dossier de sortie]

Ouvre la map, liste les outils, rend une vue et une vue de dessus, interroge un pixel, lance un rayon, liste les
pièces. Échoue (code 1) si une réponse manque ou si une image n'est pas rendue.
"""
import base64
import json
import os
import subprocess
import sys
import time

EXE = os.path.join(os.environ.get("CARGO_TARGET_DIR", os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "target")), r"release\hammer-master-mcp.exe")
CARTE = sys.argv[1] if len(sys.argv) > 1 else os.path.expanduser(r"~\Desktop\rp_megalopole\out\rp_megalopole_v1.vmf")
SORTIE = sys.argv[2] if len(sys.argv) > 2 else os.path.join(os.path.dirname(__file__), "..", "essais", "mcp")
os.makedirs(SORTIE, exist_ok=True)

p = subprocess.Popen([EXE], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
n = 0


def envoyer(methode, params=None, notification=False):
    global n
    msg = {"jsonrpc": "2.0", "method": methode}
    if params is not None:
        msg["params"] = params
    if not notification:
        n += 1
        msg["id"] = n
    p.stdin.write((json.dumps(msg) + "\n").encode("utf8"))
    p.stdin.flush()
    if notification:
        return None
    while True:
        ligne = p.stdout.readline()
        if not ligne:
            raise SystemExit("le serveur s'est arrêté : " + p.stderr.read().decode("utf8", "replace")[-800:])
        rep = json.loads(ligne)
        if rep.get("id") == n:
            if "error" in rep:
                raise SystemExit(f"{methode} : {rep['error']}")
            return rep["result"]


def outil(nom, args, image=None):
    t = time.time()
    r = envoyer("tools/call", {"name": nom, "arguments": args})
    dt = time.time() - t
    textes = [c["text"] for c in r["content"] if c["type"] == "text"]
    images = [c for c in r["content"] if c["type"] == "image"]
    if r.get("isError"):
        raise SystemExit(f"{nom} : {textes}")
    print(f"--- {nom} ({dt:.2f} s, {len(images)} image(s))")
    print("\n".join(textes)[:900])
    if image:
        if not images:
            raise SystemExit(f"{nom} : aucune image rendue")
        data = base64.b64decode(images[0]["data"])
        open(os.path.join(SORTIE, image), "wb").write(data)
        print(f"    image : {image}, {len(data) // 1024} Ko, {images[0]['mimeType']}")
    return textes


info = envoyer("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "essai", "version": "0"}})
print("serveur :", info.get("serverInfo"), "| protocole", info.get("protocolVersion"))
envoyer("notifications/initialized", notification=True)
outils = envoyer("tools/list")["tools"]
print(len(outils), "outils :", ", ".join(o["name"] for o in outils))
assert len(outils) >= 26
outil("carte_ouvrir", {"vmf": CARTE})
outil("vue", {"pos": [-10783.8, -807.8, 64], "angles": [-9.6, 3.2]}, "vue.jpg")
outil("sous_le_pixel", {"x": 640, "y": 200})
outil("vue", {"pos": [-2875.7, 13644.7, 12160], "angles": [4.4, 130], "joueur": True}, "vue_joueur.jpg")
outil("vue_dessus", {"centre": [-2400, 13200], "demi": 1100, "zmin": 12150, "zmax": 12380}, "dessus_etage.jpg")
outil("rayon", {"origine": [-10783.8, -807.8, 64], "vers": [-10112, -770, 178]})
outil("lister", {"quoi": "pieces", "limite": 5})
outil("lister", {"quoi": "modeles", "filtre": "voie", "limite": 5})
t = outil("signalements_lire", {})
assert any("signalement(s)" in x for x in t), t
t = outil("signalement_comparer", {"id": "L06"}, "signalement_L06.jpg")
assert any("sign_cybercafe" in x for x in t), t
# règles de vérification : le rapport, puis la vue d'un constat
t = outil("valider", {"familles": ["physique", "budgets", "assets"], "max": 3})
assert any("phys.flotte" in x and "content_cstrike" in x and "props statiques" in x for x in t), t
t = outil("constat_vue", {"regle": "phys.flotte", "rang": 1}, "constat_flotte.jpg")
assert any("phys.flotte n° 1" in x for x in t), t
outil("carte_etat", {})
# construire : la démo du skill, dans un dossier de travail ; elle s'ouvre dans la session et passe les règles
import shutil
import tempfile
demo = os.path.join(os.path.expanduser("~"), ".claude", "skills", "gmod-map-maker", "exemples", "demo", "demo_skill.yaml")
if os.path.isfile(demo):
    travail = tempfile.mkdtemp(prefix="hm_mcp_construire_")
    t = outil("construire", {"spec": demo, "sortie": os.path.join(travail, "demo_skill.vmf")})
    assert any("110 brushes, 53 entités" in x and "map ouverte" in x for x in t), t
    reference = os.path.join(os.path.dirname(demo), "demo_skill.vmf")
    assert open(os.path.join(travail, "demo_skill.vmf"), "rb").read() == open(reference, "rb").read(), "le VMF construit diffère de celui du skill"
    t = outil("valider", {"familles": ["semantique", "physique"]})
    assert any("0 erreur(s)" in x for x in t), t
    outil("vue", {"pos": [960, -96, 300], "vers": [1300, 400, 120]}, "demo_construite.jpg")
    shutil.rmtree(travail, ignore_errors=True)
p.stdin.close()
p.wait(timeout=20)
print("OK")
