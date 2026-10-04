//! Debug aid: what ImageIO and Fino see in a HEIC and in its converted JPEG.
//!
//!     cargo run -p fino-core --example heic_debug -- photo.heic [out.jpg]
fn main() {
    let path = std::env::args().nth(1).expect("path");
    let data = std::fs::read(&path).unwrap();
    let info = fino_core::heic::inspect(&data).unwrap();
    println!(
        "container aux: {:?} iso={} bits={}",
        info.aux_types, info.iso_gain_map, info.bit_depth
    );
    #[cfg(target_os = "macos")]
    {
        println!(
            "imageio aux in source: {:?}",
            fino_core::heic::decode::aux_present(&data)
        );
        let d = fino_core::heic::decode::decode(&data, true).unwrap();
        println!(
            "orientation {} native {} carrier {:?} bytes",
            d.orientation,
            d.native_space,
            d.carrier.as_ref().map(Vec::len)
        );
        if let Some(c) = &d.carrier {
            println!("carrier aux: {:?}", fino_core::heic::decode::aux_present(c));
            match fino_core::jpeg::mpf::secondary_images(c) {
                Ok(s) => println!(
                    "carrier secondaries: {:?}",
                    s.iter()
                        .map(|i| (i.attribute, i.bytes.len()))
                        .collect::<Vec<_>>()
                ),
                Err(e) => println!("carrier MPF: {e}"),
            }
            std::fs::write(std::env::temp_dir().join("fino-carrier.jpg"), c).unwrap();
        }
    }
    if let fino_core::Outcome::Optimized(o) =
        fino_core::optimize(&data, &Default::default()).unwrap()
    {
        let out = std::env::args()
            .nth(2)
            .unwrap_or("/tmp/fino-converted.jpg".into());
        std::fs::write(&out, &o.bytes).unwrap();
        println!(
            "converted: {} bytes (q{}) → {out}",
            o.bytes.len(),
            o.quality
        );
        #[cfg(target_os = "macos")]
        println!(
            "output aux: {:?}",
            fino_core::heic::decode::aux_present(&o.bytes)
        );
    }
}
