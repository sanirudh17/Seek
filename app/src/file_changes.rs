//! Apply Shell-confirmed removals immediately, independently of journal catch-up.
use std::path::{Path, PathBuf};
pub fn under_any(path: &Path, removed: &[PathBuf]) -> bool {
    let mut path = seek_engine::paths::internal(path);
    path.make_ascii_lowercase();
    removed.iter().any(|root| {
        let mut root = seek_engine::paths::internal(root);
        root.make_ascii_lowercase();
        while root.last() == Some(&b'/') {
            root.pop();
        }
        path == root || path.starts_with(&root) && path.get(root.len()) == Some(&b'/')
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn removed_subtrees_cover_aliases_but_not_neighbor_names() {
        let roots = vec![PathBuf::from(r"C:\Owned\Folder")];
        assert!(under_any(
            Path::new(r"\\?\c:\owned\folder\child.txt"),
            &roots
        ));
        assert!(!under_any(
            Path::new(r"C:\Owned\Folder-two\child.txt"),
            &roots
        ));
    }
}
