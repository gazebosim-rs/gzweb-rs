//! `gz-parity` binary; see the library docs.

fn main() -> std::process::ExitCode {
    gz_parity::cli::main_with(std::env::args().skip(1))
}
