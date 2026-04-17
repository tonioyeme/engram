//! Intake pipeline for extracting content from URLs and feeding it into the
//! import pipeline.
//!
//! Supports pluggable [`ContentExtractor`] implementations. Ships with
//! [`JinaExtractor`] (Jina Reader API) and [`GenericExtractor`] (plain HTTP
//! fetch). The pipeline produces [`MemoryCandidate`]s that the caller can
//! feed into [`super::import::ImportPipeline`].

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use chrono::{DateTime, Utc};

use super::types::*;

// ═══════════════════════════════════════════════════════════════════════════════
//  CONTENT EXTRACTOR TRAIT
// ═══════════════════════════════════════════════════════════════════════════════

/// Extracts readable content from a URL.
pub trait ContentExtractor: Send + Sync {
    /// Check if this extractor handles the given URL.
    fn can_handle(&self, url: &str) -> bool;

    /// Extract content from the URL. Returns extracted text + metadata.
    fn extract(&self, url: &str) -> Result<ExtractedContent, KcError>;
}

// ═══════════════════════════════════════════════════════════════════════════════
//  EXTRACTED CONTENT
// ═══════════════════════════════════════════════════════════════════════════════

/// Content extracted from a URL source.
#[derive(Debug, Clone)]
pub struct ExtractedContent {
    pub title: String,
    pub author: Option<String>,
    pub content: String,
    pub published: Option<DateTime<Utc>>,
    pub url: String,
    pub platform: String,
}

// ═══════════════════════════════════════════════════════════════════════════════
//  INTAKE REPORT
// ═══════════════════════════════════════════════════════════════════════════════

/// Result of an intake operation.
#[derive(Debug, Clone)]
pub struct IntakeReport {
    pub url: String,
    pub title: String,
    pub memory_candidate: MemoryCandidate,
    pub content_length: usize,
    pub platform: String,
}

// ═══════════════════════════════════════════════════════════════════════════════
//  URL HASHING
// ═══════════════════════════════════════════════════════════════════════════════

/// Compute a hex-encoded hash of a URL using the standard library hasher.
/// Not cryptographic, but sufficient for deduplication by URL.
fn url_hash(url: &str) -> String {
    use std::collections::hash_map::DefaultHasher;

    let mut h1 = DefaultHasher::new();
    url.hash(&mut h1);
    let v1 = h1.finish();

    let mut h2 = DefaultHasher::new();
    "salt".hash(&mut h2);
    url.hash(&mut h2);
    let v2 = h2.finish();

    format!("{:016x}{:016x}", v1, v2)
}

// ═══════════════════════════════════════════════════════════════════════════════
//  HELPERS
// ═══════════════════════════════════════════════════════════════════════════════

/// Extract the domain from a URL string (e.g. `"https://example.com/path"` → `"example.com"`).
fn extract_domain(url: &str) -> String {
    // Strip scheme
    let without_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);

    // Take everything before the first '/'
    let domain = without_scheme
        .split('/')
        .next()
        .unwrap_or(without_scheme);

    // Strip port if present
    domain
        .split(':')
        .next()
        .unwrap_or(domain)
        .to_owned()
}

// ═══════════════════════════════════════════════════════════════════════════════
//  JINA EXTRACTOR
// ═══════════════════════════════════════════════════════════════════════════════

/// Uses the [Jina Reader API](https://r.jina.ai/) to extract readable content
/// from any URL. Acts as a universal fallback extractor.
pub struct JinaExtractor {
    api_key: Option<String>,
}

impl JinaExtractor {
    /// Create a new `JinaExtractor` with an optional API key.
    pub fn new(api_key: Option<String>) -> Self {
        Self { api_key }
    }
}

impl ContentExtractor for JinaExtractor {
    fn can_handle(&self, _url: &str) -> bool {
        true // handles everything as fallback
    }

    fn extract(&self, url: &str) -> Result<ExtractedContent, KcError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| KcError::ImportError(format!("HTTP client error: {}", e)))?;

        let jina_url = format!("https://r.jina.ai/{}", url);
        let mut req = client.get(&jina_url);
        if let Some(key) = &self.api_key {
            req = req.header("Authorization", format!("Bearer {}", key));
        }
        req = req.header("Accept", "text/plain");

        let resp = req
            .send()
            .map_err(|e| KcError::ImportError(format!("Jina request failed: {}", e)))?;

        if !resp.status().is_success() {
            return Err(KcError::ImportError(format!(
                "Jina returned status {}",
                resp.status()
            )));
        }

        let text = resp
            .text()
            .map_err(|e| KcError::ImportError(format!("Failed to read Jina response: {}", e)))?;

        // Parse: first line starting with # is the title, rest is content
        let (title, content) = parse_title_and_content(&text);
        let platform = extract_domain(url);

        Ok(ExtractedContent {
            title,
            author: None,
            content,
            published: None,
            url: url.to_owned(),
            platform,
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
//  GENERIC EXTRACTOR
// ═══════════════════════════════════════════════════════════════════════════════

/// Simple HTTP fetch + text extraction for when Jina is not available.
/// Performs a plain GET request and attempts to extract meaningful text.
pub struct GenericExtractor;

impl ContentExtractor for GenericExtractor {
    fn can_handle(&self, _url: &str) -> bool {
        true
    }

    fn extract(&self, url: &str) -> Result<ExtractedContent, KcError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| KcError::ImportError(format!("HTTP client error: {}", e)))?;

        let resp = client
            .get(url)
            .header(
                "User-Agent",
                "engram-ai/1.0 (knowledge-compiler intake)",
            )
            .send()
            .map_err(|e| KcError::ImportError(format!("HTTP request failed: {}", e)))?;

        if !resp.status().is_success() {
            return Err(KcError::ImportError(format!(
                "HTTP {} for {}",
                resp.status(),
                url
            )));
        }

        let body = resp
            .text()
            .map_err(|e| KcError::ImportError(format!("Failed to read response: {}", e)))?;

        // Attempt to extract a title from <title> tag
        let title = extract_html_title(&body)
            .unwrap_or_else(|| extract_domain(url));

        // Strip HTML tags for a rough text extraction
        let content = strip_html_tags(&body);
        let platform = extract_domain(url);

        Ok(ExtractedContent {
            title,
            author: None,
            content,
            published: None,
            url: url.to_owned(),
            platform,
        })
    }
}

/// Extract text content from the `<title>` tag in HTML.
fn extract_html_title(html: &str) -> Option<String> {
    let lower = html.to_lowercase();
    let start = lower.find("<title>")?;
    let after = start + 7;
    let end = lower[after..].find("</title>")?;
    let title = html[after..after + end].trim().to_owned();
    if title.is_empty() {
        None
    } else {
        Some(title)
    }
}

/// Crude HTML tag stripper — removes everything between `<` and `>`.
fn strip_html_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;

    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }

    // Collapse excessive whitespace
    let mut cleaned = String::with_capacity(result.len());
    let mut prev_blank = false;
    for line in result.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !prev_blank {
                cleaned.push('\n');
                prev_blank = true;
            }
        } else {
            cleaned.push_str(trimmed);
            cleaned.push('\n');
            prev_blank = false;
        }
    }

    cleaned.trim().to_owned()
}

/// Parse title and content from extracted text.
/// If the first line starts with `#`, treat it as the title.
fn parse_title_and_content(text: &str) -> (String, String) {
    let trimmed = text.trim();
    if let Some(first_newline) = trimmed.find('\n') {
        let first_line = trimmed[..first_newline].trim();
        if first_line.starts_with('#') {
            let title = first_line.trim_start_matches('#').trim().to_owned();
            let content = trimmed[first_newline..].trim().to_owned();
            if title.is_empty() {
                ("Untitled".to_owned(), trimmed.to_owned())
            } else {
                (title, content)
            }
        } else {
            // Use first line as title, rest as content
            (
                first_line.to_owned(),
                trimmed[first_newline..].trim().to_owned(),
            )
        }
    } else {
        // Single line — use as both title and content
        (trimmed.to_owned(), trimmed.to_owned())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
//  INTAKE PIPELINE
// ═══════════════════════════════════════════════════════════════════════════════

/// Orchestrates URL content extraction and import.
///
/// The pipeline holds a list of [`ContentExtractor`]s and tries them in order.
/// The first extractor that reports [`ContentExtractor::can_handle`] for a URL
/// is used. The resulting [`IntakeReport`] contains a [`MemoryCandidate`] ready
/// for the caller to feed into [`super::import::ImportPipeline`].
pub struct IntakePipeline {
    extractors: Vec<Box<dyn ContentExtractor>>,
}

impl IntakePipeline {
    /// Create a new, empty `IntakePipeline` with no extractors.
    pub fn new() -> Self {
        Self {
            extractors: Vec::new(),
        }
    }

    /// Add a content extractor to the pipeline.
    pub fn add_extractor(&mut self, extractor: Box<dyn ContentExtractor>) {
        self.extractors.push(extractor);
    }

    /// Number of registered extractors.
    pub fn extractor_count(&self) -> usize {
        self.extractors.len()
    }

    /// Ingest a URL: extract content → create [`MemoryCandidate`] → return for import.
    ///
    /// Does **not** directly write to storage — the caller decides what to do
    /// with the candidate (e.g. feed it into [`super::import::ImportPipeline`]).
    pub fn ingest(&self, url: &str) -> Result<IntakeReport, KcError> {
        // Find the first extractor that can handle this URL
        let extractor = self
            .extractors
            .iter()
            .find(|e| e.can_handle(url))
            .ok_or_else(|| {
                KcError::ImportError(format!(
                    "No extractor can handle URL: {}",
                    url
                ))
            })?;

        let content = extractor.extract(url)?;
        let content_length = content.content.len();

        let candidate = MemoryCandidate {
            content: format!(
                "# {}\n\nSource: {}\nAuthor: {}\n\n{}",
                content.title,
                content.url,
                content.author.as_deref().unwrap_or("unknown"),
                content.content,
            ),
            source: url.to_owned(),
            content_hash: url_hash(&content.url),
            metadata: HashMap::from([
                ("source_url".to_owned(), content.url.clone()),
                ("platform".to_owned(), content.platform.clone()),
                (
                    "intake_timestamp".to_owned(),
                    Utc::now().to_rfc3339(),
                ),
            ]),
        };

        Ok(IntakeReport {
            url: url.to_owned(),
            title: content.title,
            memory_candidate: candidate,
            content_length,
            platform: content.platform,
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
//  TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── Mock Extractor ───────────────────────────────────────────────────

    /// Test-only extractor that returns pre-configured results.
    struct MockExtractor {
        handles: bool,
        title: String,
        content: String,
        author: Option<String>,
        platform: String,
        fail: bool,
    }

    impl MockExtractor {
        fn new(handles: bool, title: &str, content: &str) -> Self {
            Self {
                handles,
                title: title.to_owned(),
                content: content.to_owned(),
                author: None,
                platform: "mock".to_owned(),
                fail: false,
            }
        }

        fn failing(handles: bool) -> Self {
            Self {
                handles,
                title: String::new(),
                content: String::new(),
                author: None,
                platform: "mock".to_owned(),
                fail: true,
            }
        }

        fn with_author(mut self, author: &str) -> Self {
            self.author = Some(author.to_owned());
            self
        }

        fn with_platform(mut self, platform: &str) -> Self {
            self.platform = platform.to_owned();
            self
        }
    }

    impl ContentExtractor for MockExtractor {
        fn can_handle(&self, _url: &str) -> bool {
            self.handles
        }

        fn extract(&self, url: &str) -> Result<ExtractedContent, KcError> {
            if self.fail {
                return Err(KcError::ImportError("mock extraction failed".to_owned()));
            }
            Ok(ExtractedContent {
                title: self.title.clone(),
                author: self.author.clone(),
                content: self.content.clone(),
                published: None,
                url: url.to_owned(),
                platform: self.platform.clone(),
            })
        }
    }

    // ── Tests ────────────────────────────────────────────────────────────

    #[test]
    fn test_intake_pipeline_new() {
        let pipeline = IntakePipeline::new();
        assert_eq!(pipeline.extractor_count(), 0);
        assert!(pipeline.extractors.is_empty());
    }

    #[test]
    fn test_add_extractor() {
        let mut pipeline = IntakePipeline::new();
        assert_eq!(pipeline.extractor_count(), 0);

        pipeline.add_extractor(Box::new(MockExtractor::new(true, "T1", "C1")));
        assert_eq!(pipeline.extractor_count(), 1);

        pipeline.add_extractor(Box::new(MockExtractor::new(false, "T2", "C2")));
        assert_eq!(pipeline.extractor_count(), 2);

        pipeline.add_extractor(Box::new(MockExtractor::new(true, "T3", "C3")));
        assert_eq!(pipeline.extractor_count(), 3);
    }

    #[test]
    fn test_jina_can_handle() {
        let extractor = JinaExtractor::new(None);
        assert!(extractor.can_handle("https://example.com"));
        assert!(extractor.can_handle("https://github.com/user/repo"));
        assert!(extractor.can_handle("https://www.youtube.com/watch?v=abc123"));
        assert!(extractor.can_handle("http://anything.goes/here"));
        assert!(extractor.can_handle("not-even-a-url"));
    }

    #[test]
    fn test_generic_can_handle() {
        let extractor = GenericExtractor;
        assert!(extractor.can_handle("https://example.com"));
        assert!(extractor.can_handle("https://github.com/user/repo"));
        assert!(extractor.can_handle("https://www.youtube.com/watch?v=abc123"));
        assert!(extractor.can_handle("http://anything.goes/here"));
        assert!(extractor.can_handle("not-even-a-url"));
    }

    #[test]
    fn test_ingest_selects_first_matching() {
        let mut pipeline = IntakePipeline::new();

        // First extractor doesn't handle, second does, third does too
        pipeline.add_extractor(Box::new(MockExtractor::new(false, "Skip", "skip")));
        pipeline.add_extractor(Box::new(
            MockExtractor::new(true, "Second", "second content")
                .with_platform("second-platform"),
        ));
        pipeline.add_extractor(Box::new(
            MockExtractor::new(true, "Third", "third content")
                .with_platform("third-platform"),
        ));

        let report = pipeline.ingest("https://example.com/article").unwrap();

        // Should have used the second extractor (first matching)
        assert_eq!(report.title, "Second");
        assert_eq!(report.platform, "second-platform");
        assert!(report.memory_candidate.content.contains("second content"));
    }

    #[test]
    fn test_ingest_no_extractor() {
        // Empty pipeline — no extractors at all
        let pipeline = IntakePipeline::new();
        let result = pipeline.ingest("https://example.com");
        assert!(result.is_err());

        let err = result.unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("No extractor"), "Error was: {}", msg);
    }

    #[test]
    fn test_ingest_no_matching_extractor() {
        // Pipeline with extractors, but none can handle the URL
        let mut pipeline = IntakePipeline::new();
        pipeline.add_extractor(Box::new(MockExtractor::new(false, "A", "a")));
        pipeline.add_extractor(Box::new(MockExtractor::new(false, "B", "b")));

        let result = pipeline.ingest("https://example.com");
        assert!(result.is_err());
    }

    #[test]
    fn test_extracted_content_to_candidate() {
        let mut pipeline = IntakePipeline::new();
        pipeline.add_extractor(Box::new(
            MockExtractor::new(true, "Rust Guide", "Learn Rust programming.")
                .with_author("Alice")
                .with_platform("blog.example.com"),
        ));

        let report = pipeline.ingest("https://blog.example.com/rust-guide").unwrap();
        let candidate = &report.memory_candidate;

        // Content format: # Title\n\nSource: url\nAuthor: author\n\ncontent
        assert!(candidate.content.starts_with("# Rust Guide"));
        assert!(candidate.content.contains("Source: https://blog.example.com/rust-guide"));
        assert!(candidate.content.contains("Author: Alice"));
        assert!(candidate.content.contains("Learn Rust programming."));

        // Source is the URL
        assert_eq!(candidate.source, "https://blog.example.com/rust-guide");

        // Metadata
        assert_eq!(
            candidate.metadata.get("source_url").unwrap(),
            "https://blog.example.com/rust-guide"
        );
        assert_eq!(
            candidate.metadata.get("platform").unwrap(),
            "blog.example.com"
        );
        assert!(candidate.metadata.contains_key("intake_timestamp"));

        // Content hash is derived from URL
        assert!(!candidate.content_hash.is_empty());
        assert_eq!(candidate.content_hash.len(), 32); // 16 hex digits × 2
    }

    #[test]
    fn test_extracted_content_unknown_author() {
        let mut pipeline = IntakePipeline::new();
        pipeline.add_extractor(Box::new(
            MockExtractor::new(true, "Title", "Body"),
        ));

        let report = pipeline.ingest("https://example.com/page").unwrap();
        // When no author is set, should show "unknown"
        assert!(report.memory_candidate.content.contains("Author: unknown"));
    }

    #[test]
    fn test_intake_report_fields() {
        let mut pipeline = IntakePipeline::new();
        pipeline.add_extractor(Box::new(
            MockExtractor::new(true, "My Article", "Some body text here")
                .with_platform("example.com"),
        ));

        let report = pipeline.ingest("https://example.com/my-article").unwrap();

        assert_eq!(report.url, "https://example.com/my-article");
        assert_eq!(report.title, "My Article");
        assert_eq!(report.content_length, "Some body text here".len());
        assert_eq!(report.platform, "example.com");
    }

    #[test]
    fn test_content_hash_dedup() {
        // Same URL must produce the same hash (for dedup)
        let hash1 = url_hash("https://example.com/article");
        let hash2 = url_hash("https://example.com/article");
        assert_eq!(hash1, hash2);

        // Different URLs should produce different hashes
        let hash3 = url_hash("https://example.com/other-article");
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_url_hash_deterministic() {
        let hash = url_hash("https://example.com/page");
        // Hash is 32 hex chars (two u64 values, each 16 hex digits)
        assert_eq!(hash.len(), 32);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_extract_domain() {
        assert_eq!(extract_domain("https://example.com/path"), "example.com");
        assert_eq!(extract_domain("http://sub.example.com/a/b"), "sub.example.com");
        assert_eq!(extract_domain("https://example.com:8080/path"), "example.com");
        assert_eq!(extract_domain("https://example.com"), "example.com");
        assert_eq!(extract_domain("no-scheme.com/path"), "no-scheme.com");
    }

    #[test]
    fn test_parse_title_and_content() {
        // Heading line
        let (title, content) = parse_title_and_content("# My Title\n\nBody text here.");
        assert_eq!(title, "My Title");
        assert_eq!(content, "Body text here.");

        // No heading — first line becomes title
        let (title, content) = parse_title_and_content("First Line\nSecond line.");
        assert_eq!(title, "First Line");
        assert_eq!(content, "Second line.");

        // Single line
        let (title, content) = parse_title_and_content("Only line");
        assert_eq!(title, "Only line");
        assert_eq!(content, "Only line");
    }

    #[test]
    fn test_extract_html_title() {
        let html = "<html><head><title>Page Title</title></head><body>Hi</body></html>";
        assert_eq!(extract_html_title(html), Some("Page Title".to_owned()));

        let no_title = "<html><body>Hi</body></html>";
        assert_eq!(extract_html_title(no_title), None);

        let empty_title = "<html><title></title></html>";
        assert_eq!(extract_html_title(empty_title), None);
    }

    #[test]
    fn test_strip_html_tags() {
        let html = "<p>Hello <b>world</b></p><br/><p>Second paragraph</p>";
        let text = strip_html_tags(html);
        assert!(text.contains("Hello world"));
        assert!(text.contains("Second paragraph"));
        assert!(!text.contains('<'));
        assert!(!text.contains('>'));
    }

    #[test]
    fn test_ingest_extractor_failure() {
        let mut pipeline = IntakePipeline::new();
        pipeline.add_extractor(Box::new(MockExtractor::failing(true)));

        let result = pipeline.ingest("https://example.com");
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("mock extraction failed"));
    }

    #[test]
    fn test_candidate_hash_uses_url() {
        // Two ingestions of the same URL should produce the same content_hash
        let mut pipeline = IntakePipeline::new();
        pipeline.add_extractor(Box::new(MockExtractor::new(true, "T", "C")));

        let r1 = pipeline.ingest("https://example.com/same").unwrap();
        let r2 = pipeline.ingest("https://example.com/same").unwrap();
        assert_eq!(r1.memory_candidate.content_hash, r2.memory_candidate.content_hash);

        // Different URL should produce different hash
        let r3 = pipeline.ingest("https://example.com/different").unwrap();
        assert_ne!(r1.memory_candidate.content_hash, r3.memory_candidate.content_hash);
    }
}
