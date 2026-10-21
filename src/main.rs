#[tokio::main]
async fn main() {
    if let Err(error) = weft::cli::run().await {
        eprintln!("weft: {error:#}");
        std::process::exit(1);
    }
}
