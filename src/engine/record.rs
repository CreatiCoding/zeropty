//! A session as bytes for another engine (super::handover): everything but its fds, which travel beside
//! the record. The wire format, fields in order (integers little-endian, `field` = [length u32][bytes]):
//!
//! `[RECORD_VERSION u8] field(name) [pid i32] [rows cols base_rows base_cols: u16 each]`
//! `[open u8: bit 0 input, bit 1 output] field(size file) field(input pending) field(screen, Term::export)`
//! `field(line typed) [created u32]`
//!
//! The last two came later: a record from an older zeropty ends before them, and reads with an empty
//! line and the time of the import. A record of another version is refused.

use std::path::PathBuf;

/// Leads a session record; a record of another version is refused.
const RECORD_VERSION: u8 = 1;

/// A session in transit.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct SessionRecord {
    pub(super) name: String,
    pub(super) pid: i32,
    pub(super) rows: u16,
    pub(super) cols: u16,
    /// The size new-session gave it.
    pub(super) base: (u16, u16),
    /// Its input and output are still open: the fds that come with it, in that order.
    pub(super) stdin_open: bool,
    pub(super) out_open: bool,
    /// Where its program reads its size.
    pub(super) size_file: PathBuf,
    /// Input its program has not taken yet.
    pub(super) pending: Vec<u8>,
    /// The screen, as Term::export gives it.
    pub(super) screen: Vec<u8>,
    /// The line being typed while its tty is canonical.
    pub(super) line: Vec<u8>,
    /// When it was made (seconds since 1970); None from an older zeropty.
    pub(super) created: Option<u32>,
}

/// [length u32 LE][bytes]
fn field(o: &mut Vec<u8>, b: &[u8]) {
    o.extend_from_slice(&(b.len() as u32).to_le_bytes());
    o.extend_from_slice(b);
}

/// Reads a record field by field; None past its end.
struct Take<'a>(&'a [u8]);

impl<'a> Take<'a> {
    fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        let (a, b) = (self.0.len() >= n).then(|| self.0.split_at(n))?;
        self.0 = b;
        Some(a)
    }

    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.bytes(N).map(|b| b.try_into().unwrap())
    }

    fn u8(&mut self) -> Option<u8> {
        self.bytes(1).map(|b| b[0])
    }

    fn field(&mut self) -> Option<&'a [u8]> {
        let n = u32::from_le_bytes(self.array()?) as usize;
        self.bytes(n)
    }
}

impl SessionRecord {
    /// How many fds go with it.
    pub(super) fn fds(&self) -> usize {
        usize::from(self.stdin_open) + usize::from(self.out_open)
    }

    pub(super) fn encode(&self) -> Vec<u8> {
        let mut o = vec![RECORD_VERSION];
        field(&mut o, self.name.as_bytes());
        o.extend_from_slice(&self.pid.to_le_bytes());
        for v in [self.rows, self.cols, self.base.0, self.base.1] {
            o.extend_from_slice(&v.to_le_bytes());
        }
        o.push(u8::from(self.stdin_open) | u8::from(self.out_open) << 1);
        field(&mut o, self.size_file.as_os_str().as_encoded_bytes());
        field(&mut o, &self.pending);
        field(&mut o, &self.screen);
        field(&mut o, &self.line);
        o.extend_from_slice(&self.created.unwrap_or(0).to_le_bytes());
        o
    }

    /// The record in `b`; the error says why it is not one (ending in '\n', as replies do).
    pub(super) fn decode(b: &[u8]) -> Result<Self, String> {
        let mut r = Take(b);
        let bad = || "bad session record\n".to_string();
        if r.u8() != Some(RECORD_VERSION) {
            return Err("session record of another version\n".into());
        }
        let name = std::str::from_utf8(r.field().ok_or_else(bad)?).map_err(|_| bad())?.to_owned();
        let pid = i32::from_le_bytes(r.array().ok_or_else(bad)?);
        let mut size = [0u16; 4];
        for v in &mut size {
            *v = u16::from_le_bytes(r.array().ok_or_else(bad)?);
        }
        let open = r.u8().ok_or_else(bad)?;
        let size_file = PathBuf::from(<std::ffi::OsStr as std::os::unix::ffi::OsStrExt>::from_bytes(r.field().ok_or_else(bad)?));
        let pending = r.field().ok_or_else(bad)?.to_vec();
        let screen = r.field().ok_or_else(bad)?.to_vec();
        // The line being typed and when it was made (an older zeropty sends neither).
        let line = r.field().map(<[u8]>::to_vec).unwrap_or_default();
        let created = r.array().map(u32::from_le_bytes);
        Ok(Self {
            name,
            pid,
            rows: size[0],
            cols: size[1],
            base: (size[2], size[3]),
            stdin_open: open & 1 != 0,
            out_open: open & 2 != 0,
            size_file,
            pending,
            screen,
            line,
            created,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::SessionRecord;

    fn sample() -> SessionRecord {
        SessionRecord {
            name: "work/1".into(),
            pid: 4242,
            rows: 50,
            cols: 200,
            base: (24, 80),
            stdin_open: true,
            out_open: false,
            size_file: "/tmp/zeropty-size-1-2-3".into(),
            pending: b"queued".to_vec(),
            screen: b"AMS1...".to_vec(),
            line: b"half a li".to_vec(),
            created: Some(1_760_000_000),
        }
    }

    #[test]
    fn round_trip() {
        let r = sample();
        assert_eq!(SessionRecord::decode(&r.encode()).unwrap(), r);
        assert_eq!(r.fds(), 1);
    }

    /// A record from before `line` and `created` were sent still reads.
    #[test]
    fn older_records_read() {
        let r = sample();
        let mut bytes = r.encode();
        bytes.truncate(bytes.len() - (4 + r.line.len()) - 4);
        let old = SessionRecord::decode(&bytes).unwrap();
        assert_eq!((old.line.as_slice(), old.created), (&b""[..], None));
        assert_eq!(old.screen, r.screen);
    }

    #[test]
    fn bad_records_are_refused() {
        let bytes = sample().encode();
        assert_eq!(SessionRecord::decode(&[2]).unwrap_err(), "session record of another version\n");
        assert_eq!(SessionRecord::decode(&[]).unwrap_err(), "session record of another version\n");
        for cut in [1, 5, 12, 20, 30] {
            assert_eq!(SessionRecord::decode(&bytes[..cut]).unwrap_err(), "bad session record\n", "cut at {cut}");
        }
    }
}
