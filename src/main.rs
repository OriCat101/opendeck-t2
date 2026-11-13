mod bar;
mod demo;
mod layout;
mod plugin;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    if std::env::args().any(|a| a == "--demo") {
        demo::run().await
    } else {
        plugin::run().await
    }
}
