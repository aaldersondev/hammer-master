//! Raccourcis clavier : ceux de Hammer par défaut (wiki Valve, voir docs/reference_hammer.md), remplacés par ceux
//! du Hammer++ installé s'il est trouvé, pour que chacun garde les siens (le fichier de Hammer++ suit la disposition
//! du clavier : Z Q S D sur un clavier AZERTY).
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use egui::{Key, Modifiers};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Combo {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub touche: Key,
}

impl Combo {
    /// `"SHIFT+CONTROL+KEY_B"`, la forme du fichier de Hammer++.
    pub fn lire(s: &str) -> Option<Combo> {
        let mut c = Combo { ctrl: false, shift: false, alt: false, touche: Key::Escape };
        let mut touche = None;
        for p in s.split('+') {
            match p.trim() {
                "CONTROL" | "CTRL" => c.ctrl = true,
                "SHIFT" => c.shift = true,
                "ALT" => c.alt = true,
                "KEY_NONE" => return None,
                k => touche = cle(k.trim_start_matches("KEY_")),
            }
        }
        c.touche = touche?;
        Some(c)
    }

    /// La forme écrite dans le fichier de raccourcis (celle de Hammer++).
    pub fn ecrire(&self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s += "CONTROL+";
        }
        if self.shift {
            s += "SHIFT+";
        }
        if self.alt {
            s += "ALT+";
        }
        s + &nom_cle(self.touche)
    }

    pub fn texte(&self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s += "Ctrl+";
        }
        if self.shift {
            s += "Maj+";
        }
        if self.alt {
            s += "Alt+";
        }
        s + self.touche.name()
    }

    fn correspond(&self, m: &Modifiers) -> bool {
        self.ctrl == m.command && self.shift == m.shift && self.alt == m.alt
    }
}

fn nom_cle(k: Key) -> String {
    let n = match k {
        Key::OpenBracket => "LBRACKET",
        Key::CloseBracket => "RBRACKET",
        Key::Delete => "DELETE",
        Key::Enter => "ENTER",
        Key::Escape => "ESCAPE",
        Key::Space => "SPACE",
        Key::Tab => "TAB",
        Key::Plus => "PAD_PLUS",
        Key::Minus => "PAD_MINUS",
        Key::ArrowUp => "UP",
        Key::ArrowDown => "DOWN",
        Key::ArrowLeft => "LEFT",
        Key::ArrowRight => "RIGHT",
        Key::PageUp => "PAGEUP",
        Key::PageDown => "PAGEDOWN",
        Key::Insert => "INSERT",
        autre => return format!("KEY_{}", autre.name().to_uppercase()),
    };
    format!("KEY_{n}")
}

fn cle(nom: &str) -> Option<Key> {
    Some(match nom {
        "LBRACKET" => Key::OpenBracket,
        "RBRACKET" => Key::CloseBracket,
        "DELETE" => Key::Delete,
        "ENTER" => Key::Enter,
        "ESCAPE" => Key::Escape,
        "SPACE" => Key::Space,
        "TAB" => Key::Tab,
        "PAD_PLUS" => Key::Plus,
        "PAD_MINUS" => Key::Minus,
        "UP" => Key::ArrowUp,
        "DOWN" => Key::ArrowDown,
        "LEFT" => Key::ArrowLeft,
        "RIGHT" => Key::ArrowRight,
        "PAGEUP" => Key::PageUp,
        "PAGEDOWN" => Key::PageDown,
        "INSERT" => Key::Insert,
        n if n.len() == 1 => Key::from_name(n)?,
        n if n.starts_with('F') => Key::from_name(n)?,
        n => Key::from_name(n)?,
    })
}

/// Actions de l'éditeur, nommées comme dans le fichier de Hammer++ quand elles y sont.
pub const ACTIONS: &[(&str, &str, &[&str])] = &[
    ("File_New", "Nouvelle map", &["CONTROL+KEY_N"]),
    ("File_Open", "Ouvrir", &["CONTROL+KEY_O"]),
    ("File_Save", "Enregistrer", &["CONTROL+KEY_S"]),
    ("File_Run_Map", "Compiler et lancer", &["KEY_F9"]),
    ("Edit_Undo", "Annuler", &["CONTROL+KEY_Z"]),
    ("Edit_Redo", "Rétablir", &["CONTROL+KEY_Y"]),
    ("Edit_Copy", "Copier", &["CONTROL+KEY_C"]),
    ("Edit_Paste", "Coller", &["CONTROL+KEY_V"]),
    ("Edit_Cut", "Couper", &["CONTROL+KEY_X"]),
    ("Edit_Properties", "Propriétés", &["ALT+KEY_ENTER"]),
    ("Edit_To_Entity", "Lier à une entité", &["CONTROL+KEY_T"]),
    ("Edit_To_World", "Remettre au monde", &["SHIFT+CONTROL+KEY_W"]),
    ("Edit_Flip_Horizontal", "Miroir horizontal", &["CONTROL+KEY_L"]),
    ("Edit_Flip_Vertical", "Miroir vertical", &["CONTROL+KEY_I"]),
    ("Map_Grid_Higher", "Grille plus grosse", &["KEY_RBRACKET"]),
    ("Map_Grid_Lower", "Grille plus fine", &["KEY_LBRACKET"]),
    ("Map_Snap_To_Grid", "Aimanter à la grille", &["SHIFT+KEY_W"]),
    ("View_Toggle_Grid", "Afficher la grille", &["SHIFT+KEY_R"]),
    ("Tools_Snap_Selected_To_Grid", "Caler la sélection sur la grille", &["CONTROL+KEY_B"]),
    ("Tools_Group", "Grouper", &["CONTROL+KEY_G"]),
    ("Tools_Ungroup", "Dégrouper", &["CONTROL+KEY_U"]),
    ("Edit_Toggle_Group_Ignore", "Ignorer les groupes", &["CONTROL+KEY_W"]),
    ("Tools_Pointer", "Outil de sélection", &["SHIFT+KEY_S"]),
    ("Tools_Camera", "Outil caméra", &["SHIFT+KEY_C"]),
    ("Tools_Block", "Outil bloc", &["SHIFT+KEY_B"]),
    ("Tools_Entity", "Outil entité", &["SHIFT+KEY_E"]),
    ("Tools_Texture_Application", "Application de texture", &["SHIFT+KEY_A"]),
    ("Palette_Commandes", "Palette de commandes", &["SHIFT+CONTROL+KEY_P"]),
    ("Compile_Zone", "Compiler la zone de la sélection", &[]),
    ("View_Walkable_Layer", "Calque de praticabilité (joueur simulé)", &[]),
    ("View_Lighting_Preview", "Aperçu de l'éclairage de la map", &[]),
    ("Tools_Create_Prefab", "Enregistrer la sélection en préfabriqué", &["CONTROL+KEY_R"]),
    ("Tools_Sculpt", "Outil sculpter (displacements, dans la 3D)", &[]),
    ("Tools_Apply_Decals", "Outil décal (clic sur une surface dans la 3D)", &["SHIFT+KEY_D"]),
    ("Tools_Overlay", "Outil overlay (clic sur une surface dans la 3D)", &["SHIFT+KEY_O"]),
    ("Tools_Clipping", "Outil de découpe (encore : changer ce qu'on garde)", &["SHIFT+KEY_X"]),
    ("Tools_Merge", "Fusionner les solides", &["SHIFT+CONTROL+KEY_M"]),
    ("Tools_Vertex_Manipulation", "Outil sommets (encore : changer de mode)", &["SHIFT+KEY_V"]),
    ("Tools_Split_Face", "Couper la face entre deux points (outil sommets)", &["CONTROL+KEY_F"]),
    ("Tools_Carve", "Creuser avec la sélection", &["SHIFT+CONTROL+KEY_C"]),
    ("Edit_Apply_Texture", "Appliquer la texture courante", &["SHIFT+KEY_T"]),
    ("Tools_Toggle_Texture_Lock", "Verrou de texture", &["SHIFT+KEY_L"]),
    ("View_Maximize_Restore_Active_View", "Agrandir la vue", &["SHIFT+KEY_Z"]),
    ("View_Autosize4", "Vues de même taille", &["CONTROL+KEY_A"]),
    ("View_Center_On_Selection", "Centrer les vues 2D sur la sélection", &["CONTROL+KEY_E"]),
    ("View_Center_3D_Views_On_Selection", "Centrer la vue 3D sur la sélection", &["SHIFT+CONTROL+KEY_E"]),
    ("View_Clear_Selection", "Vider la sélection", &["SHIFT+KEY_Q"]),
    ("View_Quickhide", "Masquer la sélection", &["KEY_H"]),
    ("View_Quickhide_Unselected", "Masquer le reste", &["CONTROL+KEY_H"]),
    ("View_Quick_Unhide", "Tout réafficher", &["KEY_U"]),
    ("View_2D_X_Y", "Vue 2D X/Y", &["KEY_F2"]),
    ("View_2D_X_Z", "Vue 2D X/Z", &["KEY_F3"]),
    ("View_2D_Y_Z", "Vue 2D Y/Z", &["KEY_F4"]),
    ("View_3D_Textured", "Vue 3D : textures", &["KEY_F5"]),
    ("View_3D_Textured_Shaded", "Vue 3D : textures ombrées", &["SHIFT+KEY_F5"]),
    ("View_3D_Polygon", "Vue 3D : polygones pleins", &[]),
    ("View_3D_Wireframe", "Vue 3D : fil de fer", &[]),
    ("View_3D_Lighting_Preview", "Vue 3D : aperçu de l'éclairage", &["CONTROL+KEY_F5"]),
    ("View_Toggle_3D_Skybox", "Ciel et skybox 3D", &["ALT+KEY_V"]),
    ("View_Toggle_Tool_Textures", "Textures d'outils", &["SHIFT+CONTROL+KEY_F2"]),
    ("View_Toggle_Helpers", "Aides des entités", &["SHIFT+CONTROL+KEY_H"]),
    ("View_Toggle_Editor_Objects", "Icônes des entités (3D)", &["ALT+KEY_O"]),
    ("View_Show_Connections", "Liens d'entrées / sorties entre entités", &[]),
    ("Tools_Hide_Entity_Names", "Cacher les noms des entités (2D)", &["SHIFT+CONTROL+KEY_N"]),
    ("View_Quickhide_To_Visgroup", "Ranger les masqués (H) dans un nouveau visgroup", &[]),
    ("View2D_Next_Draw_Type", "Vue 2D suivante", &["KEY_TAB"]),
    ("View3D_Mouselook_Toggle", "Vue souris (3D)", &["KEY_Z"]),
    ("View3D_Move_Forward", "Avancer (3D)", &["KEY_W"]),
    ("View3D_Move_Backward", "Reculer (3D)", &["KEY_S"]),
    ("View3D_Move_Left", "Gauche (3D)", &["KEY_A"]),
    ("View3D_Move_Right", "Droite (3D)", &["KEY_D"]),
    ("View3D_Strafe_Up", "Monter (3D)", &["KEY_UP"]),
    ("View3D_Mouselook_Hold", "Regarder à la souris tant que la touche est tenue (3D)", &["KEY_SPACE"]),
    ("Tools_Magnify", "Outil loupe (clic : rapprocher, clic droit : éloigner)", &["SHIFT+KEY_G"]),
    ("Tools_Sprinkle", "Outil semer (pinceau dans la 3D)", &["SHIFT+KEY_F"]),
    ("Tools_Polygon", "Outil polygone (contour dans une vue 2D, Entrée)", &["SHIFT+KEY_N"]),
    // pas « Tools_Propper » : Hammer++ le met sur Ctrl+Maj+P, la palette de commandes ici
    ("Tools_Propper_HM", "Propper++ : la sélection en modèle…", &["CONTROL+ALT+KEY_P"]),
    ("Tools_Vertex_Scale", "Sommets choisis à l'échelle…", &["ALT+KEY_E"]),
    ("Tools_Toggle_Gizmo_Orientation", "Manipulateur 3D dans le repère de l'entité", &["KEY_J"]),
    ("Tools_Toggle_Pivot", "Tourner autour de l'origine de l'entité", &["CONTROL+KEY_P"]),
    ("View_Next_Pane", "Vue suivante", &["KEY_F7"]),
    ("View_Prev_Pane", "Vue précédente", &["SHIFT+KEY_F7"]),
    ("View3D_Strafe_Down", "Descendre (3D)", &["KEY_DOWN"]),
    ("View3D_Strafe_Left", "Glisser à gauche (3D)", &["KEY_LEFT"]),
    ("View3D_Strafe_Right", "Glisser à droite (3D)", &["KEY_RIGHT"]),
    ("View3D_Render_Distance_Closer", "Distance de rendu plus courte (3D)", &["KEY_1"]),
    ("View3D_Render_Distance_Further", "Distance de rendu plus longue (3D)", &["KEY_2"]),
    ("View3D_Toggle_Statistics", "Statistiques de la vue 3D", &["KEY_O"]),
    ("View3D_Toggle_Grid_3D", "Grille sur les surfaces (3D)", &["KEY_P"]),
    ("View3D_Color_Picker", "Pipette : couleur sous le curseur (3D) vers la sélection", &["KEY_C"]),
    ("View_Fog_Preview", "Aperçu du brouillard (env_fog_controller)", &[]),
    ("View_Background_Images", "Images de fond des vues 2D…", &[]),
    ("View_Show_Models_In_2D", "Modèles dans les vues 2D", &["SHIFT+KEY_M"]),
    ("View_2D_Logical", "Vue logique (entrées / sorties)…", &[]),
    ("View3D_Light_Bake", "Aperçu de l'éclairage (calculé en direct)", &["KEY_L"]),
    ("View2D_Zoom_In", "Zoomer la vue 2D", &["KEY_PAD_PLUS"]),
    ("View2D_Zoom_Out", "Dézoomer la vue 2D", &["KEY_PAD_MINUS"]),
    ("View2D_Zoom_In_All", "Zoomer toutes les vues 2D", &["CONTROL+KEY_PAD_PLUS"]),
    ("View2D_Zoom_Out_All", "Dézoomer toutes les vues 2D", &["CONTROL+KEY_PAD_MINUS"]),
    ("Tool_Selection_Delete", "Supprimer", &["KEY_DELETE"]),
    ("Tool_Selection_Next_Hit", "Objet suivant sous le curseur", &["KEY_PAGEDOWN"]),
    ("Tool_Selection_Previous_Hit", "Objet précédent sous le curseur", &["KEY_PAGEUP"]),
    ("Tool_Enter", "Valider l'outil", &["KEY_ENTER"]),
    ("Tool_Exit", "Abandonner / vider la sélection", &["KEY_ESCAPE"]),
    ("Map_Check_For_Problems", "Vérifier la map", &["ALT+KEY_P"]),
    // menus de Hammer++ (noms et touches de son fichier de raccourcis quand il les a)
    ("File_Save_As", "Enregistrer sous…", &[]),
    ("File_Export", "Exporter une copie de la map…", &[]),
    ("File_Export_Selection", "Exporter la sélection…", &[]),
    ("File_Export_Again", "Exporter à nouveau", &["ALT+KEY_B"]),
    ("File_Export_DXF", "Exporter en .DXF…", &[]),
    ("Edit_Select_All", "Tout sélectionner", &["SHIFT+CONTROL+KEY_A"]),
    ("Edit_Paste_Special", "Coller spécial…", &["SHIFT+CONTROL+KEY_V"]),
    ("Edit_Find_Entities", "Trouver des entités (palette)", &["SHIFT+CONTROL+KEY_F"]),
    ("Edit_Replace", "Remplacer un matériau…", &["SHIFT+CONTROL+KEY_R"]),
    ("Tools_Make_Hollow", "Évider la sélection", &[]),
    ("Tools_Transform", "Transformer…", &["CONTROL+KEY_M"]),
    ("Tools_Snap_Selected_To_Grid_Individually", "Caler chaque objet sur la grille", &["SHIFT+CONTROL+KEY_B"]),
    ("Tools_Align_Left", "Aligner à gauche (vue 2D active)", &[]),
    ("Tools_Align_Right", "Aligner à droite (vue 2D active)", &[]),
    ("Tools_Align_Top", "Aligner en haut (vue 2D active)", &[]),
    ("Tools_Align_Bottom", "Aligner en bas (vue 2D active)", &[]),
    ("Tools_Move_To_Floor", "Poser au sol", &["ALT+KEY_N"]),
    ("Tools_Move_To_Floor_Align", "Poser au sol, aligné sur la surface", &["CONTROL+ALT+KEY_N"]),
    ("Tools_Move_To_Ceiling", "Poser au plafond", &["ALT+KEY_U"]),
    ("Tools_Move_To_Ceiling_Align", "Poser au plafond, aligné sur la surface", &["CONTROL+ALT+KEY_U"]),
    ("Edit_To_Entity_Individually", "Lier chaque objet à sa propre entité", &["SHIFT+CONTROL+KEY_T"]),
    ("Tools_Center_Origins", "Centrer les origines", &[]),
    ("Edit_Duplicate_Selection", "Dupliquer la sélection", &[]),
    ("Tools_Select_Invalid", "Sélectionner les solides invalides", &[]),
    ("Tools_Keybinds", "Raccourcis clavier…", &[]),
    ("Tools_Options", "Options…", &[]),
    ("Tools_Sound_Browser", "Navigateur de sons", &["SHIFT+KEY_F12"]),
    ("Tools_Particle_Browser", "Navigateur de particules", &["SHIFT+KEY_F11"]),
    ("Tools_Model_Browser", "Navigateur de modèles", &["SHIFT+KEY_F10"]),
    ("View_Texture_Browser", "Navigateur de textures", &["ALT+KEY_F8"]),
    ("View_Goto_Coordinates", "Aller aux coordonnées…", &[]),
    ("View_Goto_Brush", "Aller au numéro de brosse…", &["SHIFT+CONTROL+KEY_G"]),
    ("View_Fit_Map", "Cadrer toute la map", &[]),
    ("View_Live_Validation", "Validation en direct", &[]),
    ("View_Toggle_Side_Panel", "Réduire le panneau de droite", &[]),
    ("Map_Show_Information", "Informations sur la map…", &[]),
    ("Map_Entity_Report", "Rapport d'entités…", &[]),
    ("Map_Entity_Gallery", "Galerie d'entités (une de chaque classe)", &[]),
    ("Map_Diff", "Différence avec un autre VMF…", &[]),
    ("Tools_Create_Instance_From_Selection", "Créer une instance depuis la sélection…", &["ALT+KEY_S"]),
    ("Instancing_Collapse_Selection", "Fondre les instances sélectionnées", &[]),
    ("Instancing_Collapse_All", "Fondre toutes les instances", &[]),
    ("Map_Properties", "Propriétés de la map (worldspawn)", &[]),
    ("Map_Load_Pointfile", "Charger le fichier de fuite", &[]),
    ("Map_Unload_Pointfile", "Oublier la fuite", &[]),
    ("Map_Load_Portalfile", "Charger le fichier de portails", &[]),
    ("Map_Unload_Portalfile", "Oublier les portails", &[]),
];

#[derive(Clone, Debug)]
pub struct Raccourcis {
    pub table: HashMap<String, Vec<Combo>>,
    /// D'où viennent les raccourcis (à montrer à l'utilisateur).
    pub source: String,
}

impl Raccourcis {
    pub fn hammer() -> Raccourcis {
        let mut table = HashMap::new();
        for (a, _, combos) in ACTIONS {
            table.insert(a.to_string(), combos.iter().filter_map(|c| Combo::lire(c)).collect());
        }
        Raccourcis { table, source: "Hammer (wiki Valve)".into() }
    }

    /// Le fichier de Hammer++ installé, s'il est trouvé.
    pub fn fichier_hammerpp() -> Option<PathBuf> {
        let g = hm_assets::jeu::garrysmod()?;
        let p = g.join("bin/win64/hammerplusplus/hammerplusplus_keybindings.cfg");
        p.is_file().then_some(p)
    }

    /// Ceux de Hammer, remplacés action par action par ceux du fichier de Hammer++.
    pub fn importer(p: &Path) -> Result<Raccourcis, String> {
        let src = std::fs::read(p).map_err(|e| format!("{} : {e}", p.display()))?;
        let (blocs, _) = hm_doc::kv::lire(&src).map_err(|e| e.to_string())?;
        let mut r = Raccourcis::hammer();
        let mut vus = std::collections::HashSet::new();
        for racine in &blocs {
            for contexte in racine.enfants() {
                for action in contexte.enfants() {
                    let combos: Vec<Combo> = action.paires().filter(|(k, _)| k.eq_ignore_ascii_case("combo")).filter_map(|(_, v)| Combo::lire(v)).collect();
                    // une action présente dans plusieurs contextes : la première trouvée
                    if ACTIONS.iter().any(|(a, _, _)| *a == action.nom) && vus.insert(action.nom.clone()) {
                        r.table.insert(action.nom.clone(), combos);
                    }
                }
            }
        }
        let n = vus.len();
        r.source = format!("Hammer++ ({} actions reprises de {})", n, p.display());
        Ok(r)
    }

    /// Les raccourcis de l'utilisateur s'il en a enregistré, sinon ceux de son Hammer++, sinon ceux de Hammer.
    pub fn charger() -> Raccourcis {
        if let Some(r) = Raccourcis::lire_fichier(&Raccourcis::fichier_utilisateur()) {
            return r;
        }
        Raccourcis::par_defaut()
    }

    pub fn par_defaut() -> Raccourcis {
        match Raccourcis::fichier_hammerpp().map(|p| Raccourcis::importer(&p)) {
            Some(Ok(r)) => r,
            _ => Raccourcis::hammer(),
        }
    }

    /// `%APPDATA%\HammerMaster\raccourcis.json`.
    pub fn fichier_utilisateur() -> PathBuf {
        let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        base.join("HammerMaster").join("raccourcis.json")
    }

    pub fn lire_fichier(p: &Path) -> Option<Raccourcis> {
        let f: FichierRaccourcis = serde_json::from_slice(&std::fs::read(p).ok()?).ok()?;
        let mut r = Raccourcis::hammer();
        for (a, combos) in f.actions {
            r.table.insert(a, combos.iter().filter_map(|c| Combo::lire(c)).collect());
        }
        r.source = format!("les tiens ({})", p.display());
        Some(r)
    }

    pub fn enregistrer(&self, p: &Path) -> Result<(), String> {
        let mut actions: Vec<(String, Vec<String>)> = self.table.iter().map(|(a, c)| (a.clone(), c.iter().map(Combo::ecrire).collect())).collect();
        actions.sort();
        let f = FichierRaccourcis { version: 1, actions: actions.into_iter().collect() };
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        std::fs::write(p, serde_json::to_string_pretty(&f).unwrap_or_default()).map_err(|e| format!("{} : {e}", p.display()))
    }

    /// Actions qui partagent une même touche (une seule des deux marchera).
    pub fn conflits(&self) -> Vec<(String, String, Combo)> {
        let mut out = Vec::new();
        let actions: Vec<(&String, &Vec<Combo>)> = self.table.iter().collect();
        for (i, (a, ca)) in actions.iter().enumerate() {
            for (b, cb) in actions.iter().skip(i + 1) {
                for c in ca.iter() {
                    // les touches de déplacement 3D et celles des outils ne vivent pas dans le même contexte
                    if cb.contains(c) && !(a.starts_with("View3D") ^ b.starts_with("View3D")) {
                        out.push(((*a).clone(), (*b).clone(), *c));
                    }
                }
            }
        }
        out
    }

    /// L'action a-t-elle été déclenchée dans cette image (touche enfoncée avec les bons modificateurs) ? Par la lettre
    /// de la touche seulement, comme Hammer++ : un fichier réglé pour l'AZERTY (Z avance, W vue souris) marche tel quel.
    /// (Lire aussi la position faisait déclencher au Z d'un AZERTY l'action liée à W, qui est à sa place en QWERTY.)
    pub fn presse(&self, action: &str, i: &egui::InputState) -> bool {
        let Some(combos) = self.table.get(action) else { return false };
        i.events.iter().any(|e| match e {
            egui::Event::Key { key, pressed: true, modifiers, .. } => combos.iter().any(|c| c.touche == *key && c.correspond(modifiers)),
            _ => false,
        })
    }

    /// La touche de l'action est-elle tenue (déplacements en 3D) ?
    pub fn tenu(&self, action: &str, i: &egui::InputState) -> bool {
        self.table.get(action).map_or(false, |c| c.iter().any(|c| i.key_down(c.touche)))
    }

    pub fn texte(&self, action: &str) -> String {
        self.table.get(action).map(|c| c.iter().map(Combo::texte).collect::<Vec<_>>().join(" ou ")).unwrap_or_default()
    }
}

#[derive(Serialize, Deserialize)]
struct FichierRaccourcis {
    version: u32,
    actions: std::collections::BTreeMap<String, Vec<String>>,
}

/// La fenêtre « Raccourcis » : chaque action, sa touche, et « Changer » qui attend la prochaine touche pressée.
#[derive(Default)]
pub struct Editeur {
    pub ouvert: bool,
    attente: Option<String>,
    filtre: String,
}

impl Editeur {
    /// Rend vrai si les raccourcis ont changé.
    pub fn ui(&mut self, ctx: &egui::Context, rc: &mut Raccourcis) -> bool {
        let mut change = false;
        if let Some(a) = self.attente.clone() {
            let presse = ctx.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Key { key, pressed: true, modifiers, .. } if !matches!(key, Key::Escape) => Some(Combo { ctrl: modifiers.command, shift: modifiers.shift, alt: modifiers.alt, touche: *key }),
                    egui::Event::Key { key: Key::Escape, pressed: true, .. } => Some(Combo { ctrl: false, shift: false, alt: false, touche: Key::Escape }),
                    _ => None,
                })
            });
            if let Some(c) = presse {
                if c.touche != Key::Escape || a == "Tool_Exit" {
                    rc.table.insert(a.clone(), vec![c]);
                    change = true;
                }
                self.attente = None;
            }
        }
        let mut ouvert = self.ouvert;
        egui::Window::new("Raccourcis").open(&mut ouvert).default_width(520.0).show(ctx, |ui| {
            ui.label(format!("source : {}", rc.source));
            ui.horizontal(|ui| {
                ui.label("Chercher");
                ui.text_edit_singleline(&mut self.filtre);
                if ui.button("Reprendre ceux de Hammer++ / Hammer").clicked() {
                    *rc = Raccourcis::par_defaut();
                    change = true;
                }
            });
            let conflits = rc.conflits();
            for (a, b, c) in conflits.iter().take(4) {
                ui.colored_label(egui::Color32::from_rgb(255, 170, 80), format!("{} sert à « {a} » et à « {b} »", c.texte()));
            }
            ui.separator();
            let f = self.filtre.to_lowercase();
            egui::ScrollArea::vertical().max_height(480.0).show(ui, |ui| {
                egui::Grid::new("raccourcis").num_columns(3).striped(true).show(ui, |ui| {
                    for (action, libelle, _) in ACTIONS {
                        if !f.is_empty() && !libelle.to_lowercase().contains(&f) && !action.to_lowercase().contains(&f) {
                            continue;
                        }
                        ui.label(*libelle).on_hover_text(*action);
                        if self.attente.as_deref() == Some(*action) {
                            ui.colored_label(egui::Color32::from_rgb(255, 200, 60), "appuyer sur la nouvelle touche…");
                        } else {
                            ui.monospace(rc.texte(action));
                        }
                        ui.horizontal(|ui| {
                            if ui.small_button("Changer").clicked() {
                                self.attente = Some(action.to_string());
                            }
                            if ui.small_button("Aucune").clicked() {
                                rc.table.insert(action.to_string(), Vec::new());
                                change = true;
                            }
                        });
                        ui.end_row();
                    }
                });
            });
        });
        self.ouvert = ouvert;
        if change {
            rc.source = format!("les tiens ({})", Raccourcis::fichier_utilisateur().display());
        }
        change
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Chaque raccourci, pressé par une touche simulée dans un vrai contexte egui, déclenche son action.
    #[test]
    fn chaque_raccourci_par_touche_simulee() {
        for rc in [Raccourcis::hammer(), Raccourcis::par_defaut()] {
            let mut essayes = 0;
            for (action, _, _) in ACTIONS {
                for c in rc.table.get(*action).cloned().unwrap_or_default() {
                    let ctx = egui::Context::default();
                    let m = Modifiers { alt: c.alt, ctrl: c.ctrl, shift: c.shift, mac_cmd: false, command: c.ctrl };
                    let raw = egui::RawInput { events: vec![egui::Event::ModifiersChanged(m), egui::Event::Key { key: c.touche, physical_key: None, pressed: true, repeat: false, modifiers: m }], ..Default::default() };
                    let mut vu = false;
                    let mut sortie = ctx.run_ui(raw, |ui| vu = ui.ctx().input(|i| rc.presse(action, i)));
                    sortie.textures_delta.clear();
                    assert!(vu, "{action} ({}) n'est pas déclenchée par sa touche", c.texte());
                    essayes += 1;
                }
            }
            assert!(essayes >= 45, "{essayes} raccourcis essayés");
        }
    }

    #[test]
    fn clavier_azerty() {
        // les raccourcis d'un Hammer++ réglé pour l'AZERTY (ceux de ce PC) : Z avance, W vue souris. La touche Z d'un
        // AZERTY envoie la lettre Z à la place du W des QWERTY ; elle doit avancer sans ouvrir la vue souris
        let mut rc = Raccourcis::hammer();
        rc.table.insert("View3D_Move_Forward".into(), vec![Combo::lire("KEY_Z").unwrap()]);
        rc.table.insert("View3D_Mouselook_Toggle".into(), vec![Combo::lire("KEY_W").unwrap()]);
        let frappe = |lettre: Key, position: Key, m: Modifiers| {
            let ctx = egui::Context::default();
            let raw = egui::RawInput { events: vec![egui::Event::ModifiersChanged(m), egui::Event::Key { key: lettre, physical_key: Some(position), pressed: true, repeat: false, modifiers: m }], ..Default::default() };
            let mut vus = Vec::new();
            let mut sortie = ctx.run_ui(raw, |ui| {
                vus = ["View3D_Mouselook_Toggle", "Edit_Undo"].into_iter().filter(|a| ui.ctx().input(|i| rc.presse(a, i))).map(String::from).collect();
                if ui.ctx().input(|i| rc.tenu("View3D_Move_Forward", i)) {
                    vus.push("avance".into());
                }
            });
            sortie.textures_delta.clear();
            vus
        };
        assert_eq!(frappe(Key::Z, Key::W, Modifiers::NONE), ["avance"], "le Z de l'AZERTY avance, sans vue souris");
        assert_eq!(frappe(Key::W, Key::Z, Modifiers::NONE), ["View3D_Mouselook_Toggle"], "le W de l'AZERTY ouvre la vue souris");
        let ctrl = Modifiers { ctrl: true, command: true, ..Modifiers::NONE };
        // Ctrl+Z annule (la vue 3D n'avance pas quand Ctrl est tenu)
        assert_eq!(frappe(Key::Z, Key::W, ctrl), ["Edit_Undo", "avance"], "Ctrl+Z annule, sans vue souris");
    }

    #[test]
    fn enregistrer_et_relire() {
        let p = std::env::temp_dir().join(format!("hm_raccourcis_{}.json", std::process::id()));
        let mut r = Raccourcis::hammer();
        r.table.insert("Edit_Undo".into(), vec![Combo { ctrl: true, shift: true, alt: false, touche: Key::U }]);
        r.enregistrer(&p).unwrap();
        let l = Raccourcis::lire_fichier(&p).unwrap();
        assert_eq!(l.table["Edit_Undo"], r.table["Edit_Undo"]);
        assert_eq!(l.table["File_Save"], r.table["File_Save"]);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn combos() {
        let c = Combo::lire("SHIFT+CONTROL+KEY_B").unwrap();
        assert!(c.ctrl && c.shift && !c.alt && c.touche == Key::B);
        assert_eq!(Combo::lire("KEY_RBRACKET").unwrap().touche, Key::CloseBracket);
        assert_eq!(Combo::lire("KEY_F9").unwrap().touche, Key::F9);
        assert!(Combo::lire("KEY_NONE").is_none());
    }

    #[test]
    fn hammerpp_importe_si_present() {
        if let Some(p) = Raccourcis::fichier_hammerpp() {
            let r = Raccourcis::importer(&p).unwrap();
            assert!(r.table.contains_key("Edit_Undo"));
            assert!(!r.table["View3D_Move_Forward"].is_empty());
        }
    }
}
