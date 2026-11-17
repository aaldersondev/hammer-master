-- Hammer Master : signalement en jeu, côté client. Voir autorun/hammer_master_signalement.lua.
--
-- Format 2 d'un signalement (une ligne JSON de signalements.txt) :
--   format, n, quand, carte, texte
--   pos, oeil, angles {tangage, lacet, roulis}      le joueur
--   vue {origine, angles, fov, fov_reglage, fov_scene, aspect, ecran}   la vue réellement dessinée (celle de la capture)
--   vise, normale, texture, monde, distance, objet  ce que le joueur regarde
--   revision, revision_source                       révision de la map (entité hm_rev_*), "" si la map n'en porte pas
--   bsp {fichier, taille, date}                     le .bsp que le jeu a chargé
--   joueur {noclip, accroupi, vehicule, au_sol, vue_externe}
--   capture                                         nom du fichier image, absent si la capture a échoué

HammerMaster = HammerMaster or {}
local HM = HammerMaster
HM.FORMAT = 2

local function arr(x)
	return math.Round(x, 1)
end

local function v3(v)
	return { arr(v.x), arr(v.y), arr(v.z) }
end

local function a3(a)
	return { arr(a.p), arr(a.y), arr(a.r) }
end

-- angles de la vue dessinée : au centième, pour que l'outil rende la même image
local function a3fin(a)
	return { math.Round(a.p, 2), math.Round(a.y, 2), math.Round(a.r, 2) }
end

function HM.Dossier()
	return "hammer_master/" .. string.lower(game.GetMap())
end

-- Le .bsp chargé : taille, date, et la révision lue dans son bloc d'entités.
-- En-tête d'un .bsp Source : identifiant (4 octets), version (4), puis 64 descripteurs de 16 octets
-- {position, longueur, version, code} ; le bloc d'entités est le premier (position à l'octet 8).
function HM.LireBsp()
	local chemin = "maps/" .. game.GetMap() .. ".bsp"
	local info = { fichier = chemin, taille = file.Size(chemin, "GAME") or -1, date = file.Time(chemin, "GAME") or 0 }
	local revision = ""
	local f = file.Open(chemin, "rb", "GAME")
	if f then
		f:Seek(8)
		local pos, long = f:ReadLong(), f:ReadLong()
		if pos and long and pos > 0 and long > 0 and long < 64 * 1024 * 1024 then
			f:Seek(pos)
			local texte = f:Read(long)
			if texte then
				revision = string.match(texte, '"targetname"%s+"hm_rev_(%x+)"') or ""
			end
		end
		f:Close()
	end
	return info, revision
end

local bsp, revisionBsp
function HM.Carte()
	if not bsp then
		bsp, revisionBsp = HM.LireBsp()
	end
	local revision, source = revisionBsp, "bsp"
	if revision == "" then
		revision, source = GetGlobalString("hm_revision", ""), "serveur"
	end
	if revision == "" then
		source = ""
	end
	return bsp, revision, source
end

-- Construit le signalement : ce que le joueur regarde, où il est, sur quelle map.
function HM.Construire(ply, texte, n)
	local tr = ply:GetEyeTrace()
	local infoBsp, revision, source = HM.Carte()
	local rec = {
		format = HM.FORMAT,
		n = n,
		quand = os.date("%Y-%m-%d %H:%M:%S"),
		carte = game.GetMap(),
		texte = texte,
		pos = v3(ply:GetPos()),
		oeil = v3(ply:EyePos()),
		angles = a3(ply:EyeAngles()),
		vise = v3(tr.HitPos),
		normale = v3(tr.HitNormal),
		texture = tr.HitTexture,
		monde = tr.HitWorld and true or false,
		distance = arr(tr.HitPos:Distance(tr.StartPos)),
		revision = revision,
		revision_source = source,
		bsp = infoBsp,
		joueur = {
			noclip = ply:GetMoveType() == MOVETYPE_NOCLIP,
			accroupi = ply:Crouching() and true or false,
			vehicule = ply:InVehicle() and true or false,
			au_sol = ply:IsOnGround() and true or false,
			vue_externe = ply:ShouldDrawLocalPlayer() and true or false,
		},
	}
	local ent = tr.Entity
	if IsValid(ent) then
		rec.objet = { classe = ent:GetClass(), modele = ent:GetModel(), pos = v3(ent:GetPos()), angles = a3(ent:GetAngles()) }
	end
	return rec
end

local enAttente -- le signalement dont on attend la vue et la capture

local function numeroSuivant()
	local deja = file.Read(HM.Dossier() .. "/signalements.txt", "DATA") or ""
	local _, n = string.gsub(deja, "\n", "\n")
	return n + 1
end

function HM.Signaler(texte)
	local ply = LocalPlayer()
	if not IsValid(ply) then return end
	file.CreateDir(HM.Dossier())
	local rec = HM.Construire(ply, texte, numeroSuivant())
	-- vue par défaut : celle du joueur ; remplacée par la vue réellement dessinée à l'image suivante
	rec.vue = {
		origine = rec.oeil,
		angles = rec.angles,
		fov_reglage = arr(ply:GetFOV()),
		ecran = { ScrW(), ScrH() },
		aspect = math.Round(ScrW() / ScrH(), 4),
	}
	enAttente = { rec = rec, images = 0 }
end

-- La vue principale telle que le moteur la dessine. RenderScene n'est appelé que pour elle : il donne l'origine et
-- les angles réels (vue externe, véhicule, CalcView d'un autre addon compris).
hook.Add("RenderScene", "hammer_master_signalement", function(origine, angles, fov)
	if not enAttente or enAttente.scene then return end
	enAttente.scene = { origine = origine, angles = angles }
	local vue = enAttente.rec.vue
	vue.origine, vue.angles, vue.fov_scene = v3(origine), a3fin(angles), math.Round(fov or 0, 2)
end)

-- Le champ de vision vient de la description de la vue en cours de dessin. D'autres vues passent par ce crochet
-- (ciel 3D, miroirs, écrans) : seule celle qui a l'origine de la vue principale est retenue.
hook.Add("PostDrawTranslucentRenderables", "hammer_master_signalement", function(_, ciel)
	if not enAttente or enAttente.vue or not enAttente.scene or ciel then return end
	local ok, vs = pcall(render.GetViewSetup)
	if not ok or not vs or not vs.origin or not vs.fov then return end
	if vs.origin:Distance(enAttente.scene.origine) > 1 then return end
	enAttente.vue = true
	local vue = enAttente.rec.vue
	vue.fov = math.Round(vs.fov, 2) -- champ horizontal, ajusté au format de l'écran
	vue.fov_reglage = math.Round(vs.fov_unscaled or 0, 2) -- champ tel que réglé (base 4/3)
	vue.aspect = math.Round(vs.aspect or (ScrW() / ScrH()), 4)
	vue.ecran = { vs.width or ScrW(), vs.height or ScrH() }
end)

-- Tant qu'un signalement attend sa capture (une à trois images), l'affichage du jeu et l'arme tenue ne sont pas
-- dessinés : à l'essai du 13/11/2026, la boîte de chat encore ouverte, le HUD et le pied-de-biche recouvraient
-- un cinquième de la capture. « CHudChat » est la boîte de chat, « CHudGMod » tout ce qui passe par HUDPaint
-- (wiki, HUD Element List). Les panneaux VGUI d'un autre addon restent visibles. Hors de ce moment, les deux
-- crochets ne rendent rien, pour ne pas gêner ceux des autres addons.
hook.Add("HUDShouldDraw", "hammer_master_signalement", function()
	if enAttente then return false end
end)

hook.Add("PreDrawViewModel", "hammer_master_signalement", function()
	if enAttente then return true end
end)

-- La capture se fait dans PostRender, seul moment où l'image finale est disponible. Elle rend nil quand le menu
-- Échap est ouvert : le signalement est alors écrit sans capture plutôt que perdu.
hook.Add("PostRender", "hammer_master_signalement", function()
	if not enAttente then return end
	enAttente.images = enAttente.images + 1
	if not enAttente.vue and enAttente.images < 3 then return end -- laisser une image à la vue pour se dessiner
	local rec = enAttente.rec
	enAttente = nil
	local nom = string.format("%03d.jpg", rec.n)
	local data = render.Capture({ format = "jpeg", quality = 85, x = 0, y = 0, w = ScrW(), h = ScrH(), alpha = false })
	if data and file.Write(HM.Dossier() .. "/" .. nom, data) then
		rec.capture = nom
	end
	file.Append(HM.Dossier() .. "/signalements.txt", util.TableToJSON(rec) .. "\n")
	chat.AddText(Color(120, 220, 160), "[Hammer Master] ", Color(235, 235, 235),
		"défaut n° " .. rec.n .. " noté" .. (rec.capture and "" or " (sans capture)")
		.. (rec.revision ~= "" and (", map " .. rec.revision) or ", map sans révision"))
end)

concommand.Add("hm_defaut", function(_, _, _, argStr)
	HM.Signaler(argStr ~= "" and argStr or "(sans texte)")
end)

concommand.Add("hm_etat", function()
	local infoBsp, revision, source = HM.Carte()
	print("[Hammer Master] map " .. game.GetMap() .. " ; " .. infoBsp.fichier .. " : " .. infoBsp.taille .. " octets, du "
		.. os.date("%Y-%m-%d %H:%M:%S", infoBsp.date) .. " ; révision "
		.. (revision ~= "" and (revision .. " (lue : " .. source .. ")") or "absente (map compilée sans estampille)"))
end)

hook.Add("OnPlayerChat", "hammer_master_signalement", function(ply, text)
	if ply ~= LocalPlayer() then return end
	if string.sub(string.lower(text), 1, 7) ~= "!defaut" then return end
	local t = string.Trim(string.sub(text, 8))
	HM.Signaler(t ~= "" and t or "(sans texte)")
	return true
end)
