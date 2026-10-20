//! tmux formats: "#{name}" replaced by a variable's value. Which variables exist and what they hold is
//! the caller's (Engine::format_var); this is the template language.

/// `fmt` with each #{name} replaced by `var(name)`; a "#{" never closed stays as it is.
pub(super) fn expand(fmt: &str, mut var: impl FnMut(&str) -> String) -> String {
    let mut out = String::with_capacity(fmt.len());
    let mut rest = fmt;
    while let Some(i) = rest.find("#{") {
        out.push_str(&rest[..i]);
        let Some(j) = rest[i..].find('}') else {
            out.push_str(&rest[i..]);
            return out;
        };
        out.push_str(&var(&rest[i + 2..i + j]));
        rest = &rest[i + j + 1..];
    }
    out.push_str(rest);
    out
}

/// Whether `fmt` has variables read from the screen: output not read yet is then read first, as for a
/// capture.
pub(super) fn reads_screen(fmt: &str) -> bool {
    ["#{cursor_", "#{history_size", "#{alternate_on"].iter().any(|v| fmt.contains(v))
}

/// Whether `fmt` asks for the server's own variable (#{version}) and for no session's: only such a
/// format is answered when there is no session and no -t. Any other format (plain text, "", a session
/// variable) then still fails with "can't find session", keeping display-message's exit codes.
pub(super) fn only_server_vars(fmt: &str) -> bool {
    fmt.contains("#{version}") && fmt.split("#{").skip(1).all(|v| v.starts_with("version}"))
}

/// `fmt` filled in with no session: the server's variables, every other one empty.
pub(super) fn without_session(fmt: &str) -> String {
    expand(fmt, |v| if v == "version" { super::VERSION.to_string() } else { String::new() })
}

#[cfg(test)]
mod tests {
    use super::{expand, only_server_vars, reads_screen, without_session};

    #[test]
    fn server_vars_alone_need_no_session() {
        assert!(only_server_vars("#{version}"));
        assert!(only_server_vars("v#{version} #{version}"));
        assert!(!only_server_vars("#{version} #{session_name}"));
        assert!(!only_server_vars("plain"));
        assert!(!only_server_vars(""));
    }

    #[test]
    fn without_a_session_only_the_server_vars_are_filled() {
        assert_eq!(without_session("v=#{version} s=#{session_name} lit"), format!("v={} s= lit", crate::engine::VERSION));
        assert_eq!(without_session(""), "");
    }

    #[test]
    fn variables_are_replaced() {
        let var = |n: &str| match n {
            "a" => "1".to_string(),
            "long_name" => "two".to_string(),
            _ => String::new(),
        };
        assert_eq!(expand("#{a}:#{long_name} #{unknown}x", var), "1:two x");
        assert_eq!(expand("no variables", var), "no variables");
        assert_eq!(expand("#{a} #{open", var), "1 #{open", "an unclosed #{{ stays");
        assert_eq!(expand("#a #{}", var), "#a ", "an empty name is unknown");
        assert_eq!(expand("한#{a}글", var), "한1글");
    }

    #[test]
    fn screen_variables() {
        assert!(reads_screen("#{cursor_x},#{cursor_y}"));
        assert!(reads_screen("#{history_size}"));
        assert!(!reads_screen("#{session_name} #{pane_pid}"));
    }
}
