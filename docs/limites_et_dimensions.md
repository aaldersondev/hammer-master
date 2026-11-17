# Limites et dimensions du moteur, avec leurs sources

Relevé du 12/11/2026. Aucune valeur de ce fichier ne vient de mémoire. Trois sortes de sources :

- **[CODE]** : lu dans `ValveSoftware/source-sdk-2013`, branche `master`, dossier `src/` ;
- **[VDC]** : page du Valve Developer Community wiki lue en entier (par le navigateur : le wiki renvoie 403 aux
  lectures automatiques) ;
- **[GMOD]** : notes de version de `gmod.facepunch.com` ; **[LUA]** : code de `Facepunch/garrysmod` ou `FPtje/DarkRP` ;
- **[LOG]** : colonne « Maxobjs » de la table écrite par les compilateurs installés (`GarrysMod/bin/win64`, vbsp du
  26/10/2026), dans `rp_megalopole/out/rp_megalopole_v1.log` du 12/11/2026.

Le moteur et les compilateurs de GMod sont fermés : quand [LOG] existe, c'est lui qui fait foi pour ce PC. **Règle
pour l'outil : les plafonds se lisent dans la table du dernier log de compilation ; ce fichier ne sert que de
valeur de départ et d'explication.**

## A. Limites de map

| Limite | SDK 2013 [CODE] | Notes de version GMod | Compilateur installé [LOG] | rp_megalopole 12/11 |
|---|---|---|---|---|
| brosses (`MAX_MAP_BRUSHES`) | 8 192 | 16 384 (02/2019), 65 535 (03/2025) | 65 535 | 7 783 |
| faces de brosses (`BRUSHSIDES`) | 65 536 | « environ 163 000 » (06/2021) | 655 350 | 48 623 |
| plans | 65 536 | — | 65 536 | 16 152 |
| **sommets (`MAX_MAP_VERTS`)** | 65 536 | — | 65 536 | **64 851** |
| faces | 65 536 | — | 65 536 | 40 319 |
| nœuds / feuilles | 65 536 / 65 536 | — | 65 536 / 65 536 | 20 759 / 20 961 |
| faces par feuille (`LEAFFACES`) | 65 536 | — | 65 536 | 51 760 |
| arêtes / surfedges | 256 000 / 512 000 | — | 256 000 / 512 000 | 172 119 / 286 281 |
| texinfo / texdata | 12 288 / 2 048 | 16 384 / 8 192 (03/2024) ; 32 768 annoncé | 32 768 / 8 192 | 7 503 / 628 |
| entités à brosses (`MODELS`) | 1 024 | 4 096 (06/2020) | 4 096 | 201 |
| areas | 256 | 1 024 (02/2019) | 1 024 | 109 |
| displacements | 2 048 | 16 384 (11/2019) | non borné dans la table | 320 |
| overlays | 512 | 8 192 (03/2024) | 8 192 | 0 |
| cubemaps | 1 024 | — | 1 024 | 301 |
| lumières (`WORLDLIGHTS`) | 8 192 | — | 8 192 | 3 128 |
| **props statiques** | pas de constante de nombre | « plafonné à des valeurs sûres » (11/2025) | 16 384 | **15 801** |
| **indices d'eau (`waterindices`)** | 65 536 (`PRIMINDICES`) | — | 65 536 | **63 822** |
| lumière (`MAX_MAP_LIGHTING`) | 16 777 216 octets | — | « variable », sans plafond affiché | 17 310 028 ⚠ |
| visibilité | 16 777 216 | — | 16 777 216 | 2 731 723 |
| données d'entités | 384 Ko conseillés [VDC] | — | 4 194 304 | 1 543 999 |
| collision | — | — | 4 194 304 | 2 823 825 |
| entités dans le VMF (`MAX_MAP_ENTITIES`) | 8 192 | 20 480 (02/2019), « 65k » (03/2024) | — | 22 242 dont 15 801 props statiques |

⚠ La lumière du dernier log dépasse la constante du SDK sans que vrad ait refusé. À éclaircir avant de s'appuyer
sur un plafond : soit le vrad de GMod a relevé la limite, soit le chiffre affiché compte autre chose que le lump.

Limites sans table dans le log, valeur du SDK [CODE] :

| Limite | Valeur | Où |
|---|---|---|
| jonctions en T : primitives / indices | 32 768 / 65 536 ; au-delà « Too many t-junctions to fix up! » | `utils/vbsp/faces.cpp` l. 686-688 |
| état des areaportals envoyé au client | 24 octets = **192 portails** (`MAX_AREA_PORTAL_STATE_BYTES`) | `public/const.h` l. 431 ; plantage vu en jeu le 31/10 |
| noms de lumières distincts | 32 (`MAX_SWITCHED_LIGHTS`) | `utils/vbsp/writebsp.cpp` l. 984-1022 |
| styles de lumière par face | 4 (`MAXLIGHTMAPS`) | `public/bspfile.h` l. 679 |
| coordonnées | ±16 384 (`MAX_COORD_INTEGER`) | `public/worldsize.h` l. 19 |
| étendue utile des brosses | **±16 376** : l'arbre est bordé de 8 au-delà des brosses, puis encore de 8 (`SIDESPACE`), et un nœud au-delà de ±(16 384 + 8) est « BSP node with unbounded volume » ; vu le 14/11 : coquille à ±16 384 → ces avertissements et vbsp **plante** (0xC0000005) ; à ±16 376 → code 0, sans fuite | `utils/vbsp/portals.cpp` (`MakeHeadnodePortals`, `MakeTreePortals_r`) |
| faces par brosse | 128 (`MAX_BRUSH_SIDES`), contrôle non retrouvé dans vbsp | `utils/vbsp/vbsp.h` l. 29 |
| points par polygone | 64 (`MAX_POINTS_ON_WINDING`) | `utils/common/polylib.h` l. 30 |
| faces touchées par un overlay | 64 | `public/bspfile.h` l. 1001 |
| lightmap d'une face | 32 luxels d'étendue, soit 32 × échelle unités avant découpe | `utils/vbsp/faces.cpp` (`g_maxLightmapDimension`) |
| puissance d'un displacement | 2 à 4 | `public/bspfile.h` |
| portails de vvis | erreur dès 32 768 | `utils/vvis/vvis.cpp` l. 472 |
| clé / valeur d'entité | 32 / 1 024 caractères | `public/bspfile.h` l. 152-153 |
| entités réseau en jeu | 2 048 dans le SDK ; 8 192 dans GMod | `public/const.h` ; [VDC] `Entity_limit` |

## B. Joueur

| Grandeur | Valeur | Source |
|---|---|---|
| boîte debout / accroupi | 32 × 32 × 72 / 32 × 32 × 36 | [CODE] `game/shared/gamerules.cpp` l. 47-61 |
| yeux debout / accroupi | 64 / 28 | idem |
| marche franchissable | 18 (`sv_stepsize`) | [CODE] `game/shared/movevars_shared.cpp` l. 94 |
| pente maximale | normale z ≥ 0,7, soit 45,57° | [CODE] `game/shared/gamemovement.cpp` l. 1564 |
| gravité | 600 | [CODE] `movevars_shared.cpp` l. 20-22 |
| saut dans GMod | `JumpPower` 200 : sommet à 200² ÷ (2 × 600) = 33,3 ; obstacle franchi mesuré : 30 | [LUA] `player_default.lua` l. 16 ; [VDC] `Dimensions` |
| saut accroupi dans GMod | 68 (mesure du wiki) | [VDC] `Dimensions` |
| saut dans HL2 | 21 ; accroupi 56 | [CODE] `gamemovement.h` l. 24 ; [VDC] |
| vitesses DarkRP | marche 160, course 240 (police 255) ; saut inchangé | [LUA] DarkRP `gamemode/config/config.lua` l. 193-302 |
| vitesses sandbox | 100 / 200 / 400 | [LUA] `player_sandbox.lua` l. 27-29 |
| passage minimal | largeur 33 ; hauteur 73 debout, 37 accroupi | [VDC] `Dimensions` |
| distance d'utilisation | 80 (`PLAYER_USE_RADIUS`) | [CODE] `game/shared/baseplayer_shared.h` l. 15 |
| chute sans dégât | 526,5 u/s, soit environ 231 unités | [CODE] `shareddefs.h` l. 409-410 |
| dégâts de chute DarkRP | vitesse ÷ 15 | [LUA] DarkRP `sv_gamemode_functions.lua` l. 1045-1049 |
| repères du wiki | porte 48 × 108, couloir 64 × 128 | [VDC] `Dimensions` |

Le saut de 56 souvent cité est le saut accroupi de HL2. Dans GMod, compter 30 debout et 68 accroupi.

## C. Messages des compilateurs à devancer

| Message | Outil | Ce que l'outil doit contrôler |
|---|---|---|
| `FloatPlane: bad normal`, `plane with no normal` | vbsp `map.cpp` | trois points du plan alignés |
| `duplicate plane`, `mirrored plane` | vbsp `map.cpp` l. 2771 | deux faces d'une brosse sur le même plan ou sur des plans opposés |
| `no visible sides on brush` | vbsp `map.cpp` l. 661 | les plans ne ferment pas un volume |
| `bounds out of range` | vbsp `map.cpp` l. 659 | brosse hors de ±16 384 |
| `WARNING, microbrush` | vbsp `brushbsp.cpp` l. 1417 | volume inférieur à 1 |
| `func_areaportal can only be a single brush` | vbsp `map.cpp` l. 1697 | une brosse par portail |
| `areaportal brush doesn't touch two areas` | vbsp `portals.cpp` l. 852 | portail qui ne sépare pas deux areas fermées |
| `displacement found on a(n) … entity` | vbsp `disp_vbsp.cpp` l. 631 | displacement hors du monde |
| `Trying to create a non-quad displacement!` | vbsp `disp_vbsp.cpp` l. 641 | face de base à quatre côtés |
| `Overlay touching too many faces` | vbsp `overlay.cpp` l. 277 | plus de 64 faces |
| `Too many t-junctions to fix up!` | vbsp `faces.cpp` l. 688 | budget des jonctions en T |
| `Too many switched lights` | vbsp `writebsp.cpp` l. 1022 | plus de 32 noms de lumière |
| `Static prop … outside the map` | vbsp `staticprop.cpp` l. 489 | origine du prop dans un solide ou dans le vide |
| `sky_camera in solid volume` | vbsp `vbsp.cpp` l. 818 | — |
| `**** leaked ****` | vbsp | inondation depuis les entités |
| `Bad surface extents` | `utils/common/bsplib.cpp` l. 3371 | face trop grande pour sa lightmap |
| `face vectors parallel to face normal` | vrad `lightmap.cpp` l. 453 | axes de texture perpendiculaires à la face |
| `Too many light styles on a face` | vrad `lightmap.cpp` l. 2536 | plus de 4 styles |

Aucun message de vbsp ne signale un sommet hors grille : ce contrôle n'existe que chez nous.

## D. Format VMF

- Orientation du plan : les trois points sont en sens horaire vus de l'extérieur ; normale sortante
  = (p0 − p1) × (p2 − p1). [CODE] `CMapFile::PlaneFromPoints`, `utils/vbsp/map.cpp` l. 384-399. Le premier exemple
  de la page du wiki contredit cette règle ; l'exemple à six plans de la même page la confirme.
- Les `id` de `side` sont uniques dans tout le fichier : cubemaps et overlays y renvoient.
- Displacement de puissance n : `normals`, `offsets`, `offset_normals` ont 2^n + 1 lignes de 3 × (2^n + 1) valeurs ;
  `distances` et `alphas` 2^n + 1 lignes de 2^n + 1 valeurs ; `triangle_tags` 2^n lignes de 2 × 2^n valeurs.
- `connections` : `"Sortie" "cible,entrée,paramètre,délai,répétitions"`, séparateur virgule ou caractère 0x1b.
- `func_instance` : `file` (relatif au VMF parent), `targetname`, `fixup_style` (0 préfixe, 1 suffixe, 2 aucun),
  `replace01` à `replace10` (`"$variable valeur"`), `origin`, `angles`. Les noms qui commencent par `@` ne sont pas
  renommés. Une entité à brosses d'une instance est tournée par sa clé `angles`.

## Ce qui reste non vérifié

1. Le plafond réel de la lumière dans le vrad de GMod (voir ⚠).
2. Ce qui se passe exactement au-delà de 192 areaportals (le message vu en jeu n'est pas dans le SDK public).
3. `MAX_SWITCHED_LIGHTS` dans le vbsp de GMod depuis le passage à 128 styles.
4. Les valeurs par défaut de GMod pour la boîte du joueur, la gravité et la distance d'utilisation (supposées
   égales au SDK ; à mesurer en jeu en phase 3).
5. La liste complète des contrôles de « Check for Problems » de Hammer : le wiki n'en documente que quatre.
