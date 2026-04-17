//! Embedding pipeline with model management, caching, and provider abstraction.
//!
//! Supports multiple embedding backends:
//! - `HttpEmbeddingProvider` — OpenAI-compatible API (also works with Ollama, llama.cpp, etc.)
//! - `StubEmbeddingProvider` — deterministic pseudo-embeddings for testing / no-embedding mode
//!
//! The `EmbeddingManager` orchestrates provider selection from config and provides
//! a convenient single/batch embedding interface.

use super::types::{KcEmbeddingConfig, KcError};

// ─── Provider Trait ──────────────────────────────────────────────────────────

/// Provider-agnostic embedding interface.
pub trait EmbeddingProvider: Send + Sync {
    /// Generate embeddings for a batch of texts.
    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, KcError>;

    /// Model name (for cache tagging).
    fn model_name(&self) -> &str;

    /// Embedding dimensions.
    fn dimensions(&self) -> usize;
}

// ═══════════════════════════════════════════════════════════════════════════════
//  HTTP PROVIDER (OpenAI-compatible)
// ═══════════════════════════════════════════════════════════════════════════════

/// Embedding provider that calls an OpenAI-compatible HTTP endpoint.
///
/// Works with OpenAI, Azure OpenAI, Ollama, llama.cpp server, and any other
/// service that implements the `/v1/embeddings` JSON contract.
pub struct HttpEmbeddingProvider {
    client: reqwest::blocking::Client,
    endpoint: String,
    api_key: String,
    model: String,
    dims: usize,
}

impl HttpEmbeddingProvider {
    /// Create a provider for the OpenAI API.
    ///
    /// Reads `OPENAI_API_KEY` from the environment.
    pub fn openai(model: &str, dimensions: usize) -> Result<Self, KcError> {
        let api_key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| KcError::InvalidConfig("OPENAI_API_KEY not set".into()))?;
        Ok(Self {
            client: reqwest::blocking::Client::new(),
            endpoint: "https://api.openai.com/v1/embeddings".into(),
            api_key,
            model: model.into(),
            dims: dimensions,
        })
    }

    /// Create a provider for a local server (Ollama, llama.cpp, etc.).
    pub fn local(endpoint: &str, model: &str, dimensions: usize) -> Self {
        Self {
            client: reqwest::blocking::Client::new(),
            endpoint: endpoint.into(),
            api_key: String::new(),
            model: model.into(),
            dims: dimensions,
        }
    }
}

impl EmbeddingProvider for HttpEmbeddingProvider {
    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, KcError> {
        let body = serde_json::json!({
            "input": texts,
            "model": self.model,
        });

        let mut req = self
            .client
            .post(&self.endpoint)
            .header("Content-Type", "application/json");

        if !self.api_key.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", self.api_key));
        }

        let resp = req
            .json(&body)
            .send()
            .map_err(|e| KcError::Storage(format!("Embedding API error: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            return Err(KcError::Storage(format!(
                "Embedding API {}: {}",
                status, text
            )));
        }

        let json: serde_json::Value = resp
            .json()
            .map_err(|e| KcError::Storage(format!("Parse error: {}", e)))?;

        let data = json["data"]
            .as_array()
            .ok_or_else(|| KcError::Storage("No data in response".into()))?;

        let mut embeddings = Vec::with_capacity(data.len());
        for item in data {
            let emb = item["embedding"]
                .as_array()
                .ok_or_else(|| KcError::Storage("No embedding in item".into()))?;
            let vec: Vec<f32> = emb
                .iter()
                .filter_map(|v| v.as_f64().map(|f| f as f32))
                .collect();
            embeddings.push(vec);
        }

        Ok(embeddings)
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn dimensions(&self) -> usize {
        self.dims
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
//  STUB PROVIDER (testing / no-embedding mode)
// ═══════════════════════════════════════════════════════════════════════════════

/// Deterministic pseudo-embedding provider for testing and no-embedding mode.
///
/// Generates normalized vectors derived from a simple byte-hash of the input
/// text, ensuring identical inputs always produce identical outputs.
pub struct StubEmbeddingProvider {
    model: String,
    dims: usize,
}

impl StubEmbeddingProvider {
    /// Create a stub provider with the given dimensionality.
    pub fn new(dims: usize) -> Self {
        Self {
            model: "stub".into(),
            dims,
        }
    }
}

impl EmbeddingProvider for StubEmbeddingProvider {
    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, KcError> {
        Ok(texts
            .iter()
            .map(|text| {
                let mut vec = vec![0.0f32; self.dims];
                // Simple hash-based fill for deterministic test output
                for (i, byte) in text.bytes().enumerate() {
                    vec[i % self.dims] += (byte as f32 - 128.0) / 128.0;
                }
                // Normalize to unit vector
                let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
                if norm > 0.0 {
                    for x in &mut vec {
                        *x /= norm;
                    }
                }
                vec
            })
            .collect())
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn dimensions(&self) -> usize {
        self.dims
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
//  EMBEDDING MANAGER
// ═══════════════════════════════════════════════════════════════════════════════

/// Orchestrates embedding provider selection and provides a convenient interface
/// for single/batch embedding generation.
pub struct EmbeddingManager {
    provider: Box<dyn EmbeddingProvider>,
}

impl EmbeddingManager {
    /// Create a manager wrapping an explicit provider.
    pub fn new(provider: Box<dyn EmbeddingProvider>) -> Self {
        Self { provider }
    }

    /// Create from config — picks the right provider automatically.
    ///
    /// Provider selection:
    /// - `"openai"` → `HttpEmbeddingProvider::openai()` (falls back to stub on error)
    /// - `"local"` → `HttpEmbeddingProvider::local()` pointing at localhost Ollama
    /// - anything else → `StubEmbeddingProvider` (until ONNX support is added)
    pub fn from_config(config: &KcEmbeddingConfig) -> Self {
        match config.provider.as_str() {
            "openai" => match HttpEmbeddingProvider::openai(&config.model, config.dimensions) {
                Ok(p) => Self::new(Box::new(p)),
                Err(e) => {
                    log::warn!(
                        "Failed to create OpenAI embedding provider: {}, using stub",
                        e
                    );
                    Self::new(Box::new(StubEmbeddingProvider::new(config.dimensions)))
                }
            },
            "local" => {
                // Default to localhost Ollama
                Self::new(Box::new(HttpEmbeddingProvider::local(
                    "http://localhost:11434/api/embeddings",
                    &config.model,
                    config.dimensions,
                )))
            }
            _ => {
                // Unknown / "stub" / future local model → stub until ONNX support
                Self::new(Box::new(StubEmbeddingProvider::new(config.dimensions)))
            }
        }
    }

    /// Generate embeddings for a batch of texts.
    pub fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, KcError> {
        self.provider.embed_batch(texts)
    }

    /// Single text embedding convenience method.
    pub fn embed_one(&self, text: &str) -> Result<Vec<f32>, KcError> {
        let results = self.embed(&[text])?;
        results
            .into_iter()
            .next()
            .ok_or_else(|| KcError::Storage("Empty embedding result".into()))
    }

    /// The model name used by the current provider (for cache tagging).
    pub fn model_name(&self) -> &str {
        self.provider.model_name()
    }

    /// The embedding dimensionality of the current provider.
    pub fn dimensions(&self) -> usize {
        self.provider.dimensions()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
//  UTILITIES
// ═══════════════════════════════════════════════════════════════════════════════

/// Cosine similarity between two embedding vectors.
///
/// Returns a value in `[-1.0, 1.0]`. Returns `0.0` if either vector is
/// zero-length or the vectors have different dimensions.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stub_provider() {
        let provider = StubEmbeddingProvider::new(64);
        let texts = &["hello world", "goodbye world"];
        let embeddings = provider.embed_batch(texts).unwrap();

        // Correct number of results
        assert_eq!(embeddings.len(), 2);

        // Correct dimensions
        assert_eq!(embeddings[0].len(), 64);
        assert_eq!(embeddings[1].len(), 64);

        // Deterministic: same input → same output
        let embeddings2 = provider.embed_batch(texts).unwrap();
        assert_eq!(embeddings[0], embeddings2[0]);
        assert_eq!(embeddings[1], embeddings2[1]);

        // Different inputs → different outputs
        assert_ne!(embeddings[0], embeddings[1]);

        // Normalized to unit vector
        let norm: f32 = embeddings[0].iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5, "norm was {}", norm);
    }

    #[test]
    fn test_embedding_manager_stub() {
        // Unknown provider falls back to stub
        let config = KcEmbeddingConfig {
            provider: "unknown_provider".into(),
            model: "whatever".into(),
            dimensions: 128,
            batch_size: 32,
        };
        let manager = EmbeddingManager::from_config(&config);

        assert_eq!(manager.model_name(), "stub");
        assert_eq!(manager.dimensions(), 128);

        let emb = manager.embed(&["test"]).unwrap();
        assert_eq!(emb.len(), 1);
        assert_eq!(emb[0].len(), 128);
    }

    #[test]
    fn test_cosine_similarity() {
        // Identical vectors → 1.0
        let a = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &a) - 1.0).abs() < 1e-6);

        // Orthogonal vectors → 0.0
        let b = vec![0.0, 1.0, 0.0];
        assert!(cosine_similarity(&a, &b).abs() < 1e-6);

        // Opposite vectors → -1.0
        let c = vec![-1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &c) + 1.0).abs() < 1e-6);

        // Mismatched dimensions → 0.0
        let d = vec![1.0, 0.0];
        assert_eq!(cosine_similarity(&a, &d), 0.0);

        // Zero vector → 0.0
        let z = vec![0.0, 0.0, 0.0];
        assert_eq!(cosine_similarity(&a, &z), 0.0);
    }

    #[test]
    fn test_embed_one() {
        let config = KcEmbeddingConfig {
            provider: "stub".into(),
            model: "stub".into(),
            dimensions: 32,
            batch_size: 16,
        };
        let manager = EmbeddingManager::from_config(&config);

        let emb = manager.embed_one("single text").unwrap();
        assert_eq!(emb.len(), 32);

        // Should match batch embed of the same text
        let batch = manager.embed(&["single text"]).unwrap();
        assert_eq!(emb, batch[0]);
    }

    #[test]
    fn test_stub_provider_metadata() {
        let provider = StubEmbeddingProvider::new(256);
        assert_eq!(provider.model_name(), "stub");
        assert_eq!(provider.dimensions(), 256);
    }

    #[test]
    fn test_embedding_manager_openai_fallback() {
        // OpenAI without API key set → falls back to stub
        // (Remove the env var just in case it's set in the test environment)
        std::env::remove_var("OPENAI_API_KEY");

        let config = KcEmbeddingConfig {
            provider: "openai".into(),
            model: "text-embedding-3-small".into(),
            dimensions: 1536,
            batch_size: 32,
        };
        let manager = EmbeddingManager::from_config(&config);

        // Should have fallen back to stub
        assert_eq!(manager.model_name(), "stub");
        assert_eq!(manager.dimensions(), 1536);
    }

    #[test]
    fn test_local_provider_metadata() {
        let provider =
            HttpEmbeddingProvider::local("http://localhost:11434/api/embeddings", "nomic", 768);
        assert_eq!(provider.model_name(), "nomic");
        assert_eq!(provider.dimensions(), 768);
    }
}
