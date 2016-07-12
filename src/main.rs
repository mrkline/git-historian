extern crate git2;
extern crate time;

use std::env;
use std::sync::mpsc::sync_channel;
use std::thread;

mod git;
mod history;

use git2::*;
use history::*;

fn main() {
    // let args: Vec<String> = env::args().collect();
    let (tx, rx) = sync_channel(0);

    thread::spawn(|| git::get_history(tx));

    while let Ok(commit) = rx.recv() {
        println!("{:?}", commit);
    }
}

fn get_oid(_: &Diff, c: &Commit) -> Oid {
    c.id()
}

fn print_history<T>(node: &Link<HistoryNode<T>>)
    where T: std::fmt::Debug {
    let nb = node.borrow();
    println!("\t{:?}", nb.data);
    if let Some(ref prev) = nb.previous {
        print_history(prev)
    }
}
