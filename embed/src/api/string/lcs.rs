use std::mem::swap;

use super::{
  r#const::{ERR_LCS_INSUFFICIENT_MEMORY, ERR_LCS_TOO_LONG, MAX_STRING_SIZE},
  opt::{
    Lcs, StringLCS, StringLCSIdxResult, StringLCSMatchedRange, StringLCSResult, StringLCSType,
  },
};
use crate::error::{Error, Result};

/// Computes Longest Common Subsequence (LCS) with dynamic programming (aligned with Kvrocks String::LCS).
/// LCS (最长公共子序列) 动态规划与匹配区间提取（1:1 对标 Kvrocks String::LCS 算法与回溯状态机）
pub fn compute_lcs(
  s1: &[u8],
  s2: &[u8],
  opt_li: impl IntoIterator<Item = Lcs>,
) -> Result<StringLCSResult> {
  let args = Lcs::parse_options(opt_li);
  compute_lcs_with(s1, s2, args)
}

pub fn compute_lcs_with(s1: &[u8], s2: &[u8], args: StringLCS) -> Result<StringLCSResult> {
  let alen = s1.len();
  let blen = s2.len();

  if alen == 0 || blen == 0 {
    return match args.lcs_type {
      StringLCSType::Len => Ok(StringLCSResult::Len(0)),
      StringLCSType::Idx => Ok(StringLCSResult::Idx(StringLCSIdxResult::default())),
      StringLCSType::None => Ok(StringLCSResult::Str(String::new())),
    };
  }

  if alen >= (u32::MAX - 1) as usize || blen >= (u32::MAX - 1) as usize {
    return Err(Error::invalid_data(ERR_LCS_TOO_LONG));
  }

  if s1 == s2 {
    let lcs_len = alen as u32;
    return match args.lcs_type {
      StringLCSType::Len => Ok(StringLCSResult::Len(lcs_len)),
      StringLCSType::Idx => {
        let matches = if args.min_match_len <= 0 || lcs_len >= args.min_match_len as u32 {
          vec![StringLCSMatchedRange::new(
            0,
            lcs_len - 1,
            0,
            lcs_len - 1,
            lcs_len,
          )]
        } else {
          Vec::new()
        };
        Ok(StringLCSResult::Idx(StringLCSIdxResult {
          matches,
          len: lcs_len,
        }))
      }
      StringLCSType::None => {
        let s = match String::from_utf8(s1.to_vec()) {
          Ok(s) => s,
          Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
        };
        Ok(StringLCSResult::Str(s))
      }
    };
  }

  // 针对仅计算长度的快速通道，采用双行滚动数组将空间复杂度由 O(M*N) 降至 O(min(M, N))
  if args.lcs_type == StringLCSType::Len {
    let (s1, s2, _alen, blen) = if alen < blen {
      (s2, s1, blen, alen)
    } else {
      (s1, s2, alen, blen)
    };
    let mut prev = vec![0u32; blen + 1];
    let mut curr = vec![0u32; blen + 1];
    for &b1 in s1 {
      for j in 1..=blen {
        if b1 == s2[j - 1] {
          curr[j] = prev[j - 1] + 1;
        } else {
          curr[j] = prev[j].max(curr[j - 1]);
        }
      }
      swap(&mut prev, &mut curr);
    }
    return Ok(StringLCSResult::Len(prev[blen]));
  }

  let dp_size = (alen + 1) * (blen + 1);
  let byte_size = dp_size.checked_mul(size_of::<u32>());
  if byte_size.is_none() || byte_size.unwrap_or(usize::MAX) > MAX_STRING_SIZE {
    return Err(Error::invalid_data(ERR_LCS_INSUFFICIENT_MEMORY));
  }

  let mut dp = vec![0u32; dp_size];
  let stride = blen + 1;
  let idx_fn = |i: usize, j: usize| -> usize { i * stride + j };

  for i in 1..=alen {
    let s1_c = s1[i - 1];
    let row_curr = i * stride;
    let row_prev = (i - 1) * stride;
    for j in 1..=blen {
      if s1_c == s2[j - 1] {
        dp[row_curr + j] = dp[row_prev + j - 1] + 1;
      } else {
        dp[row_curr + j] = dp[row_prev + j].max(dp[row_curr + j - 1]);
      }
    }
  }

  let lcs_len = dp[idx_fn(alen, blen)];

  let mut lcs_bytes = if args.lcs_type == StringLCSType::None {
    vec![0u8; lcs_len as usize]
  } else {
    Vec::new()
  };

  let mut matches = Vec::new();
  let mut idx = lcs_len as usize;
  let mut i = alen;
  let mut j = blen;
  let mut a_range_start = alen;
  let mut a_range_end = 0;
  let mut b_range_start = 0;
  let mut b_range_end = 0;

  while i > 0 && j > 0 {
    let mut emit_range = false;
    if s1[i - 1] == s2[j - 1] {
      if args.lcs_type == StringLCSType::None && idx > 0 {
        lcs_bytes[idx - 1] = s1[i - 1];
      }

      if a_range_start == alen {
        a_range_start = i - 1;
        a_range_end = i - 1;
        b_range_start = j - 1;
        b_range_end = j - 1;
      } else if a_range_start == i && b_range_start == j {
        a_range_start -= 1;
        b_range_start -= 1;
      } else {
        emit_range = true;
      }

      if a_range_start == 0 || b_range_start == 0 {
        emit_range = true;
      }
      idx = idx.saturating_sub(1);
      i -= 1;
      j -= 1;
    } else {
      let lcs1 = dp[idx_fn(i - 1, j)];
      let lcs2 = dp[idx_fn(i, j - 1)];
      if lcs1 > lcs2 {
        i -= 1;
      } else {
        j -= 1;
      }
      if a_range_start != alen {
        emit_range = true;
      }
    }

    if emit_range {
      if args.lcs_type == StringLCSType::Idx {
        let match_len = (a_range_end - a_range_start + 1) as u32;
        if args.min_match_len <= 0 || match_len >= args.min_match_len as u32 {
          matches.push(StringLCSMatchedRange::new(
            a_range_start as u32,
            a_range_end as u32,
            b_range_start as u32,
            b_range_end as u32,
            match_len,
          ));
        }
      }
      a_range_start = alen;
    }
  }

  match args.lcs_type {
    StringLCSType::Len => Ok(StringLCSResult::Len(lcs_len)),
    StringLCSType::Idx => Ok(StringLCSResult::Idx(StringLCSIdxResult {
      matches,
      len: lcs_len,
    })),
    StringLCSType::None => {
      let s = match String::from_utf8(lcs_bytes) {
        Ok(s) => s,
        Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
      };
      Ok(StringLCSResult::Str(s))
    }
  }
}
