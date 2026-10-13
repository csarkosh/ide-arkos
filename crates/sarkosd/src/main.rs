fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version") {
        println!("sarkosd {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    eprintln!("sarkosd: not implemented yet");
    std::process::exit(2);
}
