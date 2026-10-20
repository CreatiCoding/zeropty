//! `zeropty --help`, `-h`, `help` and `help <command>`: answered by the program itself, so they work with
//! no server running.

/// `zeropty --help`: answered by this process, so it works with no server running. Every flag is in
/// docs/USAGE.md; this lists what exists, with tmux's short names.
pub const HELP: &str = "\
zeropty: a session engine for AI agents, driven with tmux's commands (no PTYs)

usage: zeropty <command> [flags] [args]
       zeropty serve [--bind <socket>]
       zeropty -V | --version | -h | --help | help [command]

sessions:
  new-session (new)       [-dAP] [-s name] [-x cols] [-y rows] [-c dir] [-e VAR=value] [-F format] [command]
  has-session (has)       -t target
  kill-session            [-a] -t target
  list-sessions (ls)      [-F format]
  rename-session (rename) -t target <new-name>
  resize-window (resizew), resize-pane (resizep) -t target [-x cols] [-y rows]
  pipe-pane (pipep)       [-oO] -t target [command]
  kill-server
keys and screen:
  send-keys (send)        [-lH] [-N count] -t target <key>...
  capture-pane (capturep) [-epJNq] [-S line] [-E line] [-b buffer] -t target
  display-message (display) [-p] [-F format] -t target [format]
  load-buffer (loadb)     [-b buffer] <file>
  paste-buffer (pasteb)   [-dpr] [-b buffer] [-s separator] -t target
waiting and finding (not in tmux):
  wait-pane               -t target [-m text] [-q quiet-ms] [-T timeout-ms]
  grep-panes              [-S line] <text>
watching:
  attach-session (attach, a) [-r] [-t target]    detach with Ctrl-b d (read-only: q)
environment:
  set-environment (setenv)  [-gu] <name> [value]
  show-environment (showenv) [-g]
plugins (docs/PLUGINS.md):
  list-plugins            one line per plugin (state, events, commands, access)
  plugin-run              <plugin.dylib>    a plugin library in a process of its own
server:
  restart-server          a new server takes over every running session
  pull-sessions, push-sessions [-s socket] [name...]
  stats, mstats, mem -t target

targets: =name: (also =name, name, name:0); names match exactly
socket:  $ZEROPTY_SOCK, else ~/.zeropty/mux.sock; new-session starts the server
docs:    https://github.com/CreatiCoding/zeropty/blob/main/docs/USAGE.md
more:    zeropty help <command>
";

/// `zeropty help <command>`: every name a command answers to, what it does, and the docs/USAGE.md
/// section (anchor) with each flag explained. The synopsis line comes from `HELP`, so the two cannot drift.
const ABOUT: &[(&[&str], &str, &str)] = &[
    (&["new-session", "new"], "Start a session running a command (default $SHELL); starts the server if none is running.", "sessions"),
    (&["has-session", "has"], "Exit 0 if the session exists, 1 (can't find session) if not.", "sessions"),
    (&["kill-session"], "SIGHUP the session's process group, SIGKILL what is left after 2s; -a: every other session.", "sessions"),
    (&["list-sessions", "ls"], "List the sessions, in tmux's format unless -F gives one.", "sessions"),
    (&["rename-session", "rename"], "Rename a session.", "sessions"),
    (&["resize-window", "resizew", "resize-pane", "resizep"], "Change the screen size; the program gets SIGWINCH.", "sessions"),
    (&["pipe-pane", "pipep"], "Copy the session's output into a command's stdin; with no command, close the pipe.", "sessions"),
    (&["kill-server"], "End every session; a standalone server exits too.", "sessions"),
    (&["send-keys", "send"], "Send keys: key names (Enter, C-c, ...) or text; -l: literal, -H: hex bytes, -N: repeat.", "sending-keys"),
    (&["paste-buffer", "pasteb"], "Send a buffer as keys; -p: bracketed paste when the program turned it on.", "sending-keys"),
    (&["capture-pane", "capturep"], "Read the screen as text: -p to stdout, -e with colors, -S/-E line range (negative: scrollback).", "reading-the-screen"),
    (&["display-message", "display"], "Print a format such as '#{cursor_x},#{cursor_y}' for a session.", "reading-the-screen"),
    (&["load-buffer", "loadb"], "Load a file into a buffer (default buffer0).", "buffers"),
    (&["wait-pane"], "Wait, without polling, until text shows (-m) and/or the session is quiet (-q ms); -T: exit 1 on timeout.", "waiting-and-searching"),
    (&["grep-panes"], "Find text on every session's screen, printed as 'session: line'; exit 1 when nothing matches.", "waiting-and-searching"),
    (&["attach-session", "attach", "a"], "Attach this terminal to a session; detach with Ctrl-b d (-r: read-only, q detaches).", "viewing-attach"),
    (&["set-environment", "setenv"], "Set (-g) or unset (-u) a variable for sessions created later.", "environment"),
    (&["show-environment", "showenv"], "Show the variables set with -g (unset ones as -name).", "environment"),
    (&["list-plugins"], "List the server's plugins, one line each: name, state (running, stopped, failed), events, commands, access.", "plugins"),
    (&["plugin-run"], "Load a plugin library (.dylib) into a process of its own and attach it to the server; ends with the server.", "plugins"),
    (&["serve"], "Run a standalone server in this process (new-session usually starts one for you).", "server-and-moving-sessions"),
    (&["restart-server"], "Start a new server that takes over every running session and the socket (for upgrades).", "server-and-moving-sessions"),
    (&["pull-sessions"], "Move sessions from the server on -s into this one (no names: all).", "server-and-moving-sessions"),
    (&["push-sessions"], "Move this server's sessions to the server on -s (no names: all).", "server-and-moving-sessions"),
    (&["stats"], "One line of engine counters: sessions, heap_bytes, screen_bytes, live/frozen screens, reads.", "diagnostics"),
    (&["mstats"], "Allocator (mimalloc) statistics; standalone server only.", "diagnostics"),
    (&["mem"], "Memory breakdown of one session's screen.", "diagnostics"),
];

/// The command names a `HELP` command line lists: its first word, words in parentheses, and the word
/// after one ending in ','.
fn help_names(line: &str) -> Vec<&str> {
    let mut names = Vec::new();
    let mut more = true;
    for tok in line.split_whitespace() {
        if !(more || tok.starts_with('(')) {
            break;
        }
        more = tok.ends_with(',');
        names.push(tok.trim_matches(|c| c == '(' || c == ')' || c == ','));
    }
    names
}

/// `zeropty help <command>`: exit 0 with the command's help on stdout, or 2 for an unknown name.
pub fn help_for(cmd: &str) -> i32 {
    let Some((_, about, section)) = ABOUT.iter().find(|(names, ..)| names.contains(&cmd)) else {
        eprintln!("zeropty: unknown command: {cmd} (zeropty --help lists the commands)");
        return 2;
    };
    let synopsis = HELP
        .lines()
        .filter(|l| l.starts_with("  ") && l.as_bytes()[2].is_ascii_lowercase())
        .find(|l| help_names(l).contains(&cmd))
        .map_or("serve [--bind <socket>]", str::trim);
    let synopsis = synopsis.split_whitespace().collect::<Vec<_>>().join(" ");
    super::stdout_quiet(&format!("usage: zeropty {synopsis}\n\n{about}\n\ndocs: https://github.com/CreatiCoding/zeropty/blob/main/docs/USAGE.md#{section}\n"));
    0
}
