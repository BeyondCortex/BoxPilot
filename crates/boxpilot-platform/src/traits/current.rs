//! Atomic "current core" pointer. Linux: symlink + rename(2). Windows:
//! junction + MoveFileExW (Sub-project #2 PR 3.5). Per spec §7.2 step 14e.

use std::path::Path;

/// Atomically stage `link` → `target` through a `.new` temporary,
/// matching the pattern used for `cores/current` in the commit transaction.
/// Linux: `symlink(target, link.new)` + `rename(link.new, link)`.
/// Windows: junction (Sub-project #2 PR 3.5 will fill in real logic).
pub trait CurrentPointer: Send + Sync {
    fn set_atomic(&self, link: &Path, target: &Path) -> std::io::Result<()>;
}
