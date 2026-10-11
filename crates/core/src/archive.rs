//! RAR archives, which most CBR comics are. The UnRAR source's licence doesn't
//! allow it in a GPL program, so RAR is read with a tool already on the
//! computer, found the way calibre is (never bundled): libarchive's bsdtar
//! (macOS's and recent Windows' `tar`, and Arch's pacman needs it), 7-Zip, or
//! unrar. Each reads RAR4 and RAR5.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

/// What to say when a RAR can't be read for want of a tool.
pub const NEEDS_RAR_TOOL: &str =
    "reading RAR archives (most CBR comics) needs bsdtar (libarchive), 7-Zip or unrar installed";

/// Whether a file is a RAR archive (RAR4 or RAR5), by its first bytes.
pub fn is_rar(path: &Path) -> bool {
    let mut magic = [0u8; 6];
    std::fs::File::open(path).and_then(|mut f| f.read_exact(&mut magic)).is_ok() && magic == *b"Rar!\x1a\x07"
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Bsdtar,
    SevenZip,
    Unrar,
}

/// A program that reads RAR archives.
#[derive(Debug, Clone)]
pub struct RarTool {
    kind: Kind,
    path: PathBuf,
}

impl RarTool {
    /// The tool on this computer, looked for once.
    pub fn find() -> Option<&'static RarTool> {
        static FOUND: OnceLock<Option<RarTool>> = OnceLock::new();
        FOUND.get_or_init(detect).as_ref()
    }

    /// Its name, for Settings: "bsdtar", "7-Zip" or "unrar".
    pub fn name(&self) -> &'static str {
        match self.kind {
            Kind::Bsdtar => "bsdtar",
            Kind::SevenZip => "7-Zip",
            Kind::Unrar => "unrar",
        }
    }

    /// The archive's files (not folders), as they're named inside it.
    pub fn list(&self, archive: &Path) -> Result<Vec<String>, String> {
        let out = match self.kind {
            Kind::Bsdtar => self.run(&[os("-tf"), archive.into()])?,
            // Not -ba or -spd: p7zip 16.02, still common, has neither.
            Kind::SevenZip => self.run(&[os("l"), os("-slt"), archive.into()])?,
            Kind::Unrar => self.run(&[os("lb"), archive.into()])?,
        };
        let text = String::from_utf8_lossy(&out);
        Ok(match self.kind {
            Kind::SevenZip => {
                // After a "----------" line, blocks of "Key = value" lines, one per
                // entry (the archive's own block comes before it); folders have a D attribute.
                let mut names = Vec::new();
                let mut path: Option<String> = None;
                let entries = text.split_once("\n----------").map_or("", |(_, rest)| rest);
                for line in entries.lines().chain(std::iter::once("")) {
                    if let Some(p) = line.strip_prefix("Path = ") {
                        path = Some(p.to_owned());
                    } else if let Some(attr) = line.strip_prefix("Attributes = ") {
                        if attr.starts_with('D') {
                            path = None;
                        }
                    } else if line.is_empty()
                        && let Some(p) = path.take()
                    {
                        names.push(p);
                    }
                }
                names
            }
            _ => text.lines().filter(|l| !l.is_empty() && !l.ends_with('/')).map(str::to_owned).collect(),
        })
    }

    /// One file's bytes, if it's there and no bigger than `max`.
    pub fn read(&self, archive: &Path, entry: &str, max: u64) -> Option<Vec<u8>> {
        let args = match self.kind {
            Kind::Bsdtar => vec![os("-xOf"), archive.into(), entry.into()],
            Kind::SevenZip => vec![os("e"), os("-so"), archive.into(), entry.into()],
            Kind::Unrar => vec![os("p"), os("-inul"), archive.into(), entry.into()],
        };
        let mut child = self.command(&args).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
        let mut out = Vec::new();
        child.stdout.take()?.take(max + 1).read_to_end(&mut out).ok()?;
        if out.len() as u64 > max {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        (child.wait().ok()?.success() && !out.is_empty()).then_some(out)
    }

    /// Unpack everything into `into` (an empty folder). The tools keep entries
    /// inside it; the caller still reads only plain files from it.
    pub fn extract_all(&self, archive: &Path, into: &Path) -> Result<(), String> {
        let mut dir = into.as_os_str().to_owned();
        match self.kind {
            Kind::Bsdtar => self.run(&[os("-xf"), archive.into(), os("-C"), into.into()]),
            Kind::SevenZip => {
                let mut out = os("-o");
                out.push(into);
                self.run(&[os("x"), os("-y"), out, archive.into()])
            }
            Kind::Unrar => {
                dir.push(std::path::MAIN_SEPARATOR_STR);
                self.run(&[os("x"), os("-o+"), os("-inul"), os("-y"), archive.into(), dir])
            }
        }
        .map(|_| ())
    }

    fn command(&self, args: &[std::ffi::OsString]) -> Command {
        let mut cmd = Command::new(&self.path);
        cmd.args(args).stdin(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // No console window flashing up behind the app.
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        cmd
    }

    fn run(&self, args: &[std::ffi::OsString]) -> Result<Vec<u8>, String> {
        let out = self.command(args).output().map_err(|e| format!("{} couldn't run: {e}", self.name()))?;
        if out.status.success() {
            return Ok(out.stdout);
        }
        let log = String::from_utf8_lossy(&out.stderr);
        let last = log.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("no message").trim().to_owned();
        Err(format!("{} couldn't read it: {last}", self.name()))
    }
}

fn os(s: &str) -> std::ffi::OsString {
    s.into()
}

/// The first tool found: bsdtar, then 7-Zip, then unrar (on Windows, 7-Zip,
/// unrar, then its tar.exe).
fn detect() -> Option<RarTool> {
    let in_path = |names: &[&str]| -> Vec<PathBuf> {
        let Some(path) = std::env::var_os("PATH") else { return Vec::new() };
        std::env::split_paths(&path)
            .flat_map(|dir| {
                names.iter().map(move |n| dir.join(if cfg!(windows) { format!("{n}.exe") } else { (*n).to_owned() }))
            })
            .collect()
    };

    let mut bsdtar = in_path(&["bsdtar"]);
    if cfg!(target_os = "macos") {
        bsdtar.push("/usr/bin/bsdtar".into());
        bsdtar.push("/usr/bin/tar".into());
    } else if cfg!(windows) {
        // Windows 10 and later ship libarchive's bsdtar as tar.exe.
        if let Some(root) = std::env::var_os("SystemRoot") {
            bsdtar.push(Path::new(&root).join("System32").join("tar.exe"));
        }
    }
    let mut seven = in_path(&["7zz", "7z", "7za"]);
    let mut unrar = in_path(&["unrar"]);
    if cfg!(windows) {
        for var in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(dir) = std::env::var_os(var) {
                seven.push(Path::new(&dir).join("7-Zip").join("7z.exe"));
                unrar.push(Path::new(&dir).join("WinRAR").join("UnRAR.exe"));
            }
        }
    }

    let is_bsdtar = |p: &PathBuf| {
        p.is_file()
            && Command::new(p)
                .arg("--version")
                .stdin(Stdio::null())
                .output()
                .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("bsdtar"))
    };
    let bsdtar = || bsdtar.iter().find(|p| is_bsdtar(p)).map(|path| RarTool { kind: Kind::Bsdtar, path: path.clone() });
    let seven = || seven.iter().find(|p| p.is_file()).map(|path| RarTool { kind: Kind::SevenZip, path: path.clone() });
    let unrar = || unrar.iter().find(|p| p.is_file()).map(|path| RarTool { kind: Kind::Unrar, path: path.clone() });
    if cfg!(windows) {
        // Windows' own tar.exe may be built without RAR on older releases: it's the last resort there.
        seven().or_else(unrar).or_else(bsdtar)
    } else {
        bsdtar().or_else(seven).or_else(unrar)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A RAR4 archive of `files`, stored uncompressed: enough of the format for
    /// the tools to read, built here since nothing open writes RAR.
    pub(crate) fn make_rar(path: &Path, files: &[(&str, &[u8])]) {
        fn block(head_type: u8, flags: u16, body: &[u8]) -> Vec<u8> {
            let size = (7 + body.len()) as u16;
            let mut head = vec![head_type];
            head.extend(flags.to_le_bytes());
            head.extend(size.to_le_bytes());
            head.extend(body);
            let crc = (crc32(&head) & 0xffff) as u16;
            let mut out = crc.to_le_bytes().to_vec();
            out.extend(head);
            out
        }
        let mut out = b"Rar!\x1a\x07\x00".to_vec();
        out.extend(block(0x73, 0, &[0; 6]));
        for (name, data) in files {
            let mut body = Vec::new();
            body.extend((data.len() as u32).to_le_bytes()); // packed size
            body.extend((data.len() as u32).to_le_bytes()); // unpacked size
            body.push(3); // host OS: Unix, so "/" separates folders
            body.extend(crc32(data).to_le_bytes());
            body.extend(0x5a21_0000u32.to_le_bytes()); // a DOS date (2025)
            body.push(20); // version needed
            body.push(0x30); // stored
            body.extend((name.len() as u16).to_le_bytes());
            body.extend(0o100_644u32.to_le_bytes()); // a regular file, rw-r--r--
            body.extend(name.as_bytes());
            out.extend(block(0x74, 0x8000, &body));
            out.extend(*data);
        }
        out.extend(block(0x7b, 0x4000, &[]));
        std::fs::write(path, out).unwrap();
    }

    fn crc32(data: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &b in data {
            crc ^= u32::from(b);
            for _ in 0..8 {
                crc = if crc & 1 != 0 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
            }
        }
        !crc
    }

    #[test]
    fn reads_a_rar_with_whatever_tool_is_here() {
        let Some(tool) = RarTool::find() else {
            eprintln!("no RAR tool on this computer; skipped");
            return;
        };
        check(tool);
    }

    /// Each kind of tool that's installed here, not just the one found first.
    #[test]
    fn reads_a_rar_with_each_tool_installed() {
        for (kind, name) in [(Kind::Bsdtar, "bsdtar"), (Kind::SevenZip, "7z"), (Kind::Unrar, "unrar")] {
            let found = std::env::var_os("PATH")
                .and_then(|p| std::env::split_paths(&p).map(|d| d.join(name)).find(|p| p.is_file()));
            match found {
                Some(path) => check(&RarTool { kind, path }),
                None => eprintln!("{name} isn't installed; skipped"),
            }
        }
    }

    fn check(tool: &RarTool) {
        let tmp = tempfile::tempdir().unwrap();
        let rar = tmp.path().join("comic.cbr");
        make_rar(&rar, &[("pages/01.jpg", b"page one"), ("ComicInfo.xml", b"<ComicInfo/>")]);
        assert!(is_rar(&rar));
        let mut names = tool.list(&rar).unwrap();
        names.sort();
        assert_eq!(names, ["ComicInfo.xml", "pages/01.jpg"], "{}", tool.name());
        assert_eq!(tool.read(&rar, "pages/01.jpg", 1 << 20).as_deref(), Some(&b"page one"[..]));
        assert_eq!(tool.read(&rar, "pages/01.jpg", 3), None, "bigger than allowed");
        let out = tmp.path().join("out");
        std::fs::create_dir(&out).unwrap();
        tool.extract_all(&rar, &out).unwrap();
        assert_eq!(std::fs::read(out.join("pages/01.jpg")).unwrap(), b"page one");
    }
}
