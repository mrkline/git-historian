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

pub fn start_history(head_index: &Index, head_time: Time) -> HistoryTree {
    let mut ret = HistoryTree::new();

    for file in head_index.iter() {
        let node = HistoryNode{ change: Delta::Unmodified, when: head_time, previous: None };
        ret.insert(String::from_utf8(file.path).unwrap(), Rc::new(RefCell::new(node)));
    }

    ret
}

pub fn append_diff(history: &mut HistoryTree, diff: Diff, commit_time: Time) {
    for delta in diff.deltas() {
        let new_name = delta.new_file().path().unwrap().to_str().unwrap();

        // If we're not tracking this file, we don't care. Move along.
        if !history.contains_key(new_name) {
            continue;
        }

        let last = walk_to_end(&history.get(new_name).unwrap());

        // TODO: If it's a rename, we need to add the old name to the map.

        let node = HistoryNode{ change: delta.status(), when: commit_time, previous: None };
    }
}

fn walk_to_end(node: &Link<HistoryNode>) -> &Link<HistoryNode> {
    match node.previous {
        Some(prev) => walk_to_end(&prev),
        None => node
    }
}
