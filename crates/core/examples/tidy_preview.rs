//! Show what "Tidy file names" would change in a Magnitide library (changes nothing):
//!
//!     cargo run --release -p magnitide-core --example tidy_preview -- <magnitide library>

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: tidy_preview <magnitide library>");
        std::process::exit(2);
    };
    match magnitide_core::Library::open(&path).and_then(|lib| Ok((lib.info()?, lib.tidy_preview()?))) {
        Ok((info, p)) => {
            println!("{}: {} books; {} would be renamed ({} files)", info.name, info.book_count, p.books, p.files)
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}
