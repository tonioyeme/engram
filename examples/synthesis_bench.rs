//! Real-data synthesis benchmark.
//!
//! Runs cluster discovery on the real engram-memory.db to measure
//! actual performance of the optimized synthesis pipeline.
//!
//! Usage:
//!   cargo run --release --example synthesis_bench

use std::time::Instant;

use engramai::storage::Storage;
use engramai::synthesis::cluster;
use engramai::synthesis::types::ClusterDiscoveryConfig;

fn main() {
    env_logger::init();

    let db_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/Users/potato/rustclaw/engram-memory.db".to_string());

    println!("=== Synthesis Cluster Discovery Benchmark ===");
    println!("Database: {}", db_path);

    // Open storage
    let storage = Storage::new(&db_path).expect("Failed to open database");

    // Count memories
    let all = storage.all().expect("Failed to load memories");
    println!("Total memories: {}", all.len());

    // Count memories that pass pre-filter
    let config = ClusterDiscoveryConfig::default();
    let candidates: Vec<_> = all
        .iter()
        .filter(|m| {
            !m.access_times.is_empty()
                && m.importance >= config.min_importance
                && !m.metadata
                    .as_ref()
                    .and_then(|md: &serde_json::Value| md.get("is_synthesis"))
                    .and_then(|v: &serde_json::Value| v.as_bool())
                    .unwrap_or(false)
        })
        .collect();
    println!("Candidate memories (after pre-filter): {}", candidates.len());

    // Count Hebbian links
    let mut hebbian_count: usize = 0;
    for m in &candidates {
        if let Ok(links) = storage.get_hebbian_links_weighted(&m.id) {
            hebbian_count += links.len();
        }
    }
    println!("Hebbian links (involving candidates): {} (directed)", hebbian_count);

    // Count embeddings
    let embeddings = storage
        .get_all_embeddings("ollama/nomic-embed-text")
        .unwrap_or_default();
    let embedded_candidates = candidates
        .iter()
        .filter(|m| embeddings.iter().any(|(id, _)| id == &m.id))
        .count();
    println!(
        "Candidates with embeddings: {}/{}",
        embedded_candidates,
        candidates.len()
    );

    println!("\n--- Running cluster discovery ---");

    // Warm-up run
    print!("Warm-up run... ");
    let t0 = Instant::now();
    let _ = cluster::discover_clusters(&storage, &config, Some("ollama/nomic-embed-text"));
    println!("done in {:.3}s", t0.elapsed().as_secs_f64());

    // Benchmark runs
    let n_runs = 3;
    let mut durations = Vec::new();
    let mut last_clusters = Vec::new();

    for i in 1..=n_runs {
        print!("Run {}/{}... ", i, n_runs);
        let t = Instant::now();
        let clusters =
            cluster::discover_clusters(&storage, &config, Some("ollama/nomic-embed-text"))
                .expect("cluster discovery failed");
        let elapsed = t.elapsed();
        println!(
            "{:.3}s — {} clusters found",
            elapsed.as_secs_f64(),
            clusters.len()
        );
        durations.push(elapsed);
        last_clusters = clusters;
    }

    // Stats
    let avg_ms =
        durations.iter().map(|d| d.as_secs_f64() * 1000.0).sum::<f64>() / n_runs as f64;
    let min_ms = durations
        .iter()
        .map(|d| d.as_secs_f64() * 1000.0)
        .fold(f64::MAX, f64::min);
    let max_ms = durations
        .iter()
        .map(|d| d.as_secs_f64() * 1000.0)
        .fold(0.0_f64, f64::max);

    println!("\n=== Results ===");
    println!("Runs: {}", n_runs);
    println!("Average: {:.1}ms", avg_ms);
    println!("Min:     {:.1}ms", min_ms);
    println!("Max:     {:.1}ms", max_ms);
    println!("Clusters found: {}", last_clusters.len());

    // Cluster details
    if !last_clusters.is_empty() {
        println!("\n--- Top 10 clusters ---");
        for (i, c) in last_clusters.iter().take(10).enumerate() {
            println!(
                "  #{}: {} members, quality={:.3}, centroid={}, signal={:?}",
                i + 1,
                c.members.len(),
                c.quality_score,
                &c.centroid_id[..8.min(c.centroid_id.len())],
                c.signals_summary.dominant_signal,
            );
        }

        // Size distribution
        let sizes: Vec<usize> = last_clusters.iter().map(|c| c.members.len()).collect();
        let total_clustered: usize = sizes.iter().sum();
        println!("\n--- Size distribution ---");
        println!("Total memories in clusters: {}", total_clustered);
        println!(
            "Cluster sizes: min={}, max={}, median={}",
            sizes.iter().min().unwrap(),
            sizes.iter().max().unwrap(),
            {
                let mut s = sizes.clone();
                s.sort();
                s[s.len() / 2]
            },
        );
    }

    println!("\n--- Performance context ---");
    println!("N = {} candidates", candidates.len());
    println!(
        "N² = {} (brute-force pairs would be)",
        (candidates.len() as u64) * (candidates.len() as u64)
    );
    println!("ISS-001 target: <10s for N=14000");
    if avg_ms < 10_000.0 {
        println!("✅ PASS — {:.1}ms < 10,000ms target", avg_ms);
    } else {
        println!("❌ FAIL — {:.1}ms > 10,000ms target", avg_ms);
    }
}
