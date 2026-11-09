//! `parley publik proof` (docs/modules/06-publik-and-providers.md, Interfaces): the maintainer's one
//! real publik API call. Without `PUBLIK_PROOF=1` it writes one line to standard error, nothing to
//! standard output, sends nothing and exits 1. Unlike the other commands, it prints plain lines,
//! not JSON: they are for a person reading the proof.

use std::io::Write;

use super::{missing, unknown};

pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    match args.first().map(String::as_str) {
        Some("proof") => parley_publik::proof::run(&args[1..], out, err),
        Some(other) => unknown(err, other),
        None => missing(err, "publik proof"),
    }
}
