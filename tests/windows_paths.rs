//! Gramax: a checkout leaves paths Windows refuses off the disk instead of stopping
//! (`libgit2-sys/gramax/gramax_checkout.h`). The Windows rules are switched on for this whole test
//! binary, so it runs the same on every platform.

use std::path::Path;

use git2::build::CheckoutBuilder;
use git2::{IndexEntryFlag, Oid, Repository, Status, StatusOptions};

const REFUSED: &str = "Протокол 10:30.pdf";

extern "C" fn windows_rules() -> std::os::raw::c_int {
    1
}

fn repo() -> (tempfile::TempDir, Repository) {
    unsafe { git2::raw::git_checkout_gramax_win32_rules(Some(windows_rules)) };
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    (dir, repo)
}

fn tree(repo: &Repository, files: &[(&str, &str)]) -> Oid {
    let mut builder = repo.treebuilder(None).unwrap();
    for (name, content) in files {
        let blob = repo.blob(content.as_bytes()).unwrap();
        builder.insert(name, blob, 0o100644).unwrap();
    }
    builder.write().unwrap()
}

fn checkout(repo: &Repository, tree: Oid, opts: &mut CheckoutBuilder) -> Result<(), git2::Error> {
    repo.checkout_tree(repo.find_tree(tree).unwrap().as_object(), Some(opts))
}

fn commit(repo: &Repository, tree: Oid) {
    let sig = git2::Signature::now("t", "t@t").unwrap();
    let parent = repo.head().ok().map(|head| head.peel_to_commit().unwrap());
    let parents = parent.iter().collect::<Vec<_>>();
    repo.commit(Some("HEAD"), &sig, &sig, "c", &repo.find_tree(tree).unwrap(), &parents)
        .unwrap();
}

fn changes(repo: &Repository) -> Vec<(String, Status)> {
    let mut opts = StatusOptions::new();
    opts.include_untracked(true);
    repo.statuses(Some(&mut opts))
        .unwrap()
        .iter()
        .map(|entry| (entry.path().unwrap().to_owned(), entry.status()))
        .collect()
}

#[test]
fn a_refused_path_goes_to_the_index_only() {
    let (dir, repo) = repo();
    let tree = tree(&repo, &[(REFUSED, "pdf"), ("ok.md", "ok")]);

    checkout(&repo, tree, CheckoutBuilder::new().force()).unwrap();
    commit(&repo, tree);

    assert!(dir.path().join("ok.md").exists());
    assert!(!dir.path().join(REFUSED).exists());

    let index = repo.index().unwrap();
    let entry = index.get_path(Path::new(REFUSED), 0).unwrap();
    assert_eq!(entry.id, repo.find_tree(tree).unwrap().get_name(REFUSED).unwrap().id());
    assert_ne!(entry.flags & IndexEntryFlag::VALID.bits(), 0);
    assert_eq!(changes(&repo), vec![]);
}

#[test]
fn a_change_to_a_refused_path_is_no_conflict() {
    let (dir, repo) = repo();
    let before = tree(&repo, &[(REFUSED, "v1"), ("ok.md", "ok")]);
    checkout(&repo, before, CheckoutBuilder::new().force()).unwrap();
    commit(&repo, before);

    // A safe checkout: the file missing from the disk would read as deleted by the user.
    let after = tree(&repo, &[(REFUSED, "v2"), ("ok.md", "ok")]);
    checkout(&repo, after, &mut CheckoutBuilder::new()).unwrap();

    let entry = repo.index().unwrap().get_path(Path::new(REFUSED), 0).unwrap();
    assert_eq!(entry.id, repo.blob(b"v2").unwrap());
    assert!(!dir.path().join(REFUSED).exists());
}

#[test]
fn a_refused_path_gone_from_the_target_leaves_the_index() {
    let (_dir, repo) = repo();
    let before = tree(&repo, &[(REFUSED, "pdf"), ("ok.md", "ok")]);
    checkout(&repo, before, CheckoutBuilder::new().force()).unwrap();
    commit(&repo, before);

    let after = tree(&repo, &[("ok.md", "ok")]);
    checkout(&repo, after, &mut CheckoutBuilder::new()).unwrap();

    assert!(repo.index().unwrap().get_path(Path::new(REFUSED), 0).is_none());
}

#[test]
fn a_refused_folder_is_left_off_too() {
    let (dir, repo) = repo();
    let blob = repo.blob(b"md").unwrap();
    let mut folder = repo.treebuilder(None).unwrap();
    folder.insert("a.md", blob, 0o100644).unwrap();
    let folder = folder.write().unwrap();
    let mut root = repo.treebuilder(None).unwrap();
    root.insert("con", folder, 0o040000).unwrap();
    let root = root.write().unwrap();

    checkout(&repo, root, CheckoutBuilder::new().force()).unwrap();

    assert!(!dir.path().join("con").exists());
    assert!(repo.index().unwrap().get_path(Path::new("con/a.md"), 0).is_some());
}
