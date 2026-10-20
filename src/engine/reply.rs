//! A command's result, as the CLI prints it and the socket sends it: an exit code, standard output and
//! standard error (`Reply`), and why a command failed (`CmdError`), which becomes a failed `Reply`.
//!
//! Handlers return `Result<Reply, CmdError>` and use `?` on whatever they call; only the command table
//! turns the error into a reply, so a failure is always code 1 with its message on standard error.

use serde::{Deserialize, Serialize};

/// What a command answers: `code` 0 for success, its output in `out`, its complaints in `err` (each line
/// ending in '\n', as a tmux command prints them).
///
/// Exhaustive on purpose, and frozen: it is what the CLI prints and the socket sends, and embedders
/// build it with a struct literal, so these three fields are part of the stable surface (also the
/// plugin API's, which re-exports it). Anything a later API needs to add travels beside it, never in it.
///
/// On the socket and through the C ABI it is the JSON object `{"code": 0, "out": "...", "err": ""}`.
///
/// # Example
///
/// A host that prints replies as the CLI does:
///
/// ```
/// use zeropty::engine::{fail, ok, Reply};
///
/// fn show(r: &Reply) -> i32 {
///     print!("{}", r.out);
///     eprint!("{}", r.err);
///     r.code // the exit code
/// }
/// assert_eq!(show(&ok("w\n")), 0);
/// assert_eq!(show(&fail("can't find session: w\n")), 1);
/// assert_eq!(serde_json::to_string(&ok("")).unwrap(), r#"{"code":0,"out":"","err":""}"#);
/// ```
#[derive(Serialize, Deserialize, Default, Debug, PartialEq, Eq)]
pub struct Reply {
    /// The exit code: 0 for success.
    pub code: i32,
    /// Standard output.
    pub out: String,
    /// Standard error.
    pub err: String,
}

impl Reply {
    /// A reply with exit code `code`, standard output `out` and standard error `err`.
    pub fn new(code: i32, out: impl Into<String>, err: impl Into<String>) -> Reply {
        Reply { code, out: out.into(), err: err.into() }
    }
}

/// Success, printing `out`.
///
/// ```
/// let r = zeropty::engine::ok("hello\n");
/// assert_eq!((r.code, r.out.as_str(), r.err.as_str()), (0, "hello\n", ""));
/// ```
pub fn ok(out: impl Into<String>) -> Reply {
    Reply { code: 0, out: out.into(), err: String::new() }
}

/// Failure (code 1), saying `err`.
///
/// ```
/// let r = zeropty::engine::fail("my-command: no such thing\n");
/// assert_eq!((r.code, r.out.as_str(), r.err.as_str()), (1, "", "my-command: no such thing\n"));
/// ```
pub fn fail(err: impl Into<String>) -> Reply {
    Reply { code: 1, out: String::new(), err: err.into() }
}

/// Why a command failed, as tmux says it: one line ending in '\n' ("can't find session: x\n").
///
/// ```
/// use zeropty::engine::{CmdError, Reply};
///
/// fn find(name: &str) -> Result<Reply, CmdError> {
///     Err(CmdError::new(format!("can't find session: {name}\n")))
/// }
/// let r: Reply = find("w").unwrap_or_else(Reply::from);
/// assert_eq!((r.code, r.err.as_str()), (1, "can't find session: w\n"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmdError(String);

impl CmdError {
    /// An error saying `msg` (a line ending in '\n').
    ///
    /// ```
    /// let e = zeropty::engine::CmdError::new("no such buffer: b\n");
    /// assert_eq!(e.to_string(), "no such buffer: b\n");
    /// ```
    pub fn new(msg: impl Into<String>) -> Self {
        CmdError(msg.into())
    }

    /// The message, as printed.
    ///
    /// ```
    /// assert_eq!(zeropty::engine::CmdError::new("bad target\n").message(), "bad target\n");
    /// ```
    pub fn message(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for CmdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CmdError {}

impl From<CmdError> for Reply {
    fn from(e: CmdError) -> Reply {
        fail(e.0)
    }
}

impl From<CmdError> for String {
    fn from(e: CmdError) -> String {
        e.0
    }
}

#[cfg(test)]
mod tests {
    use super::{CmdError, Reply, fail};

    #[test]
    fn errors_become_failed_replies() {
        let e = CmdError::new("can't find session: x\n");
        assert_eq!(e.to_string(), "can't find session: x\n");
        assert_eq!(Reply::from(e.clone()), fail("can't find session: x\n"));
        assert_eq!(String::from(e), "can't find session: x\n");
    }
}
