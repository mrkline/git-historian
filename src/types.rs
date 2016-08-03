///! Types common to the entire library.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt::{self, Display, Formatter};
use std::rc::Rc;

/// A set of paths, used to track which files we care about
pub type PathSet = HashSet<String>;

/// A change to a file in Git (or at least the kinds we care about)
///
/// Copies and renames have additional info:
/// how much of the file remained the same.
#[derive(Debug)]
pub enum Change {
    Added,
    Deleted,
    Modified,
    Renamed{ percent_changed: u8},
    Copied{ percent_changed: u8},
}

/// A change made to a given file in a commit
#[derive(Debug)]
pub struct FileDelta {
    /// The change type
    pub change: Change,

    /// The current path of the file
    pub path: String,

    /// The previous path of the file if the change is a rename or copy,
    /// and an empty string otherwise
    pub from: String,
}

/// A SHA1 hash, used for identifying everything in Git.
#[derive(Copy, Clone, Debug)]
pub struct SHA1 {
    bytes: [u8; 20]
}

impl SHA1 {
    /// Parses a SHA1 from a 40 character hex string
    pub fn parse(s: &str) -> Result<SHA1, &str> {
        if s.len() != 40 { return Err("String is incorrect length") }

        let mut ret = SHA1::default();

        for i in 0..20 {
            let char_index = i * 2;
            ret.bytes[i] = match u8::from_str_radix(&s[char_index .. char_index + 2], 16) {
                    Ok(b) => b,
                    _ => { return Err("Couldn't parse string"); },
                };
        }

        Ok(ret)
    }
}

impl Display for SHA1 {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        for b in &self.bytes {
            match write!(f, "{:02x}", b) {
                Ok (()) => { },
                err => { return err; }
            };
        }
        Ok(())
    }
}

impl Default for SHA1 {
    fn default() -> SHA1 { SHA1{bytes: [0; 20]} }
}

/// Expresses an edge between `HistoryNodes` in a `HistoryTree`
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
