fn main() {
    if let Err(error) = amux::cli::run() {
        eprintln!("amux: {error:#}");
        std::process::exit(1);
    }
}
