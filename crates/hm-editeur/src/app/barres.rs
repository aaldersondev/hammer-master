//! Barre d'outils en icônes, panneau des outils, onglets du panneau de droite, menus rangés comme ceux de Hammer++
//! (Fichier, Édition, Carte, Affichage, Outils, Compiler) et les petits dialogues qu'ils ouvrent (Transformer, Aller
//! aux coordonnées, Aller au numéro de brosse, Informations sur la map). Un bouton ou une entrée de menu lance
//! l'action de la table des raccourcis par le même chemin qu'une touche (`declenchee`) ; l'infobulle donne la touche.
use std::path::PathBuf;

use egui::Ui;
use hm_doc::{Commande, Ref};
use hm_edit::disposition::{self, Transformation};

use super::{Editeur, Onglet, Outil};
use crate::icones as ic;
use crate::raccourcis::{Raccourcis, ACTIONS};
use crate::vues::Genre;

/// Les dialogues ouverts par les menus.
#[derive(Default)]
pub struct Dialogues {
    /// Mode (0 tourner, 1 échelle, 2 déplacer) et valeurs X, Y, Z.
    transformer: Option<(usize, [f64; 3])>,
    coordonnees: Option<[f64; 3]>,
    brosse: Option<String>,
    infos: Option<disposition::Infos>,
    coller: Option<CollerSpecial>,
    a_propos: bool,
    echelle_sommets: Option<f64>,
}

/// Le dialogue « Coller spécial » : le presse-papiers, la taille de son contenu et les réglages.
struct CollerSpecial {
    vmf: String,
    taille: [f64; 3],
    copies: u32,
    decalage: [f64; 3],
    rotation: [f64; 3],
    grouper: bool,
    uniques: bool,
    prefixe: String,
}

/// Un dialogue, au milieu de la fenêtre à sa première ouverture.
pub(super) fn fenetre<'a>(ctx: &egui::Context, titre: &'a str) -> egui::Window<'a> {
    egui::Window::new(titre).collapsible(false).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center())
}

fn nom(id: &str) -> &'static str {
    ACTIONS.iter().find(|(a, ..)| *a == id).map_or("?", |(_, n, _)| crate::langue::t(n))
}

const ONGLETS: [(Onglet, char, &str); 8] = [
    (Onglet::Textures, ic::TEXTURES, "Textures"),
    (Onglet::Proprietes, ic::PROPRIETES, "Propriétés"),
    (Onglet::Historique, ic::HISTORIQUE, "Historique"),
    (Onglet::Visgroups, ic::VISGROUPS, "Visgroups"),
    (Onglet::Modeles, ic::MODELES, "Modèles"),
    (Onglet::Prefabs, ic::PREFABS, "Préfabs"),
    (Onglet::Verification, ic::VERIFIER, "Vérification"),
    (Onglet::Remarques, ic::REMARQUES, "Remarques"),
];

const OUTILS: [(Outil, char, &str); 13] = [
    (Outil::Selection, ic::SELECTION, "Tools_Pointer"),
    (Outil::Camera, ic::CAMERA, "Tools_Camera"),
    (Outil::Bloc, ic::BLOC, "Tools_Block"),
    (Outil::Entite, ic::ENTITE, "Tools_Entity"),
    (Outil::Texture, ic::TEXTURE, "Tools_Texture_Application"),
    (Outil::Decoupe, ic::DECOUPE, "Tools_Clipping"),
    (Outil::Sommets, ic::SOMMETS, "Tools_Vertex_Manipulation"),
    (Outil::Decal, ic::DECAL, "Tools_Apply_Decals"),
    (Outil::Overlay, ic::OVERLAY, "Tools_Overlay"),
    (Outil::Sculpter, ic::SCULPTER, "Tools_Sculpt"),
    (Outil::Loupe, ic::LOUPE, "Tools_Magnify"),
    (Outil::Semer, ic::SEMER, "Tools_Sprinkle"),
    (Outil::Polygone, ic::POLYGONE, "Tools_Polygon"),
];

/// La barre d'outils, par groupes ; `GRILLE_TAILLE` marque où s'écrit la taille de la grille.
const GRILLE_TAILLE: &str = "";
const BARRE: &[&[(char, &str, &str)]] = &[
    &[(ic::NOUVEAU, "Nouv.", "File_New"), (ic::OUVRIR, "Ouvrir", "File_Open"), (ic::ENREGISTRER, "Enr.", "File_Save")],
    &[(ic::ANNULER, "Ann.", "Edit_Undo"), (ic::RETABLIR, "Rét.", "Edit_Redo")],
    &[(ic::COUPER, "Couper", "Edit_Cut"), (ic::COPIER, "Copier", "Edit_Copy"), (ic::COLLER, "Coller", "Edit_Paste"), (ic::SUPPRIMER, "Suppr.", "Tool_Selection_Delete")],
    &[(ic::GRILLE, "Grille", "View_Toggle_Grid"), (ic::MOINS, "−", "Map_Grid_Lower"), (' ', "", GRILLE_TAILLE), (ic::PLUS, "+", "Map_Grid_Higher"), (ic::AIMANT, "Aimant", "Map_Snap_To_Grid")],
    &[(ic::VERROU, "Verrou", "Tools_Toggle_Texture_Lock"), (ic::IGNORER_GROUPES, "Grpes", "Edit_Toggle_Group_Ignore"), (ic::GROUPER, "Grp", "Tools_Group"), (ic::DEGROUPER, "Dégrp", "Tools_Ungroup")],
    &[(ic::CREUSER, "Creuser", "Tools_Carve"), (ic::FUSIONNER, "Fusion", "Tools_Merge"), (ic::EVIDER, "Évider", "Tools_Make_Hollow"), (ic::TRANSFORMER, "Transf.", "Tools_Transform")],
    &[(ic::MASQUER, "Masq.", "View_Quickhide"), (ic::MASQUER_RESTE, "Reste", "View_Quickhide_Unselected"), (ic::REAFFICHER, "Tout", "View_Quick_Unhide")],
    &[(ic::VUES_EGALES, "4 vues", "View_Autosize4"), (ic::AGRANDIR, "Agr.", "View_Maximize_Restore_Active_View"), (ic::CADRER, "Cadrer", "View_Fit_Map")],
    &[(ic::ECLAIRAGE, "Lum.", "View_Lighting_Preview"), (ic::PRATICABILITE, "Prat.", "View_Walkable_Layer"), (ic::VALIDATION, "Valid.", "View_Live_Validation")],
    &[(ic::VERIFIER, "Vérif.", "Map_Check_For_Problems"), (ic::COMPILER, "Compiler", "File_Run_Map"), (ic::COMPILER_ZONE, "Zone", "Compile_Zone")],
    &[(ic::PALETTE, "Palette", "Palette_Commandes"), (ic::OPTIONS, "Options", "Tools_Options")],
];

impl Editeur {
    fn declencher(&mut self, ctx: &egui::Context, id: &'static str) {
        self.declenchee = Some(id);
        ctx.request_repaint();
    }

    /// « Nom (touche) ».
    fn aide(&self, id: &str) -> String {
        let t = self.rc.texte(id);
        if t.is_empty() {
            nom(id).to_string()
        } else {
            format!("{} ({t})", nom(id))
        }
    }

    /// L'action peut-elle se faire maintenant ?
    fn possible(&self, id: &str) -> bool {
        let n = self.session.selection.len();
        match id {
            "Tools_Merge" | "Tools_Align_Left" | "Tools_Align_Right" | "Tools_Align_Top" | "Tools_Align_Bottom" => n >= 2,
            "Edit_Cut" | "Edit_Copy" | "Tool_Selection_Delete" | "Tools_Carve" | "Tools_Make_Hollow" | "Tools_Group" | "Tools_Ungroup" | "Edit_To_Entity" | "Edit_To_World" | "Tools_Transform" | "Tools_Snap_Selected_To_Grid"
            | "Tools_Snap_Selected_To_Grid_Individually" | "Edit_Flip_Horizontal" | "Edit_Flip_Vertical" | "Tools_Move_To_Floor" | "Tools_Move_To_Floor_Align" | "Tools_Move_To_Ceiling" | "Tools_Move_To_Ceiling_Align" | "Edit_To_Entity_Individually" | "Tools_Center_Origins" | "Edit_Duplicate_Selection" | "File_Export_Selection" | "Tools_Create_Instance_From_Selection" | "Tools_Propper_HM" | "Instancing_Collapse_Selection" | "Tools_Create_Prefab" | "View_Center_On_Selection"
            | "View_Center_3D_Views_On_Selection" | "View_Quickhide" | "View_Quickhide_Unselected" | "Compile_Zone" | "View_Clear_Selection" => n > 0,
            "View_Quick_Unhide" | "View_Quickhide_To_Visgroup" => !self.filtres.masques.is_empty(),
            "Edit_Undo" => !self.session.historique().faits().is_empty(),
            "Edit_Redo" => !self.session.historique().annules().is_empty(),
            "Map_Unload_Pointfile" => self.fuite.is_some(),
            "File_Export_Again" => self.dernier_export.is_some(),
            "Map_Unload_Portalfile" => self.portails.is_some(),
            _ => true,
        }
    }

    /// L'état d'une bascule, `None` si l'action n'en est pas une.
    fn bascule(&self, id: &str) -> Option<bool> {
        Some(match id {
            "View_Toggle_Grid" => self.grille_visible,
            "Map_Snap_To_Grid" => self.aimant,
            "Tools_Toggle_Texture_Lock" => self.verrou_texture,
            "Edit_Toggle_Group_Ignore" => self.ignorer_groupes,
            "View_Lighting_Preview" => self.eclairage.actif,
            "View_Walkable_Layer" => self.direct.calque,
            "View_Live_Validation" => self.direct.actif,
            "View_Maximize_Restore_Active_View" => self.agrandie.is_some(),
            "View_Toggle_Side_Panel" => self.panneau_reduit,
            "View_3D_Textured_Shaded" => self.rendu_3d == 0 && !self.eclairage.actif,
            "View_3D_Textured" => self.rendu_3d == 1,
            "View_3D_Polygon" => self.rendu_3d == 2,
            "View_3D_Wireframe" => self.rendu_3d == 3,
            "View_3D_Lighting_Preview" => self.rendu_3d == 0 && self.eclairage.actif,
            "View_Toggle_3D_Skybox" => self.ciel.montrer,
            "View_Toggle_Tool_Textures" => self.afficher_outils,
            "View_Toggle_Helpers" => self.afficher_aides,
            "View_Toggle_Editor_Objects" => self.afficher_icones,
            "View_Show_Connections" => self.affichage.liens,
            "Tools_Hide_Entity_Names" => self.affichage.cacher_noms,
            "View3D_Toggle_Statistics" => self.afficher_stats,
            "View3D_Toggle_Grid_3D" => self.grille_3d,
            "Tools_Toggle_Gizmo_Orientation" => self.gizmo_local,
            "Tools_Toggle_Pivot" => self.pivot_origine,
            "View_Fog_Preview" => self.apercu_brouillard,
            "View_Show_Models_In_2D" => self.modeles_2d,
            "View_2D_Logical" => self.logique.ouverte,
            "View3D_Light_Bake" => self.eclairage.actif,
            _ => return None,
        })
    }

    /// Une entrée de menu : le nom de l'action, sa touche à droite, mise en avant si c'est une bascule active.
    fn entree(&mut self, ui: &mut Ui, id: &'static str) {
        let mut b = egui::Button::new(nom(id)).shortcut_text(self.rc.texte(id));
        if let Some(v) = self.bascule(id) {
            b = b.selected(v);
        }
        if ui.add_enabled(self.possible(id), b).clicked() {
            self.declencher(ui.ctx(), id);
            ui.close();
        }
    }

    pub(super) fn barre_outils(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (k, groupe) in BARRE.iter().enumerate() {
                if k > 0 {
                    ui.separator();
                }
                for &(icone, secours, id) in *groupe {
                    if id == GRILLE_TAILLE {
                        ui.add_sized([30.0, 30.0], egui::Label::new(format!("{}", self.grille))).on_hover_text(crate::langue::t("taille de la grille ([ et ])"));
                        continue;
                    }
                    let icone = if id == "Tools_Toggle_Texture_Lock" && !self.verrou_texture { ic::VERROU_OUVERT } else { icone };
                    if ic::bouton(ui, icone, crate::langue::t(secours), self.bascule(id).unwrap_or(false), self.possible(id), &self.aide(id)).clicked() {
                        self.declencher(ui.ctx(), id);
                    }
                }
            }
        });
    }

    /// Le panneau des outils (à gauche), en icônes sur une colonne.
    pub(super) fn panneau_outils(&mut self, ui: &mut Ui) {
        ui.vertical_centered(|ui| {
            for (o, icone, a) in OUTILS {
                if ic::bouton(ui, icone, o.nom(), self.outil == o, true, &self.aide(a)).clicked() {
                    if o == Outil::Sommets && self.outil == Outil::Sommets {
                        self.basculer_sommets();
                    } else if o == Outil::Decoupe && self.outil == Outil::Decoupe {
                        self.basculer_decoupe();
                    } else {
                        self.outil = o;
                    }
                }
            }
        });
    }

    /// Sous la barre d'outils : le nom de l'outil courant et ses réglages (bloc, découpe, sculpter).
    pub(super) fn bandeau_outil(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(self.outil.nom()).strong());
            match self.outil {
                Outil::Bloc | Outil::Decoupe => {
                    ui.separator();
                    self.reglages_outil(ui);
                }
                Outil::Sculpter => {
                    ui.separator();
                    self.reglages_sculpture(ui);
                }
                Outil::Semer => {
                    ui.separator();
                    self.reglages_semis(ui);
                }
                _ => {}
            }
        });
    }

    /// Les onglets du panneau de droite, en icônes ; le nom de l'onglet ouvert dessous. Le chevron, ou un clic sur
    /// l'onglet ouvert, réduit le panneau à une colonne d'icônes.
    pub(super) fn onglets(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (o, icone, n) in ONGLETS.map(|(o, i, n)| (o, i, crate::langue::t(n))) {
                if ic::bouton(ui, icone, n, self.onglet == o, true, n).clicked() {
                    if self.onglet == o {
                        self.panneau_reduit = true;
                    }
                    self.onglet = o;
                }
            }
            if ic::bouton(ui, ic::REDUIRE, "»", false, true, &self.aide("View_Toggle_Side_Panel")).clicked() {
                self.panneau_reduit = true;
            }
        });
        if let Some((_, _, n)) = ONGLETS.iter().find(|(o, ..)| *o == self.onglet) {
            ui.label(egui::RichText::new(crate::langue::t(n)).strong());
        }
    }

    /// Le panneau de droite réduit : les onglets en colonne ; un clic l'ouvre sur l'onglet choisi.
    pub(super) fn onglets_reduits(&mut self, ui: &mut Ui) {
        ui.vertical_centered(|ui| {
            if ic::bouton(ui, ic::AGRANDIR_PANNEAU, "«", false, true, crate::langue::t("Rouvrir le panneau de droite")).clicked() {
                self.panneau_reduit = false;
            }
            ui.separator();
            for (o, icone, n) in ONGLETS.map(|(o, i, n)| (o, i, crate::langue::t(n))) {
                if ic::bouton(ui, icone, n, self.onglet == o, true, n).clicked() {
                    self.montrer_onglet(o);
                }
            }
        });
    }

    /// Ouvre le panneau de droite sur l'onglet `o` (à la demande de l'utilisateur : rouvre le panneau réduit).
    pub(super) fn montrer_onglet(&mut self, o: Onglet) {
        self.onglet = o;
        self.panneau_reduit = false;
    }

    pub(super) fn menus(&mut self, ui: &mut Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button(crate::langue::t("Fichier"), |ui| {
                self.entree(ui, "File_New");
                self.entree(ui, "File_Open");
                ui.menu_button(crate::langue::t("Fichiers récents"), |ui| {
                    let recents = crate::recents::lire();
                    if recents.is_empty() {
                        ui.weak(crate::langue::t("aucun"));
                    }
                    for p in recents {
                        if ui.button(p.display().to_string()).clicked() {
                            self.ouvrir(&p);
                            ui.close();
                        }
                    }
                });
                self.entree(ui, "File_Save");
                self.entree(ui, "File_Save_As");
                ui.separator();
                for id in ["File_Export", "File_Export_Selection", "File_Export_Again", "File_Export_DXF"] {
                    self.entree(ui, id);
                }
                ui.separator();
                self.entree(ui, "File_Run_Map");
                ui.separator();
                if ui.button(crate::langue::t("Quitter")).clicked() {
                    ui.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button(crate::langue::t("Édition"), |ui| {
                let h = self.session.historique();
                let (fait, annule) = (h.faits().last().map(|c| c.nom.clone()), h.annules().last().map(|c| c.nom.clone()));
                if ui.add_enabled(fait.is_some(), egui::Button::new(tr!("Annuler {}", "Undo {}", fait.unwrap_or_default())).shortcut_text(self.rc.texte("Edit_Undo"))).clicked() {
                    self.declencher(ui.ctx(), "Edit_Undo");
                    ui.close();
                }
                if ui.add_enabled(annule.is_some(), egui::Button::new(tr!("Rétablir {}", "Redo {}", annule.unwrap_or_default())).shortcut_text(self.rc.texte("Edit_Redo"))).clicked() {
                    self.declencher(ui.ctx(), "Edit_Redo");
                    ui.close();
                }
                ui.separator();
                for id in ["Edit_Cut", "Edit_Copy", "Edit_Paste", "Edit_Paste_Special", "Edit_Duplicate_Selection", "Tool_Selection_Delete"] {
                    self.entree(ui, id);
                }
                ui.separator();
                for id in ["Edit_Select_All", "View_Clear_Selection"] {
                    self.entree(ui, id);
                }
                ui.separator();
                for id in ["Edit_Find_Entities", "Edit_Replace", "Edit_Properties"] {
                    self.entree(ui, id);
                }
            });
            ui.menu_button(crate::langue::t("Carte"), |ui| {
                self.entree(ui, "Map_Snap_To_Grid");
                self.entree(ui, "View_Toggle_Grid");
                ui.menu_button(tr!("Taille de la grille ({})", "Grid size ({})", self.grille), |ui| {
                    self.entree(ui, "Map_Grid_Lower");
                    self.entree(ui, "Map_Grid_Higher");
                    ui.separator();
                    for k in 0..11 {
                        let g = (1u32 << k) as f32;
                        if ui.add(egui::Button::selectable(self.grille == g, format!("{g}"))).clicked() {
                            self.grille = g;
                            ui.close();
                        }
                    }
                });
                ui.menu_button(crate::langue::t("Unités des cotes"), |ui| {
                    for (k, n) in [crate::langue::t("Unités"), crate::langue::t("Mètres (1 unité = 1 pouce)"), crate::langue::t("Pieds et pouces")].iter().enumerate() {
                        if ui.add(egui::Button::selectable(self.carte.unites == k as u8, *n)).clicked() {
                            self.carte.unites = k as u8;
                            ui.close();
                        }
                    }
                });
                ui.separator();
                for id in ["Map_Entity_Report", "Map_Entity_Gallery", "Map_Diff"] {
                    self.entree(ui, id);
                }
                ui.separator();
                self.entree(ui, "Map_Check_For_Problems");
                self.entree(ui, "Tools_Select_Invalid");
                ui.separator();
                for id in ["Map_Load_Pointfile", "Map_Unload_Pointfile", "Map_Load_Portalfile", "Map_Unload_Portalfile"] {
                    self.entree(ui, id);
                }
                ui.separator();
                self.entree(ui, "Map_Show_Information");
                self.entree(ui, "Map_Properties");
                ui.separator();
                let c = self.session.doc().cordons();
                if ui.add_enabled(!self.session.selection.is_empty(), egui::Button::new(crate::langue::t("Cordon depuis la sélection"))).on_hover_text(crate::langue::t("le cordon choisi dans l'onglet Visgroups › Cordon prend la boîte de la sélection")).clicked() {
                    self.cordon_depuis_selection();
                    ui.close();
                }
                let mut actif = c.actif;
                if ui.add_enabled(!c.liste.is_empty(), egui::Checkbox::new(&mut actif, crate::langue::t("Cordon actif"))).changed() {
                    self.executer(Commande::Cordons { actif, liste: c.liste });
                }
            });
            ui.menu_button(crate::langue::t("Affichage"), |ui| {
                ui.menu_button(crate::langue::t("Vue active"), |ui| {
                    for id in ["View_3D_Wireframe", "View_3D_Polygon", "View_3D_Textured", "View_3D_Textured_Shaded", "View_3D_Lighting_Preview"] {
                        self.entree(ui, id);
                    }
                    ui.separator();
                    for id in ["View_2D_X_Y", "View_2D_Y_Z", "View_2D_X_Z", "View_2D_Logical"] {
                        self.entree(ui, id);
                    }
                });
                for id in ["View_Autosize4", "View_Maximize_Restore_Active_View", "View_Toggle_Side_Panel", "View3D_Toggle_Statistics"] {
                    self.entree(ui, id);
                }
                ui.separator();
                for id in ["View_Center_On_Selection", "View_Center_3D_Views_On_Selection", "View_Fit_Map", "View_Goto_Coordinates", "View_Goto_Brush"] {
                    self.entree(ui, id);
                }
                ui.separator();
                for id in ["View_Quickhide", "View_Quickhide_Unselected", "View_Quick_Unhide", "View_Quickhide_To_Visgroup"] {
                    self.entree(ui, id);
                }
                self.menu_deplacer_vers_visgroup(ui);
                if ui.button(crate::langue::t("Visgroups…")).clicked() {
                    self.montrer_onglet(Onglet::Visgroups);
                    ui.close();
                }
                ui.separator();
                self.menu_eclairage(ui);
                for id in ["View_Toggle_3D_Skybox", "View_Toggle_Tool_Textures", "View_Toggle_Helpers", "View_Toggle_Editor_Objects", "View_Show_Connections", "Tools_Hide_Entity_Names", "View3D_Toggle_Grid_3D", "View_Fog_Preview", "View_Background_Images", "View_Show_Models_In_2D"] {
                    self.entree(ui, id);
                }
                self.entree(ui, "View_Live_Validation");
                self.entree(ui, "View_Walkable_Layer");
            });
            ui.menu_button(crate::langue::t("Outils"), |ui| {
                for id in ["Tools_Carve", "Tools_Merge"] {
                    self.entree(ui, id);
                }
                ui.horizontal(|ui| {
                    self.entree(ui, "Tools_Make_Hollow");
                    ui.label(crate::langue::t("murs de"));
                    ui.add(egui::DragValue::new(&mut self.epaisseur_murs).range(1.0..=512.0).speed(1.0));
                });
                ui.separator();
                for id in ["Tools_Group", "Tools_Ungroup", "Edit_Toggle_Group_Ignore", "Edit_To_Entity", "Edit_To_Entity_Individually", "Edit_To_World", "Tools_Center_Origins"] {
                    self.entree(ui, id);
                }
                ui.separator();
                for id in ["Tools_Texture_Application", "Edit_Apply_Texture", "Edit_Replace", "Tools_Toggle_Texture_Lock"] {
                    self.entree(ui, id);
                }
                for id in ["Tools_Model_Browser", "View_Texture_Browser", "Tools_Sound_Browser", "Tools_Particle_Browser"] {
                    self.entree(ui, id);
                }
                ui.separator();
                for id in ["Tools_Toggle_Gizmo_Orientation", "Tools_Toggle_Pivot"] {
                    self.entree(ui, id);
                }
                for id in ["Tools_Transform", "Tools_Vertex_Scale", "Tools_Snap_Selected_To_Grid", "Tools_Snap_Selected_To_Grid_Individually"] {
                    self.entree(ui, id);
                }
                ui.menu_button(crate::langue::t("Aligner"), |ui| {
                    for id in ["Tools_Align_Left", "Tools_Align_Right", "Tools_Align_Top", "Tools_Align_Bottom"] {
                        self.entree(ui, id);
                    }
                });
                ui.menu_button(crate::langue::t("Miroir"), |ui| {
                    for id in ["Edit_Flip_Horizontal", "Edit_Flip_Vertical"] {
                        self.entree(ui, id);
                    }
                });
                for id in ["Tools_Move_To_Floor", "Tools_Move_To_Floor_Align", "Tools_Move_To_Ceiling", "Tools_Move_To_Ceiling_Align"] {
                    self.entree(ui, id);
                }
                ui.add_enabled_ui(!self.session.selection.is_empty(), |ui| {
                    ui.menu_button(crate::langue::t("Couleur des objets"), |ui| {
                        ui.horizontal(|ui| {
                            egui::color_picker::color_edit_button_srgb(ui, &mut self.couleur_objets);
                            if ui.button(crate::langue::t("Au hasard")).clicked() {
                                let h = (self.session.version() as u64 ^ 0x9E37_79B9).wrapping_mul(0x2545_F491_4F6C_DD1D);
                                self.couleur_objets = [(h >> 8) as u8 | 48, (h >> 24) as u8 | 48, (h >> 40) as u8 | 48];
                            }
                        });
                        if ui.button(crate::langue::t("Appliquer à la sélection")).clicked() {
                            let objets = self.session.selection.clone();
                            self.executer(Commande::CouleurEditeur { objets, couleur: self.couleur_objets });
                            self.version_geo = None;
                            ui.close();
                        }
                    });
                });
                ui.separator();
                self.entree(ui, "Tools_Create_Prefab");
                self.entree(ui, "Tools_Propper_HM");
                ui.separator();
                for id in ["Palette_Commandes", "Tools_Keybinds", "Tools_Options"] {
                    self.entree(ui, id);
                }
            });
            ui.menu_button(crate::langue::t("Instances"), |ui| {
                for id in ["Tools_Create_Instance_From_Selection", "Instancing_Collapse_Selection", "Instancing_Collapse_All"] {
                    self.entree(ui, id);
                }
            });
            ui.menu_button(crate::langue::t("Compiler"), |ui| {
                self.entree(ui, "File_Run_Map");
                self.entree(ui, "Compile_Zone");
            });
            ui.menu_button(crate::langue::t("Aide"), |ui| {
                self.entree(ui, "Tools_Keybinds");
                self.entree(ui, "Palette_Commandes");
                ui.separator();
                if ui.button(crate::langue::t("Documentation de Hammer (wiki Valve)")).clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab("https://developer.valvesoftware.com/wiki/Category:Hammer"));
                    ui.close();
                }
                let limites = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/limites_et_dimensions.md");
                if ui.add_enabled(limites.is_file(), egui::Button::new(crate::langue::t("Limites et dimensions du moteur"))).clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(format!("file:///{}", limites.display().to_string().replace('\\', "/"))));
                    ui.close();
                }
                ui.separator();
                if ui.button(crate::langue::t("À propos de Hammer Master")).clicked() {
                    self.dialogues.a_propos = true;
                    ui.close();
                }
            });
        });
    }

    /// Les actions des menus que la table des raccourcis n'avait pas encore (même chemin qu'une touche).
    pub(super) fn raccourcis_menus(&mut self, ctx: &egui::Context, rc: &Raccourcis, d: Option<&str>) {
        let p = |a: &str| d == Some(a) || ctx.input(|i| rc.presse(a, i));
        // presse-papiers depuis un bouton ou un menu (au clavier, egui en fait déjà des événements)
        for (a, c) in [("Edit_Cut", egui::ViewportCommand::RequestCut), ("Edit_Copy", egui::ViewportCommand::RequestCopy), ("Edit_Paste", egui::ViewportCommand::RequestPaste)] {
            if d == Some(a) {
                ctx.send_viewport_cmd(c);
            }
        }
        let sel = self.session.selection.clone();
        if p("File_Save_As") {
            self.enregistrer(true);
        }
        if p("Edit_Paste_Special") && self.dialogues.coller.is_none() {
            // le texte arrive au prochain collage (Ctrl+Maj+V n'est pas un « coller » pour egui)
            self.attente_coller_special = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
        }
        if p("File_Export") {
            self.exporter(None, false);
        }
        if p("File_Export_Selection") && !sel.is_empty() {
            self.exporter(None, true);
        }
        if p("File_Export_Again") {
            if let Some((f, s)) = self.dernier_export.clone() {
                self.exporter(Some(f), s);
            }
        }
        if p("File_Export_DXF") {
            if let Some(f) = rfd::FileDialog::new().add_filter("DXF", &["dxf"]).set_file_name(format!("{}.dxf", self.nom_compile)).save_file() {
                match std::fs::write(&f, hm_edit::dxf::dxf(self.session.doc())) {
                    Ok(()) => self.statut = tr!("exporté en DXF : {}", "exported as DXF: {}", f.display()),
                    Err(e) => self.erreur = Some(format!("{} : {e}", f.display())),
                }
            }
        }
        if p("Edit_Select_All") {
            self.tout_selectionner();
        }
        if p("Edit_Find_Entities") {
            self.ouvrir_palette();
        }
        if p("Edit_Replace") {
            self.remplacer = Some((String::new(), self.materiau.clone(), false));
        }
        if p("Tools_Make_Hollow") && !sel.is_empty() {
            self.executer(Commande::Evider { objets: sel.clone(), epaisseur: self.epaisseur_murs });
        }
        if p("Tools_Transform") && !sel.is_empty() {
            self.dialogues.transformer = Some((0, [0.0; 3]));
        }
        if p("Tools_Snap_Selected_To_Grid_Individually") {
            let c = disposition::caler_chacun(self.session.doc(), &sel, self.grille as f64, self.verrou_texture);
            self.executer_ou(c, crate::langue::t("déjà sur la grille"));
        }
        let (a, b, _) = self.vues[self.active].genre.axes();
        for (id, vertical, vers_max) in [("Tools_Align_Left", false, false), ("Tools_Align_Right", false, true), ("Tools_Align_Top", true, true), ("Tools_Align_Bottom", true, false)] {
            if p(id) {
                let c = disposition::aligner(self.session.doc(), &sel, if vertical { b } else { a }, vers_max, self.verrou_texture);
                self.executer_ou(c, crate::langue::t("déjà alignés"));
            }
        }
        for (id, aligner, plafond) in [("Tools_Move_To_Floor", false, false), ("Tools_Move_To_Floor_Align", true, false), ("Tools_Move_To_Ceiling", false, true), ("Tools_Move_To_Ceiling_Align", true, true)] {
            if p(id) {
                self.poser_selection(aligner, plafond);
            }
        }
        if p("Edit_To_Entity_Individually") && !sel.is_empty() {
            // comme « Tie to Entity » objet par objet : chacun sa func_detail, la classe se change ensuite
            // (les solides et les groupes ; une entité déjà là reste telle quelle)
            let commandes: Vec<Commande> = sel.iter().filter(|r| !matches!(r, Ref::Entite(_))).map(|r| Commande::LierAEntite { objets: vec![*r], classe: "func_detail".into() }).collect();
            if commandes.is_empty() {
                self.statut = crate::langue::t("aucun solide ni groupe sélectionné").into();
            } else {
                self.executer(Commande::Lot { nom: tr!("lier {} objet(s) chacun à une entité", "tie {} object(s) each to an entity", commandes.len()), commandes });
                self.montrer_onglet(Onglet::Proprietes);
            }
        }
        if p("Tools_Center_Origins") {
            self.centrer_origines();
        }
        if p("Edit_Duplicate_Selection") && !sel.is_empty() {
            let g = self.grille as f64;
            self.executer(Commande::Dupliquer { objets: sel.clone(), delta: [g, g, 0.0] });
        }
        if p("Tools_Select_Invalid") {
            self.selectionner_invalides();
        }
        if p("Tools_Keybinds") {
            self.editeur_raccourcis.ouvert = true;
        }
        if p("Tools_Options") {
            self.options_ouvertes = true;
        }
        if p("View_Goto_Coordinates") {
            let c = self.vues.iter().find(|v| v.genre == Genre::Camera3D).map_or([0.0; 3], |v| v.cam.pos.as_dvec3().round().to_array());
            self.dialogues.coordonnees = Some(c);
        }
        if p("View_Goto_Brush") {
            self.dialogues.brosse = Some(String::new());
        }
        if p("View_Fit_Map") {
            if let Some(geo) = &self.geo {
                let (lo, hi) = (geo.min, geo.max);
                for v in &mut self.vues {
                    v.cadrer(lo, hi);
                }
            }
        }
        for (id, mode) in [("View_3D_Textured", 1), ("View_3D_Textured_Shaded", 0), ("View_3D_Polygon", 2), ("View_3D_Wireframe", 3), ("View_3D_Lighting_Preview", 0)] {
            if p(id) {
                // comme Hammer : la vue active devient une vue 3D dans ce mode (l'aperçu de l'éclairage est un mode)
                self.vues[self.active].genre = Genre::Camera3D;
                self.vues[self.active].cibles = None;
                self.rendu_3d = mode;
                self.eclairage.actif = id == "View_3D_Lighting_Preview";
            }
        }
        if p("View_Toggle_3D_Skybox") {
            self.ciel.montrer = !self.ciel.montrer;
        }
        if p("View_Toggle_Tool_Textures") {
            self.afficher_outils = !self.afficher_outils;
        }
        if p("View_Toggle_Helpers") {
            self.afficher_aides = !self.afficher_aides;
        }
        if p("View_Toggle_Editor_Objects") {
            self.afficher_icones = !self.afficher_icones;
            // les icônes sont dans la géométrie de la vue 3D
            self.version_geo = None;
        }
        if p("Tools_Toggle_Gizmo_Orientation") {
            self.gizmo_local = !self.gizmo_local;
            self.statut = tr!("manipulateur 3D : repère {}", "3D gizmo: {} axes", if self.gizmo_local { crate::langue::t("de l'entité") } else { crate::langue::t("du monde") });
        }
        if p("Tools_Toggle_Pivot") {
            self.pivot_origine = !self.pivot_origine;
            self.statut = tr!("rotation autour {}", "rotation around {}", if self.pivot_origine { crate::langue::t("de l'origine de l'entité") } else { crate::langue::t("du milieu de la sélection") });
        }
        if p("Tools_Vertex_Scale") {
            if self.outil == Outil::Sommets && !self.sommets.choisis.is_empty() {
                self.dialogues.echelle_sommets = Some(1.0);
            } else {
                self.statut = crate::langue::t("choisir des sommets avec l'outil sommets (Maj+V) d'abord").into();
            }
        }
        if p("Tools_Propper_HM") && !sel.is_empty() {
            self.ouvrir_propper();
        }
        if p("Tools_Polygon") {
            self.outil = Outil::Polygone;
        }
        if p("Tool_Exit") && self.outil == Outil::Polygone {
            self.abandonner_polygone();
        }
        if p("Tools_Sprinkle") {
            self.outil = Outil::Semer;
        }
        if p("Tools_Magnify") {
            self.outil = Outil::Loupe;
        }
        for (id, pas) in [("View_Next_Pane", 1), ("View_Prev_Pane", 3)] {
            if p(id) {
                // la vue agrandie passe à la suivante ; sinon la vue active (jusqu'au prochain survol)
                let n = (self.agrandie.unwrap_or(self.active) + pas) % 4;
                if self.agrandie.is_some() {
                    self.agrandie = Some(n);
                }
                self.active = n;
                self.statut = tr!("vue active : {}", "active view: {}", if self.vues[n].genre == Genre::Camera3D { "3D" } else { self.vues[n].genre.nom() });
            }
        }
        if p("View_2D_Logical") {
            self.logique.ouverte = !self.logique.ouverte;
        }
        if p("View3D_Light_Bake") {
            // Hammer++ cuit l'éclairage à la demande ; ici l'aperçu se calcule en direct
            self.eclairage.actif = !self.eclairage.actif;
        }
        if p("View_Show_Models_In_2D") {
            self.modeles_2d = !self.modeles_2d;
            self.version_geo = None;
        }
        if p("View_Background_Images") {
            self.fonds.dialogue = true;
        }
        if p("View3D_Color_Picker") {
            self.pipette(ctx);
        }
        if p("View3D_Toggle_Grid_3D") {
            self.grille_3d = !self.grille_3d;
        }
        if p("View_Fog_Preview") {
            self.apercu_brouillard = !self.apercu_brouillard;
            if self.apercu_brouillard && self.brouillard_de_la_map().is_none() {
                self.statut = crate::langue::t("aucun env_fog_controller avec fogenable dans la map").into();
            }
        }
        if p("View3D_Toggle_Statistics") {
            self.afficher_stats = !self.afficher_stats;
        }
        for (id, k) in [("View3D_Render_Distance_Closer", 1.0 / 1.5), ("View3D_Render_Distance_Further", 1.5)] {
            if p(id) {
                self.options.distance = (self.options.distance * k).clamp(1000.0, 200000.0);
                self.statut = tr!("distance de rendu : {:.0}", "render distance: {:.0}", self.options.distance);
            }
        }
        for (id, k, toutes) in [("View2D_Zoom_In", 1.5, false), ("View2D_Zoom_Out", 1.0 / 1.5, false), ("View2D_Zoom_In_All", 1.5, true), ("View2D_Zoom_Out_All", 1.0 / 1.5, true)] {
            if p(id) {
                let active = self.active;
                for (j, v) in self.vues.iter_mut().enumerate() {
                    if v.genre != Genre::Camera3D && (toutes || j == active) {
                        v.zoom = (v.zoom * k).clamp(0.001, 64.0);
                    }
                }
            }
        }
        if p("View_Show_Connections") {
            self.affichage.liens = !self.affichage.liens;
        }
        if p("Tools_Hide_Entity_Names") {
            self.affichage.cacher_noms = !self.affichage.cacher_noms;
        }
        if p("View_Quickhide_To_Visgroup") {
            self.masquage_en_visgroup();
        }
        if p("View_Toggle_Side_Panel") {
            self.panneau_reduit = !self.panneau_reduit;
        }
        if p("View_Live_Validation") {
            self.direct.actif = !self.direct.actif;
        }
        if p("Tools_Create_Instance_From_Selection") && !sel.is_empty() {
            self.creer_instance();
        }
        if p("Instancing_Collapse_Selection") {
            self.fondre_instances(false);
        }
        if p("Instancing_Collapse_All") {
            self.fondre_instances(true);
        }
        if p("Tools_Sound_Browser") {
            self.navigateurs.sons = true;
        }
        if p("Tools_Particle_Browser") {
            self.navigateurs.particules = true;
        }
        if p("Tools_Model_Browser") {
            self.montrer_onglet(Onglet::Modeles);
        }
        if p("View_Texture_Browser") {
            self.navigateur.fenetre.ouverte = true;
        }
        if p("Map_Entity_Report") {
            self.carte.rapport = true;
        }
        if p("Map_Entity_Gallery") {
            self.galerie_entites();
        }
        if p("Map_Diff") {
            self.difference_avec_un_fichier();
        }
        if p("Map_Show_Information") {
            self.dialogues.infos = Some(disposition::infos(self.session.doc()));
        }
        if p("Map_Properties") {
            // le panneau Propriétés montre le monde quand rien n'est sélectionné
            self.session.selection.clear();
            self.outil = Outil::Selection;
            self.montrer_onglet(Onglet::Proprietes);
        }
        if p("Map_Load_Pointfile") {
            if let Some(f) = self.fichier_de_compilation("lin") {
                match hm_compile::lire_fuite(&f) {
                    Some(points) => {
                        self.statut = tr!("fuite : {} points ({})", "leak: {} points ({})", points.len(), f.display());
                        self.fuite = Some(points);
                        self.version_geo = None;
                    }
                    None => self.erreur = Some(tr!("fichier de fuite illisible : {}", "unreadable leak file: {}", f.display())),
                }
            }
        }
        if p("Map_Unload_Pointfile") {
            self.fuite = None;
            self.version_geo = None;
        }
        if p("Map_Load_Portalfile") {
            if let Some(f) = self.fichier_de_compilation("prt") {
                match hm_compile::lire_portails(&f) {
                    Some(portails) => {
                        self.statut = tr!("{} portails ({})", "{} portals ({})", portails.len(), f.display());
                        self.portails = Some(portails);
                        self.portails_visibles = true;
                    }
                    None => self.erreur = Some(tr!("fichier de portails illisible : {}", "unreadable portal file: {}", f.display())),
                }
            }
        }
        if p("Map_Unload_Portalfile") {
            self.portails = None;
        }
    }

    /// « Center Origins » : l'origine de chaque entité à brosses sélectionnée qui en a une revient au milieu de sa boîte.
    fn centrer_origines(&mut self) {
        let doc = self.session.doc();
        let Ok((_, ents, _)) = hm_doc::resoudre(doc, &self.session.selection) else { return };
        let mut commandes = Vec::new();
        for id in ents {
            let Some(e) = doc.entite(id).filter(|e| !e.solides.is_empty() && e.get("origin").is_some()) else { continue };
            let Some((lo, hi)) = hm_doc::boite_de(doc, &[Ref::Entite(e.id)]) else { continue };
            let c = (lo + hi) * 0.5;
            commandes.push(Commande::Cles { entite: id, cles: vec![("origin".into(), Some(format!("{} {} {}", c.x, c.y, c.z)))] });
        }
        match commandes.len() {
            0 => self.statut = crate::langue::t("aucune entité à brosses avec une origine dans la sélection").into(),
            n => self.executer(Commande::Lot { nom: tr!("centrer {n} origine(s)", "center {n} origin(s)"), commandes }),
        }
    }

    /// « Export… » : une copie de la map (ou de la sélection, coordonnées gardées) dans un autre fichier, sans changer
    /// celui de la map ouverte ; `vers` : refaire le dernier export (« Export Again », Alt+B).
    fn exporter(&mut self, vers: Option<PathBuf>, selection: bool) {
        let Some(f) = vers.or_else(|| rfd::FileDialog::new().add_filter(crate::langue::t("Map Hammer"), &["vmf"]).set_file_name(format!("{}_export.vmf", self.nom_compile)).save_file()) else { return };
        let octets = if selection {
            match hm_doc::presse::copier(self.session.doc(), &self.session.selection) {
                Ok(t) => t.into_bytes(),
                Err(e) => {
                    self.erreur = Some(tr!("export : {e}", "export: {e}"));
                    return;
                }
            }
        } else {
            self.session.doc().ecrire()
        };
        match hm_doc::session::ecrire_sur(&f, &octets) {
            Ok(()) => {
                self.statut = tr!("exporté{} : {}", "exported{}: {}", if selection { crate::langue::t(" (sélection)") } else { "" }, f.display());
                self.dernier_export = Some((f, selection));
            }
            Err(e) => self.erreur = Some(e),
        }
    }

    /// Le brouillard du premier env_fog_controller allumé (wiki Valve : fogcolor, fogstart, fogend, fogmaxdensity).
    pub(super) fn brouillard_de_la_map(&self) -> Option<([f32; 3], f32, f32, f32)> {
        let e = self.session.doc().entites().find(|e| e.classe().eq_ignore_ascii_case("env_fog_controller") && e.get("fogenable").map_or(false, |v| v.trim() != "0"))?;
        let nombre = |k: &str, d: f32| e.get(k).and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(d);
        let c: Vec<f32> = e.get("fogcolor").unwrap_or("255 255 255").split_whitespace().filter_map(|x| x.parse::<f32>().ok()).collect();
        let couleur = [c.first().copied().unwrap_or(255.0) / 255.0, c.get(1).copied().unwrap_or(255.0) / 255.0, c.get(2).copied().unwrap_or(255.0) / 255.0];
        Some((couleur, nombre("fogstart", 500.0), nombre("fogend", 2000.0), nombre("fogmaxdensity", 1.0).clamp(0.0, 1.0)))
    }

    /// La pipette (C dans la vue 3D, comme Hammer++) : la couleur du pixel sous le curseur va dans les clés de
    /// couleur des entités sélectionnées (`_light` garde sa luminosité) ; sans elles, dans le presse-papiers.
    fn pipette(&mut self, ctx: &egui::Context) {
        let Some(p) = ctx.pointer_hover_pos() else { return };
        let Some(i) = self.vues.iter().position(|v| v.genre == Genre::Camera3D && v.rect.contains(p)) else {
            self.statut = crate::langue::t("pipette : survoler la vue 3D").into();
            return;
        };
        let (Some(g), Some((c, _))) = (&self.gfx, &self.vues[i].cibles) else { return };
        let ppp = ctx.pixels_per_point();
        let r = self.vues[i].rect;
        let px = g.r.couleur_sous(c, ((p.x - r.left()) * ppp) as u32, ((p.y - r.top()) * ppp) as u32);
        let rgb = format!("{} {} {}", px[0], px[1], px[2]);
        let doc = self.session.doc();
        let ents = hm_doc::resoudre(doc, &self.session.selection).map(|x| x.1).unwrap_or_default();
        let mut commandes = Vec::new();
        for id in ents {
            let Some(e) = doc.entite(id) else { continue };
            for k in ["_light", "_ambient", "rendercolor", "fogcolor", "fogcolor2", "color"] {
                if let Some(v) = e.get(k) {
                    // la 4e valeur (luminosité de _light) est gardée
                    let reste: Vec<&str> = v.split_whitespace().skip(3).collect();
                    let nv = if reste.is_empty() { rgb.clone() } else { tr!("{rgb} {}", "{rgb} {}", reste.join(" ")) };
                    commandes.push(Commande::Cles { entite: id, cles: vec![(k.to_string(), Some(nv))] });
                }
            }
        }
        if commandes.is_empty() {
            ctx.copy_text(rgb.clone());
            self.statut = tr!("pipette : {rgb} (copiée ; aucune entité sélectionnée n'a de couleur)", "eyedropper: {rgb} (copied; no selected entity has a color)");
        } else {
            self.executer(Commande::Lot { nom: tr!("pipette : {rgb}", "eyedropper: {rgb}"), commandes });
        }
    }

    fn executer_ou(&mut self, c: Option<Commande>, rien: &str) {
        match c {
            Some(c) => self.executer(c),
            None => self.statut = rien.into(),
        }
    }

    /// Comme « Select All » : tout ce qui est visible, groupes compris (sauf s'ils sont ignorés).
    fn tout_selectionner(&mut self) {
        let mut choix: Vec<Ref> = Vec::new();
        if let Some(geo) = &self.geo {
            for f in &geo.formes {
                let r = self.a_selectionner(f.designe);
                if !choix.contains(&r) {
                    choix.push(r);
                }
            }
        }
        self.statut = tr!("sélection : {} objet(s)", "selection: {} object(s)", choix.len());
        self.session.selection = choix;
    }

    /// Le `.lin` ou le `.prt` de la dernière compilation de la map (dossier de compilation, puis à côté du VMF),
    /// sinon celui que l'on choisit (comme Hammer).
    fn fichier_de_compilation(&self, ext: &str) -> Option<PathBuf> {
        let mut chemins = vec![hm_compile::dossier_compile(self.session.chemin()).join(format!("{}.{ext}", self.nom_compile))];
        if let Some(v) = self.session.chemin() {
            chemins.push(v.with_extension(ext));
        }
        chemins.into_iter().find(|p| p.is_file()).or_else(|| rfd::FileDialog::new().add_filter(ext, &[ext]).pick_file())
    }

    pub(super) fn ouvrir_coller_special(&mut self, vmf: String) {
        let taille = hm_doc::Document::lire(vmf.as_bytes()).ok().and_then(|(frag, _)| {
            let tous: Vec<Ref> = frag.solides_monde().map(|s| Ref::Solide(s.id)).chain(frag.entites().map(|e| Ref::Entite(e.id))).collect();
            hm_doc::boite_de(&frag, &tous)
        });
        let Some((lo, hi)) = taille else {
            self.erreur = Some(crate::langue::t("le presse-papiers ne contient pas d'objets de map").into());
            return;
        };
        self.dialogues.coller = Some(CollerSpecial { vmf, taille: (hi - lo).to_array(), copies: 1, decalage: [0.0; 3], rotation: [0.0; 3], grouper: false, uniques: false, prefixe: String::new() });
    }

    /// Les dialogues des menus.
    pub(super) fn dialogues_menus(&mut self, ctx: &egui::Context) {
        if let Some(mut k) = self.dialogues.echelle_sommets.take() {
            let (mut ouvert, mut faire) = (true, false);
            fenetre(ctx, crate::langue::t("Sommets à l'échelle")).resizable(false).open(&mut ouvert).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(crate::langue::t("facteur"));
                    ui.add(egui::DragValue::new(&mut k).speed(0.01).range(0.01..=100.0));
                });
                ui.weak(crate::langue::t("autour du centre des sommets choisis"));
                faire = ui.button(crate::langue::t("Appliquer")).clicked();
            });
            if faire {
                self.echelle_sommets(k);
            } else if ouvert {
                self.dialogues.echelle_sommets = Some(k);
            }
        }
        if self.dialogues.a_propos {
            let mut ouvert = true;
            fenetre(ctx, crate::langue::t("À propos")).resizable(false).open(&mut ouvert).show(ctx, |ui| {
                ui.heading("Hammer Master");
                ui.label(tr!("version {}", "version {}", env!("CARGO_PKG_VERSION")));
                ui.label(crate::langue::t("Éditeur de maps Source / Garry's Mod : les VMF de Hammer et Hammer++, lus et écrits sans perte."));
                ui.weak(tr!("jeu : {}", "game: {}", self.gameinfo.display()));
                ui.weak(tr!("raccourcis : {}", "shortcuts: {}", self.rc.source));
            });
            self.dialogues.a_propos = ouvert;
        }
        if let Some(mut c) = self.dialogues.coller.take() {
            let (mut ouvert, mut faire) = (true, false);
            fenetre(ctx, crate::langue::t("Coller spécial")).resizable(false).open(&mut ouvert).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(crate::langue::t("Nombre de copies"));
                    ui.add(egui::DragValue::new(&mut c.copies).range(1..=1000));
                });
                ui.label(crate::langue::t("Décalage (cumulé)")).on_hover_text(crate::langue::t("chaque copie est décalée de cette valeur de plus que la précédente ; « < » prend la taille du contenu"));
                ui.horizontal(|ui| {
                    for i in 0..3 {
                        ui.label(["X", "Y", "Z"][i]);
                        ui.add(egui::DragValue::new(&mut c.decalage[i]).speed(1.0));
                        if ui.small_button("<").on_hover_text(tr!("taille du contenu : {}", "contents size: {}", c.taille[i])).clicked() {
                            c.decalage[i] = c.taille[i];
                        }
                    }
                });
                ui.label(crate::langue::t("Rotation (cumulée, degrés)"));
                ui.horizontal(|ui| {
                    for i in 0..3 {
                        ui.label(["X", "Y", "Z"][i]);
                        ui.add(egui::DragValue::new(&mut c.rotation[i]).speed(1.0));
                    }
                });
                ui.checkbox(&mut c.grouper, crate::langue::t("Grouper les copies"));
                ui.checkbox(&mut c.uniques, crate::langue::t("Noms d'entités uniques"));
                ui.horizontal(|ui| {
                    ui.label(crate::langue::t("Préfixe des noms"));
                    ui.add(egui::TextEdit::singleline(&mut c.prefixe).desired_width(120.0));
                });
                ui.weak(crate::langue::t("à partir de l'original (« Start at center of original » de Hammer) ; les sorties entre copies suivent les noms"));
                faire = ui.button(crate::langue::t("Coller")).clicked();
            });
            if faire {
                self.executer(Commande::CollerSpecial { vmf: c.vmf, copies: c.copies, decalage: c.decalage, rotation: c.rotation, grouper: c.grouper, prefixe: c.prefixe, noms_uniques: c.uniques });
            } else if ouvert {
                self.dialogues.coller = Some(c);
            }
        }
        if let Some((mut mode, mut v)) = self.dialogues.transformer.take() {
            let (mut ouvert, mut faire) = (true, false);
            fenetre(ctx, crate::langue::t("Transformer")).resizable(false).open(&mut ouvert).show(ctx, |ui| {
                let avant = mode;
                ui.horizontal(|ui| {
                    for (k, n) in [crate::langue::t("Tourner"), crate::langue::t("Échelle"), crate::langue::t("Déplacer")].iter().enumerate() {
                        ui.radio_value(&mut mode, k, *n);
                    }
                });
                if mode != avant {
                    v = if mode == 1 { [1.0; 3] } else { [0.0; 3] };
                }
                ui.horizontal(|ui| {
                    for (i, x) in v.iter_mut().enumerate() {
                        ui.label(["X", "Y", "Z"][i]);
                        ui.add(egui::DragValue::new(x).speed(if mode == 1 { 0.01 } else { 1.0 }).max_decimals(3));
                    }
                });
                ui.weak([crate::langue::t("degrés autour des axes du monde, X puis Y puis Z, sens direct, autour du centre de la sélection"), crate::langue::t("facteurs par axe, autour du centre de la sélection"), crate::langue::t("unités")][mode]);
                faire = ui.button(crate::langue::t("Appliquer")).clicked();
            });
            if faire {
                let t = [Transformation::Tourner(v), Transformation::Echelle(v), Transformation::Deplacer(v)][mode];
                let sel = self.session.selection.clone();
                match disposition::transformer(self.session.doc(), &sel, t, self.verrou_texture) {
                    Ok(c) => self.executer_ou(c, crate::langue::t("rien à transformer")),
                    Err(e) => self.erreur = Some(e),
                }
            }
            if ouvert && !faire {
                self.dialogues.transformer = Some((mode, v));
            }
        }
        if let Some(mut c) = self.dialogues.coordonnees.take() {
            let (mut ouvert, mut aller) = (true, false);
            fenetre(ctx, crate::langue::t("Aller aux coordonnées")).resizable(false).open(&mut ouvert).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (i, x) in c.iter_mut().enumerate() {
                        ui.label(["X", "Y", "Z"][i]);
                        ui.add(egui::DragValue::new(x).speed(8.0));
                    }
                });
                aller = ui.button(crate::langue::t("Aller")).clicked();
            });
            if aller {
                let p = glam::DVec3::from(c);
                self.cadrer_sur(p, p);
            }
            if ouvert && !aller {
                self.dialogues.coordonnees = Some(c);
            }
        }
        if let Some(mut t) = self.dialogues.brosse.take() {
            let (mut ouvert, mut aller) = (true, false);
            fenetre(ctx, crate::langue::t("Aller au numéro de brosse")).resizable(false).open(&mut ouvert).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("n°");
                    let r = ui.add(egui::TextEdit::singleline(&mut t).desired_width(90.0));
                    r.request_focus();
                    aller = ui.button(crate::langue::t("Aller")).clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                });
            });
            if aller {
                match t.trim().parse::<u64>().ok().filter(|id| self.session.doc().solide(*id).is_some()) {
                    Some(id) => {
                        self.aller_a(Ref::Solide(id), None);
                        self.statut = tr!("brosse n° {id}", "brush #{id}");
                    }
                    None => self.erreur = Some(tr!("aucune brosse n° {}", "no brush #{}", t.trim())),
                }
            }
            if ouvert && !aller {
                self.dialogues.brosse = Some(t);
            }
        }
        if let Some(i) = &self.dialogues.infos {
            let mut ouvert = true;
            fenetre(ctx, crate::langue::t("Informations sur la map")).open(&mut ouvert).default_width(360.0).show(ctx, |ui| {
                egui::Grid::new("infos_map").num_columns(2).show(ui, |ui| {
                    for (n, v) in [(crate::langue::t("Solides"), i.solides), (crate::langue::t("Faces"), i.faces), (crate::langue::t("Displacements"), i.displacements), (crate::langue::t("Entités ponctuelles"), i.entites_ponctuelles), (crate::langue::t("Entités à brosses"), i.entites_a_brosses), (crate::langue::t("Matériaux distincts"), i.materiaux.len())] {
                        ui.label(n);
                        ui.label(v.to_string());
                        ui.end_row();
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
                    for (m, n) in &i.materiaux {
                        ui.label(format!("{n:>6}  {m}"));
                    }
                });
            });
            if !ouvert {
                self.dialogues.infos = None;
            }
        }
    }

    /// Outils → Options ; enregistrées dès que la souris est relâchée après un changement.
    pub(super) fn fenetre_options(&mut self, ctx: &egui::Context) {
        if !self.options_ouvertes {
            return;
        }
        let mut ouvert = true;
        let mut o = self.options.clone();
        fenetre(ctx, crate::langue::t("Options")).resizable(false).open(&mut ouvert).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Langue / Language");
                egui::ComboBox::from_id_salt("langue").selected_text(if o.anglais { "English" } else { "Français" }).show_ui(ui, |ui| {
                    ui.selectable_value(&mut o.anglais, false, "Français");
                    ui.selectable_value(&mut o.anglais, true, "English");
                });
            });
            ui.separator();
            ui.strong(crate::langue::t("Vue 3D"));
            egui::Grid::new("options_3d").num_columns(2).show(ui, |ui| {
                ui.label(crate::langue::t("Champ de vision"));
                ui.add(egui::Slider::new(&mut o.fov, 30.0..=150.0).suffix("°"));
                ui.end_row();
                ui.label(crate::langue::t("Vitesse"));
                ui.add(egui::Slider::new(&mut o.vitesse, 50.0..=10000.0).logarithmic(true).suffix(" u/s"));
                ui.end_row();
                ui.label(crate::langue::t("Vitesse avec Maj"));
                ui.add(egui::Slider::new(&mut o.vitesse_rapide, 50.0..=20000.0).logarithmic(true).suffix(" u/s"));
                ui.end_row();
                ui.label(crate::langue::t("Sensibilité de la souris"));
                ui.add(egui::Slider::new(&mut o.sensibilite, 0.02..=2.0).logarithmic(true));
                ui.end_row();
                ui.label("");
                ui.checkbox(&mut o.inverser_y, crate::langue::t("Inverser l'axe vertical de la souris"));
                ui.end_row();
                ui.label(crate::langue::t("Distance de rendu"));
                ui.add(egui::Slider::new(&mut o.distance, 1000.0..=200000.0).logarithmic(true).suffix(" u"));
                ui.end_row();
                ui.label(crate::langue::t("Distance des modèles"));
                ui.add(egui::Slider::new(&mut o.distance_modeles, 0.0..=50000.0).suffix(" u")).on_hover_text(crate::langue::t("0 : sans limite"));
                ui.end_row();
                ui.label("");
                ui.checkbox(&mut o.filtrer_textures, crate::langue::t("Textures lissées (sinon pixels nets)"));
                ui.end_row();
            });
            ui.separator();
            ui.strong(crate::langue::t("Vues 2D"));
            ui.checkbox(&mut o.surligner_64, crate::langue::t("Lignes de 64 unités plus claires"));
            ui.horizontal(|ui| {
                ui.label(crate::langue::t("Une ligne sur"));
                ui.add(egui::DragValue::new(&mut o.surligner_n).range(0..=64));
                ui.label(crate::langue::t("plus claire (0 : aucune)"));
            });
            ui.checkbox(&mut o.cacher_petite_grille, crate::langue::t("Cacher la grille de moins de 4 pixels"));
            ui.checkbox(&mut o.fleches, crate::langue::t("Les flèches déplacent la sélection"));
            ui.checkbox(&mut o.grille_pointillee, crate::langue::t("Grille en pointillés"));
            ui.separator();
            ui.strong(crate::langue::t("Matériaux"));
            // le texte tel que tapé (virgule finale comprise) : on le nettoie à la lecture du fichier et au filtrage
            let mut t = o.exclus.join(",");
            ui.horizontal(|ui| {
                ui.label(crate::langue::t("Dossiers exclus du navigateur"));
                if ui.add(egui::TextEdit::singleline(&mut t).desired_width(220.0)).changed() {
                    o.exclus = t.split(',').map(String::from).collect();
                }
            });
            ui.separator();
            ui.strong(crate::langue::t("Sauvegarde automatique"));
            ui.horizontal(|ui| {
                ui.label(crate::langue::t("Copie de récupération toutes les"));
                ui.add(egui::DragValue::new(&mut o.sauvegarde_min).range(1..=60));
                ui.label("min");
            });
            ui.weak(crate::langue::t("tant que la map a des modifications non enregistrées"));
            ui.separator();
            ui.strong(crate::langue::t("Jeu"));
            ui.label(tr!("gameinfo : {}", "gameinfo: {}", self.gameinfo.display()));
            ui.weak(crate::langue::t("les exécutables de compilation se choisissent dans la fenêtre de compilation (F9)"));
        });
        if o.filtrer_textures != self.options.filtrer_textures {
            // l'échantillonneur change : les matériaux sont à refaire (rechargés au prochain affichage)
            if let Some(g) = &mut self.gfx {
                g.r.regler_filtrage(o.filtrer_textures);
                g.bib = hm_render::vivant::Bibliotheque::nouvelle(&g.r);
                g.bib.outils_reels = true;
                g.monde = None;
            }
            self.version_geo = None;
        }
        if o != self.options {
            crate::langue::choisir(o.anglais);
            self.recup.intervalle = std::time::Duration::from_secs(60 * o.sauvegarde_min as u64);
            self.navigateur.exclus = o.exclus.clone();
            self.options = o;
            self.options_a_enregistrer = true;
        }
        if self.options_a_enregistrer && !ctx.input(|i| i.pointer.any_down()) {
            self.options_a_enregistrer = false;
            if let Err(e) = self.options.enregistrer() {
                self.erreur = Some(tr!("options non enregistrées : {e}", "options not saved: {e}"));
            }
        }
        self.options_ouvertes = ouvert;
    }

    /// Le texte de la barre d'état sur la sélection (« Show Selected Brush Number » de Hammer, toujours affiché).
    pub(super) fn etat_selection(&self) -> Option<String> {
        match self.session.selection.as_slice() {
            [] => None,
            [Ref::Solide(id)] => Some(tr!("brosse n° {id}", "brush #{id}")),
            [Ref::Entite(id)] => Some(tr!("entité n° {id}", "entity #{id}")),
            [Ref::Groupe(id)] => Some(tr!("groupe n° {id}", "group #{id}")),
            s => Some(tr!("{} objets sélectionnés", "{} objects selected", s.len())),
        }
    }
}
