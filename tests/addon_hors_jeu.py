"""Fait tourner l'addon de signalement hors du jeu, avec un faux Garry's Mod, et contrôle ce qu'il écrit.

    python tests/addon_hors_jeu.py [--ecrire-echantillon]

Ce que ce test prouve : le fichier Lua se charge, la suite commande → vue → capture écrit bien une ligne JSON
conforme au schéma et une image, la révision se lit dans le bloc d'entités d'un .bsp, et la lecture du vrai .bsp
de rp_megalopole (s'il est là) tombe sur son bloc d'entités. Ce qu'il ne prouve pas : le comportement des vraies
fonctions du jeu (render.GetViewSetup, render.Capture, GM:RenderScene). Cela se voit en jeu seulement.
"""
import json
import os
import struct
import sys
import tempfile

import lupa

ICI = os.path.dirname(os.path.abspath(__file__))
ADDON = os.path.join(ICI, "..", "addon", "hammer_master_signalement")
LUA = os.path.join(ADDON, "lua", "hammer_master", "cl_signalement.lua")
SCHEMA = json.load(open(os.path.join(ADDON, "schema_signalement.json"), encoding="utf8"))
ECHANTILLON = os.path.join(ICI, "..", "crates", "hm-signal", "tests", "fixtures", "echantillon_format2.txt")
VRAI_BSP = os.path.expandvars(r"%USERPROFILE%\Desktop\rp_megalopole\addon\rp_megalopole\maps\rp_megalopole_v1.bsp")

PRELUDE = r"""
local py = ...
local V = {}
V.__index = V
function V:Distance(o) return math.sqrt((self.x-o.x)^2 + (self.y-o.y)^2 + (self.z-o.z)^2) end
function Vector(x, y, z) return setmetatable({x = x, y = y, z = z}, V) end
function Angle(p, y, r) return {p = p, y = y, r = r} end
function Color(r, g, b) return {r = r, g = g, b = b} end
function IsValid(e) return e ~= nil and e.valide == true end
MOVETYPE_NOCLIP = 8
math.Round = function(x, d) local m = 10 ^ (d or 0) return math.floor(x * m + 0.5) / m end
string.Trim = function(s) return (s:gsub("^%s+", ""):gsub("%s+$", "")) end
game = { GetMap = function() return py.carte end }
util = { TableToJSON = function(t) return py.json(t) end }
local globaux = {}
function GetGlobalString(k, d) return globaux[k] or d end
function SetGlobalString(k, v) globaux[k] = v end
file = {
	CreateDir = function(d) py.creer(d) end,
	Read = function(p, ou) return py.lire(p) end,
	Write = function(p, d) return py.ecrire(p, d) end,
	Append = function(p, d) py.ajouter(p, d) end,
	Size = function(p, ou) return py.taille(p) end,
	Time = function(p, ou) return py.date(p) end,
	Open = function(p, mode, ou)
		local h = py.ouvrir(p)
		if not h then return nil end
		return {
			Seek = function(_, n) h.seek(n) end,
			ReadLong = function(_) return py.long(h) end,
			Read = function(_, n) return py.octets(h, n) end,
			Close = function(_) h.close() end,
		}
	end,
}
hooks, commandes, messages = {}, {}, {}
hook = { Add = function(nom, id, f) hooks[nom] = f end }
concommand = { Add = function(nom, f) commandes[nom] = f end }
chat = { AddText = function(...) local t = {} for _, x in ipairs({...}) do if type(x) == "string" then t[#t+1] = x end end messages[#messages+1] = table.concat(t) end }
function ScrW() return 1920 end
function ScrH() return 1080 end
render = {
	GetViewSetup = function() return { origin = Vector(100.2, -50.4, 64.03), angles = Angle(12.34, -90.5, 0), fov = 106.26, fov_unscaled = 90, aspect = 1920/1080, width = 1920, height = 1080 } end,
	Capture = function(t) if py.menu_ouvert then return nil end return "JPEG(" .. t.format .. "," .. t.w .. "x" .. t.h .. ")" end,
}
local joueur = { valide = true }
function joueur:GetEyeTrace()
	return { HitPos = Vector(100.2, -306.4, 8.03), StartPos = Vector(100.2, -50.4, 64.03), HitNormal = Vector(0, 1, 0), HitTexture = "**studio**", HitWorld = false,
		Entity = { valide = true, GetClass = function() return "prop_physics" end, GetModel = function() return "models/props_c17/chair02a.mdl" end, GetPos = function() return Vector(100, -310, 0) end, GetAngles = function() return Angle(0, 45, 0) end } }
end
function joueur:GetPos() return Vector(100.2, -50.4, 0.03) end
function joueur:EyePos() return Vector(100.2, -50.4, 64.03) end
function joueur:EyeAngles() return Angle(12.34, -90.5, 0) end
function joueur:GetFOV() return 90 end
function joueur:GetMoveType() return 2 end
function joueur:Crouching() return false end
function joueur:InVehicle() return false end
function joueur:IsOnGround() return true end
function joueur:ShouldDrawLocalPlayer() return false end
function LocalPlayer() return joueur end
"""


def bsp_synthetique(entites: bytes) -> bytes:
    """Un .bsp réduit à son en-tête et à son bloc d'entités : identifiant, version, 64 descripteurs, révision."""
    entete = 8 + 64 * 16 + 4
    lumps = struct.pack("<iiii", entete, len(entites), 0, 0) + b"\0" * (63 * 16)
    return b"VBSP" + struct.pack("<i", 20) + lumps + struct.pack("<i", 1) + entites


def valider(doc, schema, racine=None, chemin="$"):
    """Validation réduite au sous-ensemble de JSON Schema employé par schema_signalement.json."""
    import re
    racine = racine or schema
    if "$ref" in schema:
        return valider(doc, racine["$defs"][schema["$ref"].split("/")[-1]], racine, chemin)
    err = []
    if "const" in schema and doc != schema["const"]:
        err.append(f"{chemin} : attendu {schema['const']!r}")
    if "enum" in schema and doc not in schema["enum"]:
        err.append(f"{chemin} : hors de {schema['enum']}")
    t = schema.get("type")
    types = {"object": dict, "array": list, "string": str, "boolean": bool, "number": (int, float), "integer": int}
    if t and (not isinstance(doc, types[t]) or (t in ("number", "integer") and isinstance(doc, bool))):
        return err + [f"{chemin} : type {t} attendu, reçu {type(doc).__name__}"]
    if t == "string":
        if len(doc) < schema.get("minLength", 0):
            err.append(f"{chemin} : trop court")
        if "pattern" in schema and not re.search(schema["pattern"], doc):
            err.append(f"{chemin} : ne suit pas {schema['pattern']}")
    if t in ("number", "integer"):
        if "minimum" in schema and doc < schema["minimum"]:
            err.append(f"{chemin} : sous {schema['minimum']}")
        if "exclusiveMinimum" in schema and doc <= schema["exclusiveMinimum"]:
            err.append(f"{chemin} : doit dépasser {schema['exclusiveMinimum']}")
    if t == "array":
        if not schema.get("minItems", 0) <= len(doc) <= schema.get("maxItems", 10 ** 9):
            err.append(f"{chemin} : {len(doc)} éléments")
        for i, x in enumerate(doc):
            err += valider(x, schema.get("items", {}), racine, f"{chemin}[{i}]")
    if t == "object":
        for k in schema.get("required", []):
            if k not in doc:
                err.append(f"{chemin} : champ {k} manquant")
        for k, v in doc.items():
            if k in schema.get("properties", {}):
                err += valider(v, schema["properties"][k], racine, f"{chemin}.{k}")
            elif isinstance(schema.get("additionalProperties"), dict):
                err += valider(v, schema["additionalProperties"], racine, f"{chemin}.{k}")
    return err


class FauxJeu:
    def __init__(self, racine, carte, bsp):
        self.racine, self.carte, self.bsp, self.menu_ouvert = racine, carte, bsp, False

    def _p(self, p):
        return os.path.join(self.racine, "data", p.lower())

    def _bsp(self, p):
        return self.bsp if p == f"maps/{self.carte}.bsp" else None

    def creer(self, d):
        os.makedirs(self._p(d), exist_ok=True)

    def lire(self, p):
        return open(self._p(p), encoding="utf8").read() if os.path.exists(self._p(p)) else None

    def ecrire(self, p, d):
        open(self._p(p), "wb").write(d if isinstance(d, bytes) else d.encode("latin-1"))
        return True

    def ajouter(self, p, d):
        open(self._p(p), "ab").write(d.encode("utf8"))

    def taille(self, p):
        return os.path.getsize(self._bsp(p)) if self._bsp(p) else -1

    def date(self, p):
        return int(os.path.getmtime(self._bsp(p))) if self._bsp(p) else 0

    def ouvrir(self, p):
        return open(self._bsp(p), "rb") if self._bsp(p) else None

    @staticmethod
    def long(h):
        return struct.unpack("<i", h.read(4))[0]

    def octets(self, h, n):
        # le jeu rend une chaîne d'octets ; ici une chaîne où chaque caractère vaut un octet
        return h.read(int(n)).decode("latin-1")

    def json(self, t):
        def conv(x):
            if lupa.lua_type(x) == "table":
                cles = list(x.keys())
                if cles and all(isinstance(k, int) for k in cles):
                    return [conv(x[k]) for k in sorted(cles)]
                return {str(k): conv(x[k]) for k in cles}
            if isinstance(x, float) and x == int(x):
                return int(x)
            return x
        return json.dumps(conv(t), ensure_ascii=False)


def session(carte, bsp):
    racine = tempfile.mkdtemp(prefix="hm_addon_")
    jeu = FauxJeu(racine, carte, bsp)
    lua = lupa.LuaRuntime(unpack_returned_tuples=True)
    lua.execute("return 1")
    lua.eval("function(src, py) local f = assert(load(src)) f(py) end")(PRELUDE, jeu)
    lua.execute(open(LUA, encoding="utf8").read())
    return lua, jeu, racine


def image(lua):
    """Une image du jeu : la scène, la passe des objets translucides, puis le rendu final."""
    g = lua.globals()
    g.hooks["RenderScene"](g.Vector(100.2, -50.4, 64.03), g.Angle(12.34, -90.5, 0), 106.26)
    g.hooks["PostDrawTranslucentRenderables"](False, True, False)   # ciel 3D : ignoré
    g.hooks["PostDrawTranslucentRenderables"](False, False, False)
    g.hooks["PostRender"]()


def main():
    echecs = []

    def verifier(cond, quoi):
        print(("  ok   " if cond else "  ÉCHEC ") + quoi)
        if not cond:
            echecs.append(quoi)

    # --- 1. map estampillée : la révision se lit dans le .bsp
    d = tempfile.mkdtemp(prefix="hm_bsp_")
    bsp = os.path.join(d, "essai.bsp")
    ent = b'{\n"classname" "worldspawn"\n}\n{\n"origin" "0 0 16"\n"targetname" "hm_rev_951d5486688f642f"\n"classname" "info_target"\n}\n\0'
    open(bsp, "wb").write(bsp_synthetique(ent))
    lua, jeu, racine = session("Essai_Carte", bsp)
    g = lua.globals()
    verifier(g.hooks["HUDShouldDraw"]("CHudChat") is None and g.hooks["PreDrawViewModel"]() is None, "hors signalement, l'affichage et l'arme ne sont pas touchés")
    g.commandes["hm_defaut"](None, None, None, "la chaise traverse le mur")
    verifier(g.hooks["HUDShouldDraw"]("CHudChat") is False and g.hooks["HUDShouldDraw"]("CHudGMod") is False and g.hooks["PreDrawViewModel"]() is True,
             "en attente de capture : chat, HUD et arme ne sont pas dessinés")
    image(lua)
    verifier(g.hooks["HUDShouldDraw"]("CHudChat") is None and g.hooks["PreDrawViewModel"]() is None, "capture faite : l'affichage revient")
    dossier = os.path.join(racine, "data", "hammer_master", "essai_carte")
    lignes = open(os.path.join(dossier, "signalements.txt"), encoding="utf8").read().splitlines()
    verifier(len(lignes) == 1, "une ligne écrite dans data/hammer_master/<map en minuscules>/signalements.txt")
    s = json.loads(lignes[0])
    err = valider(s, SCHEMA)
    verifier(not err, "ligne conforme au schéma" + ("" if not err else " : " + " ; ".join(err)))
    verifier(s["revision"] == "951d5486688f642f" and s["revision_source"] == "bsp", "révision lue dans le bloc d'entités du .bsp")
    verifier(s["bsp"]["taille"] == os.path.getsize(bsp) and s["bsp"]["date"] > 0, "taille et date du .bsp chargé")
    verifier(s["vue"]["fov"] == 106.26 and s["vue"]["fov_reglage"] == 90 and s["vue"]["ecran"] == [1920, 1080], "champ de vision et écran de la vue dessinée")
    verifier(s["angles"] == [12.3, -90.5, 0] and s["oeil"] == [100.2, -50.4, 64], "œil et angles arrondis au dixième")
    verifier(s.get("objet", {}).get("modele") == "models/props_c17/chair02a.mdl" and s["distance"] == 262.1, "objet visé et distance")
    verifier(s.get("capture") == "001.jpg" and os.path.exists(os.path.join(dossier, "001.jpg")), "capture écrite et nommée")
    verifier(any("défaut n° 1 noté" in m for m in g.messages.values()), "message de confirmation dans le chat")

    # --- 2. second signalement par le chat, menu Échap ouvert : écrit sans capture, numéro suivant
    jeu.menu_ouvert = True
    verifier(g.hooks["OnPlayerChat"](g.LocalPlayer(), "!defaut   texture étirée ici  ") is True, "« !defaut » est avalé par le chat")
    verifier(g.hooks["OnPlayerChat"](g.LocalPlayer(), "bonjour") is None, "un message ordinaire passe")
    image(lua)
    lignes = open(os.path.join(dossier, "signalements.txt"), encoding="utf8").read().splitlines()
    s2 = json.loads(lignes[1]) if len(lignes) == 2 else {}
    verifier(s2.get("n") == 2 and s2.get("texte") == "texture étirée ici" and "capture" not in s2, "second signalement numéroté 2, sans capture quand elle échoue")
    verifier(not valider(s2, SCHEMA), "second signalement conforme au schéma")

    # --- 3. map sans estampille : révision vide, puis celle du serveur si elle existe
    open(bsp, "wb").write(bsp_synthetique(b'{\n"classname" "worldspawn"\n}\n\0'))
    lua, jeu, racine = session("essai_carte", bsp)
    g = lua.globals()
    g.commandes["hm_defaut"](None, None, None, "")
    image(lua)
    s3 = json.loads(open(os.path.join(racine, "data", "hammer_master", "essai_carte", "signalements.txt"), encoding="utf8").read().splitlines()[0])
    verifier(s3["revision"] == "" and s3["revision_source"] == "" and s3["texte"] == "(sans texte)", "map sans estampille : révision vide, dite comme telle")
    lua, jeu, racine = session("essai_carte", bsp)
    g = lua.globals()
    g.SetGlobalString("hm_revision", "abcdef0123456789")
    g.commandes["hm_defaut"](None, None, None, "x")
    image(lua)
    s4 = json.loads(open(os.path.join(racine, "data", "hammer_master", "essai_carte", "signalements.txt"), encoding="utf8").read().splitlines()[0])
    verifier(s4["revision"] == "abcdef0123456789" and s4["revision_source"] == "serveur", "à défaut, révision donnée par le serveur")

    # --- 4. le vrai .bsp de rp_megalopole : l'en-tête mène bien au bloc d'entités
    if os.path.exists(VRAI_BSP):
        lua, jeu, racine = session("rp_megalopole_v1", VRAI_BSP)
        g = lua.globals()
        lua.execute("""
            local f = file.Open("maps/rp_megalopole_v1.bsp", "rb", "GAME")
            f:Seek(8) local pos, long = f:ReadLong(), f:ReadLong() f:Seek(pos)
            bloc_debut, bloc_long = f:Read(math.min(long, 600)), long
            f:Close()
        """)
        debut = g.bloc_debut
        verifier(debut.lstrip().startswith("{") and "worldspawn" in debut, f"vrai .bsp : bloc d'entités trouvé ({g.bloc_long} octets)")
        info, rev = g.HammerMaster.LireBsp()
        print(f"       rp_megalopole_v1.bsp : {info.taille} octets, révision inscrite : {rev or 'aucune (compilé avant l estampille)'}")
    else:
        print("  (vrai .bsp de rp_megalopole absent : contrôle sauté)")

    # --- 5. les lignes écrites par le vrai jeu (essai du 13/11/2026) suivent le même schéma
    vraies = os.path.join(os.path.dirname(ECHANTILLON), "essai_en_jeu_1311.txt")
    for i, ligne in enumerate(open(vraies, encoding="utf8").read().splitlines(), 1):
        err = valider(json.loads(ligne), SCHEMA)
        verifier(not err, f"ligne {i} du vrai jeu conforme au schéma" + ("" if not err else " : " + " ; ".join(err)))

    if "--ecrire-echantillon" in sys.argv:
        # l'échantillon relu par les tests Rust (crates/hm-signal/tests/lecture.rs) : à refaire si le format change
        os.makedirs(os.path.dirname(ECHANTILLON), exist_ok=True)
        open(ECHANTILLON, "w", encoding="utf8", newline="\n").write(json.dumps(s, ensure_ascii=False) + "\n" + json.dumps(s2, ensure_ascii=False) + "\n")
        print(f"échantillon écrit : {os.path.normpath(ECHANTILLON)}")
    if echecs:
        sys.exit(f"{len(echecs)} contrôle(s) en échec")
    print("addon hors jeu : tous les contrôles passent")


if __name__ == "__main__":
    main()
