//! Le menu Instances de Hammer++ : créer une instance depuis la sélection (« Create Instance from Selection »,
//! Alt+S) et fondre les instances dans la map (« Collapse » : la sélection, ou toutes).
use std::path::{Path, PathBuf};

use hm_doc::{Commande, Ref};

use super::Editeur;

impl Editeur {
    /// La sélection part dans un fichier VMF (milieu de son dessous à l'origine, comme un préfabriqué) choisi sous le
    /// dossier de la map, et une `func_instance` qui le désigne prend sa place, en une étape.
    pub(super) fn creer_instance(&mut self) {
        let sel = self.session.selection.clone();
        let Some(dossier) = self.session.chemin().and_then(Path::parent).map(Path::to_path_buf) else {
            self.erreur = Some("enregistrer la map d'abord : le fichier d'une instance est relatif à celui de la map".into());
            return;
        };
        let Some((lo, hi)) = hm_doc::boite_de(self.session.doc(), &sel) else { return };
        let vmf = match hm_doc::presse::prefabriquer(self.session.doc(), &sel) {
            Ok(v) => v,
            Err(e) => {
                self.erreur = Some(format!("instance : {e}"));
                return;
            }
        };
        let defaut = dossier.join("instances");
        let _ = std::fs::create_dir_all(&defaut);
        let Some(p) = rfd::FileDialog::new().add_filter("Map Hammer", &["vmf"]).set_directory(&defaut).set_file_name("instance.vmf").save_file() else { return };
        let Ok(relatif) = p.strip_prefix(&dossier) else {
            self.erreur = Some(format!("le fichier d'instance doit être sous le dossier de la map ({})", dossier.display()));
            return;
        };
        if let Err(e) = std::fs::write(&p, vmf) {
            self.erreur = Some(format!("{} : {e}", p.display()));
            return;
        }
        let file = relatif.to_string_lossy().replace('\\', "/");
        let origine = [(lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0, lo.z];
        let cles = vec![("file".to_string(), file.clone())];
        self.instances.borrow_mut().clear();
        self.executer(Commande::Lot { nom: format!("instance depuis la sélection ({file})"), commandes: vec![Commande::Supprimer { objets: sel }, Commande::CreerEntite { classe: "func_instance".into(), origine, cles }] });
    }

    /// Fond les `func_instance` désignées (toutes si `toutes`) dans la map, en une étape.
    pub(super) fn fondre_instances(&mut self, toutes: bool) {
        let doc = self.session.doc();
        let ids: Vec<u64> = if toutes {
            doc.entites().filter(|e| e.classe().eq_ignore_ascii_case("func_instance")).map(|e| e.id).collect()
        } else {
            self.session.selection.iter().filter_map(|r| if let Ref::Entite(e) = r { Some(*e) } else { None }).filter(|e| doc.entite(*e).map_or(false, |x| x.classe().eq_ignore_ascii_case("func_instance"))).collect()
        };
        if ids.is_empty() {
            self.statut = "aucune func_instance".into();
            return;
        }
        let dossier: Option<PathBuf> = self.session.chemin().and_then(Path::parent).map(Path::to_path_buf);
        let mut commandes = Vec::new();
        for id in &ids {
            let fichier = doc.entite(*id).and_then(|e| e.get("file")).unwrap_or("").to_string();
            match dossier.as_ref().map(|d| d.join(&fichier)).and_then(|p| std::fs::read_to_string(p).ok()) {
                Some(vmf) => commandes.push(Commande::FondreInstance { entite: *id, vmf }),
                None => {
                    self.erreur = Some(format!("instance n° {id} : fichier « {fichier} » introuvable à côté de la map"));
                    return;
                }
            }
        }
        self.executer(Commande::Lot { nom: format!("fondre {} instance(s)", commandes.len()), commandes });
    }
}
