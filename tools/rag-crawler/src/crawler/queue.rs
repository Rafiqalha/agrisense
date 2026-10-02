use crate::storage::{Metadata, RagRecord, StorageManager};
use rand::Rng;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};

pub type CrawlResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParserKind {
    Journal,
    News,
    Technical,
}

impl ParserKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Journal => "journal",
            Self::News => "news",
            Self::Technical => "technical",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Job {
    pub url: String,
    pub category: ParserKind,
    pub publish_date: String,
}

fn md5_hex(input: &str) -> String {
    format!("{:x}", md5::compute(input.as_bytes()))
}

fn extract_title(html: &str) -> String {
    match html.find("<title>") {
        Some(start) => match html[start..].find("</title>") {
            Some(end) => {
                let raw = &html[start + 7..start + end];
                let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
                let trimmed = collapsed.trim();
                if trimmed.is_empty() {
                    String::from("Untitled")
                } else if trimmed.len() > 500 {
                    match trimmed.char_indices().nth(500) {
                        Some((idx, _)) => trimmed[..idx].to_string(),
                        None => trimmed.to_string(),
                    }
                } else {
                    trimmed.to_string()
                }
            }
            None => String::from("Untitled"),
        },
        None => String::from("Untitled"),
    }
}

fn extract_content(html: &str) -> String {
    let mut out = String::with_capacity(html.len().min(16384));
    let mut in_tag = false;
    let mut pending_space = false;
    for c in html.chars() {
        match c {
            '<' => {
                in_tag = true;
                pending_space = true;
            }
            '>' => {
                in_tag = false;
            }
            _ if in_tag => {}
            _ if c.is_whitespace() => {
                pending_space = true;
            }
            _ => {
                if pending_space && !out.is_empty() {
                    out.push(' ');
                }
                pending_space = false;
                out.push(c);
                if out.len() >= 8000 {
                    break;
                }
            }
        }
    }
    let trimmed = out.trim();
    match trimmed.char_indices().nth(8000) {
        Some((idx, _)) => trimmed[..idx].to_string(),
        None => trimmed.to_string(),
    }
}

fn build_text_context(title: &str, category: &str, content: &str) -> String {
    let mut buf = String::with_capacity(title.len() + category.len() + content.len() + 32);
    buf.push_str("Title: ");
    buf.push_str(title);
    buf.push_str("\nCategory: ");
    buf.push_str(category);
    buf.push_str("\nContent: ");
    buf.push_str(content);
    buf
}

async fn fetch_html_with_retry(client: &reqwest::Client, url: &str) -> CrawlResult<String> {
    const MAX_RETRIES: u32 = 5;
    const BASE_MS: u64 = 200;
    const MAX_BACKOFF_MS: u64 = 8_000;
    let mut attempt: u32 = 0;
    loop {
        match client.get(url).send().await {
            Ok(resp) => {
                let status = resp.status();
                if status.as_u16() == 429 {
                    if attempt >= MAX_RETRIES {
                        return Err(format!("429 retry budget exhausted: {url}").into());
                    }
                    let jitter: u64 = rand::thread_rng().gen_range(0..150);
                    let backoff =
                        (BASE_MS.saturating_mul(1u64 << attempt)).min(MAX_BACKOFF_MS) + jitter;
                    tokio::time::sleep(Duration::from_millis(backoff)).await;
                    attempt += 1;
                    continue;
                }
                if status.is_server_error() {
                    if attempt >= MAX_RETRIES {
                        return Err(format!("server error {status} for {url}").into());
                    }
                    let jitter: u64 = rand::thread_rng().gen_range(0..100);
                    let backoff =
                        (BASE_MS.saturating_mul(1u64 << attempt)).min(MAX_BACKOFF_MS) + jitter;
                    tokio::time::sleep(Duration::from_millis(backoff)).await;
                    attempt += 1;
                    continue;
                }
                if !status.is_success() {
                    return Err(format!("non-retryable http {status} for {url}").into());
                }
                match resp.text().await {
                    Ok(body) => return Ok(body),
                    Err(e) => {
                        if attempt >= MAX_RETRIES {
                            return Err(Box::new(e));
                        }
                        let jitter: u64 = rand::thread_rng().gen_range(0..100);
                        let backoff =
                            (BASE_MS.saturating_mul(1u64 << attempt)).min(MAX_BACKOFF_MS)
                                + jitter;
                        tokio::time::sleep(Duration::from_millis(backoff)).await;
                        attempt += 1;
                    }
                }
            }
            Err(e) => {
                if attempt >= MAX_RETRIES {
                    return Err(Box::new(e));
                }
                let jitter: u64 = rand::thread_rng().gen_range(0..100);
                let backoff =
                    (BASE_MS.saturating_mul(1u64 << attempt)).min(MAX_BACKOFF_MS) + jitter;
                tokio::time::sleep(Duration::from_millis(backoff)).await;
                attempt += 1;
            }
        }
    }
}

async fn process_job(
    job: Job,
    client: Arc<reqwest::Client>,
    storage: Arc<StorageManager>,
) -> CrawlResult<()> {
    let html = fetch_html_with_retry(&client, &job.url).await?;
    let title = extract_title(&html);
    let content = extract_content(&html);
    let category = job.category.as_str();
    let text_context = build_text_context(&title, category, &content);
    let record = RagRecord {
        doc_id: md5_hex(&job.url),
        text_context,
        metadata: Metadata {
            title,
            source_url: job.url,
            category: category.to_string(),
            publish_date: job.publish_date,
        },
    };
    storage.append(&record).await?;
    Ok(())
}

pub struct CrawlerPool {
    sender: mpsc::Sender<Job>,
    handles: Vec<tokio::task::JoinHandle<()>>,
}

impl CrawlerPool {
    pub fn new(
        worker_count: usize,
        channel_capacity: usize,
        client: Arc<reqwest::Client>,
        storage: Arc<StorageManager>,
    ) -> Self {
        let (tx, rx) = mpsc::channel::<Job>(channel_capacity);
        let shared_rx = Arc::new(Mutex::new(rx));
        let mut handles = Vec::with_capacity(worker_count);
        for _ in 0..worker_count {
            let rx = Arc::clone(&shared_rx);
            let client = Arc::clone(&client);
            let storage = Arc::clone(&storage);
            let handle = tokio::spawn(async move {
                loop {
                    let next = {
                        let mut guard = rx.lock().await;
                        guard.recv().await
                    };
                    match next {
                        Some(job) => {
                            match process_job(job, Arc::clone(&client), Arc::clone(&storage))
                                .await
                            {
                                Ok(()) => {}
                                Err(e) => {
                                    eprintln!("crawler job failed: {e}");
                                }
                            }
                        }
                        None => break,
                    }
                }
            });
            handles.push(handle);
        }
        Self {
            sender: tx,
            handles,
        }
    }

    pub fn sender(&self) -> mpsc::Sender<Job> {
        self.sender.clone()
    }

    pub async fn shutdown(self) -> CrawlResult<()> {
        drop(self.sender);
        for h in self.handles {
            match h.await {
                Ok(()) => {}
                Err(e) => return Err(format!("worker join failed: {e}").into()),
            }
        }
        Ok(())
    }
}
