//! Synthetic test image generator binary.
//!
//! Usage: `photoforge-fixtures [--out DIR] [--card]`
//! Writes the standard fixture set into DIR (default `testdata/`).

fn main() {
    let mut out = std::path::PathBuf::from("testdata");
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--out" => {
                if let Some(d) = args.next() {
                    out = std::path::PathBuf::from(d);
                }
            }
            other => {
                eprintln!("unknown argument: {other}");
                eprintln!("usage: photoforge-fixtures [--out DIR]");
                std::process::exit(2);
            }
        }
    }
    match photoforge_fixtures::generate_all(&out) {
        Ok(paths) => {
            println!("wrote {} fixtures to {}", paths.len(), out.display());
            for p in &paths {
                println!("  {}", p.display());
            }
        }
        Err(e) => {
            eprintln!("failed: {e}");
            std::process::exit(1);
        }
    }
}
