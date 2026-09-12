//! The `boringbib` binary. All logic lives in the library; see `boringbib::cli`.

fn main() -> std::process::ExitCode {
    boringbib::cli::run()
}
