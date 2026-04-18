#!/bin/bash
# KC (Knowledge Compiler) End-to-End Test
# Tests the full pipeline: store memories → compile → query → inspect → health
#
# Usage: ./test-kc-e2e.sh

set -e

ENGRAM="./target/release/engram"
TEST_DB="/tmp/test-kc-e2e.db"

# Cleanup
rm -f "$TEST_DB"

echo "════════════════════════════════════════════════════════════════"
echo " KC End-to-End Test"
echo " DB: $TEST_DB"
echo "════════════════════════════════════════════════════════════════"
echo ""

# ── Step 1: Seed fragmented memories ──────────────────────────────────────────
echo "📝 Step 1: Seeding test memories..."
echo ""

# Cluster 1: Rust programming (5 memories)
$ENGRAM --database "$TEST_DB" store --type factual --importance 0.8 \
  "Rust的所有权系统是它最独特的特性。每个值有且只有一个所有者，当所有者离开作用域时值被自动drop。这消除了内存泄漏和悬垂指针。"
$ENGRAM --database "$TEST_DB" store --type factual --importance 0.7 \
  "Rust的借用检查器在编译时强制执行引用规则：你可以有多个不可变引用，或者一个可变引用，但不能同时拥有两者。这防止了数据竞争。"
$ENGRAM --database "$TEST_DB" store --type factual --importance 0.6 \
  "Rust的trait系统类似于其他语言的接口，但更强大。可以为任何类型实现trait，包括标准库类型（通过newtype pattern）。trait可以有默认实现。"
$ENGRAM --database "$TEST_DB" store --type procedural --importance 0.7 \
  "在Rust中处理错误的惯用方式是使用Result<T, E>和?运算符。自定义错误类型应该实现std::error::Error trait。thiserror crate可以简化这个过程。"
$ENGRAM --database "$TEST_DB" store --type factual --importance 0.5 \
  "Rust的生命周期标注帮助编译器理解引用之间的关系。大多数情况下编译器可以自动推断，但函数签名中有多个引用参数时需要显式标注。"

# Cluster 2: AI/LLM development (5 memories)
$ENGRAM --database "$TEST_DB" store --type factual --importance 0.9 \
  "GPT-4和Claude等大语言模型使用transformer架构。关键创新是self-attention机制，让模型能关注输入序列的不同部分。"
$ENGRAM --database "$TEST_DB" store --type factual --importance 0.8 \
  "RAG (Retrieval Augmented Generation) 通过在生成前检索相关文档来增强LLM的能力。这解决了知识截止日期问题和幻觉问题。"
$ENGRAM --database "$TEST_DB" store --type procedural --importance 0.7 \
  "构建AI agent的关键是tool use + memory + planning。agent需要能调用外部工具、记住上下文、并制定多步骤计划来完成复杂任务。"
$ENGRAM --database "$TEST_DB" store --type factual --importance 0.6 \
  "向量数据库（如Pinecone、Milvus、Qdrant）存储embedding向量并支持相似性搜索。这是RAG系统的核心组件。"
$ENGRAM --database "$TEST_DB" store --type factual --importance 0.7 \
  "Prompt engineering的核心技巧包括：few-shot examples、chain-of-thought、system prompt设计、temperature调优。好的prompt结构=角色+约束+任务+格式。"

# Cluster 3: 交易/投资 (4 memories)
$ENGRAM --database "$TEST_DB" store --type factual --importance 0.8 \
  "均值回归策略假设价格会回到长期平均水平。当资产大幅偏离均值时，买入（低于均值）或卖出（高于均值）。需要识别趋势vs震荡市场。"
$ENGRAM --database "$TEST_DB" store --type factual --importance 0.7 \
  "Kelly公式用于确定最优仓位大小：f* = (bp - q) / b，其中b是赔率，p是胜率，q=1-p。实际使用时通常用half-Kelly来降低风险。"
$ENGRAM --database "$TEST_DB" store --type procedural --importance 0.9 \
  "回测系统的关键要素：避免前视偏差（look-ahead bias）、使用真实滑点和手续费、样本外测试、walk-forward分析。没有这些，回测结果毫无意义。"
$ENGRAM --database "$TEST_DB" store --type factual --importance 0.6 \
  "因子投资的核心因子：价值(value)、动量(momentum)、质量(quality)、规模(size)、波动率(volatility)。多因子组合比单一因子更稳定。"

# Cluster 4: 日常杂项 (3 memories - should NOT cluster well)
$ENGRAM --database "$TEST_DB" store --type episodic --importance 0.3 \
  "今天去了Costco买了一箱鸡蛋和两袋咖啡豆。鸡蛋价格比上个月涨了15%。"
$ENGRAM --database "$TEST_DB" store --type episodic --importance 0.2 \
  "Mac mini M4到了，16GB内存版本。设置花了大概1小时，迁移assistant用了30分钟。"
$ENGRAM --database "$TEST_DB" store --type episodic --importance 0.3 \
  "晚上看了一部电影，Interstellar重映版。IMAX效果还是震撼的。"

echo "   ✅ Seeded 17 memories (4 clusters: Rust/AI/Trading/Daily)"
echo ""

# ── Step 2: Check memory count ───────────────────────────────────────────────
echo "📊 Step 2: Verifying memory count..."
$ENGRAM --database "$TEST_DB" stats
echo ""

# ── Step 3: Dry-run compile ──────────────────────────────────────────────────
echo "🔍 Step 3: Dry-run compilation (discover clusters without persisting)..."
echo ""
$ENGRAM --database "$TEST_DB" knowledge compile --dry-run
echo ""

# ── Step 4: Full compile ─────────────────────────────────────────────────────
echo "🔬 Step 4: Full compilation..."
echo ""
$ENGRAM --database "$TEST_DB" knowledge compile
echo ""

# ── Step 5: List topics ──────────────────────────────────────────────────────
echo "📋 Step 5: List compiled topics..."
echo ""
$ENGRAM --database "$TEST_DB" knowledge list
echo ""

# ── Step 6: Query topics ─────────────────────────────────────────────────────
echo "🔎 Step 6: Query topics..."
echo ""
echo "--- Query: 'Rust' ---"
$ENGRAM --database "$TEST_DB" knowledge query "Rust" || echo "   (no matches)"
echo ""
echo "--- Query: 'AI' ---"
$ENGRAM --database "$TEST_DB" knowledge query "AI" || echo "   (no matches)"
echo ""
echo "--- Query: '交易' ---"
$ENGRAM --database "$TEST_DB" knowledge query "交易" || echo "   (no matches)"
echo ""

# ── Step 7: Inspect first topic ──────────────────────────────────────────────
echo "🔬 Step 7: Inspect topics (first available)..."
echo ""
# Get first topic ID
FIRST_TOPIC=$($ENGRAM --database "$TEST_DB" knowledge list -j | python3 -c "import sys,json; topics=json.load(sys.stdin); print(topics[0]['id']['0'] if topics else '')" 2>/dev/null || echo "")
if [ -n "$FIRST_TOPIC" ]; then
    echo "Inspecting topic: $FIRST_TOPIC"
    $ENGRAM --database "$TEST_DB" knowledge inspect "$FIRST_TOPIC" --sources
else
    echo "   ⚠️  No topics to inspect (compile may have found no clusters)"
fi
echo ""

# ── Step 8: Health report ─────────────────────────────────────────────────────
echo "🏥 Step 8: Health report..."
echo ""
$ENGRAM --database "$TEST_DB" knowledge health
echo ""

# ── Step 9: Export ────────────────────────────────────────────────────────────
echo "📦 Step 9: Export..."
echo ""
$ENGRAM --database "$TEST_DB" knowledge export -o /tmp/kc-test-export -f md
echo ""
if [ -f /tmp/kc-test-export.md ]; then
    echo "--- Export preview (first 30 lines) ---"
    head -30 /tmp/kc-test-export.md
elif [ -d /tmp/kc-test-export ]; then
    echo "--- Export directory contents ---"
    ls -la /tmp/kc-test-export/
fi
echo ""

# ── Step 10: Decay evaluation ────────────────────────────────────────────────
echo "⏳ Step 10: Decay evaluation..."
echo ""
$ENGRAM --database "$TEST_DB" knowledge decay --evaluate
echo ""

# ── Step 11: Conflict scan ───────────────────────────────────────────────────
echo "⚡ Step 11: Conflict scan..."
echo ""
$ENGRAM --database "$TEST_DB" knowledge conflicts --scan
echo ""

# ── Step 12: Re-compile (idempotency check) ──────────────────────────────────
echo "🔄 Step 12: Re-compile (should create new topics or update existing)..."
echo ""
$ENGRAM --database "$TEST_DB" knowledge compile
echo ""

echo "════════════════════════════════════════════════════════════════"
echo " ✅ KC E2E Test Complete!"
echo "════════════════════════════════════════════════════════════════"
echo ""
echo "Test DB: $TEST_DB"
echo "Export: /tmp/kc-test-export"
echo ""
echo "To cleanup: rm -f $TEST_DB /tmp/kc-test-export*"
