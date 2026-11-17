"""Essai de bout en bout des outils d'édition du serveur MCP, par le protocole, sur une copie d'une map.

    python tests/mcp_edition.py [carte.vmf] [dossier d'essai]

Les outils passent par le noyau de l'éditeur (`commande`, `annuler`, `historique`, `enregistrer`, `difference`). Le
script crée un bloc et une entité, déplace et tourne, annule tout : la copie enregistrée doit retomber octet pour
octet sur la map de départ ; puis il rétablit et enregistre : la map relue a bien les objets créés.
"""
import hashlib
import json
import os
import shutil
import subprocess
import sys

ICI = os.path.dirname(os.path.abspath(__file__))
EXE = os.path.join(os.path.join(os.environ.get("CARGO_TARGET_DIR", os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "target")), r"release"), "hammer-master-mcp.exe")
CARTE = sys.argv[1] if len(sys.argv) > 1 else os.path.join(ICI, "..", "crates", "hm-doc", "tests", "fixtures", "adminroom.vmf")
ESSAI = sys.argv[2] if len(sys.argv) > 2 else os.path.join(os.environ.get("TEMP", "."), "hm_mcp_edition")

if os.path.isdir(ESSAI):
    shutil.rmtree(ESSAI)
os.makedirs(ESSAI)
COPIE = os.path.join(ESSAI, os.path.basename(CARTE))
shutil.copyfile(CARTE, COPIE)


def empreinte(chemin):
    return hashlib.sha256(open(chemin, "rb").read()).hexdigest()


DEPART = empreinte(COPIE)
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
            return rep


def outil(nom, args, erreur_attendue=False):
    rep = envoyer("tools/call", {"name": nom, "arguments": args})
    res = rep.get("result", {})
    t = "\n".join(c.get("text", "") for c in res.get("content", []) if c.get("type") == "text")
    if res.get("isError") != erreur_attendue:
        raise SystemExit(f"{nom} : {'erreur inattendue' if not erreur_attendue else 'aurait dû être refusé'} :\n{t}")
    print(f"--- {nom}\n{t}\n")
    return t


envoyer("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "essai", "version": "0"}})
envoyer("notifications/initialized", notification=True)
noms = [t["name"] for t in envoyer("tools/list")["result"]["tools"]]
for attendu in ["commande", "annuler", "historique", "enregistrer", "difference"]:
    assert attendu in noms, f"outil {attendu} absent"
for retire in ["retouches", "creer", "deplacer", "supprimer", "point_de_sauvegarde"]:
    assert retire not in noms, f"outil {retire} encore là"

outil("carte_ouvrir", {"vmf": COPIE})
t = outil("commande", {"commande": {"commande": "creer_bloc", "min": [0, 0, 0], "max": [128, 128, 16], "materiau": "DEV/DEV_MEASUREGENERIC01"}})
bloc = json.loads(t.split("créé : ")[1].split("\n")[0])[0]
t = outil("commande", {"commande": {"commande": "creer_entite", "classe": "info_player_start", "origine": [64, 64, 17]}})
ent = json.loads(t.split("créé : ")[1].split("\n")[0])[0]
outil("commande", {"commande": {"commande": "deplacer", "objets": [bloc, ent], "delta": [32, 0, 0]}})
outil("commande", {"commande": {"commande": "tourner", "objets": [bloc], "centre": [96, 64, 8], "axe": [0, 0, 1], "degres": 90}})
# refusé : un bloc plat ; rien ne change
outil("commande", {"commande": {"commande": "creer_bloc", "min": [0, 0, 0], "max": [64, 64, 0], "materiau": "x"}}, erreur_attendue=True)
h = outil("historique", {})
assert "4 étape(s) faite(s)" in h, h
d = outil("difference", {})
assert "ajout" in d
assert empreinte(COPIE) == DEPART, "rien ne doit être écrit avant `enregistrer`"
outil("annuler", {"n": 4})
outil("enregistrer", {})
assert empreinte(COPIE) == DEPART, "tout annulé puis enregistré : la map doit être celle de départ, à l'octet"
outil("annuler", {"retablir": True, "n": 4})
outil("enregistrer", {})
texte = open(COPIE, "rb").read().decode("utf8", "replace")
assert '"classname" "info_player_start"' in texte and "DEV/DEV_MEASUREGENERIC01" in texte
outil("vue", {"pos": [-200, -200, 200], "vers": [96, 64, 8]})
p.stdin.close()
p.wait(timeout=30)
print("ESSAI RÉUSSI : commandes, refus, historique, annulation à l'octet, rétablissement, enregistrement")
