"""Essai de bout en bout du lien entre le serveur MCP et la fenêtre de l'éditeur (jalon D), avec la vraie fenêtre.

    python tests/mcp_fenetre.py [dossier d'essai] [carte.vmf]

(HM_PAUSE : temps laissé au MCP avant le bloc C, en ms ; à monter pour une grosse map.)

Une copie d'une petite map est ouverte dans la fenêtre (mode script, LOCALAPPDATA à part : aucune fenêtre de
l'utilisateur n'est touchée). La fenêtre y fait une modification « de l'utilisateur » (bloc A, non enregistré).
Le serveur MCP ouvre la même map : il doit se lier à la fenêtre et voir A. Il crée le bloc B dans la fenêtre,
lit l'historique (A puis B), la sélection (B), annule et rétablit, montre B. La fenêtre fait ensuite le bloc C :
le MCP doit le voir sans rien rouvrir. Il enregistre par la fenêtre : le fichier a A, B et C. Deux captures de la
fenêtre (après B, après C) prouvent que tout s'est passé sous les yeux de l'utilisateur. Entre-temps : `verifier`
(Alt+P), `compiler` (la fenêtre compile, en géométrie seule), `amenager_boite` (le modèle « salle_de_bain » de
rp_megalopole, copié dans le dossier d'essai) ; la fenêtre fermée, `compiler` compile seul.
"""
import json
import os
import shutil
import subprocess
import sys
import time

ICI = os.path.dirname(os.path.abspath(__file__))
CIBLE = os.environ.get("HM_CIBLE", os.path.join(os.environ.get("CARGO_TARGET_DIR", os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "target")), r"debug"))
EDITEUR = os.path.join(CIBLE, "hammer-master.exe")
MCP = os.path.join(CIBLE, "hammer-master-mcp.exe")
ESSAI = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.environ.get("TEMP", "."), "hm_mcp_fenetre")
CARTE = sys.argv[2] if len(sys.argv) > 2 else os.path.join(ICI, "..", "demo", "editeur", "jalon_b", "b2_visgroups_cordon.vmf")
PAUSE = int(os.environ.get("HM_PAUSE", "25000"))

if os.path.isdir(ESSAI):
    shutil.rmtree(ESSAI)
os.makedirs(os.path.join(ESSAI, "lad"))
COPIE = os.path.join(ESSAI, "fenetre.vmf")
shutil.copyfile(CARTE, COPIE)
env = dict(os.environ, LOCALAPPDATA=os.path.join(ESSAI, "lad"))
# des matériaux propres à l'essai (absents de toute map : dessinés en damier, bien visibles)
A, B, C = "HM_ESSAI/A_UTILISATEUR", "HM_ESSAI/B_CLAUDE", "HM_ESSAI/C_UTILISATEUR"


def bloc(lo, hi, mat):
    return {"commande": "creer_bloc", "min": lo, "max": hi, "materiau": mat}


m = COPIE.replace("\\", "/")
with open(os.path.join(ESSAI, "script.txt"), "w", encoding="utf8") as f:
    f.write(f"""attendre 10
ouvrir {m}
attendre 30
commande {json.dumps(bloc([-512, -512, 0], [-384, -384, 128], A))}
pause {PAUSE}
attendre 5
capture {ESSAI}/fenetre_1.png
commande {json.dumps(bloc([384, 384, 0], [512, 512, 128], C))}
pause 20000
attendre 5
capture {ESSAI}/fenetre_2.png
quitter
""".replace("\\", "/"))
t0 = time.time()
fenetre = subprocess.Popen([EDITEUR, "--script", os.path.join(ESSAI, "script.txt")], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
p = subprocess.Popen([MCP], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env)
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


def outil(nom, args, erreur_permise=False):
    r = envoyer("tools/call", {"name": nom, "arguments": args})
    texte = "\n".join(c["text"] for c in r["content"] if c["type"] == "text")
    if r.get("isError") and not erreur_permise:
        raise SystemExit(f"ÉCHEC {nom} : {texte}")
    print(f"--- {nom} ({time.time() - t0:.1f} s)\n{texte[:700]}")
    return texte


def verifier(cond, quoi):
    print(("ok    " if cond else "ÉCHEC ") + quoi)
    if not cond:
        fenetre.kill()
        raise SystemExit(1)


envoyer("initialize", {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "essai", "version": "0"}})
envoyer("notifications/initialized", notification=True)

# la fenêtre démarre, ouvre la map et fait A : on rouvre jusqu'à être lié à une map modifiée (60 s au plus)
for _ in range(120):
    t = outil("carte_ouvrir", {"vmf": COPIE})
    if "liée à la fenêtre" in t and "modifiée" in t:
        break
    time.sleep(0.5)
verifier("liée à la fenêtre" in t, "carte_ouvrir se lie à la fenêtre qui a la map")
verifier("modifiée" in t, "la map de la fenêtre a une modification non enregistrée (A)")
verifier(A.lower() in outil("lister", {"quoi": "materiaux", "filtre": A.lower()}).lower(), "le MCP voit le bloc A, fait dans la fenêtre et pas enregistré")

t = outil("commande", {"commande": bloc([-128, -128, 0], [128, 128, 64], B)})
verifier("fait dans la fenêtre" in t, "la commande B est faite dans la fenêtre")
cree = json.loads(t.split("créé (sélectionné dans la fenêtre) : ")[1].splitlines()[0])
# la copie du MCP (où la commande est refaite) a le même objet, sous le numéro donné par la fenêtre
verifier(B.lower() in outil("inspecter", {"vmf_id": cree[0]["id"]}).lower(), "la copie du MCP a B sous le numéro de la fenêtre")
t = outil("historique", {})
verifier("2 étape(s) faite(s)" in t, "l'historique est celui de la fenêtre : A puis B")
verifier(json.dumps(cree, separators=(",", ":")) in outil("carte_etat", {}), "la sélection de la fenêtre est B")
outil("annuler", {})
verifier("1 étape(s) faite(s)" in outil("historique", {}), "annuler retire B de la fenêtre")
verifier(B.lower() not in outil("lister", {"quoi": "materiaux", "filtre": B.lower()}).lower(), "la copie du MCP suit l'annulation")
outil("annuler", {"retablir": True})
verifier("2 étape(s) faite(s)" in outil("historique", {}), "rétablir remet B")
outil("fenetre_montrer", {"objets": cree})

t = outil("verifier", {"limite": 5})
verifier("erreur(s)" in t and "avertissement(s)" in t, "verifier : les contrôles d'Alt+P sur la map de la fenêtre")
t = outil("compiler", {"mode": "geometrie", "copier": False})
verifier("compilée dans la fenêtre" in t and "réussie" in t, "compiler : la fenêtre compile, le MCP rend son résultat")
os.makedirs(os.path.join(ESSAI, "amenagements"))
shutil.copyfile(os.path.expanduser(r"~\Desktop\rp_megalopole\amenagements\salle_de_bain.toml"), os.path.join(ESSAI, "amenagements", "salle_de_bain.toml"))
t = outil("amenager_boite", {"modele": "salle_de_bain", "zone": [-480, -480, 0, -280, -230, 160], "porte": "y-"})
verifier("fait dans la fenêtre" in t, "amenager_boite : le modèle est posé dans la fenêtre, en une étape")

# la fenêtre fait C (vers 30 s) : le MCP doit le voir sans rien rouvrir
vu = False
for _ in range(90):
    t = outil("lister", {"quoi": "materiaux", "filtre": C.lower()})
    if C.lower() in t.lower():
        vu = True
        break
    time.sleep(1)
verifier(vu, "le MCP voit le bloc C, fait ensuite dans la fenêtre")
verifier("ouverte dans la fenêtre" in t and "NOTE" not in t, "le lien tient après la recopie")
t = outil("enregistrer", {})
t = outil("carte_etat", {})
verifier("ouverte dans la fenêtre" in t and "modifiée" not in t, "la fenêtre a enregistré : sa map n'est plus modifiée")
src = open(COPIE, encoding="utf8", errors="replace").read().upper()
verifier(all(src.count(f'"MATERIAL" "{x}"') == 6 for x in (A, B, C)), "enregistré par la fenêtre : le fichier a A, B et C (6 faces chacun)")
fenetre.wait(timeout=120)
# la fenêtre fermée : le MCP compile lui-même
t = outil("carte_ouvrir", {"vmf": COPIE})
verifier("liée à la fenêtre" not in t, "fenêtre fermée : plus de lien")
t = outil("compiler", {"mode": "geometrie", "copier": False})
verifier("compilée ici" in t and "réussie" in t, "compiler sans fenêtre : le MCP compile lui-même")
p.kill()
verifier(all(os.path.isfile(os.path.join(ESSAI, f"fenetre_{i}.png")) for i in (1, 2)), "captures de la fenêtre écrites")
print(f"\ntout passe ; captures dans {ESSAI}")
