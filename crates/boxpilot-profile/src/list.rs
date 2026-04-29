use crate::meta::{read_metadata, ProfileMetadata};
use crate::store::ProfileStorePaths;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("profile {0} not found")]
    NotFound(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone)]
pub struct ProfileStore {
    paths: ProfileStorePaths,
}

impl ProfileStore {
    pub fn new(paths: ProfileStorePaths) -> Self {
        Self { paths }
    }

    pub fn paths(&self) -> &ProfileStorePaths { &self.paths }

    pub fn list(&self) -> Result<Vec<ProfileMetadata>, StoreError> {
        let dir = self.paths.profiles_dir();
        let read = match std::fs::read_dir(&dir) {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(StoreError::Io(e)),
        };
        let mut out = Vec::new();
        for entry in read {
            let entry = match entry { Ok(e) => e, Err(_) => continue };
            if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) { continue; }
            let id = match entry.file_name().to_str() { Some(s) => s.to_string(), None => continue };
            let meta_path = self.paths.profile_metadata(&id);
            match read_metadata(&meta_path) {
                Ok(m) => out.push(m),
                Err(e) => tracing::warn!(profile_id = %id, error = %e, "skipping profile with unreadable metadata"),
            }
        }
        // Lexicographic sort assumes timestamps are RFC3339 normalised to a
        // consistent timezone (UTC); enforced by the importers in Tasks 9 and 11.
        out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(out)
    }

    pub fn get(&self, id: &str) -> Result<ProfileMetadata, StoreError> {
        let meta_path = self.paths.profile_metadata(id);
        match read_metadata(&meta_path) {
            Ok(m) => Ok(m),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(StoreError::NotFound(id.to_string()))
            }
            Err(e) => Err(StoreError::Io(e)),
        }
    }

    pub fn read_source_bytes(&self, id: &str) -> Result<Vec<u8>, StoreError> {
        let p = self.paths.profile_source(id);
        match std::fs::read(&p) {
            Ok(bytes) => Ok(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(StoreError::NotFound(id.to_string()))
            }
            Err(e) => Err(StoreError::Io(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meta::{write_metadata, ProfileMetadata};
    use pretty_assertions::assert_eq;

    fn fixture() -> (tempfile::TempDir, ProfileStore) {
        let tmp = tempfile::tempdir().unwrap();
        let store = ProfileStore::new(ProfileStorePaths::new(tmp.path().to_path_buf()));
        (tmp, store)
    }

    fn put(store: &ProfileStore, id: &str, name: &str, ts: &str) {
        let dir = store.paths().profile_dir(id);
        std::fs::create_dir_all(&dir).unwrap();
        let mut m = ProfileMetadata::new_local(id, name, ts, "h");
        m.updated_at = ts.into();
        write_metadata(&store.paths().profile_metadata(id), &m).unwrap();
    }

    #[test]
    fn list_empty_when_dir_missing() {
        let (_t, s) = fixture();
        assert!(s.list().unwrap().is_empty());
    }

    #[test]
    fn list_sorts_by_updated_at_desc() {
        let (_t, s) = fixture();
        put(&s, "a", "A", "2026-04-29T00:00:00-07:00");
        put(&s, "b", "B", "2026-04-30T00:00:00-07:00");
        put(&s, "c", "C", "2026-04-28T00:00:00-07:00");
        let ids: Vec<_> = s.list().unwrap().into_iter().map(|m| m.id).collect();
        assert_eq!(ids, vec!["b", "a", "c"]);
    }

    #[test]
    fn list_skips_bad_metadata_without_failing() {
        let (_t, s) = fixture();
        put(&s, "good", "g", "2026-04-30T00:00:00-07:00");
        let bad = s.paths().profile_dir("bad");
        std::fs::create_dir_all(&bad).unwrap();
        std::fs::write(s.paths().profile_metadata("bad"), b"{not json").unwrap();
        let ids: Vec<_> = s.list().unwrap().into_iter().map(|m| m.id).collect();
        assert_eq!(ids, vec!["good"]);
    }

    #[test]
    fn get_returns_not_found_for_unknown_id() {
        let (_t, s) = fixture();
        assert!(matches!(s.get("missing"), Err(StoreError::NotFound(_))));
    }
}
