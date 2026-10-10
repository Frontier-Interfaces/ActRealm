//! The project label shown for a session, derived from its working directory.
//!
//! The label must let the user recognise the repository, so it is not always
//! the last path component:
//!
//! 1. A path inside a git worktree folder `<Repo>.worktrees/<name>` (at any
//!    depth below `<name>`) is labelled `<Repo>`, and so is a path inside
//!    `<Repo>/.worktrees/<name>`.
//! 2. Otherwise the last component is the candidate.
//! 3. While the candidate is a generic folder or branch name ([`GENERIC`],
//!    any case), its parent folder becomes the candidate instead. This never
//!    climbs to the home directory or above it; outside the home directory it
//!    never climbs to a top-level folder such as `/tmp` or `/Volumes`. When it
//!    cannot climb further, the generic name stays.
//!
//! So `~/ActRealm/Display.worktrees/main` is `Display`, `~/code/foo/main` is
//! `foo`, `~/proj/app/src` is `proj` (both `src` and `app` are generic),
//! `~/work/ActRealm` stays `ActRealm`, `~/code` stays `code`, `~/main` stays
//! `main`, and the home directory itself keeps its own name.

use std::path::{Component, Path};

/// Folder and branch names that say nothing about the repository.
const GENERIC: &[&str] = &[
    "main",
    "master",
    "trunk",
    "develop",
    "dev",
    "src",
    "app",
    "repo",
    "workspace",
    "worktree",
    "code",
];

const WORKTREES_SUFFIX: &str = ".worktrees";

/// The project label for `cwd`, given the user's home directory.
pub(crate) fn project_label(cwd: &str, home: Option<&Path>) -> Option<String> {
    let path = Path::new(cwd);
    let Some(names) = normal_components(path) else {
        // A non-UTF-8 component: keep the earlier last-component label.
        return path
            .file_name()
            .and_then(|name| name.to_str())
            .map(ToOwned::to_owned);
    };
    let last = names.len().checked_sub(1)?;
    let floor = match home.and_then(normal_components) {
        Some(home) if !home.is_empty() && names.starts_with(&home) => home.len(),
        _ if path.is_absolute() => 1,
        _ => 0,
    };
    let (mut index, mut label) = worktree_repository(&names[..last]).unwrap_or((last, names[last]));
    while is_generic(label) && index > floor {
        index -= 1;
        label = names[index];
    }
    Some(label.to_owned())
}

/// The repository of the innermost worktree folder among `names`: `<Repo>`
/// for `<Repo>.worktrees`, or the folder that holds a bare `.worktrees`.
fn worktree_repository<'a>(names: &[&'a str]) -> Option<(usize, &'a str)> {
    let index = names
        .iter()
        .rposition(|name| name.ends_with(WORKTREES_SUFFIX))?;
    match names[index].strip_suffix(WORKTREES_SUFFIX)? {
        "" => index.checked_sub(1).map(|parent| (parent, names[parent])),
        repository => Some((index, repository)),
    }
}

fn is_generic(name: &str) -> bool {
    GENERIC
        .iter()
        .any(|generic| generic.eq_ignore_ascii_case(name))
}

/// The path's named components, or None when one is not UTF-8.
fn normal_components(path: &Path) -> Option<Vec<&str>> {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(name) => Some(name.to_str()),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::project_label;
    use std::path::Path;

    fn label(cwd: &str) -> Option<String> {
        project_label(cwd, Some(Path::new("/Users/mmx")))
    }

    #[test]
    fn worktree_folders_name_their_repository() {
        assert_eq!(
            label("/Users/mmx/ActRealm/Display.worktrees/main").as_deref(),
            Some("Display")
        );
        assert_eq!(
            label("/Users/mmx/ActRealm/ActRealm.worktrees/display-agent-redesign").as_deref(),
            Some("ActRealm")
        );
        assert_eq!(
            label("/Users/mmx/ActRealm/Display.worktrees/main/Sources/App").as_deref(),
            Some("Display")
        );
        // A generic repository name still climbs to its parent.
        assert_eq!(
            label("/Users/mmx/work/app.worktrees/feature").as_deref(),
            Some("work")
        );
        // Worktrees kept in a hidden folder inside the repository.
        assert_eq!(
            label("/Users/mmx/Shop/.worktrees/fix-login").as_deref(),
            Some("Shop")
        );
        assert_eq!(label(".worktrees/fix-login").as_deref(), Some("fix-login"));
        // The worktree container itself is an ordinary name.
        assert_eq!(
            label("/Users/mmx/ActRealm/Display.worktrees").as_deref(),
            Some("Display.worktrees")
        );
    }

    #[test]
    fn generic_names_climb_to_the_first_meaningful_parent() {
        assert_eq!(label("/Users/mmx/code/foo/main").as_deref(), Some("foo"));
        assert_eq!(label("/Users/mmx/proj/app/src").as_deref(), Some("proj"));
        assert_eq!(label("/Users/mmx/foo/Main").as_deref(), Some("foo"));
        assert_eq!(label("/Users/mmx/foo/SRC/").as_deref(), Some("foo"));
        for name in [
            "main",
            "master",
            "trunk",
            "develop",
            "dev",
            "src",
            "app",
            "repo",
            "workspace",
            "worktree",
            "code",
        ] {
            assert_eq!(
                label(&format!("/Users/mmx/Shop/{name}")).as_deref(),
                Some("Shop"),
                "{name}"
            );
        }
    }

    #[test]
    fn ordinary_names_are_unchanged() {
        assert_eq!(
            label("/Users/mmx/work/ActRealm").as_deref(),
            Some("ActRealm")
        );
        assert_eq!(
            label("/Users/mmx/work/ActRealm/crates").as_deref(),
            Some("crates")
        );
        assert_eq!(
            label("/tmp/example-project").as_deref(),
            Some("example-project")
        );
        assert_eq!(label("/Users/mmx/mainline").as_deref(), Some("mainline"));
    }

    #[test]
    fn the_home_directory_bounds_the_climb() {
        // The home directory keeps its own name.
        assert_eq!(label("/Users/mmx").as_deref(), Some("mmx"));
        assert_eq!(label("/Users/mmx/").as_deref(), Some("mmx"));
        // A generic folder right below home stays: home is never a project.
        assert_eq!(label("/Users/mmx/code").as_deref(), Some("code"));
        assert_eq!(label("/Users/mmx/main").as_deref(), Some("main"));
        assert_eq!(label("/Users/mmx/code/main").as_deref(), Some("code"));
        // Outside home a top-level folder is never a project either.
        assert_eq!(label("/tmp/main").as_deref(), Some("main"));
        assert_eq!(label("/srv/app/src").as_deref(), Some("app"));
        assert_eq!(label("/Volumes/Data/Shop/main").as_deref(), Some("Shop"));
        // Without a known home the same top-level rule applies.
        assert_eq!(
            project_label("/Users/mmx/code/foo/main", None).as_deref(),
            Some("foo")
        );
        assert_eq!(project_label("/Users/main", None).as_deref(), Some("main"));
        // A home directory that is a prefix of another user's is not theirs.
        assert_eq!(label("/Users/mmx2/main").as_deref(), Some("mmx2"));
    }

    #[test]
    fn empty_root_and_relative_paths() {
        assert_eq!(label(""), None);
        assert_eq!(label("/"), None);
        assert_eq!(label("main").as_deref(), Some("main"));
        assert_eq!(label("shop/main").as_deref(), Some("shop"));
    }
}
