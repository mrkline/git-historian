extern crate git2;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use git2::*;

/// Expresses an edge between HistoryNodes in a HistoryTree
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

/// The history of a file can be tracked as a tree.
pub struct PathEntry {
    pub head: Link<HistoryNode>,
    tail: Link<HistoryNode>,
}

fn new_node(c: Delta, w: Time) -> Link<HistoryNode> {
    Rc::new(RefCell::new(
        HistoryNode{ change: c, when: w, previous: None }))
}

pub type HistoryTree = HashMap<String, PathEntry>;

pub type PathSet = HashSet<String>;

type PendingRenames = HashMap<String, Vec<String>>;

pub fn path_set_from_index(i: &Index) -> PathSet {
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

    // Renames aren't changes per se (at least of the file contents),
    // so when we see file A renamed to B, we'll just add it to this list.
    // Then, next time we see a change in A, we'll link the back of B to A.
    let mut pending_renames = PendingRenames::new();

    // Start walking.
    let mut walk = repo.revwalk().unwrap();
    walk.set_sorting(SORT_TIME | SORT_TOPOLOGICAL);
    walk.push_head().unwrap();

    for id in walk {
        let id = id.unwrap();
        let commit = repo.find_commit(id).unwrap();
        println!("Visiting {:?}", id);
        println!("\tCurrent set: {:?}", paths);
        println!("\tCurrent keys: {:?}", history.keys().collect::<Vec<&String>>());
        if let Some(diff) = diff_commit(&commit, &repo) {
            append_diff(&mut history, &mut paths, &mut pending_renames,
                        diff, commit.time());
        }
    }

    history
}

fn append_diff(history: &mut HistoryTree, current_paths: &mut PathSet,
               pending_renames: &mut PendingRenames,
               diff: Diff, commit_time: Time) {
    for delta in diff.deltas() {

        match delta.status() {
            Delta::Modified => {
                let path = new_file_path(&delta);
                println!("{} was modified.", path);
                if current_paths.contains(path) {
                    append_node(history, new_node(Delta::Modified, commit_time), path);
                    link_renames(history, pending_renames, path);
                }
            }

            Delta::Added => {
                let path = new_file_path(&delta);
                println!("{} was added.", path);
                if current_paths.contains(path) {
                    append_node(history, new_node(Delta::Added, commit_time), path);
                    link_renames(history, pending_renames, path);
                    current_paths.remove(path);
                }
            }

            Delta::Renamed |
            Delta::Copied => {
                let new_path = new_file_path(&delta);
                if current_paths.contains(new_path) {
                    let old_path = old_file_path(&delta).to_string();
                    println!("{} was renamed/copied to {}.", old_path, new_path);
                    pending_renames.entry(old_path.clone()).or_insert(Vec::new())
                        .push(new_path.to_string());

                    current_paths.insert(old_path);
                    current_paths.remove(new_path);
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

fn append_node(tree: &mut HistoryTree, node: Link<HistoryNode>, key: &str) {
    println!("Appending to {}.", key);
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

/// If file A is renamed or copied to B, we must link B's history to that of A.
/// We can't do so immediately, since a rename isn't a content change,
/// so we add an entry to do so in pending_renames, then do it here next time
/// we see a change to `A`.
fn link_renames(mut tree: &mut HistoryTree, pending_renames: &mut PendingRenames,
                key: &str) {
    {
        let to_link = match pending_renames.get(key) {
                None => return, // Bail if there are no changes to link.
                Some(renames) => renames
            };

        for l in to_link { // For each rename/copy of <key> to <l>,
            println!("Linking {} to {}", l, key);
            // Append the tail of <l> to <key>'s history.
            let link_from = tree.get(l).unwrap().tail.clone();
            append_node(&mut tree, link_from, key);
        }
    } // Expire borrow of pending_renames

    // Remove the renames we just did.
    pending_renames.remove(key).unwrap();
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
        diff_to_parents = Some(r.diff_tree_to_tree(None,
                                                   Some(&current_tree),
                                                   None).unwrap());
    }

    let mut diff_to_parents = diff_to_parents.unwrap();

    // Set the options with which the diff should be analyzed
    let mut dfo = DiffFindOptions::new();
    dfo.renames(true)
       .ignore_whitespace(true);

    diff_to_parents.find_similar(Some(&mut dfo)).unwrap();

    Some(diff_to_parents)
}
