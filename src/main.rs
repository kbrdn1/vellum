use clap::Parser;
use vellum::cli;

fn main() {
  let args = cli::Cli::parse();
  if let Err(e) = cli::run(args) {
    eprintln!("error: {}", e);
    std::process::exit(1);
  }
}
