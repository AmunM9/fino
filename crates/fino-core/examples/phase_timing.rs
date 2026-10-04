//! Where the time goes for one photo, single file (all cores available to it).
use fino_core::codec::{self, Effort, EncodeParams};
use fino_core::{metric::Reference, search, Strength};
use std::time::Instant;
fn main() {
    for path in std::env::args().skip(1) {
        let data = std::fs::read(&path).unwrap();
        let t0 = Instant::now();
        let px = codec::decode(&data, false).unwrap();
        let t_dec = t0.elapsed().as_millis();
        let t1 = Instant::now();
        let r = Reference::new(&px).unwrap();
        let t_ref = t1.elapsed().as_millis();
        let chroma = fino_core::jpeg::inspect(&data).unwrap().chroma_block();
        let t2 = Instant::now();
        let c = search::find_smallest(&px, &r, chroma, Strength::Identical, None)
            .unwrap()
            .unwrap();
        let t_search = t2.elapsed().as_millis();
        let t3 = Instant::now();
        let fast = codec::encode(
            &px,
            EncodeParams::new(c.quality as f32, chroma, Effort::Fast),
        )
        .unwrap();
        let t_fast = t3.elapsed().as_millis();
        let t4 = Instant::now();
        let max = codec::encode(
            &px,
            EncodeParams::new(c.quality as f32, chroma, Effort::Max),
        )
        .unwrap();
        let t_max = t4.elapsed().as_millis();
        println!("{path}\n  decode {t_dec}ms  reference {t_ref}ms  search+finalize {t_search}ms -> q{} {:?}", c.quality, c.effort);
        println!(
            "  at q{}: fast {} bytes ({t_fast}ms)  max {} bytes ({t_max}ms)  max is {:.1}% smaller",
            c.quality,
            fast.len(),
            max.len(),
            100.0 * (1.0 - max.len() as f64 / fast.len() as f64)
        );
    }
}
