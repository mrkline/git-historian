extern crate git2;

use git2::*;

use std::env;

mod history;

use history::*;

fn main() {
    let args: Vec<String> = env::args().collect();

    let repo = Repository::open(&args[1]).expect("Couldn't open repo");

    // We'll need some history
    let mut history = HistoryTree::new();

    // Start walking.
    let mut walk = repo.revwalk().unwrap();
    walk.set_sorting(SORT_TIME | SORT_TOPOLOGICAL);
    walk.push_head().unwrap();

    for id in walk {
        let id = id.unwrap();
        let commit = repo.find_commit(id).unwrap();
        if let Some(diff) = diff_commit(&commit, &repo) {
            append_diff(&mut history, diff, commit.time());
        }
    }

    for (key, val) in &history {
        println!("{}:", key);
        print_history(val);
    }
}

fn print_history(node: &Link<HistoryNode>) {
    let nb = node.borrow();
    println!("\t{}", nb.when.seconds());
    if let Some(ref prev) = nb.previous {
        print_history(prev)
    }
}

fn diff_commit<'repo>(commit: &Commit, r: &'repo Repository) -> Option<Diff<'repo>> {
    let current_tree = commit.tree().unwrap();

    let mut diff_to_parents : Option<Diff> = None;

    for parent in commit.parents() {
        let parent_tree = parent.tree().unwrap();
        let diff = r.diff_tree_to_tree(Some(&parent_tree),
                                       Some(&current_tree),
                                       None).unwrap();

        match diff_to_parents {
            // If we don't have a diff yet, make this it.
            None => diff_to_parents = Some(diff),
            // If we do have a diff, merge the current one into it.
            Some(ref mut d) => d.merge(&diff).unwrap()
        };
    }

    if diff_to_parents.is_none() { return None; } // No parents?

    let mut diff_to_parents = diff_to_parents.unwrap();

    // Set the options with which the diff should be analyzed
    let mut dfo = DiffFindOptions::new();
    dfo.renames(true)
       .ignore_whitespace(true);

    diff_to_parents.find_similar(Some(&mut dfo)).unwrap();

    Some(diff_to_parents)
}
