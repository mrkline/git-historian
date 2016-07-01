extern crate git2;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use git2::*;

pub type Link<T> = Rc<RefCell<T>>;

/// A change in a file through Git history
pub struct HistoryNode {
    /// What kind of change?
    pub change: Delta,
    /// When was the change?
    pub when: Time,
    /// What's the previous change?
    pub previous: Option<Link<HistoryNode>>,
}

fn new_node(c: Delta, w: Time) -> Link<HistoryNode> {
    Rc::new(RefCell::new(
        HistoryNode{ change: c, when: w, previous: None }))
}

/// The history of a file can be tracked as a tree.
pub struct PathEntry {
    pub head: Link<HistoryNode>,
    tail: Link<HistoryNode>,
}

pub type HistoryTree = HashMap<String, PathEntry>;

pub type PathSet = HashSet<String>;

fn append_node(tree: &mut HistoryTree, node: Link<HistoryNode>, key: &str) {
    if tree.contains_key(key) {
        // If we already have an entry for the given path,
        // append the new node to the tail.
        let entry = tree.get_mut(key).unwrap();
        assert!(entry.tail.borrow().previous.is_none());

        let new_tail = node.clone();
        entry.tail.borrow_mut().previous = Some(node);
        entry.tail = new_tail;
    }
    else {
        // Otherwise we'll just create a new entry.
        let t = node.clone(); // Bump the refcount

        // Head and tail will be the same since the branch has one node.
        tree.insert(key.to_string(), PathEntry{ head: node, tail: t});
    }
}

fn path_set_from_index(i: &Index) -> PathSet {
    let mut ret = PathSet::new();

    for file in i.iter() {
        ret.insert(String::from_utf8(file.path).unwrap());
    }

    ret
}

pub fn path_set_from_reference(name: &str, repo: &Repository) -> PathSet {
    let ref_id = repo.refname_to_id(name).unwrap();
    let ref_commit = repo.find_commit(ref_id).unwrap();

    let mut idx = Index::new().unwrap();
    idx.read_tree(&ref_commit.tree().unwrap()).unwrap();

    path_set_from_index(&idx)
}

pub fn gather_history(mut paths: PathSet, repo: &Repository) -> HistoryTree {
    // We'll need some history
    let mut history = HistoryTree::new();

    // Start walking.
    let mut walk = repo.revwalk().unwrap();
    walk.set_sorting(SORT_TIME | SORT_TOPOLOGICAL);
    walk.push_head().unwrap();

    for id in walk {
        let id = id.unwrap();
        let commit = repo.find_commit(id).unwrap();
        println!("Visiting {:?}", id);
        if let Some(diff) = diff_commit(&commit, &repo) {
            append_diff(&mut history, &mut paths, diff, commit.time());
        }
    }

    history
}

fn append_diff(history: &mut HistoryTree, current_paths: &mut PathSet,
                   diff: Diff, commit_time: Time) {
    for delta in diff.deltas() {
        // TODO: Filter out files we don't care about

        match delta.status() {
            Delta::Modified => {
                let path = new_file_path(&delta);
                if current_paths.contains(path) {
                    append_node(history, new_node(Delta::Modified, commit_time), path);

                    // TODO: Transitions
                }
            }

            Delta::Added => {
                let path = new_file_path(&delta);
                if current_paths.contains(path) {
                    append_node(history, new_node(Delta::Added, commit_time), path);

                    // TODO: Transitions

                    current_paths.remove(path);
                }
            }
            _ => ()
        }
    }
}

fn new_file_path<'a>(df : &'a DiffDelta) -> &'a str {
    df.new_file().path().unwrap().to_str().unwrap()
}

fn old_file_path<'a>(df : &'a DiffDelta) -> &'a str {
    df.old_file().path().unwrap().to_str().unwrap()
}

fn diff_commit<'repo>(commit: &Commit, r: &'repo Repository) -> Option<Diff<'repo>> {
    let current_tree = commit.tree().unwrap();

    let mut diff_to_parents : Option<Diff> = None;

    for parent in commit.parents() {
        let parent_tree = parent.tree().unwrap();
        let diff = r.diff_tree_to_tree(Some(&parent_tree),
                                       Some(&current_tree),
                                       None).unwrap();

        match diff_to_parents {
            // If we don't have a diff yet, make this it.
            None => diff_to_parents = Some(diff),
            // If we do have a diff, merge the current one into it.
            Some(ref mut d) => d.merge(&diff).unwrap()
        };
    }

    if diff_to_parents.is_none() {
        diff_to_parents = Some(r.diff_tree_to_tree(None, Some(&current_tree), None).unwrap());
    }

    let mut diff_to_parents = diff_to_parents.unwrap();

    // Set the options with which the diff should be analyzed
    let mut dfo = DiffFindOptions::new();
    dfo.renames(true)
       .ignore_whitespace(true);

    diff_to_parents.find_similar(Some(&mut dfo)).unwrap();

    Some(diff_to_parents)
}
