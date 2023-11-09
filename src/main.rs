use std::time::Duration;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let state = ASHA::ASHA::get_adapter_state().await;

    let mut test = ASHA::ASHA::new().await;

    test.start_scan(1).await;
    // tokio::time::sleep(Duration::from_secs(10000)).await;
}