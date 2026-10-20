//! The engine run by the host's own event loop, with no thread of its own, and a wait-pane that polls
//! nothing: cargo run --release --example polled
//!
//! A host loop: wait until the engine's fd is readable (or its timer is due), let it process, repeat.
//! The wait is answered from inside that loop as the session's output comes in.
use std::time::Instant;
use zeropty::embed::Polled;
use zeropty::engine::Options;

fn argv(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

fn main() {
    let mut zp = Polled::start(Options::default());
    // The session stays alive after printing (exec cat): a session that ends right after its last output
    // can be reported as ended before that output is matched (wait-pane: session ended).
    let r = zp.command(&argv(&["new-session", "-d", "-s", "job", "sh -c 'sleep 0.5; echo build done; exec cat'"]), "/");
    assert_eq!(r.code, 0, "{}", r.err);

    // wait-pane -t job -m 'build done' -T 5000, without the command name.
    let start = Instant::now();
    let wait = zp.engine().wait_start(&argv(&["-t", "=job", "-m", "build done", "-T", "5000"])).unwrap_or_else(|r| panic!("{}", r.err));
    let reply = loop {
        if let Some(reply) = zp.engine().wait_check(&wait) {
            break reply;
        }
        // The host's loop: here poll(2) on the engine's fd alone; a real host adds its own fds.
        let mut p = libc::pollfd { fd: zp.fd(), events: libc::POLLIN, revents: 0 };
        unsafe { libc::poll(&mut p, 1, zp.timeout_ms().min(i32::MAX as u64) as i32) };
        zp.process();
    };
    zp.engine().wait_end(&wait);
    println!("wait-pane: code {} after {} ms", reply.code, start.elapsed().as_millis());
    print!("{}", zp.command(&argv(&["capture-pane", "-p", "-t", "=job:"]), "/").out.trim_end());
    println!();
    zp.command(&argv(&["kill-server"]), "/");
    if reply.code != 0 {
        eprint!("{}", reply.err);
        std::process::exit(reply.code);
    }
}
