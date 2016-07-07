extern crate git2;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use git2::*;

/// Expresses an edge between HistoryNodes in a HistoryTree
pub type Link<T> = Rc<RefCell<T>>;

/// A change in a file through Git history
pub struct HistoryNode<T> {
    /// A callback is issued for each delta, allowing the user to store
    /// whatever info they want about the change.
    pub data: T,

    /// What's the previous change?
    pub previous: Option<Link<HistoryNode<T>>>,
}

/// For each key in the map, the value is a branch of a tree
/// (i.e. a linked list) of all changes.
/// This extends past name changes
pub type HistoryTree<T> = HashMap<String, Link<HistoryNode<T>>>;

/// A set of paths, used to track which files we care about
pub type PathSet = HashSet<String>;

/// Given a Git index, returns a set of all paths in the index
pub fn path_set_from_index(i: &Index) -> PathSet {
    let mut ret = PathSet::new();

    for file in i.iter() {
        ret.insert(String::from_utf8(file.path).unwrap());
    }

    ret
}

/// Given a git ref name, returns a set of all files in that ref
pub fn path_set_from_reference(name: &str, repo: &Repository) -> PathSet {
    let ref_id = repo.refname_to_id(name).unwrap();
    let ref_commit = repo.find_commit(ref_id).unwrap();

    let mut idx = Index::new().unwrap();
    idx.read_tree(&ref_commit.tree().unwrap()).unwrap();

    path_set_from_index(&idx)
}

/// All the fun state we need to hang onto while building up our history tree.
struct HistoryState<'a, T, F: Fn(&Diff, &Commit) -> T> {
    /// The tree we'll return
    history: HistoryTree<T>,

    /// History is generated by noting (from the diff type) what a file's
    /// name was during its previous change.
    /// Those edges (between HistoryNodes) are stored here, where
    /// pending_edges[p] lists all nodes that should be connected to the next
    /// node for path `p`.
    pending_edges: HashMap<String, Vec<Link<HistoryNode<T>>>>,

    /// Hold a reference to which paths we care about, for culling output.
    path_set: &'a PathSet,

    /// The user-provided callback that's issued for each diff,
    /// returning info the user cares about.
    visitor: F,
}

impl<'a, T, F> HistoryState<'a, T, F> where F: Fn(&Diff, &Commit) -> T {
    fn new(set: &'a PathSet, vis: F) -> HistoryState<T, F> {
        let mut pending = HashMap::new();

        // Due to the check at the start of append_diff(), we must insert
        // entries into pending_edges so that we care about the first diff found
        // for a given file.
        for path in set {
            pending.insert(path.clone(), Vec::new());
        }

        HistoryState{ history: HistoryTree::new(),
                      pending_edges: pending,
                      path_set: set,
                      visitor: vis,
                    }
    }

    /// Uses the user's callback to generate a new node
    fn new_node(&self, d: &Diff, c: &Commit) -> Link<HistoryNode<T>> {
        Rc::new(RefCell::new(HistoryNode{data: (self.visitor)(d, c),
                                         previous: None}))
    }

    fn append_diff(&mut self, diff: Diff, commit: &Commit) {
        for delta in diff.deltas() {

            let new_path = new_file_path(&delta).to_string();

            // If we have no edges leading to the next node for this path,
            // skip to the next diff.
            if !self.pending_edges.contains_key(&new_path) {
                continue;
            }

            // In all cases where we care about the given path,
            // create a new node for it and link its pending_edges to it.
            let new_node = self.new_node(&diff, commit);
            self.append_node(&new_path, new_node.clone());

            match delta.status() {
                // If a file was modified, its next node is under the same path.
                Delta::Unmodified | // Probably unneeded... right?
                Delta::Typechange |
                Delta::Modified => {
                    self.pending_edges.entry(new_path).or_insert(Vec::new())
                        .push(new_node);
                }

                // If a file was added, it has no next node (that we care about).
                Delta::Added => { }

                // We don't care about deletions. If a file is deleted,
                // it didn't make it - at least in that form - to the present.
                Delta::Deleted => { }

                // If a file was moved or copied,
                // its next node is under the old path.
                Delta::Copied |
                Delta::Renamed => {
                    let old_path = old_file_path(&delta).to_string();
                    self.pending_edges.entry(old_path).or_insert(Vec::new())
                        .push(new_node);
                }

                _ => { println!("Wat: {:?}", delta.status()); }
            }
        }
    }

    fn append_node(&mut self, key: &str, node: Link<HistoryNode<T>>) {
        self.build_edges(key, &node);

        // If we don't have a node for this path yet, it's the top of the branch.
        if !self.history.contains_key(key) && self.path_set.contains(key) {
            self.history.insert(key.to_string(), node);
        }
    }

    fn build_edges(&mut self, for_path: &str, link_to: &Link<HistoryNode<T>>) {
        let from_set = match self.pending_edges.remove(for_path) {
                None => return, // Bail if there are no changes to link.
                Some(to_link) => to_link
            };

        for l in from_set { // For each rename/copy of <key> to <l>,
            assert!(l.borrow().previous.is_none());
            l.borrow_mut().previous = Some(link_to.clone());
        }
    }
}

/// The whole shebang. Build up the needed state and walk the Git tree.
pub fn gather_history<T, F>(paths: &PathSet, v: F, repo: &Repository) -> HistoryTree<T>
    where F: Fn(&Diff, &Commit) -> T {
    let mut state = HistoryState::new(paths, v);

    // Start walking.
    let mut walk = repo.revwalk().unwrap();
    walk.set_sorting(SORT_TIME);
    walk.push_head().unwrap();

    for id in walk {
        let id = id.unwrap();
        let commit = repo.find_commit(id).unwrap();

        if let Some(diff) = diff_commit(&commit, &repo) {
            state.append_diff(diff, &commit);
        }
    }

    // We should have consumed all edges by now.
    assert!(state.pending_edges.is_empty());

    state.history
}

/// Unwraps the "new" file path from a delta
fn new_file_path<'a>(df : &'a DiffDelta) -> &'a str {
    df.new_file().path().unwrap().to_str().unwrap()
}

/// Unwraps the "old" file path from a delta
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
