use trident::cli;

fn main() {
    if let Err(error) = cli::run() {
        eprintln!("trident: {error}");
        std::process::exit(1);
    }
}
