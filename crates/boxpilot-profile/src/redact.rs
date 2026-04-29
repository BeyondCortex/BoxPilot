use url::Url;

const SENSITIVE_KEYS: &[&str] = &[
    "token", "key", "secret", "password", "auth",
    "t", "sub", "subscription", "apikey", "api_key",
];

/// Returns a string suitable for display, logs, and the system-side
/// `manifest.json`'s `source_url_redacted` field.
///
/// - Drops `userinfo` (`user:pass@`).
/// - Replaces sensitive query values with `***`.
/// - Returns the original string unchanged on parse failure (with a
///   `tracing::warn` so we know we couldn't parse it). Better to display
///   a possibly-tokenful URL than to silently drop the whole field — but
///   we should never store an un-redacted URL in a system-side manifest,
///   so the bundle composer (Task 15) treats parse failure as a fatal
///   error rather than calling this function blindly.
pub fn redact_url_for_display(url: &str) -> String {
    let mut parsed = match Url::parse(url) {
        Ok(u) => u,
        Err(_) => {
            tracing::warn!(target: "redact", "could not parse URL for redaction; returning input");
            return url.to_string();
        }
    };
    let _ = parsed.set_username("");
    let _ = parsed.set_password(None);

    let pairs: Vec<(String, String)> = parsed
        .query_pairs()
        .map(|(k, v)| {
            let key = k.to_string();
            let lower = key.to_ascii_lowercase();
            let redacted = if SENSITIVE_KEYS.iter().any(|s| *s == lower) {
                "***".to_string()
            } else {
                v.to_string()
            };
            (key, redacted)
        })
        .collect();

    if !pairs.is_empty() {
        let mut q = parsed.query_pairs_mut();
        q.clear();
        for (k, v) in &pairs {
            q.append_pair(k, v);
        }
    }

    parsed.to_string()
}

/// Strict variant for system-side manifest writing. Returns `None` on
/// parse failure so callers can refuse to compose a manifest.
pub fn redact_url_strict(url: &str) -> Option<String> {
    Url::parse(url).ok()?;
    Some(redact_url_for_display(url))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn redacts_token_query_param() {
        let r = redact_url_for_display("https://host/path?token=ABC&keep=1");
        assert!(r.contains("token=***"));
        assert!(r.contains("keep=1"));
    }

    #[test]
    fn drops_userinfo() {
        let r = redact_url_for_display("https://user:pass@host/p");
        assert!(!r.contains("user"));
        assert!(!r.contains("pass"));
        assert!(r.contains("host"));
    }

    #[test]
    fn case_insensitive_key_matching() {
        let r = redact_url_for_display("https://h/p?Token=X&KEY=Y&Subscription=Z");
        assert!(r.contains("Token=***"));
        assert!(r.contains("KEY=***"));
        assert!(r.contains("Subscription=***"));
    }

    #[test]
    fn passes_through_url_with_no_secrets() {
        let r = redact_url_for_display("https://host/p?lang=en");
        assert_eq!(r, "https://host/p?lang=en");
    }

    #[test]
    fn strict_rejects_garbage() {
        assert!(redact_url_strict("not a url").is_none());
        assert!(redact_url_strict("https://h/p?token=x").is_some());
    }
}
