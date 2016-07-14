Git Historian allows you to collect arbitrary data about a file at each point
in its Git history.

Think of it as `git log --follow` for every file in a repo, all at once.

## Why?

It can be useful for automating tasks that require knowledge of a file's history,
e.g., updating each source file's copyright header with the years during which
the file was modified (because Legal said so).

## How?

The library gathers commit info by parsing the output of `git log --name-status`,
then builds a tree of the history of all files we care about.
See `parsing.rs` and `history.rs` for details.

## Why Rust?

[Because](https://www.youtube.com/watch?v=_-fweBvtifA) [it's awesome](http://www.smbc-comics.com/?id=2088)
(and I wanted to try it out for a Real™ project).
