extern crate git2;

use std::cell::RefCell;
use std::collections::HashMap;
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

pub type HistoryTree = HashMap<String, Link<HistoryNode>>;

type PathSet = HashSet<String>;

pub fn start_history(head_index: &Index, head_time: Time) -> HistoryTree {
    let mut ret = HistoryTree::new();

    for file in head_index.iter() {
        let node = Rc::new(RefCell::new(
            HistoryNode{ change: Delta::Unmodified, when: head_time, previous: None }));
        ret.insert(String::from_utf8(file.path).unwrap(), node);
    }

    ret
}

fn append_diff(history: &mut HistoryTree, current_paths: &mut PathSet,
               diff: Diff, commit_time: Time) {
    for delta in diff.deltas() {
        // TODO: Filter out files we don't care about

        match delta.status() {
            Delta::Deleted |
            Delta::Copied |
            Delta::Renamed => {
                let old_name = delta.old_file().path().unwrap().to_str().unwrap();
                history.insert(old_name.to_string(), node);
            }
            _ => ()
        }

        let new_name = delta.new_file().path().unwrap().to_str().unwrap();

        let node = Rc::new(RefCell::new(
            HistoryNode{ change: delta.status(), when: commit_time, previous: None }));

        // If we're not tracking this file, start!
        if !history.contains_key(new_name) {
            history.insert(new_name.to_string(), node);
            return;
        }

        let last = walk_to_end(&history.get(new_name).unwrap());

    }
}

fn walk_to_end(node: &Link<HistoryNode>) -> Link<HistoryNode> {
    let nb = node.borrow();
    match nb.previous {
        Some(ref prev) => walk_to_end(prev),
        None => node.clone()
    }
}
