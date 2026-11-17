# Comportement de Hammer et Hammer++ : ce qui a été vérifié, et où

Relevé le 14/11/2026. Chaque comportement repris dans l'éditeur renvoie ici.

## Sources

| Source | Lue le | Ce qu'on y a pris |
|---|---|---|
| VDC, [Hammer Viewports](https://developer.valvesoftware.com/wiki/Hammer_Viewports) | 14/11/2026 | quatre vues, la 3D en haut à gauche ; vues 2D Top (X/Y), Front (Y/Z), Side (X/Z) ; vue active = celle sous la souris ; Shift+Z plein écran, Ctrl+A tailles égales ; molette = zoom en 2D, avance en 3D ; Ctrl+molette = zoom de toutes les vues 2D ; espace + glisser = déplacer la vue |
| VDC, [Hammer Hotkey Reference](https://developer.valvesoftware.com/wiki/Hammer_Hotkey_Reference) | 14/11/2026 | table complète des raccourcis par outil (résumée ci-dessous) |
| `GarrysMod\bin\win64\hammerplusplus\hammerplusplus_keybindings.cfg` (Hammer++ installé sur ce PC) | 14/11/2026 | les raccourcis **tels que tu les utilises** (déplacement 3D en Z/Q/S/D, bascule de la vue souris sur W : clavier AZERTY) ; format KeyValues `"Keybinds" { "<contexte>" { "<action>" { "combo" "SHIFT+KEY_B" } } }` |
| `…\hammerplusplus_settings.ini` | 14/11/2026 | grille par défaut 64, nudge aux flèches activé, champ de vision 3D 90°, sauvegarde auto toutes les 15 min (5 par map), 100 niveaux d'annulation ; couleurs de grille : lignes (60 60 60), toutes les 10 (115 115 115), tous les 1024 (100 50 5) |

## Raccourcis retenus par défaut (wiki, confirmés par le fichier de Hammer++)

| Raccourci | Action |
|---|---|
| Shift+S, Shift+C, Shift+B, Shift+E, Shift+A, Shift+X, Shift+V, Shift+D, Shift+O, Shift+G | outils : sélection, caméra, bloc, entité, application de texture, découpe, sommets, décal, overlay, loupe |
| Shift+T | appliquer la texture courante à la sélection |
| Ctrl+N / O / S | nouveau, ouvrir, enregistrer |
| F9 | compiler et lancer |
| Ctrl+Z / Ctrl+Y | annuler / rétablir |
| Ctrl+C / V / X, Ctrl+Shift+V | copier, coller, couper, collage spécial |
| [ et ] | grille plus fine / plus grosse |
| Shift+R, Shift+W | afficher la grille, aimanter à la grille |
| Ctrl+B, Ctrl+Shift+B | caler la sélection sur la grille (ensemble, un par un) |
| Ctrl+G, Ctrl+U | grouper, dégrouper |
| Ctrl+T, Ctrl+Shift+W | lier à une entité, remettre au monde |
| Ctrl+W | ignorer les groupes |
| H, Ctrl+H, U | masquer la sélection, masquer le reste, tout réafficher |
| Shift+Z, Ctrl+A | vue plein écran, tailles égales |
| Ctrl+E, Ctrl+Shift+E | centrer les vues 2D / la vue 3D sur la sélection |
| F2, F3, F4, F5 | vue 2D X/Y, X/Z, Y/Z, 3D texturée |
| Tab (vue 2D) | passer à la vue 2D suivante |
| pavé + / − | zoom (Ctrl : toutes les vues 2D) |
| flèches (nudge activé) | décaler la sélection d'un pas de grille ; Shift+flèche : cloner puis décaler |
| Alt (en glissant) | sans aimantation |
| Shift (en glissant la sélection) | cloner |
| Ctrl+clic | ajouter / retirer de la sélection |
| clic sur un objet déjà sélectionné | poignées : redimensionner → tourner → cisailler |
| Shift+Q, Échap | vider la sélection |
| PgUp / PgDn | objet précédent / suivant sous le curseur |
| Entrée / Échap (outil bloc ou entité) | créer / abandonner |
| Alt+Entrée | propriétés de l'objet |
| Shift+L | verrou de texture |
| Ctrl+M | transformation numérique |
| Alt+P | vérifier la map |
| Ctrl+Shift+G, Ctrl+Shift+F | aller au solide n°, chercher des entités |
| Ctrl+L, Ctrl+I | miroir horizontal, vertical |
| Suppr | supprimer |
| 3D : W A S D (Z Q S D chez toi), Z (W chez toi) | se déplacer, basculer la vue souris |
| 3D : espace + clic gauche / droit / les deux | tourner / glisser / avancer |
| 3D : 1 / 2 | distance d'affichage plus courte / plus longue |

Le fichier de Hammer++ diffère du wiki par endroits (Shift+T = appliquer la texture courante ; Alt+C = créer
l'objet ; Shift+W = aimantation). L'éditeur importe ce fichier s'il le trouve : tes raccourcis restent les tiens.

## Visgroups, masquage, cordon (wiki Valve, relevé le 14/11/2026)

- « Grouping and VisGrouping » : un visgroup décoché n'est pas compilé ; « Hide selected objects » crée un visgroup
  décoché nommé « n objects » ; un visgroup sans objet disparaît de la liste ; au-delà de 128 visgroups, Hammer se
  comporte mal ; un visgroup peut avoir des enfants (décocher le parent décoche les enfants).
- « Hammer Filter Control Toolbar » : case à cocher par visgroup ; bouton « Show » qui montre tout sans changer les
  états ; fusion et parenté par glisser (non fait ici).
- « Hammer View Menu » : QuickHide Objects (H), QuickHide Unselected Objects (Ctrl+H), Unhide QuickHide Objects (U),
  « Convert QuickHide objects to VisGroup » (visgroup nommé « _FromQuickHide » suivi du nombre d'objets).
- « Hammer Cordon Usage » : seuls les objets au moins en partie dans la boîte sont compilés, la boîte est fermée par des
  brosses de la texture de cordon (champ « Cordon texture » de la configuration de jeu ; `CordonTexture` vaut
  `tools	oolsskybox` dans la configuration de Hammer++ de ce PC) ; cordon actif : seuls les objets compilés sont
  montrés, la boîte est tracée en rouge épais ; inactif, elle n'est pas montrée.
- Format relevé dans les VMF du PC : `visgroups { visgroup { "name" "visgroupid" "color" } }`, appartenance par
  `visgroupid` dans le bloc `editor` de l'objet avec `visgroupshown` et `visgroupautoshown` ;
  `cordons { "active" cordon { "name" "active" box { "mins" "maxs" } } }`.

## Props (wiki Valve, relevé le 14/11/2026)

- « prop_dynamic » : clé « Bodygroup (body / SetBodyGroup) », entier qui choisit les sous-modèles ; si `body` et
  `SetBodyGroup` sont tous deux là, `body` l'emporte. Le FGD de Garry's Mod n'a que `SetBodyGroup` (base.fgd).
  Astuce Hammer++ notée sur la même page : Ctrl + molette sur un prop dans la 3D change `modelscale` (non fait ici).
- « prop_static » : pas de bodygroups (seul le premier sous-modèle est montré en jeu) ; les skins marchent.
- Aucune page du wiki trouvée sur le navigateur de modèles de Hammer++ ni sur la pose des props sur une surface :
  comportement choisi ici, non comparé à Hammer++.

## Non vérifié à ce jour

- Disposition exacte des trois vues 2D par défaut (seule la place de la 3D est écrite dans le wiki) : l'éditeur met
  X/Y en haut à droite, Y/Z en bas à gauche, X/Z en bas à droite, et chaque vue se change par F2 à F5 ou par son titre.
