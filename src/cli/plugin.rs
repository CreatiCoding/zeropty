//! `zeropty plugin-run lib.dylib`: a plugin library (docs/PLUGINS.md) in a process of its own, attached
//! to the server through the socket; it returns when the server goes away. A crash in the library ends
//! this process only, never the server (which the plugin directory's launcher then restarts).

use super::sock_path;

/// Runs the library named by `args[0]`; the process's exit status.
pub fn run(args: &[String]) -> i32 {
    let Some(path) = args.first() else {
        eprintln!("usage: zeropty plugin-run <plugin.dylib>");
        return 2;
    };
    // SAFETY: loading a library runs its code; the user named the library to run, as they would name a
    // program to execute.
    match unsafe { zeropty::plugin::__private::native::run(std::path::Path::new(path), &sock_path()) } {
        Ok(()) => 0,
        Err(e) => {
            eprint!("{e}");
            1
        }
    }
}
