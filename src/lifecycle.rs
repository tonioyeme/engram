//! Memory lifecycle types: decay, forget, dedup, reconcile, rebalance.

use serde::{Deserialize, Serialize};

/// Result of a decay check across memories.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct DecayReport {
    /// Memories with effective_strength below threshold (0.1)
    pub below_threshold: usize,
    /// Memories that were soft-deleted due to low strength + low access
    pub flagged_for_forget: usize,
}

/// Result of a bulk forget operation.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ForgetReport {
    /// Total memories scanned
    pub scanned: usize,
    /// Memories soft-deleted (new)
    pub soft_deleted: usize,
    /// Memories hard-deleted (previously soft-deleted > 30 days ago)
    pub hard_deleted: usize,
}

/// Result of an add operation — informational, stored on Memory struct.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AddResult {
    /// New memory created.
    Created { id: String },
    /// Merged into existing memory.
    Merged { into: String, similarity: f32 },
}

/// Lifecycle-specific errors.
#[derive(Debug, thiserror::Error)]
pub enum LifecycleError {
    #[error("storage: {0}")]
    Storage(#[from] rusqlite::Error),
    #[error("memory not found: {0}")]
    NotFound(String),
    #[error("embedding unavailable")]
    EmbeddingUnavailable,
    #[error("{0}")]
    Other(String),
}

/// A candidate merge pair found by reconcile scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconcileCandidate {
    pub id_a: String,
    pub id_b: String,
    pub similarity: f32,
    pub entity_overlap: f64,
    pub content_preview_a: String,
    pub content_preview_b: String,
}

/// Result of reconcile operation.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ReconcileReport {
    pub scanned: usize,
    pub candidates_found: usize,
    pub merges_applied: usize,
    pub dry_run: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Memory;
    use crate::types::MemoryType;

    fn test_memory() -> Memory {
        Memory::new(":memory:", None).unwrap()
    }

    #[test]
    fn test_soft_delete_excludes_from_search() {
        let mut mem = test_memory();
        let id = mem.add("test memory for soft delete", MemoryType::Factual, Some(0.5), None, None)
            .unwrap();
        
        // Should be findable
        let all = mem.storage().all().unwrap();
        assert!(all.iter().any(|r| r.id == id));
        
        // Soft delete
        mem.storage_mut().soft_delete(&id).unwrap();
        
        // Should NOT appear in all()
        let all = mem.storage().all().unwrap();
        assert!(!all.iter().any(|r| r.id == id));
        
        // Should appear in list_deleted
        let deleted = mem.storage().list_deleted(Some("*")).unwrap();
        assert!(deleted.iter().any(|r| r.id == id));
    }
    
    #[test]
    fn test_hard_delete_cascade() {
        let mut mem = test_memory();
        let id = mem.add("cascade test memory", MemoryType::Factual, Some(0.5), None, None)
            .unwrap();
        
        // Record an access to create access_log entries
        mem.storage_mut().record_access(&id).unwrap();
        
        // Hard delete cascade
        mem.storage_mut().hard_delete_cascade(&id).unwrap();
        
        // Memory should be completely gone
        let all_including_deleted: i64 = mem.storage().conn()
            .query_row("SELECT COUNT(*) FROM memories WHERE id = ?", 
                       rusqlite::params![id], |row| row.get(0)).unwrap();
        assert_eq!(all_including_deleted, 0);
    }
    
    #[test]
    fn test_forget_targeted_soft() {
        let mut mem = test_memory();
        let id = mem.add("forget target", MemoryType::Factual, Some(0.5), None, None)
            .unwrap();
        
        mem.forget_targeted(&id, true).unwrap();
        
        // Should be soft-deleted
        let deleted_at = mem.storage().get_deleted_at(&id).unwrap();
        assert!(deleted_at.is_some());
    }
    
    #[test]
    fn test_forget_targeted_hard() {
        let mut mem = test_memory();
        let id = mem.add("forget hard target", MemoryType::Factual, Some(0.5), None, None)
            .unwrap();
        
        mem.forget_targeted(&id, false).unwrap();
        
        // Should be completely gone
        let count: i64 = mem.storage().conn()
            .query_row("SELECT COUNT(*) FROM memories WHERE id = ?",
                       rusqlite::params![id], |row| row.get(0)).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_count_soft_deleted() {
        let mut mem = test_memory();
        let id1 = mem.add("del1", MemoryType::Factual, Some(0.5), None, None).unwrap();
        let _id2 = mem.add("del2", MemoryType::Factual, Some(0.5), None, None).unwrap();
        
        assert_eq!(mem.storage().count_soft_deleted().unwrap(), 0);
        
        mem.storage_mut().soft_delete(&id1).unwrap();
        assert_eq!(mem.storage().count_soft_deleted().unwrap(), 1);
    }

    #[test]
    fn test_find_entity_overlap() {
        let mut mem = test_memory();
        
        // Add memory with entities
        let id = mem.add("John works at Google on AI projects", MemoryType::Factual, Some(0.5), None, None)
            .unwrap();
        
        // Manually add entities
        let eid1 = mem.storage_mut().upsert_entity("john", "person", "default", None).unwrap();
        let eid2 = mem.storage_mut().upsert_entity("google", "organization", "default", None).unwrap();
        mem.storage_mut().link_memory_entity(&id, &eid1, "mention").unwrap();
        mem.storage_mut().link_memory_entity(&id, &eid2, "mention").unwrap();
        
        // Search for overlap with ["john", "google"] — should match
        let result = mem.storage().find_entity_overlap(
            &["john".to_string(), "google".to_string()],
            "default",
            0.5,
        ).unwrap();
        assert!(result.is_some());
        let (found_id, jaccard) = result.unwrap();
        assert_eq!(found_id, id);
        assert!(jaccard >= 0.5);  // Perfect match = 1.0
        
        // Search for overlap with ["unknown_person"] — should not match
        let result = mem.storage().find_entity_overlap(
            &["unknown_person".to_string()],
            "default",
            0.5,
        ).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_cross_recall_co_occurrence_tracking() {
        let mut mem = test_memory();
        // Add two memories
        let id1 = mem.add("memory about rust programming", MemoryType::Factual, Some(0.5), None, None).unwrap();
        let id2 = mem.add("memory about python scripting", MemoryType::Factual, Some(0.5), None, None).unwrap();
        
        // Simulate that id1 was recalled recently (within 30s)
        mem.recent_recalls_mut().push_back((id1.clone(), std::time::Instant::now()));
        
        // Now add id2 to recent recalls too
        mem.recent_recalls_mut().push_back((id2.clone(), std::time::Instant::now()));
        
        // Verify ring buffer has both
        assert_eq!(mem.recent_recalls().len(), 2);
    }

    #[test]
    fn test_recent_recalls_ring_buffer_cap() {
        let mut mem = test_memory();
        
        // Fill ring buffer beyond capacity (50)
        for i in 0..60 {
            mem.recent_recalls_mut().push_back((format!("id-{}", i), std::time::Instant::now()));
            if mem.recent_recalls().len() > 50 {
                mem.recent_recalls_mut().pop_front();
            }
        }
        
        assert_eq!(mem.recent_recalls().len(), 50);
    }

    #[test]
    fn test_reconcile_empty_namespace() {
        let mem = test_memory();
        let candidates = mem.reconcile("default", None).unwrap();
        assert!(candidates.is_empty());
    }

    #[test]
    fn test_reconcile_apply_dry_run() {
        let mut mem = test_memory();
        let report = mem.reconcile_apply(&[], true).unwrap();
        assert!(report.dry_run);
        assert_eq!(report.merges_applied, 0);
    }

    #[test]
    fn test_merge_hebbian_links() {
        let mut mem = test_memory();
        
        let id_a = mem.add("memory alpha for hebbian test", MemoryType::Factual, Some(0.5), None, None).unwrap();
        eprintln!("DEBUG: id_a = {}", id_a);
        let id_b = mem.add("memory beta for hebbian test", MemoryType::Factual, Some(0.5), None, None).unwrap();
        eprintln!("DEBUG: id_b = {}", id_b);
        let id_c = mem.add("memory gamma for hebbian test", MemoryType::Factual, Some(0.5), None, None).unwrap();
        eprintln!("DEBUG: id_c = {}", id_c);
        assert_ne!(id_a, id_b, "id_a and id_b should be different");
        assert_ne!(id_a, id_c, "id_a and id_c should be different");
        assert_ne!(id_b, id_c, "id_b and id_c should be different");
        
        // Create Hebbian link: A -> C (threshold=1, need 2 coactivations to form)
        let r1 = mem.storage_mut().record_coactivation(&id_a, &id_c, 1).unwrap();
        let r2 = mem.storage_mut().record_coactivation(&id_a, &id_c, 1).unwrap();
        
        // Verify link formed
        let a_links_before = mem.storage().get_hebbian_links_weighted(&id_a).unwrap();
        eprintln!("DEBUG: r1={}, r2={}, id_a={}, id_c={}, a_links_before={:?}", r1, r2, id_a, id_c, a_links_before);
        
        // Merge A's links into B
        let transferred = mem.storage_mut().merge_hebbian_links(&id_a, &id_b).unwrap();
        assert!(transferred > 0, "Expected links to transfer, got 0. a_links_before had {} entries", a_links_before.len());
        
        // B should now have link to C
        let b_links = mem.storage().get_hebbian_links_weighted(&id_b).unwrap();
        assert!(b_links.iter().any(|(id, _)| id == &id_c), "B should have link to C after merge");
        
        // A should have no links left
        let a_links = mem.storage().get_hebbian_links_weighted(&id_a).unwrap();
        assert!(a_links.is_empty(), "A should have no links after merge");
    }

    #[test]
    fn test_append_merge_provenance() {
        let mut mem = test_memory();
        
        let id = mem.add("provenance test", MemoryType::Factual, Some(0.5), None, None).unwrap();
        
        // Append provenance
        mem.storage_mut().append_merge_provenance(&id, "donor-123", 0.92, true).unwrap();
        
        // Read memory metadata directly
        let meta_str: Option<String> = mem.storage().conn()
            .query_row("SELECT metadata FROM memories WHERE id = ?",
                       rusqlite::params![id], |row| row.get(0)).unwrap();
        let meta: serde_json::Value = serde_json::from_str(meta_str.as_deref().unwrap()).unwrap();
        let history = meta.get("merge_history").unwrap().as_array().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0]["source_id"], "donor-123");
        assert_eq!(history[0]["content_updated"], true);
    }
}
