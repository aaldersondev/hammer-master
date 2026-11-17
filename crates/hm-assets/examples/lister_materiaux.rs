fn main() {
    let t = std::time::Instant::now();
    let mut vfs = hm_assets::Vfs::new();
    vfs.add_gameinfo(hm_assets::jeu::gameinfo_gmod().unwrap()).unwrap();
    println!("montage {:.2} s, {} points de montage", t.elapsed().as_secs_f32(), vfs.mounts().len());
    let t = std::time::Instant::now();
    let l = vfs.list_dans("materials", ".vmt");
    println!("{} matériaux en {:.2} s", l.len(), t.elapsed().as_secs_f32());
}
