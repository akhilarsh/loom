use super::*;

/// `1 XY sub mH mI mW hH hI <path>` entry.
fn changed(xy: &str, path: &str) -> String {
    format!("1 {xy} N... 100644 100644 100644 aaaa bbbb {path}\0")
}

fn untracked(path: &str) -> String {
    format!("? {path}\0")
}

fn diff(entries: &[(&str, &str)]) -> String {
    entries
        .iter()
        .map(|(status, path)| format!("{status}\0{path}\0"))
        .collect()
}

fn never(_: &str) -> bool {
    false
}

fn always(_: &str) -> bool {
    true
}

#[test]
fn unrelated_changes_do_not_overlap() {
    let status = changed(".M", "other.txt") + &changed("M.", "staged.txt") + &untracked("new.txt");
    let touched = diff(&[("M", "merged.txt"), ("A", "added.txt")]);
    assert_eq!(
        classify(&status, &touched, never),
        Classification::NoOverlap
    );
}

#[test]
fn equal_untracked_overlap_is_removed() {
    let status = untracked("added.txt");
    let touched = diff(&[("A", "added.txt")]);
    assert_eq!(
        classify(&status, &touched, always),
        Classification::RemoveUntracked {
            paths: vec!["added.txt".to_string()]
        }
    );
}

#[test]
fn tracked_overlap_asks_for_a_reapply() {
    let status = changed(".M", "merged.txt") + &untracked("added.txt");
    let touched = diff(&[("M", "merged.txt"), ("A", "added.txt")]);
    assert_eq!(
        classify(&status, &touched, always),
        Classification::Reapply {
            tracked: vec!["merged.txt".to_string()],
            remove_untracked: vec!["added.txt".to_string()],
        }
    );
}

#[test]
fn staged_only_change_counts_as_tracked() {
    let status = changed("M.", "merged.txt");
    let touched = diff(&[("M", "merged.txt")]);
    assert_eq!(
        classify(&status, &touched, never),
        Classification::Reapply {
            tracked: vec!["merged.txt".to_string()],
            remove_untracked: Vec::new(),
        }
    );
}

#[test]
fn differing_untracked_overlap_blocks() {
    let status = changed(".M", "merged.txt") + &untracked("added.txt");
    let touched = diff(&[("M", "merged.txt"), ("A", "added.txt")]);
    assert_eq!(
        classify(&status, &touched, never),
        Classification::Blocked {
            paths: vec!["added.txt".to_string()]
        }
    );
}

#[test]
fn rename_in_the_checkout_counts_both_paths() {
    let status = "2 R. N... 100644 100644 100644 aaaa bbbb R100 new name.txt\0old.txt\0";
    let touched = diff(&[("D", "old.txt")]);
    assert_eq!(
        classify(status, &touched, never),
        Classification::Reapply {
            tracked: vec!["old.txt".to_string()],
            remove_untracked: Vec::new(),
        }
    );
}

#[test]
fn rename_in_the_merge_is_a_delete_plus_add() {
    let status = changed(".M", "old.txt") + &untracked("renamed.txt");
    let touched = diff(&[("D", "old.txt"), ("A", "renamed.txt")]);
    assert_eq!(
        classify(&status, &touched, always),
        Classification::Reapply {
            tracked: vec!["old.txt".to_string()],
            remove_untracked: vec!["renamed.txt".to_string()],
        }
    );
}

#[test]
fn paths_with_spaces_and_newlines_survive() {
    let status = changed(".M", "dir/with space.txt") + &untracked("odd\nname.txt");
    let touched = diff(&[("M", "dir/with space.txt"), ("A", "odd\nname.txt")]);
    assert_eq!(
        classify(&status, &touched, always),
        Classification::Reapply {
            tracked: vec!["dir/with space.txt".to_string()],
            remove_untracked: vec!["odd\nname.txt".to_string()],
        }
    );
}

#[test]
fn unmerged_entries_block_even_without_overlap() {
    let status = "u UU N... 100644 100644 100644 100644 aaaa bbbb cccc sp ace.txt\0";
    let touched = diff(&[("M", "merged.txt")]);
    assert_eq!(
        classify(status, &touched, never),
        Classification::Blocked {
            paths: vec!["sp ace.txt".to_string()]
        }
    );
}

#[test]
fn ignored_and_header_lines_are_skipped() {
    let status = "# branch.oid abc\0! target/debug\0";
    let touched = diff(&[("M", "target/debug")]);
    assert_eq!(classify(status, &touched, never), Classification::NoOverlap);
}
