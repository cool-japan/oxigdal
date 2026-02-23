//! OxiGDAL Server Template

use axum::{routing::get, Router};
use std::net::SocketAddr;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let app = Router::new()
        .route("/", get(|| async { "OxiGDAL Server" }))
        .route("/health", get(|| async { "OK" }));

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("Server running on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
