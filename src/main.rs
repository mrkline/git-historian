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

    let history = gather_history(paths.clone(), &get_oid, &repo);

    for (key, val) in history.iter()
        .filter(|&(k, _)| paths.contains(k)) {
        println!("{}:", key);
        print_history(&val);
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
