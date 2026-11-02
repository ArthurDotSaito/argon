use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeRow {
    Dir {
        depth: usize,
        key: String,
        label: String,
        collapsed: bool,
    },
    File {
        depth: usize,
        file: usize,
        name: String,
    },
}

#[derive(Default)]
struct Dir {
    dirs: BTreeMap<String, Dir>,
    files: Vec<(String, usize)>,
}

impl Dir {
    fn build(paths: &[&str]) -> Self {
        let mut root = Dir::default();
        for (file, path) in paths.iter().enumerate() {
            let mut parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
            let name = parts.pop().unwrap_or(path).to_string();
            let mut dir = &mut root;
            for part in parts {
                dir = dir.dirs.entry(part.to_string()).or_default();
            }
            dir.files.push((name, file));
        }
        root.sort();
        root
    }

    fn sort(&mut self) {
        self.files
            .sort_by_key(|(name, _)| (name.to_lowercase(), name.clone()));
        for dir in self.dirs.values_mut() {
            dir.sort();
        }
    }

    fn sorted_dirs(&self) -> Vec<(&String, &Dir)> {
        let mut dirs: Vec<_> = self.dirs.iter().collect();
        dirs.sort_by_key(|(name, _)| (name.to_lowercase(), (*name).clone()));
        dirs
    }

    fn compact(&self, name: &str) -> (String, &Dir) {
        let mut label = name.to_string();
        let mut dir = self;
        while dir.files.is_empty() && dir.dirs.len() == 1 {
            let Some((child_name, child)) = dir.dirs.iter().next() else {
                break;
            };
            label = format!("{label}/{child_name}");
            dir = child;
        }
        (label, dir)
    }

    fn matches(&self, paths: &[&str], filter: &str) -> bool {
        self.files
            .iter()
            .any(|(_, file)| path_matches(paths[*file], filter))
            || self.dirs.values().any(|dir| dir.matches(paths, filter))
    }
}

fn path_matches(path: &str, filter: &str) -> bool {
    filter.is_empty() || path.to_lowercase().contains(filter)
}

struct Walk<'a> {
    paths: &'a [&'a str],
    collapsed: &'a HashSet<String>,
    filter: String,
    rows: Vec<TreeRow>,
}

impl Walk<'_> {
    fn dir(&mut self, dir: &Dir, prefix: &str, depth: usize) {
        for (name, child) in dir.sorted_dirs() {
            if !self.filter.is_empty() && !child.matches(self.paths, &self.filter) {
                continue;
            }
            let (label, target) = child.compact(name);
            let key = if prefix.is_empty() {
                label.clone()
            } else {
                format!("{prefix}/{label}")
            };
            let collapsed = self.filter.is_empty() && self.collapsed.contains(&key);
            self.rows.push(TreeRow::Dir {
                depth,
                key: key.clone(),
                label,
                collapsed,
            });
            if !collapsed {
                self.dir(target, &key, depth + 1);
            }
        }
        for (name, file) in &dir.files {
            if path_matches(self.paths[*file], &self.filter) {
                self.rows.push(TreeRow::File {
                    depth,
                    file: *file,
                    name: name.clone(),
                });
            }
        }
    }
}

pub fn rows(paths: &[&str], collapsed: &HashSet<String>, filter: &str) -> Vec<TreeRow> {
    let root = Dir::build(paths);
    let mut walk = Walk {
        paths,
        collapsed,
        filter: filter.trim().to_lowercase(),
        rows: Vec::new(),
    };
    walk.dir(&root, "", 0);
    walk.rows
}

pub fn file_order(paths: &[&str]) -> Vec<usize> {
    rows(paths, &HashSet::new(), "")
        .into_iter()
        .filter_map(|row| match row {
            TreeRow::File { file, .. } => Some(file),
            TreeRow::Dir { .. } => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATHS: [&str; 5] = [
        "README.md",
        "src/reschedule/engine.ts",
        "src/reschedule/Slot-finder.ts",
        "src/config/flags.ts",
        "test/reschedule/engine.spec.ts",
    ];

    fn labels(rows: &[TreeRow]) -> Vec<String> {
        rows.iter()
            .map(|row| match row {
                TreeRow::Dir { depth, label, .. } => format!("{depth}:{label}/"),
                TreeRow::File { depth, name, .. } => format!("{depth}:{name}"),
            })
            .collect()
    }

    #[test]
    fn dirs_come_first_and_single_child_chains_are_compacted() {
        let rows = rows(&PATHS, &HashSet::new(), "");
        assert_eq!(
            labels(&rows),
            vec![
                "0:src/",
                "1:config/",
                "2:flags.ts",
                "1:reschedule/",
                "2:engine.ts",
                "2:Slot-finder.ts",
                "0:test/reschedule/",
                "1:engine.spec.ts",
                "0:README.md",
            ]
        );
    }

    #[test]
    fn dir_keys_are_full_paths() {
        let rows = rows(&PATHS, &HashSet::new(), "");
        let keys: Vec<&str> = rows
            .iter()
            .filter_map(|row| match row {
                TreeRow::Dir { key, .. } => Some(key.as_str()),
                TreeRow::File { .. } => None,
            })
            .collect();
        assert_eq!(
            keys,
            vec!["src", "src/config", "src/reschedule", "test/reschedule"]
        );
    }

    #[test]
    fn collapsed_dirs_hide_their_children() {
        let collapsed = HashSet::from(["src".to_string()]);
        let rows = rows(&PATHS, &collapsed, "");
        assert_eq!(
            labels(&rows),
            vec![
                "0:src/",
                "0:test/reschedule/",
                "1:engine.spec.ts",
                "0:README.md"
            ]
        );
        assert!(matches!(
            rows[0],
            TreeRow::Dir {
                collapsed: true,
                ..
            }
        ));
    }

    #[test]
    fn filter_keeps_matching_files_with_their_ancestors() {
        let collapsed = HashSet::from(["src".to_string()]);
        let rows = rows(&PATHS, &collapsed, " ENGINE ");
        assert_eq!(
            labels(&rows),
            vec![
                "0:src/",
                "1:reschedule/",
                "2:engine.ts",
                "0:test/reschedule/",
                "1:engine.spec.ts",
            ]
        );
    }

    #[test]
    fn file_order_follows_the_tree() {
        assert_eq!(file_order(&PATHS), vec![3, 1, 2, 4, 0]);
    }
}
