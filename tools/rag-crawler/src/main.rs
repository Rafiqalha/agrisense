mod crawler;
mod storage;

use crawler::{CrawlerPool, Job, ParserKind};
use std::sync::Arc;
use std::time::Duration;
use storage::StorageManager;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

fn worker_count_from_env() -> usize {
    match std::env::var("CRAWLER_WORKERS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
    {
        Some(n) if n > 0 && n <= 128 => n,
        _ => 16,
    }
}

/// Output path for the RAG pool, relative to the current working directory
/// (run from the repo root). Override with `RAG_POOL_PATH`.
fn dataset_path() -> String {
    match std::env::var("RAG_POOL_PATH") {
        Ok(v) if !v.trim().is_empty() => v,
        _ => "dataset/agrisense_rag_pool.jsonl".to_string(),
    }
}

fn seed_jobs() -> Vec<Job> {
    let cli: Vec<String> = std::env::args().skip(1).collect();
    if !cli.is_empty() {
        return cli
            .into_iter()
            .map(|url| Job {
                url,
                category: ParserKind::Technical,
                publish_date: "2024-01-01".to_string(),
            })
            .collect();
    }
    vec![
        Job {
            url: "https://id.wikipedia.org/wiki/Melon".to_string(),
            category: ParserKind::Journal,
            publish_date: "2024-01-01".to_string(),
        },
        Job {
            url: "https://id.wikipedia.org/wiki/Padi".to_string(),
            category: ParserKind::Technical,
            publish_date: "2024-02-15".to_string(),
        },
        Job {
            url: "https://example.com/".to_string(),
            category: ParserKind::News,
            publish_date: "2024-03-10".to_string(),
        },
    ]
}

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    let dataset_path = dataset_path();
    let storage = Arc::new(StorageManager::new(&dataset_path).await?);
    let client = Arc::new(
        reqwest::Client::builder()
            .user_agent("AgrisenseRAG/1.0")
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(10))
            .pool_max_idle_per_host(20)
            .tcp_keepalive(Duration::from_secs(60))
            .build()?,
    );

    let workers = worker_count_from_env();
    let pool = CrawlerPool::new(workers, 256, Arc::clone(&client), Arc::clone(&storage));
    let tx = pool.sender();

    for job in seed_jobs() {
        match tx.send(job).await {
            Ok(()) => {}
            Err(e) => return Err(format!("enqueue failed: {e}").into()),
        }
    }
    drop(tx);

    match pool.shutdown().await {
        Ok(()) => {}
        Err(e) => return Err(e),
    }
    storage.flush().await?;
    println!("done, output: {dataset_path}");
    Ok(())
}
