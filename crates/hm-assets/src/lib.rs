//! Fichiers du jeu : où ils sont (dossiers, archives VPK), et ce que le rendu en tire (matériaux, textures,
//! modèles). Les lecteurs de formats viennent de `vformats` et de `source-mdl`, dont les versions sont fixées ;
//! le reste du projet ne voit que les types de cette crate.
/// Arêtes vives d'un modèle (fil de fer des vues 2D).
pub mod aretes;
pub mod collision;
pub mod jeu;
pub mod material;
pub mod model;
/// Noms des systèmes de particules d'un .pcf (DMX binaire).
pub mod particules;
/// Noms des soundscripts et fichiers du manifeste des sons.
pub mod sons;
pub mod texture;
pub mod vfs;

pub use collision::{load_collision, Hull};
pub use material::{load_material, Material, Tool};
pub use model::{load_model, Model, ModelMesh, ModelVertex};
pub use texture::{load_texture, texture_size, PixelFormat, Texture};
pub use vfs::Vfs;
