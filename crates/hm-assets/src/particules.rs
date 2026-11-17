//! Noms des systèmes de particules d'un `.pcf` (DMX binaire, wiki Valve « DMX/Binary ») : en-tête texte, dictionnaire
//! de chaînes (version 2 et plus), puis l'en-tête de chaque élément (type, nom, identifiant de 16 octets) ; les
//! attributs, rangés après tous les en-têtes, ne sont pas lus. Les systèmes sont les éléments de type
//! `DmeParticleSystemDefinition`.

struct Lecteur<'a> {
    o: &'a [u8],
    i: usize,
}

impl Lecteur<'_> {
    fn octets(&mut self, n: usize) -> Result<&[u8], String> {
        let b = self.o.get(self.i..self.i + n).ok_or(hm_langue::t("fichier tronqué"))?;
        self.i += n;
        Ok(b)
    }
    fn i16(&mut self) -> Result<usize, String> {
        let b = self.octets(2)?;
        Ok(i16::from_le_bytes([b[0], b[1]]).max(0) as usize)
    }
    fn i32(&mut self) -> Result<usize, String> {
        let b = self.octets(4)?;
        Ok(i32::from_le_bytes([b[0], b[1], b[2], b[3]]).max(0) as usize)
    }
    fn chaine(&mut self) -> Result<String, String> {
        let fin = self.o[self.i..].iter().position(|c| *c == 0).ok_or(hm_langue::t("chaîne sans fin"))?;
        let s = String::from_utf8_lossy(&self.o[self.i..self.i + fin]).into_owned();
        self.i += fin + 1;
        Ok(s)
    }
}

pub fn systemes(pcf: &[u8]) -> Result<Vec<String>, String> {
    let fin_entete = pcf.windows(3).position(|w| w == b"-->").ok_or(hm_langue::t("pas un DMX"))? + 3;
    let entete = String::from_utf8_lossy(&pcf[..fin_entete]);
    let mots: Vec<&str> = entete.split_whitespace().collect();
    let k = mots.iter().position(|m| *m == "binary").ok_or(hm_langue::t("DMX qui n'est pas binaire"))?;
    let v: u32 = mots.get(k + 1).and_then(|x| x.parse().ok()).ok_or(hm_langue::t("version du DMX illisible"))?;
    let mut l = Lecteur { o: pcf, i: fin_entete };
    // le texte d'en-tête se termine par une fin de ligne et un zéro
    while matches!(l.o.get(l.i), Some(b'\n' | b'\r' | 0)) {
        l.i += 1;
        if l.o.get(l.i - 1) == Some(&0) {
            break;
        }
    }
    let mut dico = Vec::new();
    if v >= 2 {
        let n = if v >= 4 { l.i32()? } else { l.i16()? };
        for _ in 0..n {
            dico.push(l.chaine()?);
        }
    }
    let indice = |l: &mut Lecteur, dico: &Vec<String>| -> Result<String, String> {
        let i = if v >= 5 { l.i32()? } else { l.i16()? };
        dico.get(i).cloned().ok_or_else(|| hm_langue::tr!("indice de chaîne {i} hors du dictionnaire", "string index {i} outside the dictionary"))
    };
    let n = l.i32()?;
    let mut noms = Vec::new();
    for _ in 0..n {
        let genre = if v >= 2 { indice(&mut l, &dico)? } else { l.chaine()? };
        let nom = if v >= 4 { indice(&mut l, &dico)? } else { l.chaine()? };
        l.octets(16)?;
        if genre == "DmeParticleSystemDefinition" {
            noms.push(nom);
        }
    }
    Ok(noms)
}
