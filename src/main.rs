use praxis::cli;
use praxis::slog;

fn main() {
    if let Err(error) = cli::run() {
        slog::error(
            "cli_exit",
            slog::context()
                .with_str("outcome", "error")
                .with_str("error_kind", "cli")
                .with_str("error_message", error.to_string()),
        );
        std::process::exit(1);
    }
}
