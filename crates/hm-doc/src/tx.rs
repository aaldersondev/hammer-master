//! Transactions et historique.
//!
//! Une commande modifie le document à travers une transaction. Au premier accès à un objet (un solide du monde, une
//! entité avec ses solides, un groupe, le monde, un bloc de premier niveau), la transaction garde une copie de son
//! état et de sa place. À la fin, elle relève l'état d'après des mêmes objets. Annuler remet les états d'avant,
//! rétablir ceux d'après : aucune commande n'écrit son propre inverse.
use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::kv::Bloc;
use crate::modele::{Document, Entite, Groupe, Haut, Proprio, Solide};

/// Ce que la transaction sait copier et remettre en place.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Cle {
    /// Clés et réglages du monde (pas ses solides).
    Monde,
    SolideMonde(u64),
    /// Une entité avec ses solides.
    Entite(u64),
    Groupe(u64),
    /// Un bloc de premier niveau, par son rang.
    Haut(usize),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Objet {
    Monde(Entite),
    Solide(Solide),
    Entite(Entite),
    Groupe(Groupe),
    Haut(Bloc),
}

/// État d'un objet : absent, ou présent à cette place.
pub type Etat = Option<(usize, Objet)>;

/// Une modification faite, annulable.
#[derive(Clone, Debug)]
pub struct Changement {
    pub nom: String,
    pub avant: Vec<(Cle, Etat)>,
    pub apres: Vec<(Cle, Etat)>,
    /// Sélection avant et après : annuler et rétablir la remettent aussi.
    pub selection_avant: Vec<crate::commandes::Ref>,
    pub selection_apres: Vec<crate::commandes::Ref>,
}

impl Changement {
    pub fn cles(&self) -> impl Iterator<Item = Cle> + '_ {
        self.avant.iter().map(|(c, _)| *c)
    }

    pub fn vide(&self) -> bool {
        self.avant.is_empty()
    }
}

impl Document {
    fn etat(&self, cle: Cle) -> Etat {
        match cle {
            Cle::Monde => {
                let mut m = self.monde.clone();
                m.solides.clear();
                Some((0, Objet::Monde(m)))
            }
            Cle::SolideMonde(id) => self.solides.get_full(&id).map(|(i, _, s)| (i, Objet::Solide(s.clone()))),
            Cle::Entite(id) => self.entites.get_full(&id).map(|(i, _, e)| (i, Objet::Entite(e.clone()))),
            Cle::Groupe(id) => self.groupes.get_full(&id).map(|(i, _, g)| (i, Objet::Groupe(g.clone()))),
            Cle::Haut(i) => match self.haut.get(i) {
                Some(Haut::Bloc(b) | Haut::Ajoute(b)) => Some((i, Objet::Haut(b.clone()))),
                _ => None,
            },
        }
    }

    fn retirer_cle(&mut self, cle: Cle) {
        match cle {
            Cle::Monde | Cle::Haut(_) => {}
            Cle::SolideMonde(id) => {
                if self.solides.shift_remove(&id).is_some() {
                    self.proprio.remove(&id);
                }
            }
            Cle::Entite(id) => {
                if let Some(e) = self.entites.shift_remove(&id) {
                    for s in &e.solides {
                        if self.proprio.get(&s.id) == Some(&Proprio::Entite(id)) {
                            self.proprio.remove(&s.id);
                        }
                    }
                }
            }
            Cle::Groupe(id) => {
                self.groupes.shift_remove(&id);
            }
        }
    }

    fn poser(&mut self, cle: Cle, etat: &Etat) {
        let Some((i, o)) = etat else { return };
        match (cle, o) {
            (Cle::Monde, Objet::Monde(m)) => self.monde = m.clone(),
            (Cle::SolideMonde(id), Objet::Solide(s)) => {
                let i = (*i).min(self.solides.len());
                self.solides.shift_insert(i, id, s.clone());
                self.proprio.insert(id, Proprio::Monde);
            }
            (Cle::Entite(id), Objet::Entite(e)) => {
                let i = (*i).min(self.entites.len());
                for s in &e.solides {
                    self.proprio.insert(s.id, Proprio::Entite(id));
                }
                self.entites.shift_insert(i, id, e.clone());
            }
            (Cle::Groupe(id), Objet::Groupe(g)) => {
                let i = (*i).min(self.groupes.len());
                self.groupes.shift_insert(i, id, g.clone());
            }
            (Cle::Haut(i), Objet::Haut(b)) => {
                if let Some(Haut::Bloc(x) | Haut::Ajoute(x)) = self.haut.get_mut(i) {
                    *x = b.clone();
                }
            }
            _ => {}
        }
    }

    /// Remet les objets dans les états donnés : tous retirés, puis reposés par place croissante, ce qui rend
    /// exactement l'ordre d'origine (les objets non touchés n'ont pas bougé entre eux).
    fn restaurer(&mut self, etats: &[(Cle, Etat)]) {
        for (c, _) in etats {
            self.retirer_cle(*c);
        }
        let mut ordre: Vec<&(Cle, Etat)> = etats.iter().collect();
        ordre.sort_by_key(|(c, e)| (rang_genre(*c), e.as_ref().map_or(usize::MAX, |(i, _)| *i)));
        for (c, e) in ordre {
            self.poser(*c, e);
        }
    }
}

fn rang_genre(c: Cle) -> u8 {
    match c {
        Cle::Monde => 0,
        Cle::Haut(_) => 1,
        Cle::Groupe(_) => 2,
        Cle::SolideMonde(_) => 3,
        Cle::Entite(_) => 4,
    }
}

/// Accès en écriture au document pendant une commande.
pub struct Tx<'a> {
    pub(crate) doc: &'a mut Document,
    avant: Vec<(Cle, Etat)>,
    vus: HashSet<Cle>,
    /// Ordre de départ d'une collection, figé avant qu'on y retire ou ajoute : les places notées ensuite restent
    /// celles du début de la transaction, seules valables pour tout remettre.
    ordre: [Option<HashMap<u64, usize>>; 3],
}

#[derive(Clone, Copy)]
enum Coll {
    Solides = 0,
    Entites = 1,
    Groupes = 2,
}

impl<'a> Tx<'a> {
    pub(crate) fn new(doc: &'a mut Document) -> Tx<'a> {
        Tx { doc, avant: Vec::new(), vus: HashSet::new(), ordre: [None, None, None] }
    }

    /// Lecture du document pendant la transaction.
    pub fn doc(&self) -> &Document {
        self.doc
    }

    fn noter(&mut self, cle: Cle) {
        if self.vus.insert(cle) {
            let mut e = self.doc.etat(cle);
            let (coll, id) = match cle {
                Cle::SolideMonde(id) => (Some(Coll::Solides), id),
                Cle::Entite(id) => (Some(Coll::Entites), id),
                Cle::Groupe(id) => (Some(Coll::Groupes), id),
                _ => (None, 0),
            };
            if let (Some(c), Some((i, _))) = (coll, e.as_mut()) {
                if let Some(o) = &self.ordre[c as usize] {
                    if let Some(j) = o.get(&id) {
                        *i = *j;
                    }
                }
            }
            self.avant.push((cle, e));
        }
    }

    /// À appeler avant tout retrait ou ajout dans une collection.
    fn figer(&mut self, c: Coll) {
        if self.ordre[c as usize].is_none() {
            let o: HashMap<u64, usize> = match c {
                Coll::Solides => self.doc.solides.keys().enumerate().map(|(i, k)| (*k, i)).collect(),
                Coll::Entites => self.doc.entites.keys().enumerate().map(|(i, k)| (*k, i)).collect(),
                Coll::Groupes => self.doc.groupes.keys().enumerate().map(|(i, k)| (*k, i)).collect(),
            };
            self.ordre[c as usize] = Some(o);
        }
    }

    pub fn nouvel_id(&mut self) -> u64 {
        self.doc.nouvel_id()
    }

    pub fn nouvelle_face(&mut self) -> u64 {
        self.doc.nouvelle_face()
    }

    pub fn monde_mut(&mut self) -> &mut Entite {
        self.noter(Cle::Monde);
        &mut self.doc.monde
    }

    pub fn solide_mut(&mut self, id: u64) -> Option<&mut Solide> {
        match self.doc.proprio.get(&id).copied()? {
            Proprio::Monde => {
                self.noter(Cle::SolideMonde(id));
                self.doc.solides.get_mut(&id)
            }
            Proprio::Entite(e) => {
                self.noter(Cle::Entite(e));
                self.doc.entites.get_mut(&e)?.solides.iter_mut().find(|s| s.id == id)
            }
        }
    }

    pub fn entite_mut(&mut self, id: u64) -> Option<&mut Entite> {
        if !self.doc.entites.contains_key(&id) {
            return None;
        }
        self.noter(Cle::Entite(id));
        self.doc.entites.get_mut(&id)
    }

    pub fn groupe_mut(&mut self, id: u64) -> Option<&mut Groupe> {
        if !self.doc.groupes.contains_key(&id) {
            return None;
        }
        self.noter(Cle::Groupe(id));
        self.doc.groupes.get_mut(&id)
    }

    pub fn bloc_haut_mut(&mut self, nom: &str) -> Option<&mut Bloc> {
        let i = self.doc.haut.iter().position(|h| matches!(h, Haut::Bloc(b) | Haut::Ajoute(b) if b.nom.eq_ignore_ascii_case(nom)))?;
        self.noter(Cle::Haut(i));
        match &mut self.doc.haut[i] {
            Haut::Bloc(b) | Haut::Ajoute(b) => Some(b),
            _ => None,
        }
    }

    /// Ajoute un solide au monde (à la fin) ou à une entité.
    pub fn ajouter_solide(&mut self, proprio: Proprio, s: Solide) -> u64 {
        let id = s.id;
        match proprio {
            Proprio::Monde => {
                self.figer(Coll::Solides);
                self.noter(Cle::SolideMonde(id));
                self.doc.proprio.insert(id, Proprio::Monde);
                self.doc.solides.insert(id, s);
            }
            Proprio::Entite(e) => {
                self.noter(Cle::Entite(e));
                if let Some(ent) = self.doc.entites.get_mut(&e) {
                    self.doc.proprio.insert(id, Proprio::Entite(e));
                    ent.solides.push(s);
                }
            }
        }
        id
    }

    /// Retire un solide, où qu'il soit, et le rend.
    pub fn retirer_solide(&mut self, id: u64) -> Option<Solide> {
        match self.doc.proprio.get(&id).copied()? {
            Proprio::Monde => {
                self.noter(Cle::SolideMonde(id));
                self.figer(Coll::Solides);
                self.doc.proprio.remove(&id);
                self.doc.solides.shift_remove(&id)
            }
            Proprio::Entite(e) => {
                self.noter(Cle::Entite(e));
                let ent = self.doc.entites.get_mut(&e)?;
                let i = ent.solides.iter().position(|s| s.id == id)?;
                self.doc.proprio.remove(&id);
                Some(ent.solides.remove(i))
            }
        }
    }

    pub fn ajouter_entite(&mut self, e: Entite) -> u64 {
        let id = e.id;
        self.figer(Coll::Entites);
        self.noter(Cle::Entite(id));
        for s in &e.solides {
            self.doc.proprio.insert(s.id, Proprio::Entite(id));
        }
        self.doc.entites.insert(id, e);
        id
    }

    pub fn retirer_entite(&mut self, id: u64) -> Option<Entite> {
        if !self.doc.entites.contains_key(&id) {
            return None;
        }
        self.noter(Cle::Entite(id));
        self.figer(Coll::Entites);
        let e = self.doc.entites.shift_remove(&id)?;
        for s in &e.solides {
            self.doc.proprio.remove(&s.id);
        }
        Some(e)
    }

    pub fn ajouter_groupe(&mut self, g: Groupe) -> u64 {
        let id = g.id;
        self.figer(Coll::Groupes);
        self.noter(Cle::Groupe(id));
        self.doc.groupes.insert(id, g);
        id
    }

    pub fn retirer_groupe(&mut self, id: u64) -> Option<Groupe> {
        if !self.doc.groupes.contains_key(&id) {
            return None;
        }
        self.noter(Cle::Groupe(id));
        self.figer(Coll::Groupes);
        self.doc.groupes.shift_remove(&id)
    }

    /// Termine la transaction : relève l'état d'après de chaque objet touché.
    pub(crate) fn finir(self, nom: &str) -> Changement {
        // un objet touché mais rendu identique ne compte pas : une commande sans effet ne laisse pas de trace
        let (mut avant, mut apres) = (Vec::new(), Vec::new());
        for (c, e) in self.avant {
            let f = self.doc.etat(c);
            if f != e {
                avant.push((c, e));
                apres.push((c, f));
            }
        }
        Changement { nom: nom.to_string(), avant, apres, selection_avant: Vec::new(), selection_apres: Vec::new() }
    }

    /// Abandonne la transaction : le document revient à son état d'avant.
    pub(crate) fn abandonner(self) {
        let avant = self.avant;
        self.doc.restaurer(&avant);
    }
}

/// Historique sans limite : ce qui est fait, et ce qui a été annulé et peut être rétabli.
#[derive(Clone, Debug, Default)]
pub struct Historique {
    pub(crate) faits: Vec<Changement>,
    pub(crate) annules: Vec<Changement>,
}

impl Historique {
    pub fn faits(&self) -> &[Changement] {
        &self.faits
    }

    pub fn annules(&self) -> &[Changement] {
        &self.annules
    }

    pub(crate) fn annuler(&mut self, doc: &mut Document) -> Option<&Changement> {
        let c = self.faits.pop()?;
        doc.restaurer(&c.avant);
        self.annules.push(c);
        self.annules.last()
    }

    pub(crate) fn retablir(&mut self, doc: &mut Document) -> Option<&Changement> {
        let c = self.annules.pop()?;
        doc.restaurer(&c.apres);
        self.faits.push(c);
        self.faits.last()
    }
}
