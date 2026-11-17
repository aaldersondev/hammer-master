-- Hammer Master : signaler un défaut de map depuis le jeu, sur n'importe quelle map.
--   dans le chat :      !defaut <ce qui ne va pas>
--   dans la console :   hm_defaut <ce qui ne va pas>      (hm_etat dit quelle map est chargée)
-- Chaque signalement va dans garrysmod/data/hammer_master/<map>/ : une ligne JSON dans signalements.txt et une
-- capture NNN.jpg. Hammer Master les relit (outils signalements_lire et signalement_comparer).

if SERVER then
	AddCSLuaFile()
	AddCSLuaFile("hammer_master/cl_signalement.lua")

	-- La révision de la map est inscrite à la compilation dans une entité info_target nommée hm_rev_<révision>.
	-- Le client la lit lui-même dans le .bsp ; cette copie par le serveur sert quand il n'y arrive pas.
	hook.Add("InitPostEntity", "hammer_master_revision", function()
		for _, e in ipairs(ents.FindByClass("info_target")) do
			local nom = e:GetName() or ""
			if string.sub(nom, 1, 7) == "hm_rev_" then
				SetGlobalString("hm_revision", string.sub(nom, 8))
				return
			end
		end
	end)
	return
end

include("hammer_master/cl_signalement.lua")
