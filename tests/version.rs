//! One version everywhere: the crates, the C header, the Node clients, `zeropty --version` and CHANGELOG.md
//! must agree, or a release ships pieces that claim to be something else. scripts/release.mjs writes them all;
//! this catches a hand edit that missed one (docs/RELEASING.md).
use std::path::Path;
use std::process::Command;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn read(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The value of `#define NAME value` in the C header, without quotes.
fn define(header: &str, name: &str) -> String {
    let prefix = format!("#define {name} ");
    let line = header.lines().find(|l| l.starts_with(&prefix)).unwrap_or_else(|| panic!("zeropty.h: no {name}"));
    line[prefix.len()..].trim().trim_matches('"').to_owned()
}

/// MAJOR.MINOR.PATCH of a SemVer string, without pre-release or build metadata.
fn core(v: &str) -> [u64; 3] {
    let mut it = v.split(['-', '+']).next().unwrap().split('.').map(|p| p.parse::<u64>().unwrap_or_else(|_| panic!("not SemVer: {v}")));
    let r = [it.next().unwrap(), it.next().unwrap(), it.next().unwrap()];
    assert!(it.next().is_none(), "not SemVer: {v}");
    r
}

#[test]
fn version_is_semver() {
    core(VERSION);
}

#[test]
fn crates_take_the_workspace_version() {
    let root = read("Cargo.toml");
    let ws = root.split("[workspace.package]").nth(1).expect("Cargo.toml: no [workspace.package]");
    let line = ws.lines().find(|l| l.starts_with("version = ")).expect("[workspace.package]: no version");
    assert_eq!(line, format!("version = \"{VERSION}\""));
    for manifest in ["Cargo.toml", "bindings/c/Cargo.toml", "bindings/node/Cargo.toml"] {
        let text = read(manifest);
        let package = text.split("[package]").nth(1).unwrap().split("\n[").next().unwrap();
        assert!(package.contains("\nversion.workspace = true"), "{manifest}: version must be version.workspace = true");
        assert!(!package.contains("\nversion = "), "{manifest}: a version of its own");
    }
}

#[test]
fn lockfile_has_the_version() {
    let lock = read("Cargo.lock");
    for name in ["zeropty", "zeropty-c", "zeropty-node"] {
        let entry = format!("name = \"{name}\"\nversion = \"{VERSION}\"\n");
        assert!(lock.contains(&entry), "Cargo.lock: {name} is not {VERSION} (run cargo update --workspace)");
    }
}

#[test]
fn c_header_macros() {
    let h = read("bindings/c/include/zeropty.h");
    let [major, minor, patch] = core(VERSION);
    assert_eq!(define(&h, "ZEROPTY_VERSION_MAJOR"), major.to_string());
    assert_eq!(define(&h, "ZEROPTY_VERSION_MINOR"), minor.to_string());
    assert_eq!(define(&h, "ZEROPTY_VERSION_PATCH"), patch.to_string());
    assert_eq!(define(&h, "ZEROPTY_VERSION_STRING"), VERSION);
    // ZEROPTY_VERSION_NUMBER has no room for the pre-release: these two tell 0.2.0-rc.1 from 0.2.0.
    let pre = VERSION.split_once('-').map_or("", |(_, p)| p.split('+').next().unwrap());
    assert_eq!(define(&h, "ZEROPTY_VERSION_PRERELEASE"), pre);
    assert_eq!(define(&h, "ZEROPTY_VERSION_IS_PRERELEASE"), if pre.is_empty() { "0" } else { "1" });
    assert!(h.contains("const char *zeropty_version(void);"));
}

#[test]
fn node_clients() {
    for file in ["client/zeropty.mjs", "client/embedded.mjs"] {
        let text = read(file);
        assert!(text.contains(&format!("export const VERSION = '{VERSION}'")), "{file}: VERSION is not {VERSION}");
    }
}

#[test]
fn cli_prints_the_version() {
    for flag in ["--version", "-V"] {
        let out = Command::new(env!("CARGO_BIN_EXE_zeropty")).arg(flag).env("ZEROPTY_SOCK", "/nonexistent/zeropty.sock").output().unwrap();
        assert!(out.status.success(), "zeropty {flag}");
        assert_eq!(String::from_utf8_lossy(&out.stdout), format!("zeropty {VERSION}\n"), "zeropty {flag}");
    }
}

#[test]
fn changelog_has_the_version() {
    let log = read("CHANGELOG.md");
    assert!(log.contains("\n## [Unreleased]\n"), "CHANGELOG.md: no ## [Unreleased] section");
    let heading = format!("\n## [{VERSION}] - ");
    let at = log.find(&heading).unwrap_or_else(|| panic!("CHANGELOG.md: no ## [{VERSION}] - YYYY-MM-DD section"));
    let date = &log[at + heading.len()..][..10];
    assert!(date.bytes().enumerate().all(|(i, b)| if i == 4 || i == 7 { b == b'-' } else { b.is_ascii_digit() }), "CHANGELOG.md: {VERSION} date {date:?}");
    assert!(log.contains(&format!("\n[{VERSION}]: ")), "CHANGELOG.md: no link for [{VERSION}]");
    // The newest release comes first, right under Unreleased.
    let first = log.find("\n## [").and_then(|u| log[u + 1..].find("\n## [").map(|n| u + 1 + n));
    assert_eq!(first, Some(at), "CHANGELOG.md: {VERSION} is not the newest release");
}

#[test]
fn engine_reports_its_version() {
    // tmux's #{version}: what a running server (or an embedded engine) is, asked over the socket or in process.
    let dir = std::env::temp_dir().join(format!("zeropty-version-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let e = zeropty::embed::Embedded::start(zeropty::engine::Options { size_dir: dir.clone(), ..Default::default() });
    let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    // No session needed for the server's own variables.
    let r = e.command(&argv(&["display-message", "-p", "zeropty #{version}"]), "/");
    assert_eq!((r.code, r.out.as_str()), (0, format!("zeropty {VERSION}\n").as_str()));
    // A session's variables still need one, and so does a format without #{version}: plain text
    // and "" keep failing with "can't find session" when there is none (exit codes are public).
    for fmt in ["hello", "", "#{session_name}"] {
        let r = e.command(&argv(&["display-message", "-p", fmt]), "/");
        assert_ne!(r.code, 0, "display -p {fmt:?} with no session");
        assert!(r.out.is_empty() && r.err.contains("can't find session"), "display -p {fmt:?}: {:?}", r.err);
    }
    assert_ne!(e.command(&argv(&["display-message", "-p", "#{version} #{session_name}"]), "/").code, 0);
    assert_eq!(e.command(&argv(&["new-session", "-d", "-s", "v", "exec cat"]), "/").code, 0);
    let r = e.command(&argv(&["display-message", "-p", "-t", "=v:", "#{session_name} #{version}"]), "/");
    assert_eq!(r.out, format!("v {VERSION}\n"));
    assert_eq!(zeropty::engine::VERSION, VERSION);
    e.command(&argv(&["kill-server"]), "/");
    drop(e);
    let _ = std::fs::remove_dir_all(&dir);
}
