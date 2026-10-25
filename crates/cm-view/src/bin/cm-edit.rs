//! `cm-edit <file> [--read-only] [--wrap] [--no-line-numbers] [--mode MIME]
//!         [--script steps.txt]`
//!
//! A native CodeMirror 5 editor window (winit + wgpu). `--script` replays input
//! steps (see `cm_view::app::parse_script`) and exits; use it only under
//! `xvfb-run -a` when testing.

use std::path::PathBuf;

use cm_view::app::AppOptions;

fn main() {
    let mut args = std::env::args().skip(1);
    let mut path: Option<PathBuf> = None;
    let mut opts = AppOptions::default();
    let mut script = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--read-only" => opts.read_only = true,
            "--wrap" => opts.line_wrapping = true,
            "--no-line-numbers" => opts.no_line_numbers = true,
            "--mode" => opts.mode = args.next(),
            "--script" => {
                let p = args.next().unwrap_or_else(|| usage());
                let src =
                    std::fs::read_to_string(&p).unwrap_or_else(|e| fail(&format!("{p}: {e}")));
                script = Some(cm_view::app::parse_script(&src).unwrap_or_else(|e| fail(&e)));
            }
            "-h" | "--help" => usage(),
            _ if path.is_none() && !a.starts_with("--") => path = Some(PathBuf::from(a)),
            _ => usage(),
        }
    }
    let Some(path) = path else { usage() };
    if let Err(e) = cm_view::window::run(path, opts, script) {
        fail(&e);
    }
}

fn usage() -> ! {
    eprintln!(
        "usage: cm-edit <file> [--read-only] [--wrap] [--no-line-numbers] [--mode MIME] [--script steps.txt]"
    );
    std::process::exit(2)
}

fn fail(msg: &str) -> ! {
    eprintln!("cm-edit: {msg}");
    std::process::exit(1)
}
