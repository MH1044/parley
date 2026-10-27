//! `parley-fixtures --out <dir>` writes the generated fixtures and their manifest;
//! `parley-fixtures --check <dir>` verifies a directory against its manifest (docs/TESTING.md §3).

use std::path::Path;
use std::process::ExitCode;

use parley_cli::fixtures;

const HELP: &str = "Usage: parley-fixtures --out <dir> | --check <dir>

  --out <dir>     write every generated fixture and manifest.json into <dir>
  --check <dir>   compare the files in <dir> with its manifest.json; exit 1 on any difference
  --help, -h      print this help
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut iter = args.iter();
    let Some(mode) = iter.next() else {
        print!("{HELP}");
        return ExitCode::from(2);
    };
    match mode.as_str() {
        "--help" | "-h" => {
            print!("{HELP}");
            ExitCode::SUCCESS
        }
        "--out" | "--check" => {
            let Some(dir) = iter.next() else {
                eprintln!("error: {mode} needs a directory. Run parley-fixtures --help.");
                return ExitCode::from(2);
            };
            if let Some(extra) = iter.next() {
                eprintln!("error: unknown argument {extra}. Run parley-fixtures --help.");
                return ExitCode::from(2);
            }
            if mode == "--out" {
                write(Path::new(dir))
            } else {
                check(Path::new(dir))
            }
        }
        other => {
            eprintln!("error: unknown argument {other}. Run parley-fixtures --help.");
            ExitCode::from(2)
        }
    }
}

fn write(dir: &Path) -> ExitCode {
    match fixtures::write(dir) {
        Ok(manifest) => {
            let names: Vec<&String> = manifest.keys().collect();
            println!(
                "{}",
                serde_json::json!({ "files": names, "manifest": fixtures::MANIFEST })
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: could not write the fixtures: {e}");
            ExitCode::from(1)
        }
    }
}

fn check(dir: &Path) -> ExitCode {
    match fixtures::check(dir) {
        Ok(differing) if differing.is_empty() => {
            println!("{}", serde_json::json!({ "ok": true }));
            ExitCode::SUCCESS
        }
        Ok(differing) => {
            for name in differing {
                eprintln!("fixture {name} differs from {}", fixtures::MANIFEST);
            }
            ExitCode::from(1)
        }
        Err(e) => {
            eprintln!("error: could not read {}: {e}", fixtures::MANIFEST);
            ExitCode::from(1)
        }
    }
}
