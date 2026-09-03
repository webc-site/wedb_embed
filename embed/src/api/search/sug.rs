use std::cmp::Ordering;

use rapidhash::RapidHashMap as HashMap;

/// Default limit on retrieved autocomplete suggestion count.
/// 默认建议项检索限制数量
pub use super::r#const::DEFAULT_SUG_LIMIT;
use crate::search::{opt::SuggestionItem, tokenizer::levenshtein_distance};

/// Autocomplete suggestion dictionary aligned with RediSearch FT.SUG* commands.
/// 自动补全建议字典（对标 RediSearch FT.SUGADD, FT.SUGGET, FT.SUGDEL, FT.SUGLEN）
#[derive(Debug, Clone, Default)]
pub struct SuggestionDict {
  pub entries: HashMap<String, (f64, Option<String>)>,
}

impl SuggestionDict {
  #[inline]
  pub fn new() -> Self {
    Self::default()
  }

  /// Adds a suggestion string with weight and optional payload.
  /// 添加建议项
  pub fn sug_add(
    &mut self,
    string: &str,
    score: f64,
    incr: bool,
    payload: Option<String>,
  ) -> usize {
    let entry = self
      .entries
      .entry(string.to_string())
      .or_insert((0.0, None));
    if incr {
      entry.0 += score;
    } else {
      entry.0 = score;
    }
    if payload.is_some() {
      entry.1 = payload;
    }
    self.entries.len()
  }

  /// Retrieves suggestions matching prefix or fuzzy edit distance sorted by score.
  /// 检索建议项（快速前缀与编辑距离过滤 + 依据真实权重降序截断排序）
  pub fn sug_get(
    &self,
    prefix: &str,
    fuzzy: bool,
    withscores: bool,
    withpayloads: bool,
    max: Option<usize>,
  ) -> Vec<SuggestionItem> {
    let limit = max.unwrap_or(DEFAULT_SUG_LIMIT);
    let prefix_lower = prefix.to_lowercase();

    // 收集符合前缀或编辑距离的条目：(string, real_score, payload)
    let mut matched: Vec<(&str, f64, Option<&str>)> = self
      .entries
      .iter()
      .filter_map(|(s, (score, payload))| {
        let s_str = s.as_str();
        // 快速 ASCII 前缀检查
        if s_str.len() >= prefix.len() && s_str[..prefix.len()].eq_ignore_ascii_case(prefix) {
          Some((s_str, *score, payload.as_deref()))
        } else {
          let s_lower = s.to_lowercase();
          if s_lower.starts_with(&prefix_lower)
            || (fuzzy && levenshtein_distance(&s_lower, &prefix_lower) <= 1)
          {
            Some((s_str, *score, payload.as_deref()))
          } else {
            None
          }
        }
      })
      .collect();

    // 依据真实分数降序排序，分数相同按字母序升序
    let cmp_fn = |a: &(&str, f64, Option<&str>), b: &(&str, f64, Option<&str>)| {
      b.1
        .partial_cmp(&a.1)
        .unwrap_or(Ordering::Equal)
        .then_with(|| a.0.cmp(b.0))
    };

    if matched.len() > limit {
      matched.select_nth_unstable_by(limit, cmp_fn);
      matched.truncate(limit);
    }

    matched.sort_by(cmp_fn);

    // 格式化输出 SuggestionItem
    matched
      .into_iter()
      .map(|(s, score, payload)| SuggestionItem {
        string: s.to_string(),
        score: if withscores { score } else { 0.0 },
        payload: if withpayloads {
          payload.map(str::to_string)
        } else {
          None
        },
      })
      .collect()
  }

  /// Deletes a suggestion string from dictionary.
  /// 删除建议项
  #[inline]
  pub fn sug_del(&mut self, string: &str) -> bool {
    self.entries.remove(string).is_some()
  }

  /// Returns total number of suggestions in dictionary.
  /// 返回建议字典长度
  #[inline]
  pub fn sug_len(&self) -> usize {
    self.entries.len()
  }
}
