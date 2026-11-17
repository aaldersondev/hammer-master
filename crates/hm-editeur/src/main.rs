//! Hammer Master : éditeur de maps Source / Garry's Mod.
//!
//!     hammer-master.exe [carte.vmf] [--script gestes.txt]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod gestes;
mod icones;
mod modeles;
mod options;
mod proprietes;
mod raccourcis;
mod recents;
mod recup;
mod scene;
mod script;
mod textures;
mod vues;
mod visibilite;

use std::path::PathBuf;
use std::sync::Arc;

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut carte = None;
    let mut script = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--script" => {
                script = args.get(i + 1).and_then(|p| std::fs::read_to_string(p).ok());
                i += 1;
            }
            a => carte = Some(PathBuf::from(a)),
        }
        i += 1;
    }
    let mut wgpu_options = egui_wgpu::WgpuConfiguration::default();
    if let egui_wgpu::WgpuSetup::CreateNew(c) = &mut wgpu_options.wgpu_setup {
        // les textures du jeu vont telles quelles à la carte graphique (DXT = BC1/BC3)
        c.device_descriptor = Arc::new(|adapter: &wgpu::Adapter| wgpu::DeviceDescriptor {
            label: Some("hammer-master"),
            required_features: adapter.features() & wgpu::Features::TEXTURE_COMPRESSION_BC,
            required_limits: adapter.limits(),
            ..Default::default()
        });
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("Hammer Master").with_inner_size([1600.0, 900.0]),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options,
        ..Default::default()
    };
    eframe::run_native("Hammer Master", options, Box::new(move |cc| {
        icones::charger(&cc.egui_ctx);
        Ok(Box::new(app::Editeur::new(carte, script)))
    }))
}
