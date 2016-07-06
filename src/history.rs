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
    history: HistoryTree<T>,

    pending_edges: HashMap<String, Vec<Link<HistoryNode<T>>>>,

    visitor: F
}

impl<T, F> HistoryState<T, F> where F: Fn(&Diff, &Commit) -> T {
    fn new(set: &PathSet, vis: F) -> HistoryState<T, F> {
        let mut pending = HashMap::new();
        for path in set {
            pending.insert(path.clone(), Vec::new());
        }
        HistoryState{ history: HistoryTree::new(),
                      pending_edges: pending,
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

            let new_path = new_file_path(&delta).to_string();
            if !self.pending_edges.contains_key(&new_path) {
                println!("Ignoring that {} was {:?} (not in set)", new_path, delta.status());
                continue;
            }

            // In all cases where we care about the given path,
            // add a node to its branch.
            let new_node = self.new_node(delta.status(), &diff, commit);
            self.append_node(&new_path, new_node.clone());

            match delta.status() {
                // If a file was modified, all we need to do is add a node
                // (done above) and link any copies to said node.
                Delta::Modified => {
                    println!("{} was modified.", new_path);
                    self.pending_edges.entry(new_path).or_insert(Vec::new())
                        .push(new_node);
                }

                // If a file was added, we need to add a node (done above),
                // link any copies to said node, and remove its path from
                // the set (since previous changes are uninteresting to us).
                Delta::Added => {
                    println!("{} was added.", new_path);
                }

                // If a file was copied, along with adding a node (done above),
                // we need to leave a note to ourselves to link this path to
                // its progenitor. We do this using "pending_renames",
                // which is used by build_edges (see its implementation).
                Delta::Renamed |
                Delta::Copied => {
                    let old_path = old_file_path(&delta).to_string();
                    println!("{} was renamed/copied to {}.", old_path, new_path);
                    self.pending_edges.entry(old_path).or_insert(Vec::new())
                        .push(new_node);
                }

                _ => ()
            }
        }
    }

    fn append_node(&mut self, key: &str, node: Link<HistoryNode<T>>) {
        println!("Adding node for {}.", key);
        self.build_edges(key, &node);

        // If we don't have a node for this path yet, it's the top of the branch.
        if !self.history.contains_key(key) {
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

fn print_history<T>(node: &Link<HistoryNode<T>>)
    where T: std::fmt::Debug {
    let nb = node.borrow();
    println!("\t\t{:?}", nb.data);
    if let Some(ref prev) = nb.previous {
        print_history(prev);
    }
}

pub fn gather_history<T, F>(paths: &PathSet, v: F, repo: &Repository) -> HistoryTree<T>
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
        println!("\tCurrent map:\r");
        for (key, val) in state.history.iter() {
            print!("\t{}:\r", key);
            print_history(&val);
        }

        if let Some(diff) = diff_commit(&commit, &repo) {
            state.append_diff(diff, &commit);
        }
        print!("\n");
    }

    state.history
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
