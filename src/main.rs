#[tokio::main]
async fn main() -> std::io::Result<()> {
    uptime_monitor::run_app().await
}
