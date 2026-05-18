//! 各存储后端共用的关键词重叠排序（POC，非向量）。

use super::retriever::MemoryChunk;
use super::types::MemoryHit;

pub(crate) fn score_chunk(query: &str, content: &str) -> f32 {
    let q = query.to_lowercase();
    let c = content.to_lowercase();
    if q.is_empty() {
        return 0.0;
    }
    if c.contains(&q) {
        return 1.0;
    }
    let words: Vec<&str> = q.split_whitespace().filter(|w| !w.is_empty()).collect();
    if words.is_empty() {
        return 0.0;
    }
    let matched = words.iter().filter(|w| c.contains(*w)).count();
    matched as f32 / words.len() as f32
}

pub(crate) fn rank_chunks(query: &str, chunks: &[MemoryChunk], top_k: usize) -> Vec<MemoryHit> {
    let mut scored: Vec<(f32, &MemoryChunk)> = chunks
        .iter()
        .map(|c| (score_chunk(query, &c.content), c))
        .filter(|(s, _)| *s > 0.0)
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored
        .into_iter()
        .take(top_k)
        .map(|(score, c)| MemoryHit {
            id: c.id.clone(),
            content: c.content.clone(),
            score: Some(score),
            source: c.source.clone(),
        })
        .collect()
}
