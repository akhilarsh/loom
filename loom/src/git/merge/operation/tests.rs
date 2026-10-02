use super::*;
use crate::git::merge::test_support::init_repo;

#[test]
fn a_clean_repository_has_no_operation() {
    let repo = init_repo();
    assert_eq!(operator_operation(repo.path()).unwrap(), None);
}

#[test]
fn each_marker_is_reported() {
    let repo = init_repo();
    for marker in OPERATOR_MARKERS {
        let path = repo.path().join(".git").join(marker);
        std::fs::create_dir(&path).unwrap();
        assert_eq!(
            operator_operation(repo.path()).unwrap().as_deref(),
            Some(marker)
        );
        std::fs::remove_dir(&path).unwrap();
    }
}

#[test]
fn a_failure_other_than_not_found_is_an_error() {
    let repo = init_repo();
    let file = repo.path().join("a.txt");
    assert!(!path_present(&repo.path().join("missing")).unwrap());
    assert!(path_present(&file).unwrap());
    // A path below a regular file fails with "not a directory", not NotFound.
    assert!(path_present(&file.join("below")).is_err());
}
