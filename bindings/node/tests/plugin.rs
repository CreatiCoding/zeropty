//! The Node module with a plugin: `await zeropty()` waits for a plugin's command without holding the JS
//! thread. This test lives in the module's own package so that `cargo test --release --workspace` (or
//! `-p zeropty-node`) builds the module first: the package's lib is `["cdylib", "rlib"]`, and cargo builds a package's lib (all its
//! crate types in one rustc run) before its integration tests, which writes the module to
//! target/<profile>/deps/libzeropty_node.dylib, beside this test's executable. With the cdylib alone,
//! cargo test built nothing and the test could only read whatever an earlier `cargo build` left.
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The repository.
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

/// Whether `tool arg` runs and succeeds.
fn have(tool: &str, arg: &str) -> bool {
    std::process::Command::new(tool).arg(arg).output().is_ok_and(|o| o.status.success())
}

/// A test that needs a tool this machine lacks: reported as skipped on the process's stderr (a passing
/// test's `eprintln!` is captured and thrown away), or a failure when ZEROPTY_REQUIRE_ALL=1
/// (the release check, `ZEROPTY_REQUIRE_ALL=1 cargo test --release --workspace`).
fn skip(why: &str) {
    if std::env::var("ZEROPTY_REQUIRE_ALL").is_ok_and(|v| v == "1") {
        panic!("{why}, and ZEROPTY_REQUIRE_ALL=1 requires every test to run");
    }
    use std::io::Write as _;
    let test = std::thread::current().name().unwrap_or("?").to_string();
    let _ = writeln!(std::io::stderr(), "SKIPPED {test}: {why} (set ZEROPTY_REQUIRE_ALL=1 to fail instead)");
}

/// The newest modification time of the Rust sources under `dir`, leaving out the binary's own
/// (src/main.rs, src/cli/), which the module is not built from: editing them must not mark it stale.
fn newest_source(dir: &Path, newest: &mut (SystemTime, PathBuf)) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let binary_only = [Path::new(ROOT).join("src/main.rs"), Path::new(ROOT).join("src/cli")];
    for e in entries.flatten() {
        let p = e.path();
        if binary_only.contains(&p) {
            continue;
        }
        if p.is_dir() {
            newest_source(&p, newest);
        } else if p.extension().is_some_and(|x| x == "rs")
            && let Ok(t) = e.metadata().and_then(|m| m.modified())
            && t > newest.0
        {
            *newest = (t, p);
        }
    }
}

/// The module cargo built for this test (target/<profile>/deps, this executable's own directory).
/// Under `cargo test` it is always current; run by hand after editing a source, the executable can sit
/// beside an older module, so one older than any source of the library or the binding is refused (Err
/// names the newer source) instead of testing old code. (cargo's dep-info in deps/ lists only the
/// binding's own file, not the library's, so the sources are walked here.)
fn node_module() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("no test path: {e}"))?;
    let module = exe.parent().ok_or("no target directory")?.join("libzeropty_node.dylib");
    let built = std::fs::metadata(&module)
        .and_then(|m| m.modified())
        .map_err(|_| format!("no Node module at {} (run this test through cargo test -p zeropty-node)", module.display()))?;
    let mut newest = (SystemTime::UNIX_EPOCH, PathBuf::new());
    // Rust sources only: cargo does not rebuild for a manifest's comment, so a manifest newer than the
    // module is no sign of a stale one.
    for d in ["src", "bindings/node/src"] {
        newest_source(&Path::new(ROOT).join(d), &mut newest);
    }
    if newest.0 > built {
        return Err(format!("the Node module {} is older than {} (run cargo test -p zeropty-node)", module.display(), newest.1.display()));
    }
    Ok(module)
}

/// Node's embedded engine (polled, on the JS thread) with a plugin attached from the same process:
/// `zeropty()` (commandStart) leaves the JS thread free while the plugin works, so the plugin, served by
/// that thread, answers, and timers run meanwhile.
#[test]
fn node_waits_for_a_plugin_without_holding_its_loop() {
    if !have("node", "-v") {
        return skip("no node");
    }
    let module = match node_module() {
        Ok(m) => m,
        // A stale module is a broken build, not a missing tool: never a silent pass.
        Err(why) => panic!("{why}"),
    };
    // Short: socket paths are limited to 104 bytes.
    let d = std::env::temp_dir().join(format!("zp-nd-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    // Node loads native modules by their .node name.
    let node_module = d.join("zeropty.node");
    std::fs::copy(&module, &node_module).unwrap();
    let out = std::process::Command::new("node")
        .arg(Path::new(ROOT).join("tests/node/plugin_in_process.mjs"))
        .arg(d.join("s"))
        .env("ZEROPTY_NODE", &node_module)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["out"], "later x\n", "{v}");
    assert_eq!(v["engine"], true);
    assert_eq!(v["promised"], true, "commandStart of an engine command is a Promise too: {v}");
    assert_eq!(v["code"], 0, "{v}");
    let (ticks, took) = (v["ticks"].as_u64().unwrap(), v["took"].as_u64().unwrap());
    assert!((300..5000).contains(&took), "{v}");
    // 10 ms timers kept firing through the 300 ms wait.
    assert!(ticks >= 10, "{v}");
    let _ = std::fs::remove_dir_all(&d);
}
