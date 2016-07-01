extern crate git2;

use git2::*;

use std::env;

mod history;

use history::*;

fn main() {
    let args: Vec<String> = env::args().collect();

    let repo = Repository::open(&args[1]).expect("Couldn't open repo");

    // And a set of files we want to track
    let paths = path_set_from_reference("HEAD", &repo);

    let history = gather_history(paths, &repo);

    for (key, val) in &history {
        println!("{}:", key);
        print_history(&val.head);
    }
}

fn print_history(node: &Link<HistoryNode>) {
    let nb = node.borrow();
    println!("\t{}", nb.when.seconds());
    if let Some(ref prev) = nb.previous {
        print_history(prev)
    }
}
