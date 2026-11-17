//! Montre le premier écart entre un VMF et sa réécriture : `cargo run --example diff_octet -- carte.vmf`.
fn main() {
    let p = std::env::args().nth(1).expect("carte.vmf");
    let src = std::fs::read(&p).unwrap();
    let (d, r) = hm_doc::Document::lire(&src).unwrap();
    for x in &r.renumerotes {
        println!("{x}");
    }
    let out = d.ecrire();
    let i = src.iter().zip(&out).position(|(a, b)| a != b).unwrap_or(src.len().min(out.len()));
    println!("taille {} -> {}, premier écart à l'octet {i}", src.len(), out.len());
    let a = i.saturating_sub(200);
    println!("--- source\n{}\n--- réécrit\n{}", String::from_utf8_lossy(&src[a..(i + 200).min(src.len())]), String::from_utf8_lossy(&out[a..(i + 200).min(out.len())]));
}
