//! Icônes des barres d'outils : les glyphes de la police d'icônes de Windows (Segoe Fluent Icons sous Windows 11,
//! Segoe MDL2 Assets sous Windows 10), lue sur le système au démarrage : rien à livrer avec l'éditeur. Sans elle, les
//! boutons gardent un texte court.
use std::sync::atomic::{AtomicBool, Ordering};

use egui::{FontFamily, Response, RichText, Ui};

static CHARGEE: AtomicBool = AtomicBool::new(false);

fn famille() -> FontFamily {
    FontFamily::Name("icones".into())
}

/// Ajoute la police d'icônes du système à egui (famille « icones »).
pub fn charger(ctx: &egui::Context) {
    let fonts = std::env::var_os("WINDIR").map(std::path::PathBuf::from).unwrap_or_else(|| "C:\\Windows".into()).join("Fonts");
    // shortcut: sous Windows 10, quelques glyphes récents (cube, autocollant) manquent à MDL2 et s'affichent en carré vide
    let Some(octets) = ["SegoeIcons.ttf", "segmdl2.ttf"].iter().find_map(|f| std::fs::read(fonts.join(f)).ok()) else { return };
    ctx.add_font(egui::epaint::text::FontInsert::new(
        "icones",
        egui::FontData::from_owned(octets),
        vec![egui::epaint::text::InsertFontFamily { family: famille(), priority: egui::epaint::text::FontPriority::Highest }],
    ));
    CHARGEE.store(true, Ordering::Relaxed);
    // en dernier recours pour tout le texte : les symboles que les polices d'egui n'ont pas (→, ✓…)
    if let Ok(o) = std::fs::read(fonts.join("seguisym.ttf")) {
        let familles = [FontFamily::Proportional, FontFamily::Monospace].map(|family| egui::epaint::text::InsertFontFamily { family, priority: egui::epaint::text::FontPriority::Lowest }).to_vec();
        ctx.add_font(egui::epaint::text::FontInsert::new("symboles", egui::FontData::from_owned(o), familles));
    }
}

/// Le glyphe, ou le texte de secours si la police manque.
pub fn texte(icone: char, secours: &str, taille: f32) -> RichText {
    if CHARGEE.load(Ordering::Relaxed) {
        RichText::new(icone.to_string()).family(famille()).size(taille)
    } else {
        RichText::new(secours).size(11.0)
    }
}

/// Bouton carré à icône, avec son infobulle ; `choisi` le montre enfoncé (outil courant, bascule active).
pub fn bouton(ui: &mut Ui, icone: char, secours: &str, choisi: bool, actif: bool, aide: &str) -> Response {
    let b = egui::Button::selectable(choisi, texte(icone, secours, 18.0)).min_size(egui::vec2(30.0, 30.0));
    ui.add_enabled(actif, b).on_hover_text(aide).on_disabled_hover_text(aide)
}

// Outils
pub const SELECTION: char = '\u{E8B0}';
pub const CAMERA: char = '\u{E722}';
pub const BLOC: char = '\u{F158}';
pub const ENTITE: char = '\u{E82F}';
pub const TEXTURE: char = '\u{E790}';
pub const DECOUPE: char = '\u{E7A8}';
pub const SOMMETS: char = '\u{EBD2}';
pub const DECAL: char = '\u{F4AA}';
pub const OVERLAY: char = '\u{EB9F}';
pub const SCULPTER: char = '\u{E771}';
pub const LOUPE: char = '\u{E8A3}';
pub const SEMER: char = '\u{F4A5}';
pub const POLYGONE: char = '\u{E879}';
// Fichier, édition
pub const NOUVEAU: char = '\u{E8A5}';
pub const OUVRIR: char = '\u{E838}';
pub const ENREGISTRER: char = '\u{E74E}';
pub const ANNULER: char = '\u{E7A7}';
pub const RETABLIR: char = '\u{E7A6}';
pub const COUPER: char = '\u{E8C6}';
pub const COPIER: char = '\u{E8C8}';
pub const COLLER: char = '\u{E77F}';
pub const SUPPRIMER: char = '\u{E74D}';
// Grille et réglages
pub const GRILLE: char = '\u{E80A}';
pub const MOINS: char = '\u{E949}';
pub const PLUS: char = '\u{E948}';
pub const AIMANT: char = '\u{E718}';
pub const VERROU: char = '\u{E72E}';
pub const VERROU_OUVERT: char = '\u{E785}';
pub const IGNORER_GROUPES: char = '\u{F5ED}';
pub const GROUPER: char = '\u{E7C4}';
pub const DEGROUPER: char = '\u{ECA5}';
// Affichage
pub const MASQUER: char = '\u{ED1A}';
pub const MASQUER_RESTE: char = '\u{F19D}';
pub const REAFFICHER: char = '\u{E890}';
pub const VUES_EGALES: char = '\u{E8A9}';
pub const AGRANDIR: char = '\u{E740}';
pub const CADRER: char = '\u{E9A6}';
pub const ECLAIRAGE: char = '\u{E706}';
pub const PRATICABILITE: char = '\u{E805}';
pub const VALIDATION: char = '\u{E930}';
// Brosses
pub const CREUSER: char = '\u{ED62}';
pub const FUSIONNER: char = '\u{EA3C}';
pub const EVIDER: char = '\u{E739}';
pub const TRANSFORMER: char = '\u{E7C2}';
// Carte, compilation
pub const VERIFIER: char = '\u{E9D5}';
pub const COMPILER: char = '\u{E768}';
pub const COMPILER_ZONE: char = '\u{E8B3}';
pub const PALETTE: char = '\u{E721}';
pub const OPTIONS: char = '\u{E713}';
// Onglets du panneau de droite
pub const TEXTURES: char = '\u{E91B}';
pub const PROPRIETES: char = '\u{E946}';
pub const HISTORIQUE: char = '\u{E81C}';
pub const VISGROUPS: char = '\u{E81E}';
pub const MODELES: char = '\u{E7B8}';
pub const PREFABS: char = '\u{E74C}';
pub const REMARQUES: char = '\u{E8BD}';
/// Réduire le panneau de droite (chevron vers la droite), le rouvrir (vers la gauche).
pub const REDUIRE: char = '\u{E76C}';
pub const MONTER: char = '\u{E70E}';
pub const DESCENDRE: char = '\u{E70D}';
pub const AGRANDIR_PANNEAU: char = '\u{E76B}';
