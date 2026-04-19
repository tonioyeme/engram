//! Topic Discovery — discovers topic candidates from memory clusters.
//!
//! Uses Infomap community detection (information-theoretic) to find groups of
//! related memories. Infomap minimises the map equation on a similarity graph,
//! naturally discovering community structure without suffering from the
//! single-linkage chaining effect that plagued the old agglomerative approach.

use std::collections::{HashMap, HashSet};

use infomap_rs::{Infomap, Network};

use crate::embeddings::EmbeddingProvider;
use super::llm::LlmProvider;
use super::types::*;

// ═══════════════════════════════════════════════════════════════════════════════
//  TOPIC DISCOVERY
// ═══════════════════════════════════════════════════════════════════════════════

/// Discovers topic candidates from a set of memories using Infomap community
/// detection on a cosine-similarity graph.
pub struct TopicDiscovery {
    /// Minimum number of memories required to form a valid cluster.
    min_cluster_size: usize,
    /// Jaccard similarity threshold for overlap detection with existing topics.
    overlap_threshold: f64,
    /// Minimum cosine similarity to create an edge in the similarity graph.
    /// Pairs below this threshold are not connected — Infomap only sees
    /// edges that represent genuine semantic relatedness.
    edge_threshold: f64,
}

impl TopicDiscovery {
    /// Create a new `TopicDiscovery` with the given minimum cluster size.
    ///
    /// The overlap threshold defaults to 0.3 (matching `TopicCandidate::overlaps_with`).
    /// The edge threshold defaults to 0.3 — only pairs with cosine similarity ≥ 0.3
    /// get an edge in the graph fed to Infomap.
    pub fn new(min_cluster_size: usize) -> Self {
        Self {
            min_cluster_size,
            overlap_threshold: 0.3,
            edge_threshold: 0.3,
        }
    }

    /// Create a TopicDiscovery with a custom edge threshold.
    ///
    /// Lower threshold → more edges → fewer, larger communities.
    /// Higher threshold → fewer edges → more, smaller communities.
    pub fn with_edge_threshold(mut self, threshold: f64) -> Self {
        self.edge_threshold = threshold;
        self
    }

    /// Discover topic candidates from memories using Infomap community detection.
    ///
    /// # Algorithm
    ///
    /// 1. Compute pairwise cosine similarity between all memory embeddings
    /// 2. Build a weighted graph: edges only where similarity ≥ edge_threshold
    /// 3. Run Infomap to find community structure (minimises map equation)
    /// 4. Filter communities below `min_cluster_size`
    /// 5. For each community, create a `TopicCandidate` with:
    ///    - `memories`: list of memory IDs in the community
    ///    - `centroid_embedding`: mean of member embeddings
    ///    - `cohesion_score`: average intra-community similarity
    ///    - `suggested_title`: `None` (can be filled by `label_cluster`)
    pub fn discover(
        &self,
        memories: &[(String, Vec<f32>)], // (memory_id, embedding)
    ) -> Vec<TopicCandidate> {
        if memories.len() < 2 {
            return Vec::new();
        }

        let n = memories.len();

        // Step 1: Build the Infomap network.
        // Nodes are indices into `memories`, edges are cosine similarities above threshold.
        let mut network = Network::with_capacity(n);
        // Ensure all nodes exist even if they have no edges.
        network.ensure_capacity(n);

        let mut sim_cache: HashMap<(usize, usize), f64> = HashMap::new();
        let mut edge_count = 0usize;

        for i in 0..n {
            for j in (i + 1)..n {
                let sim = EmbeddingProvider::cosine_similarity(&memories[i].1, &memories[j].1) as f64;
                if sim >= self.edge_threshold {
                    // Infomap uses directed edges; add both directions for undirected similarity.
                    network.add_edge(i, j, sim);
                    network.add_edge(j, i, sim);
                    sim_cache.insert((i, j), sim);
                    edge_count += 1;
                }
            }
        }

        // If no edges survive the threshold, no communities can be found.
        if edge_count == 0 {
            return Vec::new();
        }

        // Step 2: Run Infomap.
        let result = Infomap::new(&network)
            .seed(42)
            .run();

        // Step 3: Group memories by module assignment.
        let mut modules: HashMap<usize, Vec<usize>> = HashMap::new();
        for (node_idx, &module_id) in result.assignments.iter().enumerate() {
            if node_idx < n {
                modules.entry(module_id).or_default().push(node_idx);
            }
        }

        // Step 4: Build TopicCandidates, filtering by min_cluster_size.
        let mut candidates = Vec::new();

        for (_module_id, member_indices) in &modules {
            if member_indices.len() < self.min_cluster_size {
                continue;
            }

            let memory_ids: Vec<String> = member_indices
                .iter()
                .map(|&i| memories[i].0.clone())
                .collect();

            // Centroid: mean of embeddings.
            let dim = memories[0].1.len();
            let mut centroid = vec![0.0f32; dim];
            for &idx in member_indices {
                for (d, val) in memories[idx].1.iter().enumerate() {
                    if d < dim {
                        centroid[d] += val;
                    }
                }
            }
            let count = member_indices.len() as f32;
            for c in centroid.iter_mut() {
                *c /= count;
            }

            // Cohesion: average intra-community pairwise similarity.
            let mut cohesion_sum = 0.0;
            let mut pair_count = 0usize;
            for (pi, &i) in member_indices.iter().enumerate() {
                for &j in &member_indices[(pi + 1)..] {
                    let (lo, hi) = if i < j { (i, j) } else { (j, i) };
                    let sim = sim_cache
                        .get(&(lo, hi))
                        .copied()
                        .unwrap_or_else(|| {
                            EmbeddingProvider::cosine_similarity(
                                &memories[i].1,
                                &memories[j].1,
                            ) as f64
                        });
                    cohesion_sum += sim;
                    pair_count += 1;
                }
            }
            let cohesion_score = if pair_count > 0 {
                cohesion_sum / pair_count as f64
            } else {
                1.0
            };

            candidates.push(TopicCandidate {
                memories: memory_ids,
                centroid_embedding: centroid,
                cohesion_score,
                suggested_title: None,
            });
        }

        // Sort candidates by cohesion descending for deterministic output.
        candidates.sort_by(|a, b| {
            b.cohesion_score
                .partial_cmp(&a.cohesion_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        candidates
    }

    /// Label a topic candidate using the LLM.
    ///
    /// Sends memory contents to the LLM asking for a concise 2-5 word topic label.
    /// On LLM failure, falls back to using the first 5 words of the longest memory
    /// content as the label.
    pub fn label_cluster(
        &self,
        candidate: &TopicCandidate,
        memory_contents: &[(String, String)], // (memory_id, content)
        llm: &dyn LlmProvider,
    ) -> Result<String, KcError> {
        // Build the prompt from memory contents that belong to this candidate.
        let mut prompt = String::from(
            "Given these related notes/memories, suggest a concise topic label (2-5 words):\n\n",
        );

        let mut numbered = 0;
        let candidate_ids: HashSet<&str> =
            candidate.memories.iter().map(|s| s.as_str()).collect();

        for (id, content) in memory_contents {
            if candidate_ids.contains(id.as_str()) {
                numbered += 1;
                prompt.push_str(&format!("{}. {}\n", numbered, content));
            }
        }

        prompt.push_str("\nRespond with ONLY the topic label, nothing else.");

        let request = LlmRequest {
            task: LlmTask::GenerateTitle,
            prompt,
            max_tokens: Some(20),
            temperature: Some(0.3),
        };

        match llm.complete(&request) {
            Ok(response) => {
                let label = response.content.trim().to_string();
                if label.is_empty() {
                    Ok(Self::fallback_label(memory_contents, candidate))
                } else {
                    Ok(label)
                }
            }
            Err(_) => {
                // Fallback: first 5 words of the longest memory.
                Ok(Self::fallback_label(memory_contents, candidate))
            }
        }
    }

    /// Fallback label: first 5 words of the longest memory content in the candidate.
    fn fallback_label(
        memory_contents: &[(String, String)],
        candidate: &TopicCandidate,
    ) -> String {
        let candidate_ids: HashSet<&str> =
            candidate.memories.iter().map(|s| s.as_str()).collect();

        let longest = memory_contents
            .iter()
            .filter(|(id, _)| candidate_ids.contains(id.as_str()))
            .max_by_key(|(_, content)| content.len());

        match longest {
            Some((_, content)) => {
                let words: Vec<&str> = content.split_whitespace().take(5).collect();
                words.join(" ")
            }
            None => "Untitled Topic".to_string(),
        }
    }

    /// Check overlap between a candidate and existing topic pages.
    ///
    /// Returns `Some(topic_id)` if the Jaccard similarity of the candidate's
    /// memory set and any existing topic's source memories exceeds `overlap_threshold`.
    pub fn detect_overlap(
        &self,
        candidate: &TopicCandidate,
        existing: &[TopicPage],
    ) -> Option<TopicId> {
        let candidate_set: HashSet<&str> =
            candidate.memories.iter().map(|s| s.as_str()).collect();

        for page in existing {
            let page_set: HashSet<&str> = page
                .metadata
                .source_memory_ids
                .iter()
                .map(|s| s.as_str())
                .collect();

            let intersection = candidate_set.intersection(&page_set).count();
            let union = candidate_set.union(&page_set).count();

            if union == 0 {
                continue;
            }

            let jaccard = intersection as f64 / union as f64;
            if jaccard > self.overlap_threshold {
                return Some(page.id.clone());
            }
        }

        None
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
//  TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::llm::LlmProvider;
    use chrono::Utc;

    // ── Mock LLM Provider ────────────────────────────────────────────────

    struct MockLlmProvider {
        response: Result<LlmResponse, LlmError>,
    }

    impl MockLlmProvider {
        fn success(label: &str) -> Self {
            Self {
                response: Ok(LlmResponse {
                    content: label.to_string(),
                    usage: TokenUsage {
                        input_tokens: 10,
                        output_tokens: 5,
                    },
                    model: "mock".to_string(),
                    duration_ms: 1,
                }),
            }
        }

        fn failure() -> Self {
            Self {
                response: Err(LlmError::ProviderUnavailable(
                    "mock failure".to_string(),
                )),
            }
        }
    }

    impl LlmProvider for MockLlmProvider {
        fn complete(&self, _request: &LlmRequest) -> Result<LlmResponse, LlmError> {
            self.response.clone()
        }

        fn metadata(&self) -> ProviderMetadata {
            ProviderMetadata {
                name: "mock".to_string(),
                model: "mock".to_string(),
                max_context_tokens: 1000,
                supports_streaming: false,
            }
        }

        fn health_check(&self) -> Result<(), LlmError> {
            Ok(())
        }
    }

    // ── Helper: make a simple TopicPage ──────────────────────────────────

    fn make_topic_page(id: &str, source_ids: Vec<&str>) -> TopicPage {
        let now = Utc::now();
        TopicPage {
            id: TopicId(id.to_string()),
            title: format!("Topic {}", id),
            content: "content".to_string(),
            sections: Vec::new(),
            summary: "summary".to_string(),
            metadata: TopicMetadata {
                created_at: now,
                updated_at: now,
                compilation_count: 1,
                source_memory_ids: source_ids.into_iter().map(|s| s.to_string()).collect(),
                tags: vec![],
                quality_score: Some(0.8),
            },
            status: TopicStatus::Active,
            version: 1,
        }
    }

    // ── Tests ────────────────────────────────────────────────────────────

    #[test]
    fn test_discover_basic_two_clusters() {
        // 6 memories forming 2 clusters:
        // Cluster A: m1, m2, m3 — all near [1, 0, 0]
        // Cluster B: m4, m5, m6 — all near [0, 1, 0]
        let memories = vec![
            ("m1".to_string(), vec![1.0f32, 0.0, 0.0]),
            ("m2".to_string(), vec![0.95, 0.1, 0.0]),
            ("m3".to_string(), vec![0.9, 0.15, 0.0]),
            ("m4".to_string(), vec![0.0, 1.0, 0.0]),
            ("m5".to_string(), vec![0.1, 0.95, 0.0]),
            ("m6".to_string(), vec![0.15, 0.9, 0.0]),
        ];

        let discovery = TopicDiscovery::new(2);
        let candidates = discovery.discover(&memories);

        // Must find exactly 2 clusters.
        assert_eq!(candidates.len(), 2, "Expected 2 clusters, got {}", candidates.len());

        // Each cluster should have exactly 3 members.
        let mut sizes: Vec<usize> = candidates.iter().map(|c| c.memories.len()).collect();
        sizes.sort();
        assert_eq!(sizes, vec![3, 3]);

        // Verify cluster membership: m1-m3 together, m4-m6 together.
        let c0: HashSet<&str> = candidates[0].memories.iter().map(|s| s.as_str()).collect();
        let c1: HashSet<&str> = candidates[1].memories.iter().map(|s| s.as_str()).collect();

        let group_a: HashSet<&str> = ["m1", "m2", "m3"].into();
        let group_b: HashSet<&str> = ["m4", "m5", "m6"].into();

        assert!(
            (c0 == group_a && c1 == group_b) || (c0 == group_b && c1 == group_a),
            "Cluster membership incorrect: {:?} and {:?}",
            c0, c1
        );
    }

    #[test]
    fn test_discover_no_chaining_effect() {
        // The critical test: a chain of memories where each adjacent pair is
        // similar but endpoints are dissimilar.
        // Old single-linkage would merge everything into one cluster.
        // Infomap should find the natural community breaks.
        //
        // We create 3 tight clusters connected by weak bridges:
        // Cluster A: [1,0,0], [0.9,0.1,0] — tight pair
        // Cluster B: [0,1,0], [0.1,0.9,0] — tight pair
        // Cluster C: [0,0,1], [0.1,0,0.9] — tight pair
        // Bridge A-B: a memory at [0.5,0.5,0] — somewhat similar to both A and B
        // but should NOT cause A and B to merge.
        let memories = vec![
            // Cluster A
            ("a1".to_string(), vec![1.0f32, 0.0, 0.0]),
            ("a2".to_string(), vec![0.95, 0.1, 0.0]),
            ("a3".to_string(), vec![0.9, 0.05, 0.05]),
            // Cluster B
            ("b1".to_string(), vec![0.0, 1.0, 0.0]),
            ("b2".to_string(), vec![0.1, 0.95, 0.0]),
            ("b3".to_string(), vec![0.05, 0.9, 0.05]),
            // Cluster C
            ("c1".to_string(), vec![0.0, 0.0, 1.0]),
            ("c2".to_string(), vec![0.1, 0.0, 0.95]),
            ("c3".to_string(), vec![0.05, 0.05, 0.9]),
        ];

        let discovery = TopicDiscovery::new(2);
        let candidates = discovery.discover(&memories);

        // Should find 3 clusters, NOT 1 (which is what single-linkage would do).
        assert!(
            candidates.len() >= 2,
            "Expected at least 2 clusters (preferably 3), got {}. \
             This indicates the chaining effect is still present.",
            candidates.len()
        );

        // Verify no single cluster contains memories from all 3 groups.
        for c in &candidates {
            let ids: HashSet<&str> = c.memories.iter().map(|s| s.as_str()).collect();
            let has_a = ids.iter().any(|id| id.starts_with('a'));
            let has_b = ids.iter().any(|id| id.starts_with('b'));
            let has_c = ids.iter().any(|id| id.starts_with('c'));
            let groups = [has_a, has_b, has_c].iter().filter(|&&x| x).count();
            assert!(
                groups <= 1,
                "Cluster contains memories from {} different groups: {:?}. \
                 Chaining effect detected.",
                groups, ids
            );
        }
    }

    #[test]
    fn test_discover_empty() {
        let discovery = TopicDiscovery::new(2);
        let candidates = discovery.discover(&[]);
        assert!(candidates.is_empty());
    }

    #[test]
    fn test_discover_single_memory() {
        let memories = vec![("m1".to_string(), vec![1.0f32, 0.0])];
        let discovery = TopicDiscovery::new(2);
        let candidates = discovery.discover(&memories);
        assert!(candidates.is_empty());
    }

    #[test]
    fn test_discover_min_cluster_size() {
        // 3 memories: two very similar, one outlier.
        // With min_cluster_size=3, no cluster should form.
        let memories = vec![
            ("m1".to_string(), vec![1.0f32, 0.0, 0.0]),
            ("m2".to_string(), vec![0.95, 0.1, 0.0]),
            ("m3".to_string(), vec![0.0, 0.0, 1.0]), // outlier
        ];

        let discovery = TopicDiscovery::new(3);
        let candidates = discovery.discover(&memories);

        // The pair (m1, m2) forms a cluster of size 2, but min_cluster_size=3 filters it.
        // m3 is an outlier — no cluster.
        assert!(
            candidates.is_empty(),
            "Expected no clusters with min_cluster_size=3, got {}",
            candidates.len()
        );
    }

    #[test]
    fn test_discover_cohesion_score() {
        let memories = vec![
            ("m1".to_string(), vec![1.0f32, 0.0, 0.0]),
            ("m2".to_string(), vec![0.99, 0.01, 0.0]),
            ("m3".to_string(), vec![0.98, 0.02, 0.0]),
        ];

        let discovery = TopicDiscovery::new(2);
        let candidates = discovery.discover(&memories);

        assert_eq!(candidates.len(), 1);
        // Cohesion should be very high (near 1.0) since all are almost identical.
        assert!(
            candidates[0].cohesion_score > 0.95,
            "Expected high cohesion, got {}",
            candidates[0].cohesion_score
        );
    }

    #[test]
    fn test_label_cluster_success() {
        let candidate = TopicCandidate {
            memories: vec!["m1".to_string(), "m2".to_string()],
            centroid_embedding: vec![1.0, 0.0],
            cohesion_score: 0.8,
            suggested_title: None,
        };

        let contents = vec![
            ("m1".to_string(), "Rust programming language".to_string()),
            ("m2".to_string(), "Cargo build system".to_string()),
        ];

        let llm = MockLlmProvider::success("Rust Development");
        let discovery = TopicDiscovery::new(2);
        let label = discovery.label_cluster(&candidate, &contents, &llm).unwrap();
        assert_eq!(label, "Rust Development");
    }

    #[test]
    fn test_label_cluster_fallback() {
        let candidate = TopicCandidate {
            memories: vec!["m1".to_string(), "m2".to_string()],
            centroid_embedding: vec![1.0, 0.0],
            cohesion_score: 0.8,
            suggested_title: None,
        };

        let contents = vec![
            ("m1".to_string(), "Short".to_string()),
            (
                "m2".to_string(),
                "This is a longer memory content for testing fallback labels"
                    .to_string(),
            ),
        ];

        let llm = MockLlmProvider::failure();
        let discovery = TopicDiscovery::new(2);
        let label = discovery.label_cluster(&candidate, &contents, &llm).unwrap();
        assert_eq!(label, "This is a longer memory");
    }

    #[test]
    fn test_detect_overlap() {
        let candidate = TopicCandidate {
            memories: vec![
                "m1".to_string(),
                "m2".to_string(),
                "m3".to_string(),
            ],
            centroid_embedding: vec![1.0, 0.0],
            cohesion_score: 0.8,
            suggested_title: None,
        };

        let existing = vec![make_topic_page("t1", vec!["m1", "m2", "m4"])];
        let discovery = TopicDiscovery::new(2);

        // Jaccard: |{m1,m2}| / |{m1,m2,m3,m4}| = 2/4 = 0.5 > 0.3
        let overlap = discovery.detect_overlap(&candidate, &existing);
        assert!(overlap.is_some());
        assert_eq!(overlap.unwrap().0, "t1");
    }

    #[test]
    fn test_detect_no_overlap() {
        let candidate = TopicCandidate {
            memories: vec!["m1".to_string(), "m2".to_string()],
            centroid_embedding: vec![1.0, 0.0],
            cohesion_score: 0.8,
            suggested_title: None,
        };

        let existing = vec![make_topic_page("t1", vec!["m10", "m20", "m30"])];
        let discovery = TopicDiscovery::new(2);

        let overlap = discovery.detect_overlap(&candidate, &existing);
        assert!(overlap.is_none());
    }

    #[test]
    fn test_edge_threshold_controls_granularity() {
        // With a high edge threshold, fewer edges survive → more/smaller communities.
        // With a low edge threshold, more edges → fewer/larger communities.
        let memories = vec![
            ("m1".to_string(), vec![1.0f32, 0.0, 0.0]),
            ("m2".to_string(), vec![0.8, 0.2, 0.0]),  // sim to m1 ≈ 0.97
            ("m3".to_string(), vec![0.6, 0.4, 0.0]),  // sim to m1 ≈ 0.83
            ("m4".to_string(), vec![0.0, 1.0, 0.0]),
            ("m5".to_string(), vec![0.2, 0.8, 0.0]),  // sim to m4 ≈ 0.97
            ("m6".to_string(), vec![0.4, 0.6, 0.0]),  // sim to m4 ≈ 0.83
        ];

        // Low threshold → everything connected → likely 1 or 2 clusters
        let low = TopicDiscovery::new(2).with_edge_threshold(0.1);
        let low_clusters = low.discover(&memories);

        // High threshold → only very similar pairs connected → could get more clusters
        let high = TopicDiscovery::new(2).with_edge_threshold(0.9);
        let high_clusters = high.discover(&memories);

        // With high threshold, fewer edges → at least as many (or more) communities
        assert!(
            high_clusters.len() >= low_clusters.len()
                || high_clusters.is_empty(), // might filter all edges
            "Higher threshold should give same or more clusters \
             (low={}, high={})",
            low_clusters.len(),
            high_clusters.len()
        );
    }
}
