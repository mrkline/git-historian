extern crate git2;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use std; // Debug, for std::fmt::Debug

use git2::*;


/// Expresses an edge between HistoryNodes in a HistoryTree
pub type Link<T> = Rc<RefCell<T>>;

/// A change in a file through Git history
pub struct HistoryNode<T> {
    pub data: T,

    pub change_type: Delta,

    /// What's the previous change?
    pub previous: Option<Link<HistoryNode<T>>>,
}

pub type HistoryTree<T> = HashMap<String, Link<HistoryNode<T>>>;

pub type PathSet = HashSet<String>;

fn tail_of<T>(head: &Link<HistoryNode<T>>) -> Link<HistoryNode<T>> {
    match head.borrow().previous {
        Some(ref prev) => tail_of(prev),
        None => head.clone()
    }
}

fn tail_if_unterminated<T>(head: &Link<HistoryNode<T>>) -> Option<Link<HistoryNode<T>>> {
    let hb = head.borrow();

    match hb.change_type {
        Delta::Added => return None,
        _ => ()
    };

    match hb.previous {
        Some(ref prev) => tail_if_unterminated(prev),
        None => Some(head.clone())
    }
}

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

    fn new_node(&self, ctype: Delta, d: &Diff, c: &Commit) -> Link<HistoryNode<T>> {
        Rc::new(RefCell::new(HistoryNode{data: (self.visitor)(d, c),
                                         change_type: ctype,
                                         previous: None}))
    }

    fn append_diff(&mut self, diff: Diff, commit: &Commit) {
        for delta in diff.deltas() {

            let new_path = new_file_path(&delta);
            if !self.current_paths.contains(new_path) {
                println!("Ignoring that {} was {:?} (not in set)", new_path, delta.status());
                continue;
            }

            // In all cases where we care about the given path,
            // add a node to its branch.
            let n = self.new_node(delta.status(), &diff, commit);
            self.append_node(n, new_path);

            match delta.status() {
                // If a file was modified, all we need to do is add a node
                // (done above) and link any copies to said node.
                Delta::Modified => {
                    println!("{} was modified.", new_path);
                    self.link_copies(new_path);
                }

                // If a file was added, we need to add a node (done above),
                // link any copies to said node, and remove its path from
                // the set (since previous changes are uninteresting to us).
                Delta::Added => {
                    println!("{} was added.", new_path);
                    self.link_copies(new_path);
                    self.current_paths.remove(new_path);
                }

                // If a file was copied, along with adding a node (done above),
                // we need to leave a note to ourselves to link this path to
                // its progenitor. We do this using "pending_renames",
                // which is used by link_copies (see its implementation).
                Delta::Renamed |
                Delta::Copied => {
                    let old_path = old_file_path(&delta).to_string();
                    println!("{} was renamed/copied to {}.", old_path, new_path);

                    // Link standing renames/copies to the node we created above.
                    self.link_copies(new_path);

                    // Now create a new rename
                    self.pending_renames.entry(old_path.clone()).or_insert(Vec::new())
                        .push(new_path.to_string());

                    // We no longer care about this path,
                    // but do care about the old.
                    self.current_paths.insert(old_path);
                    self.current_paths.remove(new_path);
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
            let tail = tail_of(&self.tree.get(key).unwrap());
            assert!(tail.borrow().previous.is_none());
            tail.borrow_mut().previous = Some(node);
        }
        else {
            // Otherwise we'll just create a new entry.
            self.tree.insert(key.to_string(), node);
        }
    }

    /// If file A is renamed or copied to B, we must link B's history to that of A.
    /// We can't do so until we find another entry for A, so we add a "reminder"
    /// in pending_renames, then do it here next time we see a change to `A`.
    fn link_copies(&mut self, key: &str) {
        let to_link = match self.pending_renames.remove(key) {
                None => return, // Bail if there are no changes to link.
                Some(renames) => renames
            };

        let tail = tail_of(self.tree.get(key).unwrap());

        for l in to_link { // For each rename/copy of <key> to <l>,
            println!("Linking {} to {}", l, key);
            self.append_node(tail.clone(), &l);
        }
    }
}

fn print_history<T>(node: &Link<HistoryNode<T>>)
    where T: std::fmt::Debug {
    let nb = node.borrow();
    println!("\t\t{:?}", nb.data);
    if let Some(ref prev) = nb.previous {
        print_history(prev);
    }
}

pub fn gather_history<T, F>(paths: PathSet, v: F, repo: &Repository) -> HistoryTree<T>
    where F: Fn(&Diff, &Commit) -> T,
          T: std::fmt::Debug /* Debug */ {
    let mut state = HistoryState::new(paths, v);

    // Start walking.
    let mut walk = repo.revwalk().unwrap();
    walk.set_sorting(SORT_TIME | SORT_TOPOLOGICAL);
    walk.push_head().unwrap();

    for id in walk {
        let id = id.unwrap();
        let commit = repo.find_commit(id).unwrap();

        // Debug
        println!("Visiting {:?}", id);
        println!("\tCurrent set: {:?}", state.current_paths);
        println!("\tCurrent map:\r");
        for (key, val) in state.tree.iter() {
            print!("\t{}:\r", key);
            print_history(&val);
        }

        if let Some(diff) = diff_commit(&commit, &repo) {
            state.append_diff(diff, &commit);
        }
        print!("\n");
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
