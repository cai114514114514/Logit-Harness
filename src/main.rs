#[tokio::main]
async fn main() {
    if let Err(error) = loh::cli::run().await {
        eprintln!("loh: {error:#}");
        std::process::exit(1);
    }
}
