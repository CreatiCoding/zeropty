//! tmux-style command lines: flags read as getopt reads them, by a spec such as "t:x:y:" (a letter takes a
//! value when ':' follows it), and session targets ("=name:" and the like).

use super::reply::CmdError;
use std::collections::HashMap;

/// A command's argv split by its spec: flags (each with every value it was given) and the rest.
#[derive(Default, Debug)]
pub(super) struct Args {
    pub(super) flags: HashMap<char, Vec<String>>,
    pub(super) rest: Vec<String>,
}

/// Splits `argv` (the command's arguments, without its name `cmd`) by `spec`. Any flag not in it is an
/// error, as in tmux: ignored, it gave a different result without a word (send-keys -N 3 sent once,
/// capture-pane -E captured to the end). Flags end at the first argument that is not one, or at "--".
pub(super) fn parse(cmd: &str, argv: &[String], spec: &str) -> Result<Args, CmdError> {
    let takes = |c: char| spec.find(c).map(|i| spec[i + c.len_utf8()..].starts_with(':'));
    let mut a = Args::default();
    let mut i = 0;
    while i < argv.len() {
        let arg = &argv[i];
        if !a.rest.is_empty() || !arg.starts_with('-') || arg == "-" {
            a.rest.push(arg.clone());
        } else if arg == "--" {
            a.rest.extend(argv[i + 1..].iter().cloned());
            break;
        } else {
            // A cluster like -pe or -S-200.
            let chars: Vec<char> = arg[1..].chars().collect();
            let mut j = 0;
            while j < chars.len() {
                let c = chars[j];
                match takes(c) {
                    None if c != ':' => return Err(CmdError::new(format!("{cmd}: unknown flag -{c}\n"))),
                    None => return Err(CmdError::new(format!("{cmd}: unknown flag -:\n"))),
                    Some(true) => {
                        let v = if j + 1 < chars.len() {
                            chars[j + 1..].iter().collect()
                        } else {
                            i += 1;
                            argv.get(i).cloned().ok_or_else(|| CmdError::new(format!("{cmd}: -{c} expects an argument\n")))?
                        };
                        a.flags.entry(c).or_default().push(v);
                        break;
                    }
                    Some(false) => a.flags.entry(c).or_default().push(String::new()),
                }
                j += 1;
            }
        }
        i += 1;
    }
    Ok(a)
}

impl Args {
    /// Whether flag `c` was given.
    pub(super) fn has(&self, c: char) -> bool {
        self.flags.contains_key(&c)
    }

    /// Flag `c`'s value, the last one when given more than once.
    pub(super) fn get(&self, c: char) -> Option<&str> {
        self.flags.get(&c).and_then(|v| v.last()).map(String::as_str)
    }

    /// Every value flag `c` was given, in order (new-session -e).
    pub(super) fn all(&self, c: char) -> impl Iterator<Item = &str> {
        self.flags.get(&c).into_iter().flatten().map(String::as_str)
    }
}

/// The session a target names: `=name:`, `=name`, `name:0` -> `name`.
pub(super) fn session_name(target: &str) -> &str {
    let t = target.strip_prefix('=').unwrap_or(target);
    t.split(':').next().unwrap_or(t)
}

#[cfg(test)]
mod tests {
    use super::{parse, session_name};

    fn argv(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn flags_as_getopt_reads_them() {
        let a = parse("capture-pane", &argv(&["-pe", "-S-200", "-E", "5", "-t", "=x:", "rest", "-q"]), "eE:pqS:t:").unwrap();
        assert!(a.has('p') && a.has('e') && !a.has('q'));
        assert_eq!((a.get('S'), a.get('E'), a.get('t')), (Some("-200"), Some("5"), Some("=x:")));
        assert_eq!(a.rest, ["rest", "-q"], "flags end at the first other argument");
        let a = parse("x", &argv(&["-e", "A=1", "-e", "B=2", "--", "-e"]), "e:").unwrap();
        assert_eq!(a.all('e').collect::<Vec<_>>(), ["A=1", "B=2"]);
        assert_eq!(a.get('e'), Some("B=2"), "the last one");
        assert_eq!(a.rest, ["-e"]);
        assert_eq!(parse("x", &argv(&["-", "-a"]), "a").unwrap().rest, ["-", "-a"], "- is an argument");
    }

    #[test]
    fn bad_flags_say_so() {
        assert_eq!(parse("send-keys", &argv(&["-Z"]), "t:").unwrap_err().message(), "send-keys: unknown flag -Z\n");
        assert_eq!(parse("x", &argv(&["-t"]), "t:").unwrap_err().message(), "x: -t expects an argument\n");
        assert_eq!(parse("x", &argv(&["-:"]), "ab").unwrap_err().message(), "x: unknown flag -:\n");
    }

    #[test]
    fn targets() {
        for t in ["=name:", "=name", "name:0", "name", "name:0.1"] {
            assert_eq!(session_name(t), "name", "{t}");
        }
        assert_eq!(session_name(""), "");
    }
}
