//! Merge one book into another in a Magnitide library (the app must not have
//! the library open): `cargo run -p magnitide-core --example merge_books -- <library> <keep id> <other id>`
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let [_, lib, keep, other] = args.as_slice() else { return Err("usage: merge_books <library> <keep id> <other id>".into()) };
    let mut lib = magnitide_core::Library::open(lib)?;
    let outcome = lib.merge_books(keep.parse()?, other.parse()?)?;
    println!("{outcome:?}");
    Ok(())
}
