extern crate git2;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use git2::*;

/// Expresses an edge between HistoryNodes in a HistoryTree
pub type Link<T> = Rc<RefCell<T>>;

/// A change in a file through Git history
pub struct HistoryNode<T> {
    pub data: T,

    /// What's the previous change?
    pub previous: Option<Link<HistoryNode<T>>>,
}

/// The history of a file can be tracked as a tree.
pub struct PathEntry<T> {
    pub head: Link<HistoryNode<T>>,
    tail: Link<HistoryNode<T>>,
}

pub type HistoryTree<T> = HashMap<String, PathEntry<T>>;

pub type PathSet = HashSet<String>;

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

// All the fun state we need to hang onto while building up our history tree.
struct HistoryState<T, F: Fn(&Diff, &Commit) -> T> {
    tree: HistoryTree<T>,

    // Renames aren't changes per se (at least of the file contents),
    // so when we see file A renamed to B, we'll just add it to this list.
    // Then, next time we see a change in A, we'll link the back of B to A.
    pending_renames: HashMap<String, Vec<String>>,

    current_paths: PathSet,

    visitor: F
}

impl<T, F> HistoryState<T, F> where F: Fn(&Diff, &Commit) -> T {
    fn new(set: PathSet, vis: F) -> HistoryState<T, F> {
        HistoryState{ tree: HistoryTree::new(),
                      pending_renames: HashMap::new(),
                      current_paths: set,
                      visitor: vis,
                    }
    }

    fn new_node(&self, d: &Diff, c: &Commit) -> Link<HistoryNode<T>> {
        Rc::new(RefCell::new(HistoryNode{data: (self.visitor)(d, c), previous: None}))
    }

    fn append_diff(&mut self, diff: Diff, commit: &Commit) {
        for delta in diff.deltas() {

            match delta.status() {
                Delta::Modified => {
                    let path = new_file_path(&delta);
                    println!("{} was modified.", path);
                    if self.current_paths.contains(path) {
                        let n = self.new_node(&diff, commit);
                        self.append_node(n, path);
                        self.link_renames(path);
                    }
                }

                Delta::Added => {
                    let path = new_file_path(&delta);
                    println!("{} was added.", path);
                    if self.current_paths.contains(path) {
                        let n = self.new_node(&diff, commit);
                        self.append_node(n, path);
                        self.link_renames(path);
                        self.current_paths.remove(path);
                    }
                }

                Delta::Renamed |
                Delta::Copied => {
                    let new_path = new_file_path(&delta);
                    let old_path = old_file_path(&delta).to_string();
                    println!("{} was renamed/copied to {}.", old_path, new_path);
                    if self.current_paths.contains(new_path) {
                        self.pending_renames.entry(old_path.clone()).or_insert(Vec::new())
                            .push(new_path.to_string());

                        self.current_paths.insert(old_path);
                        self.current_paths.remove(new_path);
                    }
                }

                _ => ()
            }
        }
    }

    fn append_node(&mut self, node: Link<HistoryNode<T>>, key: &str) {
        println!("Appending to {}.", key);
        if self.tree.contains_key(key) {
            // If we already have an entry for the given path,
            // append the new node to the tail.
            let entry = self.tree.get_mut(key).unwrap();
            assert!(entry.tail.borrow().previous.is_none());

            let new_tail = node.clone();
            entry.tail.borrow_mut().previous = Some(node);
            entry.tail = new_tail;
        }
        else {
            // Otherwise we'll just create a new entry.
            let t = node.clone(); // Bump the refcount

            // Head and tail will be the same since the branch has one node.
            self.tree.insert(key.to_string(), PathEntry{ head: node, tail: t});
        }
    }

    /// If file A is renamed or copied to B, we must link B's history to that of A.
    /// We can't do so immediately, since a rename isn't a content change,
    /// so we add an entry to do so in pending_renames, then do it here next time
    /// we see a change to `A`.
    fn link_renames(&mut self, key: &str) {
        let to_link = match self.pending_renames.remove(key) {
                None => return, // Bail if there are no changes to link.
                Some(renames) => renames
            };

        for l in to_link { // For each rename/copy of <key> to <l>,
            println!("Linking {} to {}", l, key);
            // Append the tail of <l> to <key>'s history.
            let link_to = self.tree.get(key).unwrap().tail.clone();
            self.append_node(link_to, &l);
        }
    }
}

pub fn gather_history<T, F>(paths: PathSet, v: F, repo: &Repository) -> HistoryTree<T>
    where F: Fn(&Diff, &Commit) -> T  {
    let mut state = HistoryState::new(paths, v);

    // Start walking.
    let mut walk = repo.revwalk().unwrap();
    walk.set_sorting(SORT_TIME | SORT_TOPOLOGICAL);
    walk.push_head().unwrap();

    for id in walk {
        let id = id.unwrap();
        let commit = repo.find_commit(id).unwrap();
        println!("Visiting {:?}", id);
        println!("\tCurrent set: {:?}", state.current_paths);
        println!("\tCurrent keys: {:?}", state.tree.keys().collect::<Vec<&String>>());
        if let Some(diff) = diff_commit(&commit, &repo) {
            state.append_diff(diff, &commit);
        }
    }

    state.tree
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
