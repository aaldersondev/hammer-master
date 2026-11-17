//! Les sons du jeu pour le navigateur de sons : les noms des soundscripts (wiki Valve « Soundscripts » : chaque
//! entrée de premier niveau d'un fichier de sons est un nom, `"Weapon_Pistol.Single" { … }`), et les fichiers que le
//! manifeste `scripts/game_sounds_manifest.txt` déclare (`precache_file`, `preload_file`, `declare_file`).

/// Les mots d'un fichier KeyValues : chaînes entre guillemets, mots nus, accolades ; commentaires `//` sautés.
pub fn mots(t: &str) -> Vec<String> {
    let mut v = Vec::new();
    let mut c = t.chars().peekable();
    while let Some(x) = c.next() {
        match x {
            '/' if c.peek() == Some(&'/') => {
                for y in c.by_ref() {
                    if y == '\n' {
                        break;
                    }
                }
            }
            '{' | '}' => v.push(x.to_string()),
            '"' => v.push(c.by_ref().take_while(|y| *y != '"').collect()),
            x if x.is_whitespace() => {}
            x => {
                let mut m = x.to_string();
                while let Some(y) = c.peek().copied().filter(|y| !y.is_whitespace() && !matches!(y, '{' | '}' | '"')) {
                    m.push(y);
                    c.next();
                }
                v.push(m);
            }
        }
    }
    v
}

/// Les noms de sons d'un fichier de soundscripts.
pub fn noms_soundscript(texte: &str) -> Vec<String> {
    let m = mots(texte);
    let mut profondeur = 0usize;
    let mut noms = Vec::new();
    for (i, x) in m.iter().enumerate() {
        match x.as_str() {
            "{" => profondeur += 1,
            "}" => profondeur = profondeur.saturating_sub(1),
            _ if profondeur == 0 && m.get(i + 1).map(String::as_str) == Some("{") => noms.push(x.clone()),
            _ => {}
        }
    }
    noms
}

/// Les sons d'un fichier de soundscripts avec le premier fichier qu'ils jouent (`wave`, relatif à `sound/`, sans les
/// caractères de réglage du début comme `)` ou `^`).
pub fn entrees_soundscript(texte: &str) -> Vec<(String, Option<String>)> {
    let m = mots(texte);
    let mut profondeur = 0usize;
    let mut v: Vec<(String, Option<String>)> = Vec::new();
    for (i, x) in m.iter().enumerate() {
        match x.as_str() {
            "{" => profondeur += 1,
            "}" => profondeur = profondeur.saturating_sub(1),
            _ if profondeur == 0 && m.get(i + 1).map(String::as_str) == Some("{") => v.push((x.clone(), None)),
            _ if profondeur > 0 && x.eq_ignore_ascii_case("wave") => {
                if let (Some(last), Some(w)) = (v.last_mut(), m.get(i + 1)) {
                    if last.1.is_none() && w != "{" {
                        last.1 = Some(w.trim_start_matches(|c: char| ")^*#@<>!?$&~`+%".contains(c)).replace('\\', "/"));
                    }
                }
            }
            _ => {}
        }
    }
    v
}

/// Les fichiers de soundscripts que déclare le manifeste.
pub fn fichiers_du_manifeste(texte: &str) -> Vec<String> {
    let m = mots(texte);
    m.windows(2).filter(|w| matches!(w[0].to_ascii_lowercase().as_str(), "precache_file" | "preload_file" | "declare_file")).map(|w| w[1].replace('\\', "/")).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn soundscript_et_manifeste() {
        let t = "// sons\n\"Weapon_Pistol.Single\"\n{\n\t\"channel\" \"CHAN_WEAPON\"\n\t\"rndwave\" { \"wave\" \"weapons/pistol/pistol_fire2.wav\" }\n}\nDoor.Open { wave doors/door1.wav }\n";
        assert_eq!(super::noms_soundscript(t), ["Weapon_Pistol.Single", "Door.Open"]);
        let e = super::entrees_soundscript(&t.replace("weapons/pistol", ")weapons/pistol"));
        assert_eq!(e, [("Weapon_Pistol.Single".to_string(), Some("weapons/pistol/pistol_fire2.wav".to_string())), ("Door.Open".to_string(), Some("doors/door1.wav".to_string()))]);
        let m = "game_sounds_manifest\n{\n\t\"precache_file\" \"scripts/game_sounds.txt\"\n\t\"preload_file\" \"scripts\\game_sounds_world.txt\" // x\n}";
        assert_eq!(super::fichiers_du_manifeste(m), ["scripts/game_sounds.txt", "scripts/game_sounds_world.txt"]);
    }
}
