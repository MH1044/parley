//! The `parley` binary. All behaviour lives in the library so it can be tested in-process.

use std::io::Write;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let code = parley_cli::run(&args, &mut stdout.lock(), &mut stderr.lock());
    let _ = stdout.lock().flush();
    ExitCode::from(code)
}
