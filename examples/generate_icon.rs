fn main() {
    std::fs::create_dir_all("assets").unwrap();
    std::fs::write("assets/limedaw.svg", limedaw::icon::svg()).unwrap();
    // A plain PPM alongside it, so the PNG can be produced without pulling in
    // an encoder just for a build asset.
    let size = 256u32;
    let rgba = limedaw::icon::rgba(size);
    let mut ppm = format!("P6\n{size} {size}\n255\n").into_bytes();
    for px in rgba.chunks(4) {
        ppm.extend_from_slice(&px[..3]);
    }
    std::fs::write("assets/limedaw.ppm", ppm).unwrap();
    println!("wrote assets/limedaw.svg and assets/limedaw.ppm");
}
