//! Theme add-ons: theme files in the app's config folder (`themes/*.json`),
//! in the format described in `docs/themes.md`, or a Catppuccin-style
//! `palette.json` (one theme per flavour).
//!
//! A theme file is data, never CSS: every colour is checked against a small
//! grammar (a hex colour, a reference to another colour, or a mix of two) and
//! only known roles can be overridden, so a theme can recolour the app but
//! can't restyle it, hide parts of it, or load anything from the network.
//! The values returned here are safe to put in a stylesheet as they are.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager};
use ts_rs::TS;

use crate::commands::CmdResult;

/// The theme file format this version reads.
const FORMAT: u64 = 1;

/// Roles a theme may override (see `crates/app/ui/src/themes/index.css`).
const ROLES: &[&str] = &[
    "bg",
    "panel",
    "popover",
    "field",
    "element",
    "element-hover",
    "fg",
    "fg-muted",
    "fg-faint",
    "link",
    "accent",
    "on-accent",
    "focus",
    "caret",
    "hover",
    "row-stripe",
    "selection",
    "on-selection",
    "text-selection",
    "highlight",
    "success",
    "warning",
    "danger",
    "info",
    "star",
    "on-status",
    "border",
    "inverse",
    "on-inverse",
    "scrim",
    "shadow",
];

/// Variables the app uses for other things, which a palette can't name.
const RESERVED: &[&str] = &["hue", "card-shadow", "lift-shadow", "scrim"];

/// Catppuccin's accents, in its order: the picker's default offer.
const ACCENTS: &[&str] = &[
    "rosewater",
    "flamingo",
    "pink",
    "mauve",
    "red",
    "maroon",
    "peach",
    "yellow",
    "green",
    "teal",
    "sky",
    "sapphire",
    "blue",
    "lavender",
];

/// A loaded theme, ready for the theme registry and a stylesheet.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ThemeAddon {
    /// Its `data-theme` value: the file's id, prefixed with `addon-`.
    pub id: String,
    pub name: String,
    pub author: Option<String>,
    /// "light" or "dark".
    pub appearance: String,
    pub accents: Vec<AddonAccent>,
    /// Custom properties (without the leading `--`) and their CSS values.
    pub variables: Vec<(String, String)>,
    /// The file it came from.
    pub file: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct AddonAccent {
    pub name: String,
    pub label: String,
}

/// A theme file that couldn't be used.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct ThemeProblem {
    pub file: String,
    pub message: String,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct ThemeAddons {
    pub themes: Vec<ThemeAddon>,
    pub problems: Vec<ThemeProblem>,
    /// Where theme files go.
    pub folder: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ThemeFile {
    format: u64,
    id: String,
    name: String,
    #[serde(default)]
    author: Option<String>,
    appearance: String,
    palette: BTreeMap<String, String>,
    #[serde(default)]
    roles: BTreeMap<String, String>,
    #[serde(default)]
    accents: Option<Vec<AccentSpec>>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum AccentSpec {
    Name(String),
    Labelled { name: String, label: String },
}

fn folder(app: &AppHandle) -> CmdResult<PathBuf> {
    Ok(app.path().app_config_dir().map_err(|e| e.to_string())?.join("themes"))
}

/// Every theme in the themes folder, and the files that couldn't be used.
pub fn load_folder(dir: &Path) -> ThemeAddons {
    let mut out = ThemeAddons { folder: dir.display().to_string(), ..Default::default() };
    let Ok(entries) = fs::read_dir(dir) else { return out };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("json")))
        .collect();
    files.sort();
    let mut seen = HashSet::new();
    for path in files {
        let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let parsed = fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|text| parse(&file, &text));
        match parsed {
            Ok(themes) => {
                for t in themes {
                    if seen.insert(t.id.clone()) {
                        out.themes.push(t);
                    } else {
                        out.problems.push(ThemeProblem {
                            file: file.clone(),
                            message: format!("another file already has the id \"{}\"", &t.id["addon-".len()..]),
                        });
                    }
                }
            }
            Err(message) => out.problems.push(ThemeProblem { file, message }),
        }
    }
    out
}

/// The themes in one file: a theme file, or a Catppuccin-style palette.
pub fn parse(file: &str, text: &str) -> Result<Vec<ThemeAddon>, String> {
    let json: Value = serde_json::from_str(text).map_err(|e| format!("not valid JSON: {e}"))?;
    if json.get("format").is_some() {
        let t: ThemeFile = serde_json::from_value(json).map_err(|e| format!("not a theme file: {e}"))?;
        return Ok(vec![theme(file, t)?]);
    }
    let flavours = catppuccin(file, &json)?;
    if flavours.is_empty() {
        return Err("not a theme file (no \"format\") or a Catppuccin palette (no flavours with \"colors\")".into());
    }
    Ok(flavours)
}

fn theme(file: &str, t: ThemeFile) -> Result<ThemeAddon, String> {
    if t.format != FORMAT {
        return Err(format!("format {} isn't supported (this version reads format {FORMAT})", t.format));
    }
    let valid_id = (1..=48).contains(&t.id.len())
        && t.id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if !valid_id || t.id.starts_with('-') {
        return Err(format!("the id \"{}\" should be 1–48 lowercase letters, digits and hyphens", t.id));
    }
    let name = clean_text(&t.name, 60).ok_or("the name is empty")?;
    let author = t.author.as_deref().and_then(|a| clean_text(a, 80));
    if t.appearance != "light" && t.appearance != "dark" {
        return Err(format!("appearance should be \"light\" or \"dark\", not \"{}\"", t.appearance));
    }
    for key in t.palette.keys() {
        if !is_name(key) {
            return Err(format!("the palette colour \"{key}\" should be lowercase letters, digits and hyphens"));
        }
        if ROLES.contains(&key.as_str()) {
            return Err(format!("\"{key}\" is a role; put it under \"roles\", not \"palette\""));
        }
        if RESERVED.contains(&key.as_str()) || ["tw-", "color-", "shadow-"].iter().any(|p| key.starts_with(p)) {
            return Err(format!("the palette colour \"{key}\" uses a name the app reserves"));
        }
    }
    if !t.palette.contains_key("base") || !t.palette.contains_key("text") {
        return Err("the palette needs at least \"base\" and \"text\"".into());
    }
    let known = |n: &str| {
        t.palette.contains_key(n)
            || ROLES.contains(&n)
            || DERIVED.iter().any(|(k, _)| *k == n)
            || ["blue", "red", "yellow", "green", "teal"].contains(&n)
    };
    let mut variables = Vec::new();
    for (key, value) in &t.palette {
        variables.push((key.clone(), colour(value, &known).map_err(|e| format!("palette \"{key}\": {e}"))?));
    }
    // Neutral steps the palette leaves out, between base and text, and the
    // colours the roles' defaults name, so every default resolves.
    for (key, value) in DERIVED {
        if !t.palette.contains_key(*key) {
            variables.push(((*key).to_owned(), (*value).to_owned()));
        }
    }
    let blue = if t.roles.contains_key("accent") { "var(--accent)" } else { "var(--text)" };
    for (key, value) in
        [("blue", blue), ("red", "#d9534f"), ("yellow", "#c99a2e"), ("green", "#3fa266"), ("teal", "#2f9e9e")]
    {
        if !t.palette.contains_key(key) {
            variables.push((key.to_owned(), value.to_owned()));
        }
    }
    for (key, value) in &t.roles {
        if !ROLES.contains(&key.as_str()) {
            return Err(format!("\"{key}\" isn't a role (see docs/themes.md)"));
        }
        variables.push((key.clone(), colour(value, &known).map_err(|e| format!("role \"{key}\": {e}"))?));
    }
    let accents = match t.accents {
        Some(list) => list
            .into_iter()
            .map(|a| {
                let (name, label) = match a {
                    AccentSpec::Name(n) => (n.clone(), title_case(&n)),
                    AccentSpec::Labelled { name, label } => (name, label),
                };
                if !t.palette.contains_key(&name) {
                    return Err(format!("the accent \"{name}\" isn't in the palette"));
                }
                Ok(AddonAccent { label: clean_text(&label, 30).unwrap_or_else(|| title_case(&name)), name })
            })
            .collect::<Result<_, String>>()?,
        None => ACCENTS
            .iter()
            .filter(|n| t.palette.contains_key(**n))
            .map(|n| AddonAccent { name: (*n).to_owned(), label: title_case(n) })
            .collect(),
    };
    Ok(ThemeAddon {
        id: format!("addon-{}", t.id),
        name,
        author,
        appearance: t.appearance,
        accents,
        variables,
        file: file.to_owned(),
    })
}

/// Fallbacks for neutral steps a palette doesn't give, mixed from base and text.
const DERIVED: &[(&str, &str)] = &[
    ("crust", "color-mix(in srgb, var(--base) 82%, black)"),
    ("mantle", "color-mix(in srgb, var(--base) 90%, black)"),
    ("surface-0", "color-mix(in srgb, var(--text) 12%, var(--base))"),
    ("surface-1", "color-mix(in srgb, var(--text) 19%, var(--base))"),
    ("surface-2", "color-mix(in srgb, var(--text) 26%, var(--base))"),
    ("overlay-0", "color-mix(in srgb, var(--text) 38%, var(--base))"),
    ("overlay-1", "color-mix(in srgb, var(--text) 48%, var(--base))"),
    ("overlay-2", "color-mix(in srgb, var(--text) 58%, var(--base))"),
    ("subtext-0", "color-mix(in srgb, var(--text) 70%, var(--base))"),
    ("subtext-1", "color-mix(in srgb, var(--text) 84%, var(--base))"),
];

/// A colour value: `#hex`, a colour's name (`surface-2`), a name or hex at a
/// percentage (`overlay-2 25%`: that much of it over transparent), or a mix
/// (`text 62% overlay-1`).
fn colour(value: &str, known: &dyn Fn(&str) -> bool) -> Result<String, String> {
    let term = |t: &str| -> Result<String, String> {
        if let Some(hex) = t.strip_prefix('#') {
            if matches!(hex.len(), 3 | 4 | 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Ok(t.to_ascii_lowercase());
            }
            return Err(format!("\"{t}\" isn't a hex colour"));
        }
        if t == "transparent" || t == "black" || t == "white" {
            return Ok(t.to_owned());
        }
        if is_name(t) && known(t) {
            return Ok(format!("var(--{t})"));
        }
        Err(format!("\"{t}\" isn't a hex colour or a colour in this theme"))
    };
    let parts: Vec<&str> = value.split_whitespace().collect();
    match parts.as_slice() {
        [a] => term(a),
        [a, p] => Ok(format!("color-mix(in srgb, {} {}, transparent)", term(a)?, percent(p)?)),
        [a, p, b] => Ok(format!("color-mix(in srgb, {} {}, {})", term(a)?, percent(p)?, term(b)?)),
        _ => Err(format!("\"{value}\" should be a colour, \"colour N%\" or \"colour N% colour\"")),
    }
}

fn percent(p: &str) -> Result<String, String> {
    let n: f64 = p
        .strip_suffix('%')
        .and_then(|n| n.parse().ok())
        .filter(|n: &f64| (0.0..=100.0).contains(n))
        .ok_or_else(|| format!("\"{p}\" should be a percentage, 0% to 100%"))?;
    Ok(format!("{n}%"))
}

fn is_name(s: &str) -> bool {
    (1..=32).contains(&s.len())
        && s.as_bytes()[0].is_ascii_lowercase()
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Text for display: control characters removed, trimmed, at most `max` characters.
fn clean_text(s: &str, max: usize) -> Option<String> {
    let t: String = s.chars().filter(|c| !c.is_control()).take(max).collect();
    let t = t.trim();
    (!t.is_empty()).then(|| t.to_owned())
}

fn title_case(s: &str) -> String {
    s.split('-')
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A Catppuccin-style `palette.json`: flavours with `dark` and `colors`
/// (`{ "surface0": { "hex": "#313244", "accent": false }, … }`). Each flavour
/// becomes a theme set up like the built-in Catppuccin ones.
fn catppuccin(file: &str, json: &Value) -> Result<Vec<ThemeAddon>, String> {
    let Some(obj) = json.as_object() else { return Ok(Vec::new()) };
    let stem = Path::new(file).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut out = Vec::new();
    for (key, flavour) in obj {
        let Some(colors) = flavour.get("colors").and_then(Value::as_object) else { continue };
        let mut palette = BTreeMap::new();
        let mut accents = Vec::new();
        for (name, c) in colors {
            let Some(hex) = c.get("hex").and_then(Value::as_str) else { continue };
            // surface0 → surface-0, subtext1 → subtext-1.
            let name = match name.find(|ch: char| ch.is_ascii_digit()) {
                Some(i) => format!("{}-{}", &name[..i], &name[i..]),
                None => name.clone(),
            };
            if c.get("accent").and_then(Value::as_bool) == Some(true) {
                let label = c.get("name").and_then(Value::as_str).map_or_else(|| title_case(&name), str::to_owned);
                accents.push((c.get("order").and_then(Value::as_u64).unwrap_or(99), name.clone(), label));
            }
            palette.insert(name, hex.to_owned());
        }
        accents.sort();
        let mut roles = BTreeMap::new();
        for (role, colour) in [("accent", "mauve"), ("focus", "lavender"), ("caret", "rosewater")] {
            if palette.contains_key(colour) {
                roles.insert(role.to_owned(), colour.to_owned());
            }
        }
        let dark = flavour.get("dark").and_then(Value::as_bool).unwrap_or(true);
        let label = flavour.get("name").and_then(Value::as_str).unwrap_or(key);
        let t = ThemeFile {
            format: FORMAT,
            id: format!("{}-{}", slug(&stem), slug(key)),
            name: format!("{} {label}", title_case(&slug(&stem))),
            author: None,
            appearance: if dark { "dark" } else { "light" }.into(),
            palette,
            roles,
            accents: Some(accents.into_iter().map(|(_, name, label)| AccentSpec::Labelled { name, label }).collect()),
        };
        out.push(theme(file, t).map_err(|e| format!("flavour \"{key}\": {e}"))?);
    }
    Ok(out)
}

fn slug(s: &str) -> String {
    let s: String = s.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    s.split('-').filter(|w| !w.is_empty()).collect::<Vec<_>>().join("-")
}

#[tauri::command]
pub async fn theme_addons(app: AppHandle) -> CmdResult<ThemeAddons> {
    let dir = folder(&app)?;
    tauri::async_runtime::spawn_blocking(move || load_folder(&dir)).await.map_err(|e| e.to_string())
}

/// Copy a theme file into the themes folder (replacing one of the same name),
/// after checking it can be used.
#[tauri::command]
pub async fn add_theme_file(app: AppHandle, path: String) -> CmdResult<ThemeAddons> {
    let dir = folder(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let src = PathBuf::from(&path);
        let name = src.file_name().ok_or("that isn't a file")?.to_string_lossy().into_owned();
        let text = fs::read_to_string(&src).map_err(|e| e.to_string())?;
        parse(&name, &text).map_err(|e| format!("{name} can't be used: {e}"))?;
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let name = if name.to_ascii_lowercase().ends_with(".json") { name } else { format!("{name}.json") };
        fs::write(dir.join(name), text).map_err(|e| e.to_string())?;
        Ok(load_folder(&dir))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Show the themes folder (made if missing) in the file manager.
#[tauri::command]
pub async fn open_themes_folder(app: AppHandle) -> CmdResult<()> {
    let dir = folder(&app)?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    open::that_detached(&dir).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROSE_PINE: &str = r##"{
      "format": 1, "id": "rose-pine", "name": "Rosé Pine", "author": "Example", "appearance": "dark",
      "palette": {
        "crust": "#191724", "mantle": "#1f1d2e", "base": "#191724",
        "surface-0": "#26233a", "overlay-1": "#6e6a86", "subtext-0": "#908caa", "text": "#e0def4",
        "love": "#eb6f92", "gold": "#f6c177", "rose": "#ebbcba", "pine": "#31748f", "foam": "#9ccfd8", "iris": "#c4a7e7"
      },
      "roles": { "accent": "iris", "danger": "love", "selection": "iris", "hover": "text 8%", "border": "overlay-1 30% transparent" },
      "accents": ["love", "gold", { "name": "rose", "label": "Rosé" }, "pine", "foam", "iris"]
    }"##;

    fn var<'a>(t: &'a ThemeAddon, k: &str) -> &'a str {
        &t.variables.iter().find(|(n, _)| n == k).unwrap().1
    }

    #[test]
    fn reads_a_theme_file() {
        let t = parse("rose-pine.json", ROSE_PINE).unwrap().remove(0);
        assert_eq!((t.id.as_str(), t.name.as_str(), t.appearance.as_str()), ("addon-rose-pine", "Rosé Pine", "dark"));
        assert_eq!(var(&t, "love"), "#eb6f92");
        assert_eq!(var(&t, "accent"), "var(--iris)");
        assert_eq!(var(&t, "hover"), "color-mix(in srgb, var(--text) 8%, transparent)");
        assert_eq!(var(&t, "border"), "color-mix(in srgb, var(--overlay-1) 30%, transparent)");
        // Steps it left out are derived, so the roles' defaults still work.
        assert!(var(&t, "surface-1").starts_with("color-mix(in srgb, var(--text)"));
        assert_eq!(var(&t, "blue"), "var(--accent)", "links follow its accent");
        assert_eq!(var(&t, "teal"), "#2f9e9e");
        assert_eq!(
            t.accents.iter().map(|a| a.label.as_str()).collect::<Vec<_>>(),
            ["Love", "Gold", "Rosé", "Pine", "Foam", "Iris"]
        );
    }

    #[test]
    fn refuses_anything_but_colours() {
        let with =
            |field: &str, value: &str| ROSE_PINE.replace(r#""danger": "love""#, &format!(r#""{field}": "{value}""#));
        for bad in [
            "red; } body { display: none",
            "url(https://example.com/x.png)",
            "#12345",
            "var(--text)",
            "nonexistent",
            "text 150%",
            "text 10% love 20%",
        ] {
            let err = parse("x.json", &with("danger", bad)).unwrap_err();
            assert!(err.contains("role \"danger\""), "{bad}: {err}");
        }
        assert!(parse("x.json", &with("display", "love")).unwrap_err().contains("isn't a role"));
        assert!(parse("x.json", &ROSE_PINE.replace("\"rose-pine\"", "\"Rose Pine\"")).unwrap_err().contains("id"));
        assert!(parse("x.json", &ROSE_PINE.replace("\"dark\"", "\"dim\"")).unwrap_err().contains("appearance"));
        assert!(
            parse("x.json", &ROSE_PINE.replace("\"format\": 1", "\"format\": 2")).unwrap_err().contains("format 2")
        );
        assert!(parse("x.json", &ROSE_PINE.replace("\"author\"", "\"script\"")).is_err(), "unknown fields");
        assert!(parse("x.json", "[1, 2]").is_err());
        let reserved =
            ROSE_PINE.replace(r##""love": "#eb6f92""##, r##""tw-ring-color": "#eb6f92", "love": "#eb6f92""##);
        assert!(parse("x.json", &reserved).unwrap_err().contains("reserves"));
    }

    #[test]
    fn reads_a_catppuccin_palette() {
        let json = r##"{ "version": "1.8.0",
          "mocha": { "name": "Mocha", "dark": true, "colors": {
            "mauve": { "name": "Mauve", "order": 3, "hex": "#cba6f7", "accent": true },
            "rosewater": { "name": "Rosewater", "order": 0, "hex": "#f5e0dc", "accent": true },
            "lavender": { "name": "Lavender", "order": 13, "hex": "#b4befe", "accent": true },
            "surface0": { "name": "Surface 0", "order": 22, "hex": "#313244", "accent": false },
            "subtext1": { "name": "Subtext 1", "order": 15, "hex": "#bac2de", "accent": false },
            "base": { "name": "Base", "order": 23, "hex": "#1e1e2e", "accent": false },
            "text": { "name": "Text", "order": 14, "hex": "#cdd6f4", "accent": false } } },
          "latte": { "name": "Latte", "dark": false, "colors": {
            "base": { "hex": "#eff1f5" }, "text": { "hex": "#4c4f69" } } } }"##;
        let themes = parse("my palette.json", json).unwrap();
        let mocha = themes.iter().find(|t| t.id == "addon-my-palette-mocha").unwrap();
        assert_eq!((mocha.name.as_str(), mocha.appearance.as_str()), ("My Palette Mocha", "dark"));
        assert_eq!(var(mocha, "surface-0"), "#313244");
        assert_eq!(var(mocha, "subtext-1"), "#bac2de");
        assert_eq!(var(mocha, "accent"), "var(--mauve)");
        assert_eq!(
            mocha.accents.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(),
            ["rosewater", "mauve", "lavender"]
        );
        let latte = themes.iter().find(|t| t.id == "addon-my-palette-latte").unwrap();
        assert_eq!(latte.appearance, "light");
    }

    #[test]
    fn the_documented_example_loads() {
        let doc = include_str!("../../../docs/themes.md");
        let json = doc.split("```json").nth(1).and_then(|s| s.split("```").next()).unwrap();
        let t = parse("rose-pine.json", json).unwrap().remove(0);
        assert_eq!(t.name, "Rosé Pine");
        assert_eq!(var(&t, "hover"), "var(--highlight-low)");
    }

    #[test]
    fn loads_a_folder_and_reports_problems() {
        let dir = std::env::temp_dir().join(format!("magnitide-themes-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.json"), ROSE_PINE).unwrap();
        fs::write(dir.join("b.json"), ROSE_PINE).unwrap();
        fs::write(dir.join("c.json"), "{ nope").unwrap();
        fs::write(dir.join("notes.txt"), "ignored").unwrap();
        let r = load_folder(&dir);
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(r.themes.len(), 1);
        assert_eq!(r.problems.iter().map(|p| p.file.as_str()).collect::<Vec<_>>(), ["b.json", "c.json"]);
        assert!(r.problems[0].message.contains("already has the id"));
    }
}
