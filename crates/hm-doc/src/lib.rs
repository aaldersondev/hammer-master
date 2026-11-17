//! Noyau de l'éditeur Hammer Master.
//!
//! - [`Document`] : la map en objets typés, lue et écrite sans perte ;
//! - [`Commande`] : toute modification, sous forme de donnée ;
//! - [`Session`] : le document ouvert, son historique et sa sélection. C'est le seul accès en écriture : la fenêtre et
//!   le serveur MCP passent par [`Session::executer`], le compilateur interdit le reste (les champs du document ne
//!   sont modifiables qu'à l'intérieur de cette brique).
pub mod commandes;
pub mod brosses;
pub mod decals;
pub mod disp;
pub mod sommets;
pub mod geo;
pub mod io;
pub mod kv;
pub mod lumiere;
pub mod modele;
pub mod pont;
pub mod presse;
pub mod session;
pub mod textures;
pub mod tx;
pub mod visgroups;

pub use commandes::{boite_de, polygone, resoudre, Commande, Forme, Garder, Ref, RefFace, Resultat};
pub use textures::{Alignement, Justif, ReglageFaces};
pub use visgroups::{Cordon, Cordons, Visgroup};
pub use modele::{Axe, Document, Entite, Face, Groupe, Proprio, Solide};
pub use session::Session;
pub use tx::{Changement, Cle};
