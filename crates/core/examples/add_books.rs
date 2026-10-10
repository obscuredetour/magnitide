//! Add books to a Magnitide library from the command line, making the library
//! first if the folder doesn't hold one (as the landing site's screenshots do,
//! from Standard Ebooks):
//!
//!     cargo run --release -p magnitide-core --example add_books -- <library> [--name <name>] <file or folder>...

use std::path::PathBuf;

use magnitide_core::Library;

fn main() {
    let mut args = std::env::args().skip(1);
    let usage = || -> ! {
        eprintln!("usage: add_books <library> [--name <name>] <file or folder>...");
        std::process::exit(2);
    };
    let root = PathBuf::from(args.next().unwrap_or_else(|| usage()));
    let mut name = None;
    let mut paths = Vec::new();
    while let Some(a) = args.next() {
        if a == "--name" {
            name = Some(args.next().unwrap_or_else(|| usage()));
        } else {
            paths.push(PathBuf::from(a));
        }
    }
    if paths.is_empty() {
        usage();
    }

    let opened = if Library::exists_at(&root) {
        Library::open(&root)
    } else {
        let name = name.unwrap_or_else(|| root.file_name().map_or("Library".into(), |n| n.to_string_lossy().into()));
        Library::create(&root, &name)
    };
    let mut lib = opened.unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(1);
    });
    match magnitide_core::import::add_files(&mut lib, &paths, |p| eprintln!("{}/{} {}", p.done, p.total, p.title)) {
        Ok(r) => {
            println!(
                "added {} books ({} files, {} covers); {} duplicates, {} skipped, {} failed",
                r.added.len(),
                r.files,
                r.covers,
                r.duplicates.len(),
                r.skipped.len(),
                r.failed.len()
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
