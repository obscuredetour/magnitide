//! Import a calibre library from the command line:
//!
//!     cargo run --release -p magnitide-core --example import_calibre -- <calibre library> <magnitide library>

use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [src, dest] = args.as_slice() else {
        eprintln!("usage: import_calibre <calibre library> <magnitide library>");
        std::process::exit(2);
    };
    let t = Instant::now();
    let mut last = 0;
    let result = magnitide_core::import::import_calibre(src.as_ref(), dest.as_ref(), |p| {
        if p.done >= last + 1000 || p.done == p.total {
            eprintln!("{}/{}", p.done, p.total);
            last = p.done;
        }
    });
    match result {
        Ok(r) => {
            println!(
                "imported {} books ({} skipped, {} failed) in {:.2?}",
                r.books_imported,
                r.books_skipped,
                r.failed.len(),
                t.elapsed()
            );
            println!(
                "files {} · covers {} · cloned {} · copied {} ({:.1} MB) · custom fields {}",
                r.files,
                r.covers,
                r.cloned,
                r.copied,
                r.copied_bytes as f64 / 1e6,
                r.custom_fields
            );
            for w in r.warnings.iter().chain(&r.failed) {
                println!("  {w}");
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}
