# Issues: engram-ai-rust (engramai)

> 项目使用过程中发现的 bug、改进点和待办事项。
> 格式: ISS-{NNN} [{type}] [{priority}] [{status}]
> 
> 状态值: `open`, `in_progress`, `closed`, `wontfix`, `blocked`
> 
> 相关文档：
> - `INVESTIGATION-2026-03-31.md` — 生产环境问题深度调查
> - `LEARNINGS.md` — 运维笔记 + recall 质量改进方案
> - `MEMORY-SYSTEM-RESEARCH.md` — 与 Hindsight/Mem0/Zep 对比调研
> - `ENGRAM-V2-DESIGN.md` — 情感总线架构设计

---

## 📊 Recall 质量提升总结 (2026-04-09)

按**对 recall 质量的实际影响**排序。

### Tier 1 — 结构性天花板（决定 recall 能做到多好）

| # | Issue | 问题 | 状态 |
|---|-------|------|------|
| 1 | **ISS-009** | Entity 索引完全空置 — schema 有、代码零调用、数据零行。recall 无法做概念级跳转 | P1 open |
| 2 | **ISS-005** | Consolidate 不合成知识 — 只加强 activation，不合成 insight | P1 open |

### Tier 2 — 信噪比（决定 recall 结果有多干净）

| # | Issue | 问题 | 状态 |
|---|-------|------|------|
| 3 | **RustClaw 侧** | EngramStoreHook 无过滤 — heartbeat/NO_REPLY/系统指令全存，垃圾持续写入（上游污染） | 未建 issue |
| 4 | **ISS-003** | add() 无 dedup — ~5% 重复率（短期可忍），高频集群挤占排名 | P2 open |
| 5 | **RustClaw 侧** | Extractor 无 negative examples — Haiku 把系统指令当知识提取 | 未建 issue |

### Tier 3 — 排序精度（决定 top-K 里好结果的位置）

| # | Issue | 问题 | 状态 |
|---|-------|------|------|
| 6 | **ISS-002** | Recency bias 不足 — ACT-R decay 让新旧记忆权重差异不够 | P1 open |
| 7 | **ISS-007** | 无 confidence score — recall 结果无法区分高相关与噪声 | P1 open |
| 8 | **ISS-008** | Knowledge promotion — 高频记忆不自动提升到 SOUL.md/MEMORY.md | P1 open |

### 已解决

| Issue | 问题 | 修复 |
|-------|------|------|
| **ISS-010** | embedding model_id 格式不一致 — 丢失 58% 数据 | ✅ 2026-04-08 |
| **ISS-006** | 单一检索路径 | ✅ hybrid search 已实现 (FTS5 15% + Embedding 60% + ACT-R 25%) |

### 跨 Issue 能力：Supersedes（版本管理）

**问题**：同一事实存了多个版本（如"gid-rs 485 个测试" → "gid-rs 1071 个测试"），recall 不保证返回最新版本。旧的过时信息可能因 ACT-R activation 更高反而排在前面。

**方案**（已在 `docs/engram-hub-discussion.md` 讨论过）：给 Hebbian link 加 `supersedes` 关系类型 —— 检测到新版本知识时自动标记旧版本为被 supersede，recall 时降权或排除。

**这不是独立 issue**，而是 ISS-003 + ISS-005 + ISS-009 三个 issue 都解决后自然具备的能力：
1. **ISS-009 Entity 索引** — 知道两条记忆说的是同一件事（同 entity）
2. **ISS-003 Dedup on write / ISS-005 Consolidate** — 写入时检测或 consolidate 时合成，触发 supersede 标记
3. **ISS-007 Confidence** — Recall 时对 superseded 记忆降权

**参考**: `docs/engram-hub-discussion.md` §Typed Hebbian Links — `supersedes` 类型定义 + confidence 防线 + 自动触发机制（待定）

### 推进策略（2026-04-09 正式启动）

**Phase A — 止血（RustClaw 侧，不改 engramai crate）**

| Task | 描述 | 改动位置 | 规模 | 状态 |
|------|------|----------|------|------|
| **A1** | EngramStoreHook 过滤 — 不存 HEARTBEAT_OK / NO_REPLY / 系统指令 | `rustclaw/src/engram_hooks.rs` | ~30 行 | todo |
| **A2** | Extractor prompt 加 negative examples — 不提取系统指令/agent身份 | `engramai/src/extractor.rs` | ~20 行 | todo |
| **A3** | SQL 一次性清理 — 删精确重复(105条) + 系统指令垃圾 | SQL script | 一次性 | todo |

过滤策略（A1 详细）：
- ❌ `HEARTBEAT_OK` → 直接丢，零信息
- ❌ `NO_REPLY` → 直接丢，零信息
- ❌ 系统指令特征（"你是 RustClaw"、"Read SOUL.md" 等）→ 直接丢
- ⚠️ Heartbeat 有实质内容的响应 → 保留，让 extractor 正常处理

**Phase B — 提质（改 engramai crate，走 ritual）**

| Task | 描述 | ISS | 规模 | 依赖 | 状态 |
|------|------|-----|------|------|------|
| **B1** | Entity 索引实现 — extraction on write + entity storage + entity-aware recall | ISS-009 | ~500-800 行 | A1,A2 | todo |
| **B2** | Dedup on write — embedding similarity check + merge 策略 | ISS-003 | ~200 行 | A3 | todo |
| **B3** | Confidence score — 多信号融合 recall confidence 0-1 | ISS-007 | ~100 行 | B1 | todo |
| **B4** | Recency 调参 — ACT-R decay parameter 分析 + 调整 | ISS-002 | ~20 行 + 分析 | B3 | todo |

**依赖图**：
```
A1 (hook filter) ──┐
A2 (extractor)  ───┼── A3 (SQL cleanup) ──→ B1 (entity) ──→ B3 (confidence)
                   │                   └──→ B2 (dedup)       └──→ B4 (recency)
```

**GID 跟踪**: 所有任务在 RustClaw `.gid/graph.yml` 中管理（engram 项目无独立 graph）

---

## ISS-001 [bug] [P0] [closed]
**标题**: consolidate 命令 SQLite corruption
**发现日期**: 2026-03-29
**发现者**: RustClaw
**组件**: consolidate (SQLite UPDATE operations)
**跨项目引用**: —

**描述**:
`engram consolidate` 命令失败，报错 `database disk image is malformed`。UPDATE 操作触发 SQLite corruption。

**上下文**:
从 2026-03-29 首次发现，持续存在。FTS5 full-text search index 曾重建过一次，但 consolidate 的 UPDATE 操作仍然偶发失败。可能是 WAL mode 下的并发写入问题（RustClaw crate + CLI 同时写 DB），或者 FTS5 index 再次损坏。

**建议方案**:
- 检查是否有并发写入（RustClaw + CLI 同时写 DB）
- consolidate 前做 `PRAGMA integrity_check`
- 如果 FTS5 损坏，自动 rebuild：`INSERT INTO memories_fts(memories_fts) VALUES('rebuild')`
- 考虑 consolidate 用 exclusive lock

**相关**:
- `INVESTIGATION-2026-03-31.md` 有详细分析

---

## ISS-002 [improvement] [P1] [open]
**标题**: Recall recency bias 不足 — ACT-R decay 参数需调整
**发现日期**: 2026-04-05
**发现者**: RustClaw
**组件**: recall (ACT-R activation scoring)
**跨项目引用**: —

**描述**:
Recall 的 recency bias 不足。ACT-R activation 的 decay parameter `d` 可能需要调整，让近期记忆权重更高。当前旧记忆和新记忆在 scoring 上差异不够明显。

**上下文**:
实际使用中，几天前的相关记忆经常排在刚刚存入的记忆后面。对于 agent 场景，recency 应该比学术 ACT-R 模型更重要。

**建议方案**:
- 检查当前 decay parameter d 的值
- 增大 d 值让 recency 权重更高
- 或者在 scoring 中加一个 recency bonus factor

**相关**:
- `LEARNINGS.md` "Recall Quality Improvements" 部分

---

## ISS-003 [improvement] [P2] [closed]
**标题**: add() 缺少 dedup 检查 — 重复记忆导致 DB 膨胀
**发现日期**: 2026-03-31
**更新日期**: 2026-04-09
**发现者**: RustClaw
**组件**: add() / memory extractor
**跨项目引用**: —

**描述**:
`add()` / `add_raw()` 没有任何 dedup 检查。相同或极其相似的内容可以重复存入，导致 DB 膨胀和 recall 结果重复。

**量化调查 (2026-04-09)**:
- 精确重复：105 条（content hash 完全相同）
- 近似重复：217 条（embedding cosine > 0.95）
- 语义集群过度集中：265 条（如 "potato 身份" 268 条同义表述、"heartbeat 指令" 150 条、任务引用 110 条）
- **实际重复率 ~5%**（非之前估计的 20-30%），短期可接受
- 但高频集群在搜索时挤占排名，影响 recall 质量
- **根因**：`add_raw()` 零 dedup 检查，直接写入

**关键设计决策 — 合并而非删除**:
Dedup 时必须**合并元数据**，不能简单删除重复条目。原因：
- ACT-R base-level activation 基于 access 历史。105 条重复各自有独立 access，激活值分散。合并后 1 条继承所有 access → activation 更准确
- Hebbian links 在重复记忆之间形成噪声链接（同义内容共现 ≠ 真正知识关联）。Dedup 后噪声消失 → hebbian 图谱更干净
- **结论：dedup 后 ACT-R 排序和 hebbian 质量都会改善，不会损害**

合并策略：
```
access_count = sum(所有重复的 access)
importance = max(所有重复的 importance)
created_at = min(最早的那条)
hebbian_links = 去重合并（指向非重复记忆的保留）
content = 保留最完整/最新的版本
```

**建议方案**:
- add() 前做 embedding similarity 检查，>0.95 的跳过或合并
- 或者用 content hash 做精确 dedup
- 更好的方案：借鉴 Mem0 的 Reconcile —— 新 fact 与已有记忆对比，决定 ADD / UPDATE / DELETE / NOOP
- extractor 端加 negative examples 避免提取系统指令
- **实现时可与 ISS-009 entity 建设一起做**（建实体时顺便去重）

**相关**:
- `MEMORY-SYSTEM-RESEARCH.md` §1.2 Mem0 Reconcile 阶段
- `INVESTIGATION-2026-03-31.md` 垃圾记忆分析
- ISS-009 (entity 索引) — 可一并实现

---

## ISS-004 [improvement] [P2] [open]
**标题**: 中文分词对 recall 质量的影响 — FTS5 tokenizer 支持有限
**发现日期**: 2026-04-05
**发现者**: RustClaw
**组件**: recall (中文支持)
**跨项目引用**: —

**描述**:
中文分词对 recall 质量有影响。搜"认知层"可能找不到包含"认知"的记忆，因为 FTS5 默认 tokenizer 对中文支持有限。

**上下文**:
Engram 的用户（potato + RustClaw）中英混用。embedding-based recall 缓解了这个问题，但 FTS5 的 keyword recall 部分仍有问题。

**建议方案**:
- 加 jieba 分词或 bigram indexing for FTS5
- 或者更依赖 embedding recall，降低 FTS5 权重
- hybrid scoring 已有 `score_alignment_hybrid()` 可以参考

**相关**:
- IDEA-20260405-01 (Engram 认知层协议)

---

## ISS-005 [missing] [P1] [open]
**标题**: consolidate 缺少知识合成 — 只加强 activation 不合成 insight
**发现日期**: 2026-03-31
**发现者**: RustClaw
**组件**: consolidate
**跨项目引用**: —

**描述**:
当前 consolidate 只是"加强"记忆（增加 activation），而不是"合成新知识"。Hindsight 的 Observation Consolidation 能从多条 facts 合成高阶 insight，这是 engram 最大的功能缺口。

**上下文**:
见 MEMORY-SYSTEM-RESEARCH.md §1.1 Hindsight 分析。真正有价值的 consolidation 应该是：多条相关记忆 → 合成一条新的 insight 记忆，而不只是更新 activation score。

**建议方案**:
- LLM-based consolidation：定期扫描相关记忆簇，让 LLM 合成高阶 insight
- 新 insight 存储时标记 source memories
- 与 Hebbian links 结合：频繁共激活的记忆优先做 consolidation

**相关**:
- `MEMORY-SYSTEM-RESEARCH.md` §1.1 Hindsight
- `ENGRAM-V2-DESIGN.md` 情感总线设计

---

## ISS-006 [missing] [P1] [closed]
**标题**: ~~Recall 缺少多路检索~~ → Hybrid search 已实现
**发现日期**: 2026-03-31
**关闭日期**: 2026-04-09
**发现者**: RustClaw
**组件**: recall (多路检索)
**跨项目引用**: —

**描述**:
~~目前 recall 只有 FTS5 + ACT-R，缺少 embedding 向量搜索和 entity graph 检索。~~

**已解决**: Hybrid search 已在 engramai 中实现，三路融合：
- **FTS5** (15%) — 关键词匹配
- **Embedding vector search** (60%) — 语义相似度（cosine similarity）
- **ACT-R activation** (25%) — 记忆衰减 + 使用频率

截至 2026-04-09，7,058 / 7,972 memories 已有 embeddings（88.5%）。`recall()` 默认走 hybrid 路径，embedding 通道权重最高。

**剩余改进空间（不再是 blocker，降级为 nice-to-have）**:
- Entity graph 检索（第4路）— 见 ISS-009
- RRF (Reciprocal Rank Fusion) 替代当前 weighted sum — 可能改善排序质量
- 权重可配置化（目前硬编码 15/60/25）

**相关**:
- `MEMORY-SYSTEM-RESEARCH.md` §1.1 TEMPR 4路检索
- RustClaw 的 `score_alignment_hybrid()` 是起点

---

## ISS-007 [improvement] [P1] [open]
**标题**: Recall 结果缺少 confidence score — 无法区分高相关与噪声
**发现日期**: 2026-04-05
**发现者**: RustClaw
**组件**: recall (confidence scoring)
**跨项目引用**: rustclaw (auto-recall hook 需要 confidence 做过滤)

**描述**:
Recall 结果没有有意义的 confidence score。难以区分"高度相关匹配"和"模糊相关噪声"。

**上下文**:
下游消费者（RustClaw auto-recall hook）无法做有效过滤，导致低质量记忆被注入 system prompt 浪费 context window。

**建议方案**:
- 从多个信号计算 confidence：embedding similarity, ACT-R activation, recency, keyword overlap
- 归一化到 0.0-1.0
- 返回每个 recall 结果的 confidence，让消费者按阈值过滤

**相关**:
- `LEARNINGS.md` "Confidence Score Calculation" 部分

---

## ISS-008 [feature] [P1] [open]
**标题**: Knowledge promotion — 高频记忆自动提升到上层文档
**发现日期**: 2026-04-07
**发现者**: potato
**组件**: consolidate → knowledge promotion
**跨项目引用**: rustclaw (engram_soul_suggestions 工具是早期原型)

**描述**:
当某类记忆反复出现并被 consolidate 强化后，应该自动提取总结，提升（promote）到上层文档（如 SOUL.md、MEMORY.md、AGENTS.md 等）。当前 consolidate 只增加 activation，不会产生"这个 pattern 已经稳定到可以写进 SOUL"的判断。

**上下文**:
例如 potato 多次强调"root fix, not patch"、"不要简化问题"、"第一性原理"—— 这些分散在几十条 engram 记忆里，但直到 potato 手动要求，才被写进 SOUL.md。理想情况下，engram 应该能检测到"这个原则已经被重复提到 N 次，activation 超过阈值，应该 promote 到 SOUL/MEMORY"。

**设计思路**:
- **检测层**：consolidate 时扫描高 activation 记忆簇（同主题、高 Hebbian link density）
- **提取层**：对簇内记忆做 LLM 总结，生成一条 crystallized principle/fact
- **推荐层**：不自动改文件 — 生成 promotion suggestion（"建议把以下原则写入 SOUL.md：..."），等人类 approve
- **去重层**：检查目标文档是否已有类似内容，避免重复写入
- **阈值**：activation > X 且 cluster size > N 且 time span > T（不是一天内重复说的，而是跨天持续出现的）

**与现有 ISS 的关系**:
- ISS-005（consolidate 合成新知识）是基础 — 先能合成 insight，才能 promote
- ISS-003（dedup）保证不会因为重复存入而人为抬高 activation
- ISS-007（confidence scoring）提供 promotion 决策的信号之一

**建议方案**:
1. 在 consolidate 流程末尾加 promotion check（高 activation 簇 → 生成 suggestion）
2. suggestion 输出到 `engram suggestions` 命令（或 `engram soul-suggestions`）
3. Agent 在 heartbeat 时检查 suggestions，向用户汇报
4. 用户 approve 后，agent 写入目标文档 + 标记相关记忆为 "promoted"

**相关**:
- ISS-005 (consolidate 合成新知识)
- ISS-003 (dedup)
- RustClaw 的 `engram_soul_suggestions` 工具是这个思路的早期原型
- 今天 SOUL.md 加 Engineering Philosophy 就是手动版的 promotion

---

## ISS-009 [feature] [P1] [open]
**标题**: Entity 索引 — schema 已建但完全空置，需实现写入+检索
**发现日期**: 2026-04-08
**更新日期**: 2026-04-09
**发现者**: RustClaw + potato
**组件**: entities, memory_entities, entity_relations (schema), write path, recall path
**跨项目引用**: rustclaw (agent 侧 recall 策略), gidhub (触发场景涉及 gid infer)

**触发场景**:
potato 问"我们之前讨论过 gid infer 吗？" — RustClaw 搜 `"gid infer"` 找不到，但 gidhub requirements（含相关概念）实际存在于 engram 中。问题是 "infer" 和 "gidhub requirements" 在人脑中关联，但 engram 中是孤立的 embedding 向量。

**问题本质 (2026-04-09 调查更新)**:

Schema 完备但**零数据、零代码调用**：
- `entities` 表：0 行（应有项目名、概念、人名等）
- `memory_entities` 表：0 行（应有 memory↔entity 关联）
- `entity_relations` 表：0 行（应有 entity↔entity 关系）
- **代码中零处 `INSERT INTO entities`** — 这三张表是死代码
- 唯一的关联机制是 `hebbian_links`（34,859 行），但这是 memory↔memory 级别，无法做概念级跳转

**这是 recall 质量的天花板。** 当前 engram 是纯向量搜索 + 统计共现的系统，缺少知识图谱层。Entity 索引就是那个缺失的层。

**影响分析**:
- 搜 "infer" → 只能命中包含这个词的记忆
- 无法做 "infer" → gid-rs 功能 → gid-rs 还有哪些相关记忆？这种概念级跳转
- 7300+ 条记忆之间的关联全靠 embedding 相似度 + hebbian 共激活，没有结构化实体图谱

**需要实现的三层**:

### 1. Entity Extraction on Write（写入时提取）
- `add()` / `add_raw()` 写入记忆时，自动提取实体（项目名、概念、人名、工具名等）
- 写入 `entities` 表（去重 upsert）
- 写入 `memory_entities` 关联表
- 提取方式：LLM extraction 或 regex/NER 混合

### 2. Entity Relation Building（实体关系构建）
- 同一条记忆中共现的实体 → 自动建立 entity↔entity 关系
- 关系类型：has_feature, part_of, related_to, created_by 等
- 写入 `entity_relations` 表
- 可与 Hebbian 机制协同：高频共现的实体对加强关系权重

### 3. Entity-Aware Recall（实体感知检索）
- Recall 时先匹配 entity → 再找 entity 关联的记忆
- 多跳：query → entity → related entities → memories
- 作为第 4 路检索通道，融入现有 hybrid search（FTS5 15% + Embedding 60% + ACT-R 25% + Entity ?%）

**相关 issue**:
- ISS-003 (dedup) — 可一并实现：建实体时顺便去重
- ISS-005 (consolidate 知识合成) — entity 图谱为 consolidation 提供结构化输入
- ISS-007 (confidence scoring) — entity match 可作为 confidence 信号之一
- ISS-008 (knowledge promotion) — entity 高频出现可触发 promotion

---

## ISS-010 [bug] [P0] [closed]
**标题**: Recall 丢失 58% 数据 — embedding model_id 格式不一致
**发现日期**: 2026-04-08
**修复日期**: 2026-04-08
**发现者**: potato
**组件**: storage (embedding model_id format)
**跨项目引用**: —

**描述**:
生产数据库中 embedding 的 `model` 字段存在两种格式：
- **旧格式** (3,742 条): `nomic-embed-text` — v0.2.0 时用 `config.model` 直接写入
- **新格式** (2,710 条): `ollama/nomic-embed-text` — 后来改为 `config.embedding.model_id()` 返回带 provider 前缀

recall 查询按当前 model_id (`ollama/nomic-embed-text`) 过滤，导致 3,742 条旧数据全部被跳过，**58% 的 embedding 数据对 recall 不可见**。

**根因**:
1. v0.2.0 → v0.2.1 改了 model_id 生成逻辑（加了 provider 前缀），但未迁移已有数据
2. v1→v2 schema migration 中有修复逻辑，但生产 DB 已经是 v2，该迁移不会再执行

**修复方案（两步 root fix）**:

### 1. 数据修复
```sql
UPDATE memory_embeddings SET model = 'ollama/nomic-embed-text' WHERE model = 'nomic-embed-text';
-- 3,742 rows updated
```

### 2. 代码防御
在 storage 层新增 `normalize_model_id()` 函数，对 5 个关键函数（写入/读取/删除）的 model 参数自动规范化：
- 无论调用方传入 `nomic-embed-text` 还是 `ollama/nomic-embed-text`，都统一为带 provider 前缀的格式
- 防止未来任何代码路径再写入裸模型名

**验证**: 测试通过，生产数据已全部统一为 `ollama/nomic-embed-text`。

**相关**:
- ISS-009 (recall 质量低) — 本 bug 是 recall 丢数据的直接原因之一

---
