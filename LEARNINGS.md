# Engram Learnings & Operational Notes

> Shared with TS version. See `../agent-memory-prototype/engram-ts/LEARNINGS.md` for full details.

## Rust-Specific Notes (2026-03-15)

### Schema Incompatibility
- Rust binary expects `namespace` column in `hebbian_links` table (added in Phase 1)
- Existing DBs created by TS/Python version don't have this column
- Binary starts in 13ms but fails on schema init
- **Need**: Migration command or graceful fallback for missing columns

### Priority for Rust CLI
1. Schema migration (ALTER TABLE ADD COLUMN IF NOT EXISTS)
2. `engram reindex` command (re-embed all memories with current provider)
3. Ollama embedding integration (match Python CLI capability)
4. Benchmark: target <50ms for recall with 768d embeddings on 6K memories
