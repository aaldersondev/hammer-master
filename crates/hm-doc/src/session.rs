//! La session : un document ouvert, son historique, sa sélection. C'est le seul point d'entrée en écriture : la
//! fenêtre et le serveur MCP n'ont que `doc()` en lecture et `executer`.
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::commandes::{Commande, Ref, Res, Resultat};
use crate::modele::{Document, RapportLecture};
use crate::tx::{Changement, Cle, Historique, Tx};

pub struct Session {
    /// Partagé sans copie avec les lecteurs en arrière-plan (validation) ; copié seulement si on le modifie pendant
    /// qu'ils le lisent (`Arc::make_mut`).
    doc: Arc<Document>,
    histo: Historique,
    /// Objets sélectionnés, dans l'ordre où ils l'ont été. Hors historique, comme dans Hammer.
    pub selection: Vec<Ref>,
    chemin: Option<PathBuf>,
    /// Compte des changements (faits, annulés, rétablis) : la fenêtre s'en sert pour savoir quoi redessiner.
    version: u64,
    version_enregistree: u64,
    /// Objets touchés par le dernier changement, annulation ou rétablissement.
    touches: Vec<Cle>,
    pub rapport_lecture: RapportLecture,
    cle_continue: Option<String>,
    version_continue: u64,
}

impl Default for Session {
    fn default() -> Self {
        Session::nouvelle()
    }
}

impl Session {
    pub fn nouvelle() -> Session {
        Session { doc: Arc::new(Document::nouveau()), histo: Historique::default(), selection: Vec::new(), chemin: None, version: 0, version_enregistree: 0, touches: Vec::new(), rapport_lecture: RapportLecture::default(), cle_continue: None, version_continue: u64::MAX }
    }

    pub fn depuis_octets(src: &[u8], chemin: Option<PathBuf>) -> Res<Session> {
        let (doc, rapport) = Document::lire(src).map_err(|e| e.to_string())?;
        let mut s = Session::nouvelle();
        s.doc = Arc::new(doc);
        s.chemin = chemin;
        s.rapport_lecture = rapport;
        Ok(s)
    }

    pub fn ouvrir(chemin: &Path) -> Res<Session> {
        let src = std::fs::read(chemin).map_err(|e| format!("{} : {e}", chemin.display()))?;
        Session::depuis_octets(&src, Some(chemin.to_path_buf()))
    }

    /// Enregistre dans le fichier de la session, ou dans `chemin` (qui devient celui de la session). L'écriture
    /// passe par un fichier voisin renommé ensuite : une coupure pendant l'écriture ne laisse jamais un VMF à moitié
    /// écrit.
    pub fn enregistrer(&mut self, chemin: Option<&Path>) -> Res<PathBuf> {
        let p = chemin.map(Path::to_path_buf).or_else(|| self.chemin.clone()).ok_or("pas de fichier : choisir où enregistrer")?;
        ecrire_sur(&p, &self.doc.ecrire())?;
        self.chemin = Some(p.clone());
        self.version_enregistree = self.version;
        Ok(p)
    }

    /// Une map récupérée après un plantage : elle reprend le fichier d'où elle venait et compte comme modifiée
    /// (rien n'est encore enregistré dans ce fichier).
    pub fn recuperee(src: &[u8], origine: Option<PathBuf>) -> Res<Session> {
        let mut s = Session::depuis_octets(src, origine)?;
        s.version_enregistree = u64::MAX;
        Ok(s)
    }

    /// Le document, partagé sans copie (pour un lecteur en arrière-plan).
    pub fn doc_partage(&self) -> Arc<Document> {
        self.doc.clone()
    }

    pub fn doc(&self) -> &Document {
        &self.doc
    }

    pub fn chemin(&self) -> Option<&Path> {
        self.chemin.as_deref()
    }

    pub fn historique(&self) -> &Historique {
        &self.histo
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn modifiee(&self) -> bool {
        self.version != self.version_enregistree
    }

    pub fn touches(&self) -> &[Cle] {
        &self.touches
    }

    /// Exécute une commande. En cas d'erreur, le document n'a pas changé.
    pub fn executer(&mut self, c: Commande) -> Res<Resultat> {
        let mut tx = Tx::new(Arc::make_mut(&mut self.doc));
        match c.executer(&mut tx) {
            Ok(r) => {
                let mut ch = tx.finir(&c.nom());
                ch.selection_avant = self.selection.clone();
                if !r.crees.is_empty() {
                    self.selection = r.crees.clone();
                }
                self.elaguer_selection();
                ch.selection_apres = self.selection.clone();
                if !ch.vide() {
                    self.noter(ch);
                }
                Ok(r)
            }
            Err(e) => {
                tx.abandonner();
                Err(e)
            }
        }
    }

    /// Comme `executer`, mais une suite de commandes de même `cle` (un champ qu'on fait glisser, une valeur qu'on tape
    /// chiffre à chiffre) ne fait qu'une étape d'historique : la dernière remplace la précédente, l'état d'avant reste
    /// celui du début.
    pub fn executer_continu(&mut self, c: Commande, cle: &str) -> Res<Resultat> {
        let suite = self.cle_continue.as_deref() == Some(cle) && self.histo.annules.is_empty() && self.version == self.version_continue;
        let r = self.executer(c)?;
        if suite && self.histo.faits.len() >= 2 {
            let dernier = self.histo.faits.pop().unwrap();
            let premier = self.histo.faits.last_mut().unwrap();
            for (k, e) in dernier.avant {
                if !premier.avant.iter().any(|(x, _)| *x == k) {
                    premier.avant.push((k, e));
                }
            }
            for (k, e) in dernier.apres {
                match premier.apres.iter_mut().find(|(x, _)| *x == k) {
                    Some(x) => x.1 = e,
                    None => premier.apres.push((k, e)),
                }
            }
            premier.selection_apres = dernier.selection_apres;
            premier.nom = dernier.nom;
        }
        self.cle_continue = Some(cle.to_string());
        self.version_continue = self.version;
        Ok(r)
    }

    /// La prochaine commande continue ouvrira une nouvelle étape (fin d'un glisser).
    pub fn couper_continu(&mut self) {
        self.cle_continue = None;
    }

    fn noter(&mut self, ch: Changement) {
        self.touches = ch.cles().collect();
        self.histo.faits.push(ch);
        self.histo.annules.clear();
        self.version += 1;
    }

    /// Annule le dernier changement ; rend son nom.
    pub fn annuler(&mut self) -> Option<String> {
        let c = self.histo.annuler(Arc::make_mut(&mut self.doc))?;
        let (nom, cles) = (c.nom.clone(), c.cles().collect());
        self.selection = c.selection_avant.clone();
        self.touches = cles;
        self.version += 1;
        self.elaguer_selection();
        Some(nom)
    }

    pub fn retablir(&mut self) -> Option<String> {
        let c = self.histo.retablir(Arc::make_mut(&mut self.doc))?;
        let (nom, cles) = (c.nom.clone(), c.cles().collect());
        self.selection = c.selection_apres.clone();
        self.touches = cles;
        self.version += 1;
        self.elaguer_selection();
        Some(nom)
    }

    /// Revient à l'état qui suit le changement n° `n` de l'historique (0 = avant tout changement).
    pub fn aller_a(&mut self, n: usize) {
        while self.histo.faits.len() > n && self.annuler().is_some() {}
        while self.histo.faits.len() < n && self.retablir().is_some() {}
    }

    /// Retire de la sélection ce qui n'existe plus.
    fn elaguer_selection(&mut self) {
        let d = &self.doc;
        self.selection.retain(|r| match r {
            Ref::Solide(id) => d.solide(*id).is_some(),
            Ref::Entite(id) => d.entite(*id).is_some(),
            Ref::Groupe(id) => d.groupe(*id).is_some(),
        });
    }

    /// Le texte du presse-papiers pour la sélection.
    pub fn copier(&self) -> Res<String> {
        crate::presse::copier(&self.doc, &self.selection)
    }
}

/// Écrit d'abord à côté puis renomme : le fichier d'avant reste entier tant que le nouveau n'est pas complet.
pub fn ecrire_sur(p: &Path, octets: &[u8]) -> Res<()> {
    let tmp = p.with_extension("vmf.hm-ecriture");
    std::fs::write(&tmp, octets).map_err(|e| format!("{} : {e}", tmp.display()))?;
    std::fs::rename(&tmp, p).map_err(|e| format!("{} : {e}", p.display()))?;
    Ok(())
}
