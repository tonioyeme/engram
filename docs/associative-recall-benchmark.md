# Engram Associative Recall Benchmark

> 证明 Hebbian 自动建链创造了新能力，而不仅仅是微调精度。

## Status: Benchmark Spec (Draft)

## 1. 核心命题

> **"Multi-signal Hebbian 让系统能回答一类其他系统根本回答不了的问题。"**

不是测 recall 准不准（那是 precision@K 的事），而是测**关联召回能力**——答案分散在多条记忆中，必须通过关联链才能找全。

### 什么是 Associative Recall

传统 recall 测试：
```
写入: "Rust 比 Python 快 10 倍"
Query: "Rust 和 Python 速度对比"
期望: 返回上面那条 ✓
```

这是 **direct recall**，embedding search 就能做。

Associative recall 测试：
```
写入:
  M1: "potato 喜欢用 Rust 写高性能系统"
  M2: "potato 的交易系统需要亚毫秒延迟"
  M3: "potato 上个月用 Rust 重写了之前的 Python 交易引擎"

Query: "为什么 potato 用 Rust 做交易？"

期望: 返回 M1 + M2 + M3（三条都需要才能完整回答）
```

单条记忆都不能完整回答这个 query。需要系统**关联** M1（Rust 偏好）+ M2（延迟需求）+ M3（实际行动）。

## 2. 指标定义

### 2.1 Primary Metric: Associative Recall@K (AR@K)

对于每个测试 case：
- 标注一组**必要记忆集** R = {M_1, M_2, ..., M_n}（回答 query 需要的所有记忆）
- 系统返回 top-K 结果集 S
- AR@K = |R ∩ S| / |R|

**AR@K = 1.0** 意味着所有必要记忆都被召回了。

### 2.2 Secondary Metrics

**Mean AR@K** — 所有测试 case 的 AR@K 均值

**Full Recall Rate** — AR@K = 1.0 的 case 占总 case 的比例（最严格）

**Hop Distance** — 关联链中最远的两条必要记忆之间的语义距离（embedding cosine distance）。距离越大，关联越难发现。

**Discovery Latency** — 从记忆写入到链接形成的时间。co-recall only 可能需要几周，multi-signal 应该是 write-time 即建链。

## 3. 测试集设计

### 3.1 难度分层

#### Level 1: Direct Association（2 条记忆，高语义重叠）

记忆之间有明显的实体或主题共享。即使没有 Hebbian，embedding search 也有一定概率找到。

这一层用于验证 Hebbian **至少不损害**基本 recall。

```yaml
- name: "direct-entity-overlap"
  memories:
    - id: L1-01-A
      content: "engram 使用 ACT-R 模型计算记忆激活度"
      type: factual
    - id: L1-01-B
      content: "ACT-R 的 base-level activation 会随时间对数衰减"
      type: factual
  query: "engram 的记忆激活度怎么衰减的？"
  required_memories: [L1-01-A, L1-01-B]
  shared_entities: ["ACT-R", "engram"]
  expected_difficulty: easy
```

**这一层预期结果：**
| 系统 | AR@5 |
|------|------|
| embedding only | ~0.7 |
| engram (co-recall Hebbian) | ~0.75 |
| engram (multi-signal Hebbian) | ~0.85 |

#### Level 2: Indirect Association（3 条记忆，链式关联）

A→B→C 链式关联。A 和 C 之间**没有直接的实体或语义共享**，但 A-B 有关联，B-C 有关联。

这是 Hebbian 开始发挥独特价值的层级。

```yaml
- name: "chain-association"
  memories:
    - id: L2-01-A
      content: "RustClaw 使用 engram 作为认知记忆层"
      type: factual
    - id: L2-01-B
      content: "engram 的 Hebbian learning 模块负责建立记忆间的关联链接"
      type: factual
    - id: L2-01-C
      content: "Hebbian learning 的核心原理来自神经科学：neurons that fire together wire together"
      type: factual
  query: "RustClaw 的记忆系统和神经科学有什么关系？"
  required_memories: [L2-01-A, L2-01-B, L2-01-C]
  chain: "RustClaw → engram → Hebbian → neuroscience"
  expected_difficulty: medium
```

**这一层预期结果：**
| 系统 | AR@5 |
|------|------|
| embedding only | ~0.4 |
| engram (co-recall Hebbian) | ~0.5 |
| engram (multi-signal Hebbian) | ~0.75 |

#### Level 3: Cross-Domain Association（3-5 条记忆，跨域）

记忆分布在不同主题域（技术 vs 商业 vs 个人），但存在非显式关系。

这是最有价值的测试——"创造性联想"能力。

```yaml
- name: "cross-domain-insight"
  memories:
    - id: L3-01-A
      content: "Infomap 社区检测算法可以在加权图上发现模块结构"
      type: factual
      domain: tech/algorithms
    - id: L3-01-B
      content: "xinfluencer 需要发现 Twitter 上的影响力社区和关键节点"
      type: factual
      domain: product/social
    - id: L3-01-C
      content: "gid-core 已经有完整的 Infomap 实现，支持 4700 行的加权网络聚类"
      type: factual
      domain: tech/codebase
  query: "有什么现成的技术可以用来做 xinfluencer 的社区发现？"
  required_memories: [L3-01-A, L3-01-B, L3-01-C]
  cross_domains: ["tech/algorithms", "product/social", "tech/codebase"]
  expected_difficulty: hard
```

**这一层预期结果：**
| 系统 | AR@5 |
|------|------|
| embedding only | ~0.2 |
| engram (co-recall Hebbian) | ~0.3 |
| engram (multi-signal Hebbian) | ~0.55 |

#### Level 4: Temporal Span Association（时间跨度 > 30 天）

两条相关记忆写入时间间隔很长。co-recall Hebbian 几乎不可能在这种跨度下建链（需要碰巧在 30 天后的某次 query 中同时召回两条旧记忆）。

```yaml
- name: "temporal-span"
  memories:
    - id: L4-01-A
      content: "3月初决定用 Rust 重写 engram，替换 TypeScript 版本"
      type: episodic
      created_at: "2026-03-01"
    - id: L4-01-B
      content: "4月中旬 engram Rust 版发布 v0.2.2 到 crates.io，性能提升 50 倍"
      type: episodic
      created_at: "2026-04-15"
  query: "engram 从决定重写到发布用了多久？"
  required_memories: [L4-01-A, L4-01-B]
  time_gap_days: 45
  expected_difficulty: hard
```

**这一层预期结果：**
| 系统 | AR@5 |
|------|------|
| embedding only | ~0.3 |
| engram (co-recall Hebbian) | ~0.35 |
| engram (multi-signal Hebbian) | ~0.6 |

#### Level 5: Negative Association（不应关联的记忆）

验证系统不会过度关联。两条记忆表面上有实体重叠，但实际不相关。

```yaml
- name: "false-positive-check"
  memories:
    - id: L5-01-A
      content: "Apple 公司 2026Q1 财报显示服务收入创新高"
      type: factual
    - id: L5-01-B
      content: "苹果是维生素C含量较高的水果之一"
      type: factual
  query: "Apple 的财务表现怎么样？"
  required_memories: [L5-01-A]
  should_not_recall: [L5-01-B]
  expected_difficulty: easy
```

### 3.2 测试集规模

| Level | Cases 数量 | 每 case 记忆数 | 总记忆数 |
|-------|-----------|---------------|---------|
| L1 Direct | 20 | 2 | 40 |
| L2 Indirect | 20 | 3 | 60 |
| L3 Cross-Domain | 15 | 3-5 | 60 |
| L4 Temporal | 10 | 2-3 | 25 |
| L5 Negative | 10 | 2 | 20 |
| **Total** | **75** | | **~205** |

另加 **200 条干扰记忆（noise）**——和测试 case 无关的真实记忆，增加检索难度。

总数据集：~405 条记忆，75 个测试 query。

## 4. 对照组设计

### 4.1 系统配置

| 系统 | 描述 | 配置 |
|------|------|------|
| **Baseline: embedding-only** | 纯向量检索 | hebbian_enabled=false, entity=false, actr=false |
| **Baseline: embedding + FTS** | 混合文本检索 | hebbian_enabled=false, entity=false |
| **Control: engram-current** | 当前 engram（co-recall Hebbian）| 默认配置，标准 co-recall 建链 |
| **Treatment: engram-multi** | Multi-signal Hebbian | 写入时关联发现 + co-recall |

### 4.2 预热策略

为了让 co-recall Hebbian 有公平的机会建链，需要预热：

```
1. 写入所有记忆
2. 执行 100 轮随机 recall（模拟正常使用）
3. 等待链接自然形成
4. 然后跑测试 query
```

Multi-signal Hebbian 在写入时就建链了，不需要预热。这个差异本身就是一个重要结果：**Discovery Latency**。

### 4.3 不同 K 值

测试 K = 3, 5, 10, 20。

AR@5 是主指标（Agent 通常给 LLM 提供 5-10 条记忆作为上下文）。

## 5. 实现计划

### 5.1 测试框架

```rust
// tests/bench_associative_recall.rs

struct TestCase {
    name: String,
    level: Level,
    memories: Vec<TestMemory>,
    query: String,
    required_memory_ids: Vec<String>,
    should_not_recall: Vec<String>,
    metadata: CaseMetadata,
}

struct BenchResult {
    system: String,
    case_name: String,
    level: Level,
    k: usize,
    ar_at_k: f64,           // |required ∩ returned| / |required|
    full_recall: bool,       // ar_at_k == 1.0
    false_positive_count: i32,
    recall_latency_ms: f64,
}
```

### 5.2 测试数据格式

YAML 文件存在 `tests/fixtures/associative_recall/`:

```
tests/fixtures/associative_recall/
├── level1_direct.yaml
├── level2_indirect.yaml
├── level3_cross_domain.yaml
├── level4_temporal.yaml
├── level5_negative.yaml
└── noise_memories.yaml
```

### 5.3 执行流程

```
for each system_config in [baseline, control, treatment]:
    1. 创建干净的内存数据库
    2. 写入所有记忆（test cases + noise）
    3. 如果是 co-recall 系统，执行预热 recall 循环
    4. for each test_case:
         for each k in [3, 5, 10, 20]:
           results = system.recall(test_case.query, k)
           ar = compute_ar_at_k(results, test_case.required)
           fp = count_false_positives(results, test_case.should_not)
           record(system, case, k, ar, fp)
    5. 输出报告
```

### 5.4 报告格式

```
╔══════════════════════════════════════════════════════════════╗
║              Associative Recall Benchmark Report            ║
╠══════════════════════════════════════════════════════════════╣
║ System              │ L1    │ L2    │ L3    │ L4    │ L5 FP ║
║ embedding-only      │ 0.65  │ 0.20  │ 0.10  │ 0.15  │ 0.3  ║
║ embedding+FTS       │ 0.70  │ 0.25  │ 0.12  │ 0.18  │ 0.2  ║
║ engram-current      │ 0.75  │ 0.35  │ 0.20  │ 0.22  │ 0.1  ║
║ engram-multi        │ 0.88  │ 0.60  │ 0.45  │ 0.55  │ 0.1  ║
╠══════════════════════════════════════════════════════════════╣
║ Key Finding: Multi-signal Hebbian delivers 2-3x improvement ║
║ in indirect/cross-domain recall (L2-L4), the scenarios      ║
║ where embedding-only systems fundamentally cannot reach.     ║
╚══════════════════════════════════════════════════════════════╝
```

## 6. 预期结果及论证逻辑

### 6.1 如果结果符合预期

L1 改善小（+10-15%），L2-L4 改善大（+100-200%）。

**论证**：
> "Hebbian 自动建链不是在所有场景下都显著提升 recall。它的价值集中在 indirect/cross-domain/temporal-span 这类关联推理场景——而这些恰恰是 LLM agent 最需要的能力：跨对话、跨主题、跨时间的知识关联。在这些场景下，从 20% 到 55% 的 AR@5 提升代表的是能力边界的扩展，不是精度的微调。"

### 6.2 如果结果不符合预期

可能原因：
- **L1 没有改善** — 正常，embedding 在直接关联上已经很强
- **L2-L3 改善不大** — 可能是信号权重需要调优，或者 link_threshold 太高
- **L5 误报增加** — 过度关联，需要提高阈值或加 negative signal
- **全面没有改善** — 说明 Hebbian 链接在 recall 的 6 通道融合中权重太低，被其他信号淹没了。需要调 hebbian_recall_weight。

### 6.3 关键洞察

benchmark 的目的不仅是证明"更好"，还要找到**阈值和边界**：

- 什么 link_threshold 值给出最好的 precision/recall 平衡？
- 多少条链接per memory 是最优的（太少不够，太多噪音）？
- 哪些 signal 组合最有效（entity alone vs embedding alone vs combined）？
- 链接衰减速率对长期 AR@K 的影响？

## 7. 与竞品的对标

如果 benchmark 设计足够好，可以把其他系统也拉进来测：

- **Mem0** — 纯 embedding recall，无 Hebbian
- **Zep** — 有 temporal awareness 但无自动关联
- **LangChain Memory** — conversation buffer，无关联发现
- **Cognee** — 有知识图谱但不同的关联机制

这不是为了 marketing，是为了验证 engram 的方法论是否在结构上更优。

## 8. 开放问题

1. **测试数据的真实性** — 合成数据 vs 真实记忆数据。合成数据可以精确控制变量，但可能不代表真实使用模式。建议：先用合成数据验证框架，再用 potato 的真实 engram 数据跑一轮。

2. **预热公平性** — 100 轮随机 recall 是否足够让 co-recall Hebbian 建链？可能需要分析建链概率来决定。如果 100 轮还不够，说明 co-recall 建链的实际效率比理论更差——这本身也是一个发现。

3. **K 值选择** — Agent 实际使用时给 LLM 几条记忆？RustClaw 目前用的是 limit=5-10。benchmark 应该以实际值为主指标。

4. **增量 vs 批量** — benchmark 应该测两种场景：(a) 所有记忆一次性写入后测试，(b) 记忆逐渐写入并穿插 recall（更真实）。

---

*Created: 2026-04-18*
*Author: RustClaw + potato*
