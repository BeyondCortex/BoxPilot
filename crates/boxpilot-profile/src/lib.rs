//! User-side profile store, editor, and activation-bundle composer.
//!
//! Everything in this crate runs as the desktop user — never as root.
//! It owns `~/.local/share/boxpilot/` per spec §5.6 and produces the
//! constrained activation bundle described in §9.2 for plan #5 to
//! transfer to `boxpilotd`.

#[cfg(test)]
mod sanity {
    #[test]
    fn crate_compiles() {
        assert_eq!(2 + 2, 4);
    }
}
