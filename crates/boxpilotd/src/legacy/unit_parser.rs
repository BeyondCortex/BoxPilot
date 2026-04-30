//! Minimal systemd unit parser. We only need `[Service] ExecStart=` and the
//! `-c <path>` / `--config <path>` argument — the full systemd grammar (line
//! continuations, expansion, multiple ExecStart=, environment files, etc.)
//! is out of scope.

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecStart {
    pub raw: String,
    pub config_path: Option<PathBuf>,
}

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("[Service] section not found")]
    NoServiceSection,
    #[error("ExecStart= not found in [Service]")]
    NoExecStart,
}

pub fn parse_exec_start(unit_text: &str) -> Result<ExecStart, ParseError> {
    let mut in_service = false;
    let mut exec_start_line: Option<String> = None;

    for raw_line in unit_text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_service = line.eq_ignore_ascii_case("[Service]");
            continue;
        }
        if !in_service {
            continue;
        }
        // Accept the optional `-`, `+`, `@`, `:`, `!` modifiers systemd
        // allows on ExecStart= (ignore-failure / elevated / etc.). We don't
        // care which one was used; we just want the command line.
        if let Some(after) = line.strip_prefix("ExecStart=") {
            let stripped = after.trim_start_matches(['-', '+', '@', ':', '!']);
            exec_start_line = Some(stripped.trim().to_string());
        }
    }
    if !in_service && exec_start_line.is_none() {
        return Err(ParseError::NoServiceSection);
    }
    let raw = exec_start_line.ok_or(ParseError::NoExecStart)?;
    let config_path = extract_config_arg(&raw);
    Ok(ExecStart { raw, config_path })
}

fn extract_config_arg(cmdline: &str) -> Option<PathBuf> {
    // Tokenize on whitespace. We don't try to honor shell-style quoting
    // because sing-box.service in the wild does not use it; if we see
    // single/double quotes around the path, strip them and move on.
    let mut iter = cmdline.split_whitespace().peekable();
    while let Some(tok) = iter.next() {
        let mut next = || iter.next().map(|s| s.to_string());
        let path = match tok {
            "-c" | "--config" | "-C" => next(),
            _ if tok.starts_with("--config=") => Some(tok["--config=".len()..].to_string()),
            _ if tok.starts_with("-c=") => Some(tok["-c=".len()..].to_string()),
            _ => None,
        };
        if let Some(p) = path {
            let trimmed = p.trim_matches(['"', '\'']).to_string();
            return Some(PathBuf::from(trimmed));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn unit(body: &str) -> String {
        format!("[Unit]\nDescription=x\n\n[Service]\n{body}\n\n[Install]\nWantedBy=multi-user.target\n")
    }

    #[test]
    fn parses_simple_exec_start_with_dash_c() {
        let u = unit("ExecStart=/usr/bin/sing-box run -c /etc/sing-box/config.json");
        let r = parse_exec_start(&u).unwrap();
        assert_eq!(r.raw, "/usr/bin/sing-box run -c /etc/sing-box/config.json");
        assert_eq!(
            r.config_path,
            Some(PathBuf::from("/etc/sing-box/config.json"))
        );
    }

    #[test]
    fn parses_long_form_config_flag() {
        let u = unit("ExecStart=/usr/bin/sing-box run --config /etc/sb/c.json");
        let r = parse_exec_start(&u).unwrap();
        assert_eq!(r.config_path, Some(PathBuf::from("/etc/sb/c.json")));
    }

    #[test]
    fn parses_equals_form() {
        let u = unit("ExecStart=/usr/bin/sing-box run --config=/etc/sb/c.json");
        let r = parse_exec_start(&u).unwrap();
        assert_eq!(r.config_path, Some(PathBuf::from("/etc/sb/c.json")));
    }

    #[test]
    fn returns_none_when_no_config_flag() {
        let u = unit("ExecStart=/usr/bin/sing-box run");
        let r = parse_exec_start(&u).unwrap();
        assert_eq!(r.raw, "/usr/bin/sing-box run");
        assert_eq!(r.config_path, None);
    }

    #[test]
    fn skips_comments_and_blank_lines() {
        let u = "
# comment
[Unit]
Description=x

[Service]
; inline comment
ExecStart=/usr/bin/sing-box run -c /etc/sb/c.json
";
        let r = parse_exec_start(u).unwrap();
        assert_eq!(r.config_path, Some(PathBuf::from("/etc/sb/c.json")));
    }

    #[test]
    fn rejects_unit_without_service_section() {
        let u = "[Unit]\nDescription=x\n[Install]\nWantedBy=x\n";
        assert!(matches!(parse_exec_start(u), Err(ParseError::NoServiceSection)));
    }

    #[test]
    fn rejects_service_without_exec_start() {
        let u = "[Service]\nUser=root\n";
        assert!(matches!(parse_exec_start(u), Err(ParseError::NoExecStart)));
    }

    #[test]
    fn handles_dash_modifier_on_exec_start() {
        let u = unit("ExecStart=-/usr/bin/sing-box run -c /etc/sb/c.json");
        let r = parse_exec_start(&u).unwrap();
        assert_eq!(r.config_path, Some(PathBuf::from("/etc/sb/c.json")));
    }
}
