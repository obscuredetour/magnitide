//! `mgp`: check and run a Magnitide plugin package without the app.
//!
//! ```text
//! mgp check <plugin.mgp>                      the manifest, and what the component handles
//! mgp identify <plugin.mgp> <title> [author]  ask a metadata source (a title "isbn:978…" asks by ISBN)
//! mgp read <plugin.mgp> <book file>           read a file's metadata through a file-metadata plugin
//! mgp hook <plugin.mgp> <book file>           run an import hook on a file
//! ```
//!
//! The package is installed into a temporary folder, with the capabilities
//! its manifest asks for; the plugin's log follows the answer.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use magnitide_core::hooks::{ImportHooks, ImportOutcome, MetadataPlugins};
use magnitide_plugins_wasm::{Kind, WasmPlugins};
use magnitide_sources::Lookup;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let (command, rest) = args.split_first().ok_or(USAGE)?;
    let (mgp, rest) = rest.split_first().ok_or(USAGE)?;
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let plugins = WasmPlugins::new(dir.path().join("plugins"))?;
    let installed = plugins.install(Path::new(mgp))?;
    let m = &installed.manifest;
    let result = match (command.as_str(), rest) {
        ("check", []) => {
            println!("{} {} by {}", m.name, m.version, if m.author.is_empty() { "(nobody)" } else { &m.author });
            println!("  {}", m.description);
            println!("  kind: {:?}, API {}, capabilities: {:?}", m.kind, m.api, m.capabilities);
            let h = &installed.handles;
            match m.kind {
                Kind::MetadataSource => {
                    let (name, description, version) = plugins.info(&installed)?;
                    println!("  info(): {name} {version}: {description}");
                }
                Kind::ImportHook => println!("  runs on: {}", h.file_types.join(", ")),
                Kind::FileMetadata => {
                    let reads: Vec<String> = h
                        .reads
                        .iter()
                        .map(|r| format!("{}{}", r.format, if r.overrides { " (overrides)" } else { "" }))
                        .collect();
                    println!("  reads: {}\n  writes: {}", reads.join(", "), h.writes.join(", "));
                }
                Kind::LibraryAction => {
                    for item in &h.menu {
                        println!(
                            "  menu: {} ({}{})",
                            item.text,
                            item.id,
                            if item.needs_selection { ", needs a selection" } else { "" }
                        );
                    }
                }
            }
            Ok(())
        }
        ("identify", [title, rest @ ..]) => {
            let lookup = match title.strip_prefix("isbn:") {
                Some(isbn) => Lookup { title: String::new(), authors: rest.to_vec(), isbn: Some(isbn.to_owned()) },
                None => Lookup { title: title.clone(), authors: rest.to_vec(), isbn: None },
            };
            for (name, answer) in plugins.identify(&lookup) {
                match answer {
                    Ok(records) => {
                        println!("{name}: {} record(s)", records.len());
                        for r in records {
                            println!("  {} by {} ({})", r.title, r.authors.join(", "), r.source_key);
                            for (k, v) in &r.identifiers {
                                println!("    {k}: {v}");
                            }
                            if let Some(p) = &r.published {
                                println!("    published: {p}");
                            }
                            if let Some(c) = &r.cover_url {
                                println!("    cover: {c}");
                            }
                        }
                    }
                    Err(e) => println!("{name}: error: {e}"),
                }
            }
            Ok(())
        }
        ("read", [file]) => {
            let (path, format) = book(file)?;
            let meta = plugins.read(&path, &format)?;
            println!("{meta:#?}");
            Ok(())
        }
        ("hook", [file]) => {
            let (path, format) = book(file)?;
            match plugins.on_import(&path, &format) {
                ImportOutcome::Unchanged => println!("unchanged"),
                ImportOutcome::Replaced { path: new, .. } => {
                    let kept = std::env::current_dir()
                        .map_err(|e| e.to_string())?
                        .join(new.file_name().ok_or("the plugin made a file with no name")?);
                    std::fs::rename(&new, &kept)
                        .or_else(|_| std::fs::copy(&new, &kept).map(|_| ()))
                        .map_err(|e| e.to_string())?;
                    println!("replaced: {}", kept.display());
                }
                ImportOutcome::Failed(why) => println!("failed: {why}"),
            }
            Ok(())
        }
        _ => Err(USAGE.to_owned()),
    };
    let log = plugins.log(&m.name);
    if !log.is_empty() {
        eprintln!("log:");
        for line in log {
            eprintln!("  {line}");
        }
    }
    result
}

fn book(file: &str) -> Result<(PathBuf, String), String> {
    let path = PathBuf::from(file);
    let format = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_uppercase)
        .ok_or_else(|| format!("{file} has no extension"))?;
    Ok((path, format))
}

const USAGE: &str = "usage: mgp check <plugin.mgp> | identify <plugin.mgp> <title or isbn:…> [author…] | read <plugin.mgp> <book file> | hook <plugin.mgp> <book file>";
