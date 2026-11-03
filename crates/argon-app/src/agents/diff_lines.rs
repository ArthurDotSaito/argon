use std::collections::{HashMap, HashSet};

use argon_core::{Side, parse_patch};

use crate::review::patch_order;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiffLines {
    files: HashMap<String, HashSet<(u32, Side)>>,
}

impl DiffLines {
    pub fn from_patches<'a>(files: impl IntoIterator<Item = (&'a str, Option<&'a str>)>) -> Self {
        let files = files
            .into_iter()
            .map(|(path, patch)| {
                let lines = patch
                    .and_then(|patch| parse_patch(patch).ok())
                    .map(|hunks| patch_order(&hunks).into_keys().collect())
                    .unwrap_or_default();
                (path.to_owned(), lines)
            })
            .collect();
        Self { files }
    }

    pub fn contains(&self, path: &str, line: u32, side: Side) -> bool {
        self.files
            .get(path)
            .is_some_and(|lines| lines.contains(&(line, side)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATCH: &str = "@@ -10,3 +10,4 @@ fn a() {\n ctx\n-old\n+new\n+more\n ctx2\n";

    #[test]
    fn lines_inside_hunks_on_the_right_side_are_commentable() {
        let lines = DiffLines::from_patches([("src/a.rs", Some(PATCH)), ("bin.png", None)]);
        assert!(lines.contains("src/a.rs", 11, Side::Right));
        assert!(lines.contains("src/a.rs", 11, Side::Left));
        assert!(lines.contains("src/a.rs", 10, Side::Right));
        assert!(!lines.contains("src/a.rs", 40, Side::Right));
        assert!(!lines.contains("src/a.rs", 13, Side::Left));
        assert!(!lines.contains("bin.png", 1, Side::Right));
        assert!(!lines.contains("src/missing.rs", 11, Side::Right));
    }
}
