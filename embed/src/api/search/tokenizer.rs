use std::mem::{swap, take};

use rapidhash::RapidHashSet as HashSet;

/// Default English stop words list (aligned with Apache Kvrocks and RediSearch).
/// 默认英文停用词表（对标 Apache Kvrocks 与 RediSearch 默认停用词）
pub use super::r#const::DEFAULT_STOP_WORDS;

/// Stack-allocated buffer size threshold for Levenshtein distance calculations.
/// 栈上编辑距离计算最大长度阈值
const STACK_BUFFER_MAX_LEN: usize = 64;

/// Text tokenizer with lowercase normalization and punctuation splitting.
/// 文本分词器（标准小写规范化与标点切分）
#[inline]
pub fn tokenize_text(text: &str) -> Vec<String> {
  tokenize_text_with_stopwords(text, None)
}

/// Text tokenizer supporting stopword filtering with single-pass state machine.
/// 文本分词器（支持指定停用词过滤，单次循环高效状态机）
pub fn tokenize_text_with_stopwords(
  text: &str,
  stop_words: Option<&HashSet<String>>,
) -> Vec<String> {
  let mut words = Vec::new();
  let mut cur = String::with_capacity(16);

  for ch in text.chars() {
    if ch.is_alphanumeric() || ch == '_' {
      cur.push(ch.to_ascii_lowercase());
    } else if !cur.is_empty() {
      if stop_words.is_none_or(|sw| !sw.contains(&cur)) {
        words.push(take(&mut cur));
      } else {
        cur.clear();
      }
    }
  }
  if !cur.is_empty() && stop_words.is_none_or(|sw| !sw.contains(&cur)) {
    words.push(cur);
  }
  words
}

/// Unescapes character escape sequences in extracted tag strings in a single pass.
/// 转义字符反转义处理（针对已提取出的 tag 字符串单次遍历还原，带无转义快速路径）
#[inline]
pub fn unescape_tag_string(s: &str) -> String {
  if !s.contains('\\') {
    return s.to_string();
  }
  let mut res = String::with_capacity(s.len());
  let mut chars = s.chars();
  while let Some(ch) = chars.next() {
    if ch == '\\' {
      if let Some(next_ch) = chars.next() {
        res.push(next_ch);
      }
    } else {
      res.push(ch);
    }
  }
  res
}

/// Splits tag field with custom delimiter, escape handling, and case normalization.
/// 标签字段分割（支持自定义分隔符、转义字符、引号去除及大小写规范化）
pub fn tokenize_tags(text: &str, separator: char, case_sensitive: bool) -> Vec<String> {
  let mut tags = Vec::new();
  let mut cur = String::with_capacity(32);
  let mut escaped = false;

  for ch in text.chars() {
    if escaped {
      cur.push('\\');
      cur.push(ch);
      escaped = false;
      continue;
    }
    if ch == '\\' {
      escaped = true;
      continue;
    }
    if ch == separator {
      let trimmed = cur.trim().trim_matches('"').trim_matches('\'');
      if !trimmed.is_empty() {
        let unescaped = unescape_tag_string(trimmed);
        tags.push(if case_sensitive {
          unescaped
        } else {
          unescaped.to_lowercase()
        });
      }
      cur.clear();
    } else {
      cur.push(ch);
    }
  }

  if escaped {
    cur.push('\\');
  }

  let trimmed = cur.trim().trim_matches('"').trim_matches('\'');
  if !trimmed.is_empty() {
    let unescaped = unescape_tag_string(trimmed);
    tags.push(if case_sensitive {
      unescaped
    } else {
      unescaped.to_lowercase()
    });
  }

  tags
}

/// Levenshtein edit distance calculation with stack buffer and zero heap allocation.
/// 字符串编辑距离（Levenshtein Distance，带栈上零堆分配与 ASCII 极速切片加速）
pub fn levenshtein_distance(s1: &str, s2: &str) -> usize {
  if s1 == s2 {
    return 0;
  }
  if s1.is_empty() {
    return s2.chars().count();
  }
  if s2.is_empty() {
    return s1.chars().count();
  }

  // ASCII 快速路径
  if s1.is_ascii() && s2.is_ascii() {
    let b1 = s1.as_bytes();
    let b2 = s2.as_bytes();
    let (b1, b2) = if b1.len() < b2.len() {
      (b2, b1)
    } else {
      (b1, b2)
    };
    let len1 = b1.len();
    let len2 = b2.len();

    if len2 <= STACK_BUFFER_MAX_LEN {
      let mut prev = [0usize; STACK_BUFFER_MAX_LEN + 1];
      let mut curr = [0usize; STACK_BUFFER_MAX_LEN + 1];
      for (j, item) in prev.iter_mut().enumerate().take(len2 + 1) {
        *item = j;
      }

      for i in 1..=len1 {
        curr[0] = i;
        for j in 1..=len2 {
          let cost = usize::from(b1[i - 1] != b2[j - 1]);
          curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        swap(&mut prev, &mut curr);
      }
      return prev[len2];
    }

    let mut prev: Vec<usize> = (0..=len2).collect();
    let mut curr = vec![0; len2 + 1];

    for i in 1..=len1 {
      curr[0] = i;
      for j in 1..=len2 {
        let cost = usize::from(b1[i - 1] != b2[j - 1]);
        curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
      }
      swap(&mut prev, &mut curr);
    }
    return prev[len2];
  }

  // 通用 Unicode 路径
  let s1_chars: Vec<char> = s1.chars().collect();
  let s2_chars: Vec<char> = s2.chars().collect();
  let (s1_chars, s2_chars) = if s1_chars.len() < s2_chars.len() {
    (s2_chars, s1_chars)
  } else {
    (s1_chars, s2_chars)
  };
  let len1 = s1_chars.len();
  let len2 = s2_chars.len();

  if len2 <= STACK_BUFFER_MAX_LEN {
    let mut prev = [0usize; STACK_BUFFER_MAX_LEN + 1];
    let mut curr = [0usize; STACK_BUFFER_MAX_LEN + 1];
    for (j, item) in prev.iter_mut().enumerate().take(len2 + 1) {
      *item = j;
    }

    for i in 1..=len1 {
      curr[0] = i;
      for j in 1..=len2 {
        let cost = usize::from(s1_chars[i - 1] != s2_chars[j - 1]);
        curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
      }
      swap(&mut prev, &mut curr);
    }
    return prev[len2];
  }

  let mut prev: Vec<usize> = (0..=len2).collect();
  let mut curr = vec![0; len2 + 1];

  for i in 1..=len1 {
    curr[0] = i;
    for j in 1..=len2 {
      let cost = usize::from(s1_chars[i - 1] != s2_chars[j - 1]);
      curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
    }
    swap(&mut prev, &mut curr);
  }

  prev[len2]
}
