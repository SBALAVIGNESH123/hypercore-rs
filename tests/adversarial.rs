use hypercore_rs::engine::llama::LlamaEngine;
use hypercore_rs::runtime::governor::EngineMetrics;
use hypercore_rs::runtime::RuntimeState;
use hypercore_rs::server::api;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch};

const TEST_MODEL: &str = "qwen2.5-0.5b-instruct-q5_k_m.gguf";

async fn boot_test_server() -> u16 {
    let (request_tx, request_rx) = mpsc::channel(100);
    let (_, state_rx) = watch::channel(RuntimeState::default());
    let (metrics_tx, _) = watch::channel(EngineMetrics::default());

    let engine = LlamaEngine::new(
        TEST_MODEL.to_string(),
        8192,
        4,
        state_rx,
        metrics_tx,
        request_rx,
    );

    tokio::task::spawn_blocking(move || {
        let _ = engine.run_loop();
    });

    let port = 9999; // Simple hardcoded for test
    let tx = request_tx.clone();
    let (_drain_tx, drain_rx) = watch::channel(false);
    tokio::spawn(async move {
        let _ = api::start_server("127.0.0.1", port, tx, drain_rx).await;
    });

    tokio::time::sleep(Duration::from_millis(500)).await;
    port
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_adversarial_suite() {
    let port = boot_test_server().await;

    // 1. Slowloris
    println!("Testing Slowloris...");
    let mut slow_socket = TcpStream::connect(format!("127.0.0.1:{}", port))
        .await
        .unwrap();
    slow_socket
        .write_all(b"POST /v1/chat/completions HTTP/1.1\r\n")
        .await
        .unwrap();
    for _ in 0..5 {
        slow_socket.write_all(b"Host: localhost\r\n").await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    drop(slow_socket);

    // 2. Socket Churn
    println!("Testing Socket Churn...");
    for _ in 0..100 {
        let mut sock = TcpStream::connect(format!("127.0.0.1:{}", port))
            .await
            .unwrap();
        let _ = sock.write_all(b"GET /health HTTP/1.1\r\n\r\n").await;
        // Instantly drop to send RST / FIN
    }

    // 3. Half-Open Client Starvation
    println!("Testing Half-Open Starvation...");
    let mut half_open = TcpStream::connect(format!("127.0.0.1:{}", port))
        .await
        .unwrap();
    let req =
        "POST /v1/chat/completions HTTP/1.1\r\nHost: localhost\r\nContent-Length: 2\r\n\r\n{}";
    half_open.write_all(req.as_bytes()).await.unwrap();

    let mut buf = [0u8; 128];
    // Read headers
    let _ = half_open.read(&mut buf).await.unwrap();

    // Stop reading entirely, but keep socket open!
    // The engine should fill the OS buffer, then fill the bounded channel, and then drop tokens!
    // It must NOT stall.
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Test if engine is still alive by making a normal healthy request
    let mut check_socket = TcpStream::connect(format!("127.0.0.1:{}", port))
        .await
        .unwrap();
    check_socket
        .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();
    let mut check_buf = [0u8; 128];
    let n = check_socket.read(&mut check_buf).await.unwrap();
    assert!(n > 0);
    assert!(String::from_utf8_lossy(&check_buf[..n]).contains("200 OK"));

    println!("All adversarial tests passed!");
}
