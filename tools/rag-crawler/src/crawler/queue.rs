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

/// Extract readable text from HTML. Tags are dropped, whitespace is collapsed,
/// and the bodies of `<script>`/`<style>` blocks are skipped entirely — their
/// contents are code, not content, and would otherwise pollute the RAG corpus.
fn extract_content(html: &str) -> String {
    const MAX_CONTENT_BYTES: usize = 8000;
    let mut out = String::with_capacity(html.len().min(16384));
    let mut pending_space = false;
    // Set while inside a <script>/<style> block; skipped until the matching
    // closing tag.
    let mut skipping: Option<&'static str> = None;
    let mut pos = 0usize;

    while pos < html.len() && out.len() < MAX_CONTENT_BYTES {
        let Some(open_rel) = html[pos..].find('<') else {
            if skipping.is_none() {
                push_text(&mut out, &mut pending_space, &html[pos..], MAX_CONTENT_BYTES);
            }
            break;
        };
        let tag_start = pos + open_rel;
        if skipping.is_none() {
            push_text(
                &mut out,
                &mut pending_space,
                &html[pos..tag_start],
                MAX_CONTENT_BYTES,
            );
        }
        // A tag separates adjacent text nodes, so the next text starts a word.
        pending_space = true;

        let Some(close_rel) = html[tag_start..].find('>') else {
            break;
        };
        let tag_end = tag_start + close_rel;
        let tag = html[tag_start + 1..tag_end].trim();
        let closing = tag.starts_with('/');
        let name: String = tag
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();

        match skipping {
            Some(expected) if closing && name == expected => skipping = None,
            Some(_) => {}
            None if !closing && name == "script" => skipping = Some("script"),
            None if !closing && name == "style" => skipping = Some("style"),
            None => {}
        }
        pos = tag_end + 1;
    }

    let trimmed = out.trim();
    match trimmed.char_indices().nth(MAX_CONTENT_BYTES) {
        Some((idx, _)) => trimmed[..idx].to_string(),
        None => trimmed.to_string(),
    }
}

/// Append `text` to `out`, collapsing runs of whitespace into single spaces.
fn push_text(out: &mut String, pending_space: &mut bool, text: &str, max_bytes: usize) {
    for c in text.chars() {
        if c.is_whitespace() {
            *pending_space = true;
            continue;
        }
        if *pending_space && !out.is_empty() {
            out.push(' ');
        }
        *pending_space = false;
        out.push(c);
        if out.len() >= max_bytes {
            return;
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_skips_script_and_style_bodies() {
        let html = "<html><head><style>body{color:red}</style>\
                    <SCRIPT>var x = 1;</SCRIPT></head>\
                    <body><h1>Halo</h1><p>Jagung manis.</p></body></html>";
        let content = extract_content(html);
        assert!(content.contains("Halo"));
        assert!(content.contains("Jagung manis."));
        assert!(!content.contains("color:red"));
        assert!(!content.contains("var x"));
    }

    #[test]
    fn content_separates_adjacent_text_nodes() {
        let content = extract_content("<p>Padi</p>\n\n<p>  Irigasi  </p>");
        assert_eq!(content, "Padi Irigasi");
    }

    #[test]
    fn title_is_extracted_with_collapsed_whitespace() {
        assert_eq!(
            extract_title("<title>  Beras   Lokal </title>"),
            "Beras Lokal"
        );
        assert_eq!(extract_title("<html><body>no title</body></html>"), "Untitled");
    }
}
