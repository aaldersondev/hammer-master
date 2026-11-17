//! Le pont entre la fenêtre de l'éditeur et le serveur MCP : Claude agit sur la map ouverte dans la fenêtre, par les
//! mêmes commandes que la souris, et l'utilisateur voit chaque modification en direct (et l'annule par Ctrl+Z).
//!
//! La fenêtre écoute sur 127.0.0.1, à un port choisi par le système, et s'annonce dans
//! `%LOCALAPPDATA%\HammerMaster\fenetres\<pid>.json` (port, jeton). Une demande par ligne (JSON, avec le jeton), une
//! réponse par ligne. Une demande sans le bon jeton fait raccrocher : seul ce qui lit le dossier de l'utilisateur
//! peut agir sur sa map. Les demandes sont servies par la fenêtre elle-même, entre deux images.
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::commandes::{Commande, Ref};

/// Ce qu'une fenêtre écrit dans son fichier d'annonce.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Annonce {
    pub port: u16,
    pub jeton: String,
    pub pid: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "demande", rename_all = "snake_case")]
pub enum Demande {
    /// La map ouverte, sa version, la sélection.
    Etat,
    /// Le document entier, en VMF.
    Document,
    /// Une commande du noyau, une étape de l'historique de la fenêtre.
    Commande { commande: Commande },
    Annuler { n: usize, retablir: bool },
    Historique,
    Enregistrer { chemin: Option<PathBuf> },
    /// Sélectionner ces objets dans la fenêtre et y cadrer les vues.
    Montrer { objets: Vec<Ref> },
    /// Lancer une compilation dans la fenêtre (son journal s'y affiche) ; on en suit l'état par `Etat`.
    Compiler {
        /// `geometrie`, `rapide`, `normale` ou `finale`.
        mode: String,
        /// Seulement cette boîte (min, max), comme « Compiler la zone de la sélection ».
        zone: Option<[[f64; 3]; 2]>,
        copier: bool,
        embarquer: bool,
        lancer_jeu: bool,
        partielle: bool,
    },
}

/// Où en est la compilation de la fenêtre.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct EtatCompilation {
    pub en_cours: bool,
    /// Le résultat de la dernière compilation finie : réussie ou non, durée, erreur, BSP.
    pub reussi: Option<bool>,
    pub secondes: f32,
    pub erreur: Option<String>,
    pub bsp: Option<PathBuf>,
    /// Les dernières lignes du journal.
    pub journal: Vec<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Reponse {
    /// Refus ou échec : rien n'a changé.
    pub erreur: Option<String>,
    pub carte: Option<PathBuf>,
    /// Compte des changements de la session de la fenêtre (ne revient jamais en arrière, sauf réouverture).
    pub version: u64,
    pub modifiee: bool,
    pub selection: Vec<Ref>,
    /// Nombre d'étapes faites dans l'historique de la fenêtre.
    pub etapes: usize,
    /// Ce qui a été fait : noms des étapes (commande, annulation, historique), chemin enregistré.
    pub faits: Vec<String>,
    /// Étapes annulées qui peuvent être rétablies (historique), la prochaine à rétablir en premier.
    pub annules: Vec<String>,
    pub crees: Vec<Ref>,
    pub message: String,
    /// Le document (demande `Document`). Converti en texte : un octet hors UTF-8 y devient « � ».
    pub vmf: Option<String>,
    #[serde(default)]
    pub compilation: EtatCompilation,
}

#[derive(Serialize, Deserialize)]
struct Message {
    jeton: String,
    demande: Demande,
}

/// Le dossier des annonces.
pub fn dossier() -> PathBuf {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("HammerMaster").join("fenetres")
}

/// Une demande reçue par la fenêtre, et de quoi y répondre.
pub type Recue = (Demande, mpsc::Sender<Reponse>);

/// Côté fenêtre : l'écoute ouverte et annoncée. Les demandes arrivent sur `demandes` ; l'annonce est effacée quand
/// l'écoute est lâchée (fermeture de la fenêtre).
pub struct Ecoute {
    fichier: PathBuf,
    pub demandes: mpsc::Receiver<Recue>,
}

impl Drop for Ecoute {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.fichier);
    }
}

/// Ouvre l'écoute et l'annonce. `reveil` est appelé à chaque demande reçue (la fenêtre se redessine et la sert).
pub fn ecouter(reveil: impl Fn() + Send + Sync + 'static) -> std::io::Result<Ecoute> {
    let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let annonce = Annonce { port: l.local_addr()?.port(), jeton: jeton(), pid: std::process::id() };
    let d = dossier();
    std::fs::create_dir_all(&d)?;
    let fichier = d.join(format!("{}.json", annonce.pid));
    crate::session::ecrire_sur(&fichier, serde_json::to_string(&annonce)?.as_bytes()).map_err(std::io::Error::other)?;
    let (tx, demandes) = mpsc::channel();
    let reveil: Arc<dyn Fn() + Send + Sync> = Arc::new(reveil);
    let jeton = annonce.jeton;
    std::thread::spawn(move || {
        for flux in l.incoming().flatten() {
            let (tx, jeton, reveil) = (tx.clone(), jeton.clone(), reveil.clone());
            std::thread::spawn(move || servir(flux, &jeton, &tx, &*reveil));
        }
    });
    Ok(Ecoute { fichier, demandes })
}

fn servir(flux: TcpStream, jeton: &str, tx: &mpsc::Sender<Recue>, reveil: &dyn Fn()) {
    let Ok(sortie) = flux.try_clone() else { return };
    let mut sortie = BufWriter::new(sortie);
    for ligne in BufReader::new(flux).lines() {
        let Ok(ligne) = ligne else { return };
        let rep = match serde_json::from_str::<Message>(&ligne) {
            Ok(m) if m.jeton != jeton => return,
            Ok(m) => {
                let (rtx, rrx) = mpsc::channel();
                if tx.send((m.demande, rtx)).is_err() {
                    return;
                }
                reveil();
                match rrx.recv() {
                    Ok(r) => r,
                    Err(_) => return,
                }
            }
            Err(e) => Reponse { erreur: Some(format!("demande illisible : {e}")), ..Default::default() },
        };
        if serde_json::to_writer(&mut sortie, &rep).is_err() || sortie.write_all(b"\n").is_err() || sortie.flush().is_err() {
            return;
        }
    }
}

/// 128 bits tirés des clés aléatoires de la bibliothèque standard (graine du système à chaque processus).
fn jeton() -> String {
    use std::hash::{BuildHasher, Hasher};
    (0..2u64)
        .map(|i| {
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u64(i);
            h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
            format!("{:016x}", h.finish())
        })
        .collect()
}

/// Côté serveur MCP : une fenêtre jointe.
pub struct Client {
    flux: BufReader<TcpStream>,
    jeton: String,
    pub pid: u32,
}

impl Client {
    pub fn connecter(a: &Annonce) -> std::io::Result<Client> {
        let s = TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, a.port)), Duration::from_secs(2))?;
        // la fenêtre sert entre deux images ; une fenêtre réduite peut tarder
        s.set_read_timeout(Some(Duration::from_secs(30)))?;
        Ok(Client { flux: BufReader::new(s), jeton: a.jeton.clone(), pid: a.pid })
    }

    /// Envoie une demande et attend sa réponse. `Err` : la fenêtre ne répond plus (fermée, bloquée).
    pub fn demander(&mut self, demande: Demande) -> Result<Reponse, String> {
        let mut ligne = serde_json::to_string(&Message { jeton: self.jeton.clone(), demande }).map_err(|e| e.to_string())?;
        ligne.push('\n');
        self.flux.get_mut().write_all(ligne.as_bytes()).map_err(|e| format!("envoi : {e}"))?;
        let mut rep = String::new();
        match self.flux.read_line(&mut rep) {
            Ok(0) => return Err("la fenêtre a raccroché".into()),
            Ok(_) => {}
            Err(e) => return Err(format!("réponse : {e}")),
        }
        serde_json::from_str(&rep).map_err(|e| format!("réponse illisible : {e}"))
    }
}

/// Les fenêtres ouvertes qui répondent ; l'annonce d'une fenêtre qui ne répond plus (arrêtée net) est effacée.
pub fn fenetres() -> Vec<Client> {
    let Ok(entrees) = std::fs::read_dir(dossier()) else { return Vec::new() };
    let mut v = Vec::new();
    for e in entrees.flatten() {
        let Some(a) = std::fs::read(e.path()).ok().and_then(|o| serde_json::from_slice::<Annonce>(&o).ok()) else { continue };
        match Client::connecter(&a) {
            Ok(c) => v.push(c),
            Err(_) => {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    v
}

/// La fenêtre qui a ce fichier ouvert, avec son état.
pub fn fenetre_de(carte: &Path) -> Option<(Client, Reponse)> {
    let voulu = std::fs::canonicalize(carte).ok()?;
    fenetres().into_iter().find_map(|mut c| {
        let r = c.demander(Demande::Etat).ok()?;
        (r.carte.as_deref().and_then(|p| std::fs::canonicalize(p).ok()) == Some(voulu.clone())).then_some((c, r))
    })
}
