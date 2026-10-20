//! The plugin protocol's JSON lines (docs/PLUGINS.md): what the socket, plugin libraries and the C ABI
//! send and read. Hidden helpers; the format itself is the stable part.
use super::api::Reply;

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding: how output's bytes travel in JSON (they need not be UTF-8).
#[doc(hidden)]
pub fn base64(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for k in 0..4 {
            out.push(if k <= c.len() { B64[(n >> (18 - 6 * k) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

/// A command for a plugin as a line of the protocol (without the line feed): {"call":7,"argv":[...],"cwd":"/"}.
/// "call" first, so a plugin may tell the kind of a message by its start.
#[doc(hidden)]
pub fn call_json(id: u64, argv: &[String], cwd: &str) -> String {
    format!("{{\"call\":{id},\"argv\":{},\"cwd\":{}}}", serde_json::to_string(argv).unwrap_or_default(), json_str(cwd))
}

/// A reply as the protocol's JSON object, led by `key` ({"id":3,...} or {"reply":7,...}).
#[doc(hidden)]
pub fn reply_json(r: &Reply, key: &str, id: u64) -> String {
    format!("{{\"{key}\":{id},\"code\":{},\"out\":{},\"err\":{}}}", r.code, json_str(&r.out), json_str(&r.err))
}

pub(super) fn json_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_default()
}

/// A reply from a JSON object with code/out/err (what a plugin answers).
#[doc(hidden)]
pub fn reply_from_json(v: &serde_json::Value) -> Reply {
    Reply { code: v["code"].as_i64().unwrap_or(1) as i32, out: v["out"].as_str().unwrap_or("").to_owned(), err: v["err"].as_str().unwrap_or("").to_owned() }
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::api::{Event, ExitReason, ok};

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(&[0xff, 0x00, 0x80]), "/wCA");
    }

    #[test]
    fn messages_lead_with_their_kind() {
        let ev = Event::Output { session: "a\"b".into(), data: b"hi".to_vec() };
        assert_eq!(ev.to_json(), r#"{"event":"output","session":"a\"b","data":"aGk="}"#);
        let ev = Event::SessionExited { session: "w".into(), reason: ExitReason::Killed };
        assert_eq!(ev.to_json(), r#"{"event":"session-exited","session":"w","reason":"killed"}"#);
        assert_eq!(call_json(7, &["x".into(), "y z".into()], "/"), r#"{"call":7,"argv":["x","y z"],"cwd":"/"}"#);
        assert_eq!(reply_json(&ok("o\n"), "id", 3), r#"{"id":3,"code":0,"out":"o\n","err":""}"#);
    }
}
