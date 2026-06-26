use clap::Parser;
use vellum::cli;

#[tokio::main]
async fn main() {
  let args = cli::Cli::parse();
  if let Err(e) = cli::run(args).await {
    eprintln!("error: {e}");
    std::process::exit(1);
  }
}
