//! Parses `git log --name-status` to find file changes through Git history
//!
//! Originally the [Rust bindings](https://crates.io/crates/git2) for
//! [libgit2](https://libgit2.github.com/) was used,
//! but there is currently no clean way for libgit to generate diffs for merges
//! (i.e. only the changes resulting from conflict resolution) as Git does.

use std::fmt::{self, Debug, Display, Formatter};
use std::io::{self, BufReader, BufRead};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::SyncSender;

use time::Timespec;

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
#[derive(Copy, Clone)]
pub struct SHA1 {
    bytes: [u8; 20]
}

impl SHA1 {
    /// Parses a SHA1 from a 40 character hex string
    fn parse(s: &str) -> Result<SHA1, &str> {
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

// TODO: According to docs, Display should already implement Debug. What gives?
impl Debug for SHA1 {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        Display::fmt(self, f)
    }
}


impl Default for SHA1 {
    fn default() -> SHA1 { SHA1{bytes: [0; 20]} }
}

/// Info about a commit pulled from `git log` (or at least the bits we care about)
#[derive(Debug)]
pub struct ParsedCommit {
    pub id: SHA1,
    /// The Unix timestamp (in seconds) of the commit
    pub when: Timespec,
    pub deltas: Vec<FileDelta>,
}

impl Default for ParsedCommit {
    fn default() -> ParsedCommit
    {
        ParsedCommit {
            id: SHA1::default(),
            when: Timespec::new(0, 0),
            deltas: Vec::new()
        }
    }
}

/// Starts the `git log` process with the desired config
fn start_history_process() -> Result<Child, io::Error> {
    let child = try!(Command::new("git")
        .arg("log")
        .arg("--name-status")
        .arg("-M")
        .arg("-C")
        .arg("--pretty=format:%H%n%at") // Commit hash, newline, unix time
        .stdout(Stdio::piped())
        .spawn());

    Ok(child)
}

/// Parses the Git history and emits a series of ParsedCommits
///
/// The parsed commits are pushed to a SyncSender,
/// and are assumed to be consumed by another thread.
pub fn get_history(sink: SyncSender<ParsedCommit>) {

    enum ParseState { // Used for the state machine below
        Hash,
        Timestamp,
        Changes
    }

    let child = start_history_process().expect("Couldn't open repo history");
    let br = BufReader::new(child.stdout.unwrap());

    let mut state = ParseState::Hash;
    let mut current_commit = ParsedCommit::default();

    for line in br.lines().map(|l| l.unwrap()) {

        if line.is_empty() { continue; } // Blow through empty lines

        let next_state;
        match state {
            ParseState::Hash => {
                current_commit.id = SHA1::parse(&line).unwrap();
                next_state = ParseState::Timestamp;
            }

            ParseState::Timestamp => {
                current_commit.when = Timespec{ sec: line.parse().unwrap(),
                                                nsec: 0 };
                next_state = ParseState::Changes;
            }

            ParseState::Changes => {
                // If we get the next hash, we're done with the previous commit.
                if let Ok(id) = SHA1::parse(&line) {
                    commit_sink(current_commit, &sink);
                    current_commit = ParsedCommit::default();

                    // We just got the OID of the next guy,
                    // so proceed to reading the timestamp
                    current_commit.id = id;
                    next_state = ParseState::Timestamp;
                }
                else {
                    // Keep chomping deltas
                    next_state = state;

                    current_commit.deltas.push(parse_delta(&line));
                }
            }
        }
        state = next_state;
    }

    // Grab the last commit.
    commit_sink(current_commit, &sink);
}

/// The function that eats a commit when the state machine is done parsing it.
#[inline]
fn commit_sink(c: ParsedCommit, sink: &SyncSender<ParsedCommit>) {
    sink.send(c).expect("The other end stopped listening for commits.");
}

/// Parses a delta line generated by `git log --name-status`
fn parse_delta(s: &str) -> FileDelta {
    let tokens : Vec<&str> = s.split('\t').collect();

    assert!(tokens.len() > 1, "Expected at least one token");
    let c = parse_change_code(tokens[0]);
    let previous : String;
    let current : String;

    match c {
        Change::Renamed { .. } |
        Change::Copied { .. }=> {
            assert!(tokens.len() == 3, "Expected three tokens from string {:?}", s);
            current = tokens[2].to_string();
            previous = tokens[1].to_string();
        }

        _ => {
            assert!(tokens.len() == 2, "Expected two tokens from string {:?}", s);
            current = tokens[1].to_string();
            previous = String::new();
        }
    };

    FileDelta{ change: c, path: current, from: previous }
}

/// Parses the change code generated by `git log --name-status`
fn parse_change_code(c: &str) -> Change {
    assert!(!c.is_empty());
    let ret = match c.chars().nth(0).unwrap() {
        'A' => Change::Added,
        'D' => Change::Deleted,
        'M' => Change::Modified,
        'T' => Change::Modified, // Let's consider a type change a modification.
        // Renames and copies are suffixed with a percent changed, e.g. R87
        'R' => Change::Renamed{ percent_changed: c[1..].parse().unwrap() },
        'C' => Change::Copied{ percent_changed: c[1..].parse().unwrap() },
        _ => panic!("Unknown delta code: {}", c)
    };

    // Sanity check percent_changed values for renames and copies
    match ret {
        Change::Renamed{ percent_changed: r}  => { assert!(r <= 100); },
        Change::Copied{ percent_changed: c} => { assert!(c <= 100); },
        _ => { }
    };

    ret
}
