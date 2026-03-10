//! Drive Alignment Scorer — Score how well memories align with SOUL drives.
//!
//! Memories that align with core drives get automatic importance boosts.

use crate::bus::mod_io::Drive;

/// Default importance multiplier for drive-aligned memories.
pub const ALIGNMENT_BOOST: f64 = 1.5;

/// Score how well a memory content aligns with a set of drives.
///
/// Returns a score from 0.0 (no alignment) to 1.0 (strong alignment).
/// The scoring is based on keyword matching between the memory content
/// and the drives' keywords.
///
/// # Arguments
///
/// * `content` - The memory content to score
/// * `drives` - List of drives to check alignment against
pub fn score_alignment(content: &str, drives: &[Drive]) -> f64 {
    if drives.is_empty() {
        return 0.0;
    }
    
    let content_lower = content.to_lowercase();
    let content_words: Vec<&str> = content_lower.split_whitespace().collect();
    
    let mut total_score = 0.0;
    let mut matched_drives = 0;
    
    for drive in drives {
        let mut drive_matches = 0;
        let keywords = if drive.keywords.is_empty() {
            drive.extract_keywords()
        } else {
            drive.keywords.clone()
        };
        
        for keyword in &keywords {
            // Check for exact word match or substring match
            if content_words.iter().any(|w| w.contains(keyword)) {
                drive_matches += 1;
            }
        }
        
        if drive_matches > 0 {
            matched_drives += 1;
            // Score contribution: min(1.0, matches / 3) - need at least 3 matches for full score
            let drive_score = (drive_matches as f64 / 3.0).min(1.0);
            total_score += drive_score;
        }
    }
    
    if matched_drives == 0 {
        return 0.0;
    }
    
    // Average score across matched drives, capped at 1.0
    (total_score / matched_drives as f64).min(1.0)
}

/// Calculate the importance boost for a memory based on drive alignment.
///
/// Returns a multiplier (1.0 = no boost, ALIGNMENT_BOOST for perfect alignment).
///
/// # Arguments
///
/// * `content` - The memory content
/// * `drives` - List of drives from SOUL.md
pub fn calculate_importance_boost(content: &str, drives: &[Drive]) -> f64 {
    let alignment = score_alignment(content, drives);
    
    if alignment <= 0.0 {
        return 1.0; // No boost
    }
    
    // Linear interpolation between 1.0 and ALIGNMENT_BOOST based on alignment
    1.0 + (ALIGNMENT_BOOST - 1.0) * alignment
}

/// Check if content is strongly aligned with any drive.
///
/// Returns true if alignment score is above 0.5.
pub fn is_strongly_aligned(content: &str, drives: &[Drive]) -> bool {
    score_alignment(content, drives) > 0.5
}

/// Find which drives a piece of content aligns with.
///
/// Returns a list of (drive_name, alignment_score) pairs for aligned drives.
pub fn find_aligned_drives(content: &str, drives: &[Drive]) -> Vec<(String, f64)> {
    let content_lower = content.to_lowercase();
    let content_words: Vec<&str> = content_lower.split_whitespace().collect();
    
    let mut aligned = Vec::new();
    
    for drive in drives {
        let keywords = if drive.keywords.is_empty() {
            drive.extract_keywords()
        } else {
            drive.keywords.clone()
        };
        
        let mut matches = 0;
        for keyword in &keywords {
            if content_words.iter().any(|w| w.contains(keyword)) {
                matches += 1;
            }
        }
        
        if matches > 0 {
            let score = (matches as f64 / 3.0).min(1.0);
            aligned.push((drive.name.clone(), score));
        }
    }
    
    // Sort by score descending
    aligned.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    aligned
}

#[cfg(test)]
mod tests {
    use super::*;
    
    fn sample_drives() -> Vec<Drive> {
        vec![
            Drive {
                name: "curiosity".to_string(),
                description: "Always seek to understand and learn new things".to_string(),
                keywords: vec!["curiosity".to_string(), "understand".to_string(), "learn".to_string(), "new".to_string()],
            },
            Drive {
                name: "helpfulness".to_string(),
                description: "Help users solve problems effectively".to_string(),
                keywords: vec!["helpfulness".to_string(), "help".to_string(), "solve".to_string(), "problems".to_string()],
            },
            Drive {
                name: "honesty".to_string(),
                description: "Be honest and direct in communication".to_string(),
                keywords: vec!["honesty".to_string(), "honest".to_string(), "direct".to_string(), "communication".to_string()],
            },
        ]
    }
    
    #[test]
    fn test_strong_alignment() {
        let drives = sample_drives();
        
        // Content that strongly aligns with "curiosity"
        let content = "I want to learn and understand these new concepts deeply";
        let score = score_alignment(content, &drives);
        assert!(score > 0.5, "Expected strong alignment, got {}", score);
    }
    
    #[test]
    fn test_weak_alignment() {
        let drives = sample_drives();
        
        // Content with minimal alignment
        let content = "The weather is nice today";
        let score = score_alignment(content, &drives);
        assert!(score < 0.3, "Expected weak alignment, got {}", score);
    }
    
    #[test]
    fn test_no_alignment() {
        let drives = sample_drives();
        
        // Content with no alignment
        let content = "xyz abc 123";
        let score = score_alignment(content, &drives);
        assert_eq!(score, 0.0);
    }
    
    #[test]
    fn test_importance_boost() {
        let drives = sample_drives();
        
        // Strongly aligned content gets boost
        let aligned = "I want to learn and understand new concepts";
        let boost = calculate_importance_boost(aligned, &drives);
        assert!(boost > 1.0, "Expected boost > 1.0, got {}", boost);
        assert!(boost <= ALIGNMENT_BOOST);
        
        // Non-aligned content gets no boost
        let unaligned = "xyz abc 123";
        let boost = calculate_importance_boost(unaligned, &drives);
        assert_eq!(boost, 1.0);
    }
    
    #[test]
    fn test_find_aligned_drives() {
        let drives = sample_drives();
        
        let content = "I want to help people understand and solve their problems";
        let aligned = find_aligned_drives(content, &drives);
        
        assert!(aligned.len() >= 2);
        // Should find helpfulness and curiosity
        let drive_names: Vec<_> = aligned.iter().map(|(n, _)| n.as_str()).collect();
        assert!(drive_names.contains(&"helpfulness") || drive_names.contains(&"curiosity"));
    }
    
    #[test]
    fn test_empty_drives() {
        let drives: Vec<Drive> = vec![];
        let content = "any content here";
        assert_eq!(score_alignment(content, &drives), 0.0);
        assert_eq!(calculate_importance_boost(content, &drives), 1.0);
    }
}
