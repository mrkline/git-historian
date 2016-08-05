extern crate time;

// A demo app that gets the --oneline of every commit for a given file.
// Since this does so once per diff per commit, it is hilariously inefficient,
// but very easy to validate by comparing a given file's history to
// `git log --follow --oneline <file>`.

// use std::env;
use std::io::{BufReader, BufRead};
use std::process::{Command, Stdio};
use std::str;
use std::sync::mpsc::sync_channel;
use std::thread;

mod parsing;
mod history;
mod types;

use history::*;
use parsing::*;
use types::*;

fn main() {
    // let args: Vec<String> = env::args().collect();
    let (tx, rx) = sync_channel(0);

    thread::spawn(|| parsing::get_history(tx));

    let paths = get_tracked_files();

    let history = gather_history(&paths, &get_id, |_| true, rx);

    for (key, val) in history {
        println!("{}", key);
        print_history(&val);
    }
}

/// *Warning:* This currently assumes the working directory is the top-level Git
/// directory. This should (and will) be fixed ASAP.
fn get_tracked_files() -> PathSet {
    let mut ret = PathSet::new();

    // TODO: Make sure we're in the top level dir (change to it?)
    let child = Command::new("git")
        .arg("ls-files")
        .stdout(Stdio::piped())
        .spawn().unwrap();

    let br = BufReader::new(child.stdout.unwrap());

    for file in br.lines().map(|l| l.unwrap()) {
        ret.insert(file);
    }

    ret
}

fn get_id(c: &ParsedCommit) -> String {

    str::from_utf8(&Command::new("git")
        .arg("log")
        .arg("--oneline")
        .arg("--no-walk")
        .arg(c.id.to_string())
        .stdout(Stdio::piped())
        .output().unwrap()
        .stdout).unwrap().trim().to_string()
}

fn print_history<T>(node: &Link<HistoryNode<T>>)
    where T: std::fmt::Display {
    let nb = node.borrow();
    if let Some(ref data) = nb.data {
        println!("\t{}", data);
    }
    if let Some(ref prev) = nb.previous {
        print_history(prev)
    }
}
