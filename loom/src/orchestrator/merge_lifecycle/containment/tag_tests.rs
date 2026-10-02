//! A tag named like the target branch must not make an unmerged stage look
//! contained.

use super::super::tests::repo_with_stage_commit;
use super::*;
use std::process::Command;

#[test]
fn a_tag_named_like_the_target_does_not_contain_the_stage() {
    let (temp, head) = repo_with_stage_commit("tagged");
    let root = temp.path();
    let output = Command::new("git")
        .args(["tag", "main", &head])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(output.status.success());
    let work_dir = root.join(".loom").join("work");
    let lifecycle = MergeLifecycle::new("tagged", root, &work_dir);

    let refusal = containment_refusal(&lifecycle, "main").expect("cleanup must be refused");

    assert!(refusal.contains("still holds 1 commit"), "{refusal}");
}
