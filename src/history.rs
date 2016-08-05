//! Builds a tree of Git history for a given set of paths, issuing a callback
//! to gather information at each point in a file's history
//!
//! The basic algorithm is as follows: given a set of paths we care about and a
//! series of commits, do the following for each file change in each commit:
//!
//! 1. Call the user-provided callback to get desired information about this
//!    change. The callback can use the data provided by `ParsedCommit`, or it
//!    can gather its own info using the commit's SHA1 ID and git commands.
//!    (The latter is, of course, much slower.)
//!
//! 2. Create a new node representing our change, then connect it to previous
//!    nodes using the "pending edges" map (see step 3).
//!
//! 3. In a map of "pending edges", place an entry indicating what the file's
//!    name was before this change. If the change was a modification,
//!    the previous name is the same as the current one.
//!    If the change was a move or a copy, the previous name will be different.
//!    If the change was the addition of the file, there is no previous name
//!    to add.
//!
//! The net effect is that files' histories are tracked *through* name changes,
//! a la `git log --follow`.
//! Currently the act of renaming a file is considered a change, even though
//! the actual contents haven't changed at all.
//! (This seems to be consistent with `git log --follow`).
//! If, in the future, this is not desired, we *do* track the amount a file has
//! been changed during a rename, and could skip adding a node if no changes are
//! made to the contents.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::rc::Rc;

use types::{Change, HistoryNode, HistoryTree, Link, PathSet};
use parsing::ParsedCommit;


/// All the fun state we need to hang onto while building up our history tree
struct HistoryState<'a, T, V, F>
    where V: Fn(&ParsedCommit) -> T, F: Fn(&ParsedCommit) -> bool {
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
    visitor: V,

    filter: F,
}

impl<'a, T, V, F> HistoryState<'a, T, V, F>
    where V: Fn(&ParsedCommit) -> T, F: Fn(&ParsedCommit) -> bool {
    fn new(set: &'a PathSet, vis: V, fil: F) -> HistoryState<T, V, F> {
        let mut pending = HashMap::new();

        // Due to the check at the start of append_commit(), we must insert
        // entries into pending_edges so that we care about the first diff found
        // for a given file.
        for path in set {
            pending.insert(path.clone(), Vec::new());
        }

        HistoryState{ history: HistoryTree::new(),
                      pending_edges: pending,
                      path_set: set,
                      visitor: vis,
                      filter: fil
                    }
    }

    /// Uses the user's callback to generate a new node
    fn new_node(&self, c: &ParsedCommit) -> Link<HistoryNode<T>> {
        let d = if (self.filter)(c) { Some((self.visitor)(c)) }
            else { None };

        Rc::new(RefCell::new(HistoryNode{data: d, previous: None}))
    }

    /// Takes a given commit and appends its changes to the history tree
    fn append_commit(&mut self, commit: &ParsedCommit) {
        for delta in &commit.deltas {

            // If we have no edges leading to the next node for this path,
            // skip to the next diff.
            if !self.pending_edges.contains_key(&delta.path) {
                continue;
            }

            // In all cases where we care about the given path,
            // create a new node for it and link its pending_edges to it.
            let new_node = self.new_node(commit);
            self.append_node(&delta.path, new_node.clone());

            match delta.change {
                // If a file was modified, its next node is under the same path.
                Change::Modified => {
                    self.pending_edges.entry(delta.path.clone())
                        .or_insert_with(Vec::new)
                        .push(new_node);
                }

                // If a file was added, it has no next node (that we care about).
                // We also don't care about deletions. If a file is deleted,
                // it didn't make it - at least in that form - to the present.
                Change::Added |
                Change::Deleted => { }

                // If a file was moved or copied,
                // its next node is under the old path.
                Change::Copied{..} | // TODO: Use % changed
                Change::Renamed{..} => {
                    self.pending_edges.entry(delta.from.clone())
                        .or_insert_with(Vec::new)
                        .push(new_node);
                }
            }
        }
    }

    /// Uses `pending_edges` (via `build_edges()`) to link `node` into
    /// the history tree.
    fn append_node(&mut self, key: &str, node: Link<HistoryNode<T>>) {
        self.build_edges(key, &node);

        // If we don't have a node for this path yet, it's the top of the branch.
        if !self.history.contains_key(key) && self.path_set.contains(key) {
            self.history.insert(key.to_string(), node);
        }
    }

    /// Connects older nodes to `link_to` based on `pending_edges`
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

/// Traverses Git history, grabbing arbitrary data at each change for files
/// in the given set
///
/// Changes are tracked *through* file copies and renames.
/// See the module-level documentation for more info.
pub fn gather_history<T, V, F>(paths: &PathSet, v: V, f: F,
                               commit_source: Receiver<ParsedCommit>) -> HistoryTree<T>
    where V: Fn(&ParsedCommit) -> T, F: Fn(&ParsedCommit) -> bool {
    let mut state = HistoryState::new(paths, v, f);

    // Start reading commits.

    while let Ok(commit) = commit_source.recv() {
        state.append_commit(&commit);
    }

    // We should have consumed all edges by now.
    // ...but git log --name-status doesn't show the full path of subtree'd files.
    // TODO: Make the system play well with git subtree.
    /*
    if !state.pending_edges.is_empty() {
        println!("Still have edges for");
        for key in state.pending_edges.keys() {
            println!("\t{}", key);
        }
    }
    */

    state.history
}
