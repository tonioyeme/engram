# Multi-Signal Hebbian Link Formation

> From passive co-recall to proactive association discovery.

## Status: Design Spec (Draft)

## 1. Problem

现在 Hebbian 链接的形成只有**一个信号**：co-recall（两条记忆被同一次 recall 返回）。

```
记忆写入 → 等待 → 某次 recall 恰好同时返回 A 和 B → coactivation_count++
→ count >= threshold (default 3) → strength = 1.0 → 链接形成
```

问题：

1. **太被动** — 关联的形成完全依赖于"碰巧被一起查出来"。如果两条记忆语义相近但从未在同一个 query 中出现，它们永远不会建链。
2. **建链慢** — 需要 3 次 co-recall 才形成链接。一条记忆可能几周都不被 recall，更别说和特定另一条一起被 recall 3 次。
3. **信号浪费** — `cluster.rs` 的 synthesis 阶段已经计算了 4 个信号（Hebbian权重、entity Jaccard、embedding cosine、时间接近度），但这些只用来做一次性聚类，没有回馈给 Hebbian 系统。
4. **新记忆孤岛** — 新写入的记忆完全没有链接，需要经过多次 recall 才可能被关联到，这意味着新记忆的可发现性最差。

## 2. 现有代码基础

### 2.1 当前 Hebbian 实现 (`storage.rs`)

```sql
CREATE TABLE hebbian_links (
    source_id TEXT,
    target_id TEXT,
    strength REAL DEFAULT 0.0,
    coactivation_count INTEGER DEFAULT 0,
    created_at REAL,
    namespace TEXT DEFAULT 'default'
);
```

- `record_coactivation(id1, id2, threshold)` — co-recall 时调用，count++ 直到 threshold 再形成链接
- `decay_hebbian_links(factor)` — 全局衰减，strength < 0.1 的删掉
- `get_hebbian_links_weighted(memory_id)` — 获取一条记忆的所有链接和权重

### 2.2 已有的信号计算能力

**Entity overlap** — `entities.rs` 已经对每条记忆做实体抽取（Aho-Corasick + regex），存入 `memory_entities` 表。`cluster.rs` 计算 Jaccard index。

**Embedding similarity** — `hybrid_search.rs` 和 recall 都在用。每条记忆写入时已经生成 embedding，存在 `memory_embeddings` 表。

**Temporal proximity** — `cluster.rs` 用 `temporal_adjacency()` 计算时间接近度，指数衰减。

**Co-recall** — 现有 Hebbian 系统（唯一的建链信号）。

### 2.3 Recall 的 6 通道融合 (`memory.rs`)

recall 已经实现了 6 通道加权融合：FTS、embedding、ACT-R、entity、temporal、Hebbian。Hebbian channel 在 recall 时给 well-connected 的候选加分。

**关键洞察**：recall 已经能消费 Hebbian 链接。但链接的**产生**只靠 co-recall。这是瓶颈。

## 3. 设计：Multi-Signal Link Formation

### 3.1 核心思路

在记忆**写入时**（`add_raw()`），主动用多信号评估新记忆与现有记忆的关联度，直接建立 Hebbian 链接。不再被动等 co-recall。

### 3.2 Schema 变更

`hebbian_links` 表新增字段，记录链接的来源信号：

```sql
ALTER TABLE hebbian_links ADD COLUMN signal_source TEXT DEFAULT 'corecall';
-- 'corecall' | 'entity' | 'embedding' | 'temporal' | 'multi'

ALTER TABLE hebbian_links ADD COLUMN signal_detail TEXT DEFAULT NULL;
-- JSON: {"entity_jaccard": 0.4, "embedding_cos": 0.7, "temporal_hours": 2.1}
```

### 3.3 写入时关联发现（Write-time Association Discovery）

新记忆 M_new 写入后，执行以下步骤：

```
fn discover_associations(m_new: &MemoryRecord, config: &AssociationConfig) -> Vec<ProtoLink> {
    let candidates = get_candidate_memories(m_new, config.candidate_limit); // 最多 50 条
    let mut proto_links = Vec::new();

    for m_existing in candidates {
        let scores = SignalScores {
            entity_overlap:     entity_jaccard(m_new, m_existing),
            embedding_cosine:   embedding_similarity(m_new, m_existing),
            temporal_proximity: temporal_score(m_new, m_existing),
        };

        let combined = config.w_entity    * scores.entity_overlap
                     + config.w_embedding * scores.embedding_cosine
                     + config.w_temporal  * scores.temporal_proximity;

        if combined >= config.link_threshold {
            proto_links.push(ProtoLink {
                target_id: m_existing.id,
                strength: combined,
                signal_source: determine_source(&scores),
                signal_detail: scores.to_json(),
            });
        }
    }

    // 只保留 top-K 最强的链接，避免链接爆炸
    proto_links.sort_by(|a, b| b.strength.partial_cmp(&a.strength).unwrap());
    proto_links.truncate(config.max_links_per_memory);
    proto_links
}
```

### 3.4 候选选择策略（Candidate Selection）

不可能和所有记忆算相似度（N 太大了）。分层筛选：

```
第一层：时间窗口（最近 7 天的记忆） → O(1) SQL query
第二层：FTS 命中（用 m_new 的 key entities 做 FTS query）→ 快
第三层：同 namespace 的所有记忆中 embedding top-K → 需要向量搜索

合并去重 → 最多 50 候选
```

### 3.5 信号权重配置

```rust
pub struct AssociationConfig {
    /// 是否启用写入时关联发现
    pub enabled: bool,

    /// 信号权重（归一化后使用）
    pub w_entity: f64,     // default 0.3
    pub w_embedding: f64,  // default 0.5
    pub w_temporal: f64,   // default 0.2

    /// 链接形成阈值（combined score >= threshold 才建链）
    pub link_threshold: f64,  // default 0.4

    /// 每条新记忆最多建立多少条链接
    pub max_links_per_memory: usize,  // default 5

    /// 候选记忆的最大数量
    pub candidate_limit: usize,  // default 50

    /// 写入时发现的链接的初始 strength（低于 co-recall 形成的）
    pub initial_strength: f64,  // default 0.5 (co-recall 形成的是 1.0)
}
```

### 3.6 与现有 co-recall Hebbian 的关系

**不替代，叠加。**

- 写入时发现的链接：initial_strength = 0.5，signal_source = 'entity'/'embedding'/'temporal'/'multi'
- co-recall 形成的链接：strength = 1.0，signal_source = 'corecall'
- 如果一对记忆已经有写入时建的链接，后来又被 co-recall → strength 增加到 max(existing + 0.1, 1.0)
- co-recall 仍然是最强的信号——两条记忆被人类实际在同一个上下文中使用，这是最可靠的关联证据

**Strength 语义：**
```
0.0       — tracking phase (co-recall count < threshold, 还没形成链接)
0.3 - 0.5 — write-time discovered (单信号或弱 multi-signal)
0.5 - 0.8 — write-time discovered (强 multi-signal)
0.8 - 1.0 — co-recall confirmed (或 write-time + co-recall 叠加)
```

### 3.7 衰减策略调整

现有衰减：`strength *= decay_factor`，对所有链接一视同仁。

改进：根据 signal_source 差异化衰减：
```
co-recall 链接:    decay *= 0.95 (慢衰减，经过实际使用验证的)
multi-signal 链接: decay *= 0.90 (中等衰减)
单信号链接:        decay *= 0.85 (快衰减，只靠一个信号的置信度低)
```

### 3.8 性能考量

**写入时开销：**
- Entity extraction: 已有，~0.1ms
- Embedding generation: 已有（写入时已经做了），~50ms
- Candidate selection: FTS query + top-K embedding scan, ~5-20ms
- 信号计算: 50 candidates × 3 signals, ~2ms
- 总额外开销: ~10-25ms per memory write（embedding 已有的情况下）

**链接数量增长：**
- 每条记忆最多 5 个新链接 → 1000 条记忆 = 最多 5000 条链接
- 加上衰减清理，实际活跃链接数 << 5000
- SQLite 完全扛得住

## 4. 实现步骤

### Phase 1: Schema + Config (小改动)
1. `hebbian_links` 表加 `signal_source` 和 `signal_detail` 列
2. `EngineConfig` 加 `AssociationConfig` 结构
3. 迁移脚本处理旧数据（所有现有链接标记为 `corecall`）

### Phase 2: 候选选择 (新函数)
1. `fn get_association_candidates(m_new, config) -> Vec<MemoryRecord>`
2. 三层筛选：temporal window + FTS + embedding top-K
3. 合并去重

### Phase 3: 信号计算 (复用已有代码)
1. Entity Jaccard — 直接复用 `cluster.rs` 的 `entity_overlap_matrix()` 逻辑
2. Embedding cosine — 复用 `EmbeddingProvider::cosine_similarity()`
3. Temporal proximity — 复用 `cluster.rs` 的 `temporal_adjacency()`

### Phase 4: 链接写入 (扩展 record_coactivation)
1. 新函数 `record_association(id1, id2, strength, signal_source, signal_detail)`
2. 与现有 co-recall 链接合并逻辑
3. 在 `add_raw()` 末尾调用 `discover_associations()`

### Phase 5: 衰减调整
1. `decay_hebbian_links()` 根据 `signal_source` 使用不同衰减率

## 5. 开放问题

1. **embedding 计算在写入时是否已经可用？** — 看 `add_raw()` 是否在存完 embedding 之后还能拿到。如果写入和 embedding 是异步的，需要调整时序。

2. **是否需要后台批量关联发现？** — 写入时做是最自然的，但对于历史记忆（写入时没有这个功能的），需要一个批量回填任务。可以在 `consolidate()` 里加。

3. **跨 namespace 的写入时关联？** — 目前 co-recall 的跨 namespace 链接已有（`record_cross_namespace_coactivation`）。写入时关联是否也要跨 namespace？建议先同 namespace，后续再扩展。

4. **LLM 信号？** — 现在的 3 个信号都是零 LLM 的（entity、embedding、temporal）。将来可以加一个 LLM 信号："给定 A 和 B，它们是否有因果/主题/方法论上的关联？"但这会把写入延迟从 ~25ms 拉到 ~2s，需要异步。

---

*Created: 2026-04-18*
*Author: RustClaw + potato*
