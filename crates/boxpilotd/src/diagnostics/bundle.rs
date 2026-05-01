//! Bundle composition: file collection, redaction, tarball writer.

/// Drop journal/stderr lines that contain markers correlated with secrets.
/// Text-stage redaction is fundamentally heuristic — we cannot parse a
/// freeform journal line into JSON. Schema-aware walking is reserved for
/// `*.json` artifacts inside the bundle.
///
/// Shared call site between [`super::compose`] and the activation
/// pipeline's `sing-box check` stderr scrub.
pub fn redact_journal_lines(s: &str) -> String {
    s.lines()
        .filter(|line| {
            let lower = line.to_ascii_lowercase();
            !(lower.contains("password")
                || lower.contains("uuid")
                || lower.contains("private_key")
                || lower.contains("token=")
                || lower.contains("secret"))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn drops_password_lines() {
        let s = "ok 1\npassword=hunter2\nok 2";
        assert_eq!(redact_journal_lines(s), "ok 1\nok 2");
    }

    #[test]
    fn drops_uuid_and_private_key_and_token_and_secret() {
        let s = "a\nuuid=x\nb\nprivate_key=y\nc\ntoken=z\nd\nsecret=q\ne";
        assert_eq!(redact_journal_lines(s), "a\nb\nc\nd\ne");
    }

    #[test]
    fn passes_through_non_secret_lines() {
        let s = "starting up\nlistening on 127.0.0.1:9090";
        assert_eq!(
            redact_journal_lines(s),
            "starting up\nlistening on 127.0.0.1:9090"
        );
    }
}
