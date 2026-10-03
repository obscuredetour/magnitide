use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use calibre_compat::{Book, Datatype, Library, Query, Snapshot, SortKey, locate, sort_books, synth};
use clap::{Parser, Subcommand};

/// Read a calibre library without calibre. Never writes to an existing library.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// Library folder or metadata.db. Defaults to the library calibre last used.
    #[arg(short, long, global = true, env = "CALIBRE_LIBRARY")]
    library: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Schema version, counts, custom columns and load time
    Info,
    /// List books, optionally filtered
    List {
        /// e.g. 'author:"le guin" tag:fantasy -#read:yes'
        #[arg(short, long)]
        search: Option<String>,
        /// title, author, added, published, rating or series
        #[arg(long, default_value = "title")]
        sort: SortKey,
        #[arg(short = 'n', long)]
        limit: Option<usize>,
    },
    /// Everything stored for one book
    Show { id: i64 },
    /// Dump the library as JSON to stdout
    Export {
        #[arg(long)]
        pretty: bool,
    },
    /// Check that format files and covers recorded in the database exist on disk
    Verify,
    /// Time opening and fully loading the library
    Bench {
        #[arg(long, default_value_t = 5)]
        runs: usize,
    },
    /// Create a NEW synthetic library for benchmarking (refuses to overwrite)
    Synth {
        dir: PathBuf,
        #[arg(long, default_value_t = 10_000)]
        books: usize,
        /// Also create book folders with placeholder files
        #[arg(long)]
        with_files: bool,
        #[arg(long, default_value_t = 42)]
        seed: u64,
    },
}

fn main() -> Result<()> {
    // Exit quietly when piped into `head` etc. instead of panicking on EPIPE.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    let cli = Cli::parse();

    if let Cmd::Synth { dir, books, with_files, seed } = &cli.cmd {
        let t = Instant::now();
        synth::create(dir, &synth::Options { books: *books, with_files: *with_files, seed: *seed })?;
        eprintln!("created {books} books in {} ({:.2?})", dir.display(), t.elapsed());
        return Ok(());
    }

    let lib_path = cli
        .library
        .clone()
        .or_else(locate::default_library)
        .context("no library given and calibre's current library could not be found; pass --library PATH")?;

    let t = Instant::now();
    let lib = Library::open(&lib_path)?;
    let snap = lib.load()?;
    let load_time = t.elapsed();
    if let Some(note) = lib.schema_note() {
        eprintln!("note: {note}");
    }
    for w in &snap.warnings {
        eprintln!("warning: {w}");
    }

    match cli.cmd {
        Cmd::Info => info(&lib, &snap, load_time)?,
        Cmd::List { search, sort, limit } => list(&snap, search.as_deref(), sort, limit)?,
        Cmd::Show { id } => show(&lib, &snap, id)?,
        Cmd::Export { pretty } => {
            let out = std::io::stdout().lock();
            if pretty {
                serde_json::to_writer_pretty(out, &snap)?
            } else {
                serde_json::to_writer(out, &snap)?
            }
            println!();
        }
        Cmd::Verify => verify(&lib, &snap),
        Cmd::Bench { runs } => bench(&lib_path, runs, &snap)?,
        Cmd::Synth { .. } => unreachable!(),
    }
    Ok(())
}

fn info(lib: &Library, snap: &Snapshot, load_time: Duration) -> Result<()> {
    let mut formats: BTreeMap<&str, usize> = BTreeMap::new();
    for f in snap.books.iter().flat_map(|b| &b.formats) {
        *formats.entry(&f.format).or_default() += 1;
    }
    let authors: std::collections::HashSet<&str> =
        snap.books.iter().flat_map(|b| b.authors.iter().map(String::as_str)).collect();

    println!("Library:        {}", lib.root().display());
    println!("Schema version: {} (application_id {:#x})", lib.schema_version, lib.application_id);
    println!("Library UUID:   {}", snap.library_uuid.as_deref().unwrap_or("-"));
    println!("Books:          {}", snap.books.len());
    println!("Authors:        {}", authors.len());
    println!("With cover:     {}", snap.books.iter().filter(|b| b.has_cover).count());
    let fmts: Vec<String> = formats.iter().map(|(k, v)| format!("{k} {v}")).collect();
    println!("Formats:        {}", if fmts.is_empty() { "-".into() } else { fmts.join(", ") });
    if let Some(serde_json::Value::Object(m)) = lib.preference("saved_searches")? {
        println!("Saved searches: {}", m.len());
    }
    println!("Load time:      {load_time:.2?}");
    if !snap.custom_columns.is_empty() {
        println!("\nCustom columns:");
        for c in &snap.custom_columns {
            let kind = match (&c.datatype, c.is_multiple) {
                (Datatype::Composite, _) => format!("composite = {}", c.composite_template().unwrap_or("?")),
                (d, true) => format!("{d:?}, multiple").to_lowercase(),
                (d, false) => format!("{d:?}").to_lowercase(),
            };
            let used = snap.books.iter().filter(|b| b.custom.contains_key(&c.lookup_name())).count();
            println!("  #{:<16} {:<20} {:<40} {used} books", c.label, c.name, kind);
        }
    }
    Ok(())
}

fn list(snap: &Snapshot, search: Option<&str>, sort: SortKey, limit: Option<usize>) -> Result<()> {
    let query = search.map(Query::parse).transpose()?;
    let mut books: Vec<&Book> = snap.books.iter().filter(|b| query.as_ref().is_none_or(|q| q.matches(b))).collect();
    sort_books(&mut books, sort);
    let total = books.len();
    books.truncate(limit.unwrap_or(usize::MAX));

    println!("{:>6}  {:<42} {:<28} {:<26} {:<5} FORMATS", "ID", "TITLE", "AUTHORS", "SERIES", "RATE");
    for b in &books {
        let series = b.series.as_ref().map(|s| format!("{s} [{}]", b.series_index)).unwrap_or_default();
        let stars = b.stars().map(|s| format!("{s}★")).unwrap_or_default();
        let fmts: Vec<&str> = b.formats.iter().map(|f| f.format.as_str()).collect();
        println!(
            "{:>6}  {:<42} {:<28} {:<26} {:<5} {}",
            b.id,
            trunc(&b.title, 42),
            trunc(&b.authors.join(" & "), 28),
            trunc(&series, 26),
            stars,
            fmts.join(",")
        );
    }
    eprintln!("{} of {total} matching books shown ({} in library)", books.len(), snap.books.len());
    Ok(())
}

fn show(lib: &Library, snap: &Snapshot, id: i64) -> Result<()> {
    let b = snap.get(id).with_context(|| format!("no book with id {id}"))?;
    let root = lib.root();
    let date = |d: &Option<chrono::DateTime<chrono::Utc>>| d.map(|d| d.format("%Y-%m-%d %H:%M UTC").to_string());
    let rows: Vec<(&str, Option<String>)> = vec![
        ("Title", Some(b.title.clone())),
        ("Title sort", b.sort.clone()),
        ("Authors", Some(b.authors.join(" & ")).filter(|s| !s.is_empty())),
        ("Author sort", b.author_sort.clone()),
        ("Series", b.series.as_ref().map(|s| format!("{s} [{}]", b.series_index))),
        ("Tags", Some(b.tags.join(", ")).filter(|s| !s.is_empty())),
        ("Publisher", b.publisher.clone()),
        ("Rating", b.stars().map(|s| format!("{s}★"))),
        ("Languages", Some(b.languages.join(", ")).filter(|s| !s.is_empty())),
        (
            "Identifiers",
            Some(b.identifiers.iter().map(|(k, v)| format!("{k}:{v}")).collect::<Vec<_>>().join(", "))
                .filter(|s| !s.is_empty()),
        ),
        ("Published", date(&b.pubdate)),
        ("Added", date(&b.timestamp)),
        ("Modified", date(&b.last_modified)),
        ("UUID", b.uuid.clone()),
        ("Folder", Some(b.dir(root).display().to_string())),
    ];
    for (k, v) in rows {
        println!("{k:<14} {}", v.unwrap_or_else(|| "-".into()));
    }
    for f in &b.formats {
        let p = b.format_path(root, f);
        println!("{:<14} {} ({} bytes) {}", format!("Format {}", f.format), p.display(), f.size, mark(p.is_file()));
    }
    if let Some(p) = b.cover_path(root) {
        println!("{:<14} {} {}", "Cover", p.display(), mark(p.is_file()));
    }
    for c in &snap.custom_columns {
        let value = match (&c.datatype, b.custom.get(&c.lookup_name())) {
            (Datatype::Composite, _) => format!("(computed from template {})", c.composite_template().unwrap_or("?")),
            (_, Some(v)) => v.to_string(),
            (_, None) => "-".into(),
        };
        println!("{:<14} {value}", format!("#{}", c.label));
    }
    if let Some(c) = &b.comments {
        println!("\nComments (HTML):\n{c}");
    }
    Ok(())
}

fn verify(lib: &Library, snap: &Snapshot) {
    let root = lib.root();
    let (mut checked, mut missing) = (0, 0);
    for b in &snap.books {
        let paths = b.formats.iter().map(|f| b.format_path(root, f)).chain(b.cover_path(root));
        for p in paths {
            checked += 1;
            if !p.is_file() {
                missing += 1;
                println!("missing  book {:>6}  {}", b.id, p.display());
            }
        }
    }
    eprintln!("checked {checked} files, {missing} missing");
}

fn bench(path: &PathBuf, runs: usize, snap: &Snapshot) -> Result<()> {
    let mut times = Vec::with_capacity(runs);
    for _ in 0..runs {
        let t = Instant::now();
        let snap = Library::open(path)?.load()?;
        std::hint::black_box(&snap);
        times.push(t.elapsed());
    }
    times.sort();
    let q = Query::parse("the")?;
    let t = Instant::now();
    let hits = snap.books.iter().filter(|b| q.matches(b)).count();
    let search = t.elapsed();
    println!("books:            {}", snap.books.len());
    println!("open + full load: min {:.2?}, median {:.2?} ({runs} runs)", times[0], times[runs / 2]);
    println!("search \"the\":     {search:.2?} ({hits} hits, unindexed scan)");
    Ok(())
}

fn trunc(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_owned() } else { s.chars().take(n - 1).chain(['…']).collect() }
}

fn mark(ok: bool) -> &'static str {
    if ok { "✓" } else { "✗ missing" }
}
