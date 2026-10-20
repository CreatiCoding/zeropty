//! The socket protocol, as bytes: what a client sends and what comes back. Pure encoding and decoding,
//! shared by the server (crate::server), the CLI and `zeropty attach` (src/main.rs), so both ends agree
//! by construction. docs/USAGE.md describes it for clients in other languages.
//!
//! - Commands: one JSON `Request` per line; each reply a JSON `Reply` line, or with `raw` a header line
//!   `"<code> <out bytes> <err bytes>\n"` followed by out and err as they are (`reply_line`).
//! - After `attach`: frames both ways, `[kind u8][length u32 LE][bytes]`. The server sends
//!   `ServerFrame`s, the client `ClientFrame`s; each direction has its own kinds (`decode` of the wrong
//!   direction's frame is an error, never a mix-up). `Frames` reads them off a buffer.
//!
//! Invariants: a frame's length is checked against its direction's maximum before anything waits for
//! its body (`FrameError::TooLong`: the connection is not one to keep); an unknown kind is reported
//! (`FrameError::UnknownKind`) with its body skipped, so a newer peer's extra frames can be ignored.
//!
//! # Example
//!
//! A command line and its reply, as a client in any language writes and reads them:
//!
//! ```
//! use zeropty::engine::ok;
//! use zeropty::protocol::{reply_line, request_line, Request};
//!
//! let req = Request { argv: vec!["has-session".into(), "-t".into(), "=w".into()], cwd: "/".into(), raw: false };
//! assert_eq!(request_line(&req), b"{\"argv\":[\"has-session\",\"-t\",\"=w\"],\"cwd\":\"/\"}\n");
//! assert_eq!(reply_line(&ok("w\n"), false), b"{\"code\":0,\"out\":\"w\\n\",\"err\":\"\"}\n");
//! assert_eq!(reply_line(&ok("w\n"), true), b"0 2 0\nw\n"); // raw: a header line, then the bytes
//! ```

use crate::engine::Reply;
use serde::{Deserialize, Serialize};

/// One command line from a client.
///
/// ```
/// use zeropty::protocol::Request;
///
/// let req: Request = serde_json::from_str(r#"{"argv":["send-keys","-t","=w:","Enter"],"cwd":"/"}"#).unwrap();
/// assert_eq!(req.argv[0], "send-keys");
/// assert!(!req.raw); // optional, false by default
/// ```
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct Request {
    /// The command line, its name first ("capture-pane", "-p", "-t", "=x:").
    pub argv: Vec<String>,
    /// Where relative paths in it are taken from.
    pub cwd: String,
    /// The reply as `"<code> <out bytes> <err bytes>\n"` then out and err as they are, for clients that
    /// cannot decode JSON cheaply (shell/zeropty.zsh, a zsh function: no process started per command).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub raw: bool,
}

/// Bytes before a frame's body: its kind and its length.
pub const FRAME_HEAD: usize = 5;

/// A frame from the server to an attached client.
///
/// ```
/// use zeropty::protocol::{Frame as _, ServerFrame};
///
/// assert_eq!(ServerFrame::Output(b"hi").to_vec(), [0, 2, 0, 0, 0, b'h', b'i']); // kind, length LE, bytes
/// assert_eq!(ServerFrame::Ended.to_vec(), [1, 0, 0, 0, 0]);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerFrame<'a> {
    /// What the session prints (the screen first, as `attach` sends it), to write to the terminal as is.
    Output(&'a [u8]),
    /// The session ended; nothing follows.
    Ended,
}

/// A frame from an attached client to the server.
///
/// ```
/// use zeropty::protocol::{ClientFrame, Frame as _};
///
/// let resize = ClientFrame::Resize { rows: 24, cols: 80 }.to_vec();
/// assert_eq!(resize, [1, 4, 0, 0, 0, 80, 0, 24, 0]); // cols then rows, u16 LE each
/// assert_eq!(ClientFrame::decode(1, &resize[5..]), Ok(ClientFrame::Resize { rows: 24, cols: 80 }));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientFrame<'a> {
    /// Keys for the session.
    Input(&'a [u8]),
    /// The terminal's new size.
    Resize {
        /// Rows.
        rows: u16,
        /// Columns.
        cols: u16,
    },
    /// Detach; the server then closes the connection.
    Detach,
}

/// Why bytes are not a frame.
///
/// ```
/// use zeropty::protocol::{ClientFrame, Frame as _, FrameError};
///
/// assert_eq!(ClientFrame::decode(9, b""), Err(FrameError::UnknownKind(9)));
/// assert_eq!(ClientFrame::decode(1, b"abc"), Err(FrameError::BadBody(1))); // a resize is 4 bytes
/// assert_eq!(FrameError::TooLong(1 << 30).to_string(), "frame of 1073741824 bytes is too long");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameError {
    /// A kind this direction does not have (its body was skipped).
    UnknownKind(u8),
    /// A length past the direction's maximum: the stream is not this protocol, or not any more.
    TooLong(usize),
    /// A body of the wrong size for its kind (a resize is 4 bytes).
    BadBody(u8),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameError::UnknownKind(k) => write!(f, "unknown frame kind {k}"),
            FrameError::TooLong(n) => write!(f, "frame of {n} bytes is too long"),
            FrameError::BadBody(k) => write!(f, "bad body for frame kind {k}"),
        }
    }
}

impl std::error::Error for FrameError {}

/// A direction's frames (`ServerFrame`, `ClientFrame`): how each goes on the wire, and how long one may be.
///
/// ```
/// use zeropty::protocol::{ClientFrame, Frame};
///
/// let mut wire = Vec::new();
/// ClientFrame::Input(b"ls\r").encode(&mut wire);
/// ClientFrame::Detach.encode(&mut wire);
/// assert_eq!(wire.len(), 5 + 3 + 5);
/// assert!(ClientFrame::MAX_LEN >= 4096);
/// ```
pub trait Frame<'a>: Sized {
    /// The longest body accepted.
    const MAX_LEN: usize;

    /// Appends the frame to `out`.
    fn encode(&self, out: &mut Vec<u8>);

    /// The frame of `kind` with `body`.
    fn decode(kind: u8, body: &'a [u8]) -> Result<Self, FrameError>;

    /// The frame as bytes.
    fn to_vec(&self) -> Vec<u8> {
        let mut v = Vec::new();
        self.encode(&mut v);
        v
    }
}

/// Appends `[kind][length u32 LE][body]` to `out`.
fn put(out: &mut Vec<u8>, kind: u8, body: &[u8]) {
    out.push(kind);
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(body);
}

const OUTPUT: u8 = 0;
const ENDED: u8 = 1;
const INPUT: u8 = 0;
const RESIZE: u8 = 1;
const DETACH: u8 = 2;

impl<'a> Frame<'a> for ServerFrame<'a> {
    /// A burst read at once can be 64 MB (the engine's SLURP_MAX), plus echo: well under this.
    const MAX_LEN: usize = 256 << 20;

    fn encode(&self, out: &mut Vec<u8>) {
        match *self {
            ServerFrame::Output(b) => put(out, OUTPUT, b),
            ServerFrame::Ended => put(out, ENDED, &[]),
        }
    }

    fn decode(kind: u8, body: &'a [u8]) -> Result<Self, FrameError> {
        match kind {
            OUTPUT => Ok(ServerFrame::Output(body)),
            ENDED => Ok(ServerFrame::Ended),
            k => Err(FrameError::UnknownKind(k)),
        }
    }
}

impl<'a> Frame<'a> for ClientFrame<'a> {
    /// Keys are read from a terminal 4 KB at a time; a paste comes as many such frames.
    const MAX_LEN: usize = 16 << 20;

    fn encode(&self, out: &mut Vec<u8>) {
        match *self {
            ClientFrame::Input(b) => put(out, INPUT, b),
            ClientFrame::Resize { rows, cols } => put(out, RESIZE, &resize_body(rows, cols)),
            ClientFrame::Detach => put(out, DETACH, &[]),
        }
    }

    fn decode(kind: u8, body: &'a [u8]) -> Result<Self, FrameError> {
        match kind {
            INPUT => Ok(ClientFrame::Input(body)),
            RESIZE => parse_resize(body).map(|(rows, cols)| ClientFrame::Resize { rows, cols }).ok_or(FrameError::BadBody(RESIZE)),
            DETACH => Ok(ClientFrame::Detach),
            k => Err(FrameError::UnknownKind(k)),
        }
    }
}

/// The body of a resize frame: cols then rows, u16 LE each.
///
/// ```
/// assert_eq!(zeropty::protocol::resize_body(24, 80), [80, 0, 24, 0]);
/// ```
pub fn resize_body(rows: u16, cols: u16) -> [u8; 4] {
    let (c, r) = (cols.to_le_bytes(), rows.to_le_bytes());
    [c[0], c[1], r[0], r[1]]
}

/// A resize frame's body as (rows, cols); None if it is not 4 bytes.
///
/// ```
/// use zeropty::protocol::{parse_resize, resize_body};
///
/// assert_eq!(parse_resize(&resize_body(24, 80)), Some((24, 80)));
/// assert_eq!(parse_resize(b"abc"), None);
/// ```
pub fn parse_resize(body: &[u8]) -> Option<(u16, u16)> {
    let b: [u8; 4] = body.try_into().ok()?;
    Some((u16::from_le_bytes([b[2], b[3]]), u16::from_le_bytes([b[0], b[1]])))
}

/// The complete frames at the start of a buffer, in order, decoded as `F` (`ServerFrame` or
/// `ClientFrame`); `used()` says how many bytes they took, so the caller keeps the rest for when more
/// arrives. After `FrameError::TooLong` it yields nothing more: the rest of the buffer cannot be framed.
///
/// ```
/// use zeropty::protocol::{Frame as _, Frames, ServerFrame};
///
/// let mut buf = ServerFrame::Output(b"hello").to_vec();
/// buf.extend_from_slice(&ServerFrame::Ended.to_vec()[..3]); // the next frame, cut short
/// let mut frames = Frames::<ServerFrame>::new(&buf);
/// assert_eq!(frames.next(), Some(Ok(ServerFrame::Output(b"hello"))));
/// assert_eq!(frames.next(), None); // waits for the rest
/// assert_eq!(frames.used(), 10); // keep buf[10..] for when more arrives
/// ```
pub struct Frames<'a, F> {
    buf: &'a [u8],
    used: usize,
    broken: bool,
    _frame: std::marker::PhantomData<F>,
}

impl<'a, F: Frame<'a>> Frames<'a, F> {
    /// The frames at the start of `buf`.
    ///
    /// ```
    /// use zeropty::protocol::{ClientFrame, Frames};
    ///
    /// assert_eq!(Frames::<ClientFrame>::new(&[]).count(), 0);
    /// ```
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, used: 0, broken: false, _frame: std::marker::PhantomData }
    }

    /// Bytes taken by the frames returned so far.
    ///
    /// ```
    /// use zeropty::protocol::{ClientFrame, Frame as _, Frames};
    ///
    /// let buf = ClientFrame::Detach.to_vec();
    /// let mut frames = Frames::<ClientFrame>::new(&buf);
    /// assert_eq!(frames.used(), 0);
    /// frames.next();
    /// assert_eq!(frames.used(), buf.len());
    /// ```
    pub fn used(&self) -> usize {
        self.used
    }
}

impl<'a, F: Frame<'a>> Iterator for Frames<'a, F> {
    type Item = Result<F, FrameError>;

    fn next(&mut self) -> Option<Self::Item> {
        let b = &self.buf[self.used..];
        if self.broken || b.len() < FRAME_HEAD {
            return None;
        }
        let len = u32::from_le_bytes(b[1..FRAME_HEAD].try_into().unwrap()) as usize;
        if len > F::MAX_LEN {
            self.broken = true;
            return Some(Err(FrameError::TooLong(len)));
        }
        let body = b.get(FRAME_HEAD..FRAME_HEAD + len)?;
        self.used += FRAME_HEAD + len;
        Some(F::decode(b[0], body))
    }
}

/// A reply as it goes on the connection: JSON, or with `raw` `"<code> <out bytes> <err bytes>\n"` then them.
///
/// ```
/// use zeropty::{engine::fail, protocol::reply_line};
///
/// assert_eq!(reply_line(&fail("no\n"), true), b"1 0 3\nno\n");
/// ```
pub fn reply_line(reply: &Reply, raw: bool) -> Vec<u8> {
    if raw {
        let mut l = format!("{} {} {}\n", reply.code, reply.out.len(), reply.err.len()).into_bytes();
        l.extend_from_slice(reply.out.as_bytes());
        l.extend_from_slice(reply.err.as_bytes());
        l
    } else {
        // A Reply is two strings and an integer: it always serializes.
        let mut l = serde_json::to_vec(reply).expect("a Reply serializes");
        l.push(b'\n');
        l
    }
}

/// A request as a client sends it: its JSON line.
///
/// ```
/// use zeropty::protocol::{request_line, Request};
///
/// let line = request_line(&Request { argv: vec!["ls".into()], cwd: "/".into(), raw: true });
/// assert_eq!(line, b"{\"argv\":[\"ls\"],\"cwd\":\"/\",\"raw\":true}\n");
/// ```
pub fn request_line(req: &Request) -> Vec<u8> {
    // Strings and a bool: it always serializes.
    let mut line = serde_json::to_vec(req).expect("a Request serializes");
    line.push(b'\n');
    line
}

/// The pre-0.2 untyped frame API, kept for code written against it; the wire bytes are the same.
pub mod legacy {
    /// Server to client: what the session prints. Use `ServerFrame::Output`.
    pub const FRAME_OUTPUT: u8 = super::OUTPUT;
    /// Server to client: the session ended. Use `ServerFrame::Ended`.
    pub const FRAME_ENDED: u8 = super::ENDED;
    /// Client to server: keys. Use `ClientFrame::Input`.
    pub const FRAME_INPUT: u8 = super::INPUT;
    /// Client to server: a new size. Use `ClientFrame::Resize`.
    pub const FRAME_RESIZE: u8 = super::RESIZE;
    /// Client to server: detach. Use `ClientFrame::Detach`.
    pub const FRAME_DETACH: u8 = super::DETACH;

    /// Appends a frame of `kind` holding `bytes` to `out`. Use `Frame::encode`.
    pub fn frame(out: &mut Vec<u8>, kind: u8, bytes: &[u8]) {
        super::put(out, kind, bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{fail, ok};

    #[test]
    fn frames_round_trip_and_wait_for_the_rest() {
        let mut b = Vec::new();
        ClientFrame::Input(b"keys").encode(&mut b);
        ClientFrame::Resize { rows: 24, cols: 80 }.encode(&mut b);
        ClientFrame::Detach.encode(&mut b);
        let whole = b.len();
        ClientFrame::Input(b"cut short").encode(&mut b);
        b.truncate(b.len() - 2);
        let mut f = Frames::<ClientFrame>::new(&b);
        assert_eq!(f.next(), Some(Ok(ClientFrame::Input(b"keys"))));
        assert_eq!(f.next(), Some(Ok(ClientFrame::Resize { rows: 24, cols: 80 })));
        assert_eq!(f.next(), Some(Ok(ClientFrame::Detach)));
        assert_eq!(f.next(), None, "an incomplete frame waits");
        assert_eq!(f.used(), whole);
        assert_eq!(parse_resize(b"abc"), None);
    }

    /// The bytes on the wire are those of the untyped API before it (clients in other languages).
    #[test]
    fn wire_bytes_unchanged() {
        let mut old = Vec::new();
        legacy::frame(&mut old, legacy::FRAME_OUTPUT, b"hi");
        legacy::frame(&mut old, legacy::FRAME_ENDED, &[]);
        let mut new = ServerFrame::Output(b"hi").to_vec();
        ServerFrame::Ended.encode(&mut new);
        assert_eq!(new, old);
        assert_eq!(new, b"\x00\x02\x00\x00\x00hi\x01\x00\x00\x00\x00");
        let mut old = Vec::new();
        legacy::frame(&mut old, legacy::FRAME_INPUT, b"k");
        legacy::frame(&mut old, legacy::FRAME_RESIZE, &resize_body(24, 80));
        legacy::frame(&mut old, legacy::FRAME_DETACH, &[]);
        let new =
            [ClientFrame::Input(b"k"), ClientFrame::Resize { rows: 24, cols: 80 }, ClientFrame::Detach].iter().flat_map(|f| f.to_vec()).collect::<Vec<u8>>();
        assert_eq!(new, old);
    }

    #[test]
    fn unknown_kinds_are_skipped_with_an_error() {
        let mut b = Vec::new();
        legacy::frame(&mut b, 9, b"future");
        ServerFrame::Ended.encode(&mut b);
        let got: Vec<_> = Frames::<ServerFrame>::new(&b).collect();
        assert_eq!(got, [Err(FrameError::UnknownKind(9)), Ok(ServerFrame::Ended)]);
        // A server frame read as a client's: Ended (1) is a resize without its 4 bytes.
        let got: Vec<_> = Frames::<ClientFrame>::new(&b).collect();
        assert_eq!(got[1], Err(FrameError::BadBody(1)));
        // The client has no kind 3.
        legacy::frame(&mut b, 3, &[]);
        assert_eq!(Frames::<ClientFrame>::new(&b).last(), Some(Err(FrameError::UnknownKind(3))));
    }

    #[test]
    fn oversized_lengths_stop_the_stream() {
        // A length past the maximum is refused from the head alone, before waiting for 4 GB.
        let mut b = vec![0u8];
        b.extend_from_slice(&u32::MAX.to_le_bytes());
        b.extend_from_slice(b"whatever follows");
        let mut f = Frames::<ClientFrame>::new(&b);
        assert_eq!(f.next(), Some(Err(FrameError::TooLong(u32::MAX as usize))));
        assert_eq!(f.next(), None, "nothing after it can be framed");
        assert_eq!(f.used(), 0);
        let len = ClientFrame::MAX_LEN + 1;
        let mut b = vec![0u8];
        b.extend_from_slice(&(len as u32).to_le_bytes());
        assert_eq!(Frames::<ClientFrame>::new(&b).next(), Some(Err(FrameError::TooLong(len))));
        // The same length is fine from the server: screens and bursts are bigger than keys.
        assert_eq!(Frames::<ServerFrame>::new(&b).next(), None, "waits for the body");
    }

    #[test]
    fn replies_as_they_go_out() {
        assert_eq!(reply_line(&ok("hi\n"), true), b"0 3 0\nhi\n");
        assert_eq!(reply_line(&fail("no\n"), true), b"1 0 3\nno\n");
        assert_eq!(reply_line(&ok("é"), false), "{\"code\":0,\"out\":\"é\",\"err\":\"\"}\n".as_bytes());
        let line = request_line(&Request { argv: vec!["ls".into()], cwd: "/".into(), raw: false });
        assert_eq!(line, b"{\"argv\":[\"ls\"],\"cwd\":\"/\"}\n", "raw left out when false");
        let back: Request = serde_json::from_slice(&line[..line.len() - 1]).unwrap();
        assert!(!back.raw && back.argv == ["ls"]);
    }
}
