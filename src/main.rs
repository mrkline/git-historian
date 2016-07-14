extern crate time;

// A demo app that gets the --oneline of every commit for a given file.
// Since this does so once per diff per commit, it is hilariously inefficient,
// but very easy to validate by comparing a given file's history to
// `git log --follow --oneline <file>`.

// use std::env;
use std::process::{Command, Stdio};
use std::str;
use std::sync::mpsc::sync_channel;
use std::thread;

mod parsing;
mod history;

use history::*;
use parsing::*;

fn main() {
    // let args: Vec<String> = env::args().collect();
    let (tx, rx) = sync_channel(0);

    thread::spawn(|| parsing::get_history(tx));

    let paths = get_tracked_files();

    let history = gather_history(&paths, &get_id, rx);

    for (key, val) in history {
        println!("{}", key);
        print_history(&val);
    }
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
    println!("\t{}", nb.data);
    if let Some(ref prev) = nb.previous {
        print_history(prev)
    }
}
