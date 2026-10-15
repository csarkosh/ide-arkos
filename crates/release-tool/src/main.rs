//! Signs release manifests (auto-update spec, section 3). `keygen` prints a secret key (for the
//! GitHub Actions secret and the password manager) and a public key (for `release_key.rs`);
//! `sign` reads the secret from an environment variable so it never sits on a command line;
//! `verify` is the by-hand check of a published manifest.
mod sig;

use std::process::exit;

fn usage() -> ! {
    eprintln!("usage: release-tool keygen | sign --key-env NAME FILE | verify --public B64 FILE SIGFILE");
    exit(2)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    match a.as_slice() {
        ["keygen"] => {
            let (sk, pk) = sig::keygen();
            println!("secret: {sk}");
            println!("public: {pk}");
        }
        ["sign", "--key-env", name, file] => {
            let sk = std::env::var(name).unwrap_or_else(|_| {
                eprintln!("release-tool: {name} is not set");
                exit(1)
            });
            let msg = std::fs::read(file).unwrap_or_else(|e| {
                eprintln!("release-tool: {file}: {e}");
                exit(1)
            });
            match sig::sign(&sk, &msg) {
                Ok(s) => println!("{s}"),
                Err(e) => {
                    eprintln!("release-tool: {e}");
                    exit(1)
                }
            }
        }
        ["verify", "--public", pk, file, sigfile] => {
            let msg = std::fs::read(file).unwrap_or_else(|e| {
                eprintln!("release-tool: {file}: {e}");
                exit(1)
            });
            let s = std::fs::read_to_string(sigfile).unwrap_or_else(|e| {
                eprintln!("release-tool: {sigfile}: {e}");
                exit(1)
            });
            match sig::verify(pk, &msg, &s) {
                Ok(()) => println!("ok"),
                Err(e) => {
                    eprintln!("release-tool: {e}");
                    exit(1)
                }
            }
        }
        _ => usage(),
    }
}
