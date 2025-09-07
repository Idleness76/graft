use anyhow::Result;
use graft::run_demo1;

#[tokio::main]
async fn main() -> Result<()> {
    run_demo1::run_demo1().await
}
