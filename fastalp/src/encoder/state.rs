use crate::{
  constants::{FLAG_REPEAT, RD_SIZE_THRESHOLD_DEN, RD_SIZE_THRESHOLD_NUM, TYPE_MASK},
  encoder::{
    dict::{DictCandidate, dict_compressed_size, scan_dict, write_dict_chunk},
    engine::compress_into_engine,
    exception::{DEFAULT_EXCEPTIONS_CAP, Exception},
    rd::{MAX_RD_DICT_SIZE, encode_rd_fast, try_encode_rd, write_rd_chunk},
  },
  float::AlpFloat,
  header::{header_len, read_header, write_header},
  sampler::BestParams,
};

/// Caching compression scheme decision across adjacent chunks.
/// 连续数据块编码策略缓存决策
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CachedScheme {
  Alp,
  Dict,
  Rd,
}

/// Cached state of outlier target bit-width pruning.
/// 离群点目标位宽剪枝缓存状态机
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CachedTargetBw {
  #[default]
  Uninit,
  Pruned(u8),
  Disabled,
}

/// Stateful encoder that caches optimal parameters and scratch buffers across adjacent chunks.
/// 状态化编码器：在连续数据块编码时复用已探测的最优参数与内部工作缓冲区，消除重复采样开销与内存分配。
#[derive(Debug, Clone)]
pub struct Encoder<F: AlpFloat> {
  pub cached_params: Option<BestParams>,
  pub cached_use_repeat: Option<bool>,
  pub cached_scheme: Option<CachedScheme>,
  pub(crate) cached_dict: Option<DictCandidate<F>>,
  pub(crate) cached_rd: Option<(u8, usize, [u16; MAX_RD_DICT_SIZE])>,
  pub cached_target_bw: CachedTargetBw,
  pub cached_use_delta: Option<bool>,
  pub(crate) dict_indices: Vec<u64>,
  pub(crate) encoded_buf: Vec<F::Int>,
  pub(crate) exceptions: Vec<Exception<F::RawBits>>,
  pub(crate) non_repeated_buf: Vec<F>,
  pub(crate) rd_left_indices: Vec<u64>,
  pub(crate) rd_right_parts: Vec<u64>,
  pub(crate) rd_exceptions: Vec<(u16, u16)>,
  pub(crate) repeat_bitmap: Vec<u8>,
  pub(crate) scratch_dst: Vec<u8>,
  pub(crate) scratch_encoded_buf: Vec<F::Int>,
  pub(crate) scratch_exceptions: Vec<Exception<F::RawBits>>,
}

impl<F: AlpFloat> Default for Encoder<F> {
  #[inline]
  fn default() -> Self {
    Self::new()
  }
}

impl<F: AlpFloat> Encoder<F> {
  /// Creates a new encoder (empty buffers, dynamically allocated on demand and reused).
  /// 创建新的编码器（空缓冲区，按需动态分配并长久复用）
  #[inline]
  pub const fn new() -> Self {
    Self {
      cached_params: None,
      cached_use_repeat: None,
      cached_scheme: None,
      cached_dict: None,
      cached_rd: None,
      cached_target_bw: CachedTargetBw::Uninit,
      cached_use_delta: None,
      dict_indices: Vec::new(),
      encoded_buf: Vec::new(),
      exceptions: Vec::new(),
      non_repeated_buf: Vec::new(),
      rd_left_indices: Vec::new(),
      rd_right_parts: Vec::new(),
      rd_exceptions: Vec::new(),
      repeat_bitmap: Vec::new(),
      scratch_dst: Vec::new(),
      scratch_encoded_buf: Vec::new(),
      scratch_exceptions: Vec::new(),
    }
  }

  /// Creates an encoder with specified initial capacity (pre-allocated buffers for zero heap allocation).
  /// 创建具有指定初始容量的编码器（预分配缓冲区，实现全程零堆分配）
  #[inline]
  pub fn with_capacity(capacity: usize) -> Self {
    Self {
      cached_params: None,
      cached_use_repeat: None,
      cached_scheme: None,
      cached_dict: None,
      cached_rd: None,
      cached_target_bw: CachedTargetBw::Uninit,
      cached_use_delta: None,
      dict_indices: Vec::with_capacity(capacity),
      encoded_buf: Vec::with_capacity(capacity),
      exceptions: Vec::with_capacity(DEFAULT_EXCEPTIONS_CAP),
      non_repeated_buf: Vec::with_capacity(capacity),
      rd_left_indices: Vec::with_capacity(capacity),
      rd_right_parts: Vec::with_capacity(capacity),
      rd_exceptions: Vec::with_capacity(64),
      repeat_bitmap: Vec::with_capacity(capacity.div_ceil(8)),
      scratch_dst: Vec::with_capacity(capacity * 2 + 64),
      scratch_encoded_buf: Vec::with_capacity(capacity),
      scratch_exceptions: Vec::with_capacity(DEFAULT_EXCEPTIONS_CAP),
    }
  }

  /// Resets cached optimal parameters and clears internal buffers.
  /// 重置已缓存的最优参数与内部缓冲区
  #[inline]
  pub fn reset(&mut self) {
    self.cached_params = None;
    self.cached_use_repeat = None;
    self.cached_scheme = None;
    self.cached_dict = None;
    self.cached_rd = None;
    self.cached_target_bw = CachedTargetBw::Uninit;
    self.cached_use_delta = None;
    self.dict_indices.clear();
    self.encoded_buf.clear();
    self.exceptions.clear();
    self.non_repeated_buf.clear();
    self.rd_left_indices.clear();
    self.rd_right_parts.clear();
    self.rd_exceptions.clear();
    self.repeat_bitmap.clear();
    self.scratch_dst.clear();
    self.scratch_encoded_buf.clear();
    self.scratch_exceptions.clear();
  }

  #[inline]
  fn fill_repeat_buffers(&mut self, data: &[F], bitmap_len: usize) {
    self.repeat_bitmap.clear();
    self.repeat_bitmap.resize(bitmap_len, 0);
    self.non_repeated_buf.clear();
    self.non_repeated_buf.reserve(data.len());
    self.non_repeated_buf.push(data[0]);

    let count = data.len();
    let mut prev = data[0];
    let full_words = count / 64;
    let ptr_u64 = self.repeat_bitmap.as_mut_ptr().cast::<u64>();

    for w in 0..full_words {
      let mut word = 0u64;
      let base = w * 64;
      let start_j = if w == 0 { 1 } else { 0 };
      for j in start_j..64 {
        let curr = unsafe { *data.get_unchecked(base + j) };
        if curr.is_exact_same(prev) {
          word |= 1u64 << j;
        } else {
          self.non_repeated_buf.push(curr);
          prev = curr;
        }
      }
      unsafe {
        ptr_u64.add(w).write_unaligned(word);
      }
    }

    let rem_start = full_words * 64;
    let start_j = if full_words == 0 { 1 } else { 0 };
    for idx in (rem_start + start_j)..count {
      let curr = unsafe { *data.get_unchecked(idx) };
      if curr.is_exact_same(prev) {
        self.repeat_bitmap[idx >> 3] |= 1 << (idx & 7);
      } else {
        self.non_repeated_buf.push(curr);
        prev = curr;
      }
    }
  }

  #[inline]
  fn write_repeat_chunk(
    &self,
    count: usize,
    nr_hdr: &crate::header::ParsedHeader,
    nr_payload: &[u8],
    dst: &mut Vec<u8>,
  ) {
    let type_byte = (nr_hdr.type_byte & TYPE_MASK) | FLAG_REPEAT;
    let packed_params = nr_hdr.params.map(|p| p.pack());
    write_header(type_byte, count, packed_params, dst);
    dst.extend_from_slice(&self.repeat_bitmap);
    dst.extend_from_slice(nr_payload);
  }

  fn compress_chunk_inner(&mut self, data: &[F], dst: &mut Vec<u8>, force_delta: bool) {
    let count = data.len();
    if self.cached_use_repeat == Some(false) {
      self.cached_params = compress_into_engine(
        data,
        dst,
        force_delta,
        self.cached_params,
        &mut self.cached_target_bw,
        &mut self.cached_use_delta,
        &mut self.encoded_buf,
        &mut self.exceptions,
      );
      return;
    }

    let bitmap_len = count.div_ceil(8);
    if self.cached_use_repeat == Some(true) {
      self.fill_repeat_buffers(data, bitmap_len);

      self.scratch_dst.clear();
      let mut scratch_target_bw = CachedTargetBw::Uninit;
      let mut scratch_use_delta = None;
      self.cached_params = compress_into_engine(
        &self.non_repeated_buf,
        &mut self.scratch_dst,
        force_delta,
        self.cached_params,
        &mut scratch_target_bw,
        &mut scratch_use_delta,
        &mut self.scratch_encoded_buf,
        &mut self.scratch_exceptions,
      );

      if let Ok(nr_hdr) = read_header(&self.scratch_dst) {
        let nr_payload = &self.scratch_dst[nr_hdr.cursor..];
        self.write_repeat_chunk(count, &nr_hdr, nr_payload, dst);
        return;
      }
    }

    // 快速采样前置否定：测试 16 对相邻元素（严格检测时序连续重复，避免跨步长采样失效）
    let sample_step = (count / 16).max(1);
    let mut sample_matches = 0usize;
    for i in 0..16 {
      let idx = i * sample_step;
      if idx + 1 < count && data[idx].is_exact_same(data[idx + 1]) {
        sample_matches += 1;
      }
    }
    // 16 个相邻采样对中若匹配少于 2 对（< 12.5%），在 5ns 内直接跳过全量扫描
    if sample_matches < 2 {
      self.cached_use_repeat = Some(false);
      self.cached_params = compress_into_engine(
        data,
        dst,
        force_delta,
        self.cached_params,
        &mut self.cached_target_bw,
        &mut self.cached_use_delta,
        &mut self.encoded_buf,
        &mut self.exceptions,
      );
      return;
    }

    let mut repeat_count = 0usize;
    let mut prev = data[0];
    for &curr in &data[1..] {
      if curr.is_exact_same(prev) {
        repeat_count += 1;
      } else {
        prev = curr;
      }
    }

    // 数学盈亏平衡门限：Repeat 位图至少需要 1/8 字节/值 (1 bit/值) 的固定元数据成本，
    // 只有当重复率达到 20% 以上（repeat_count * 5 >= count）时，节省的编码体积才足够覆盖位图与两趟解码成本
    if repeat_count * 5 < count {
      self.cached_use_repeat = Some(false);
      self.cached_params = compress_into_engine(
        data,
        dst,
        force_delta,
        self.cached_params,
        &mut self.cached_target_bw,
        &mut self.cached_use_delta,
        &mut self.encoded_buf,
        &mut self.exceptions,
      );
      return;
    }

    // 先执行正常编码写入 dst，保留其结果与参数
    let start_len = dst.len();
    let normal_params = compress_into_engine(
      data,
      dst,
      force_delta,
      self.cached_params,
      &mut self.cached_target_bw,
      &mut self.cached_use_delta,
      &mut self.encoded_buf,
      &mut self.exceptions,
    );
    let normal_total_size = dst.len() - start_len;

    // 数学理论下界短路：Repeat 数据块含有独立块头 (>= 3 字节) 与位图 (bitmap_len)，
    // 若普通编码体积已然逼近此下界（例如常量块或超低位宽平滑块），Repeat 绝无超越可能，立即返回
    let repeat_lower_bound = bitmap_len + header_len(count) + 32;
    if normal_total_size <= repeat_lower_bound {
      self.cached_params = normal_params;
      self.cached_use_repeat = Some(false);
      return;
    }

    // 尝试 Repeat 编码
    self.fill_repeat_buffers(data, bitmap_len);

    self.scratch_dst.clear();
    let mut scratch_target_bw = CachedTargetBw::Uninit;
    let mut scratch_use_delta = None;
    let nr_params = compress_into_engine(
      &self.non_repeated_buf,
      &mut self.scratch_dst,
      force_delta,
      self.cached_params,
      &mut scratch_target_bw,
      &mut scratch_use_delta,
      &mut self.scratch_encoded_buf,
      &mut self.scratch_exceptions,
    );

    let nr_hdr = match read_header(&self.scratch_dst) {
      Ok(h) => h,
      Err(_) => {
        self.cached_params = normal_params;
        self.cached_use_repeat = Some(false);
        return;
      }
    };

    let nr_payload = &self.scratch_dst[nr_hdr.cursor..];
    let repeat_total_size = header_len(count) + bitmap_len + nr_payload.len();

    // 只有当 Repeat 体积比普通编码少至少 32 字节时才采用，避免微小收益导致展开惩罚
    if repeat_total_size + 32 < normal_total_size {
      dst.truncate(start_len);
      self.write_repeat_chunk(count, &nr_hdr, nr_payload, dst);
      self.cached_params = nr_params;
      self.cached_use_repeat = Some(true);
      self.cached_target_bw = scratch_target_bw;
    } else {
      self.cached_params = normal_params;
      self.cached_use_repeat = Some(false);
    }
  }

  fn compress_chunk(&mut self, data: &[F], dst: &mut Vec<u8>, force_delta: bool) {
    let count = data.len();
    if count <= 4 {
      self.cached_params = compress_into_engine(
        data,
        dst,
        force_delta,
        self.cached_params,
        &mut self.cached_target_bw,
        &mut self.cached_use_delta,
        &mut self.encoded_buf,
        &mut self.exceptions,
      );
      return;
    }

    if !force_delta {
      match self.cached_scheme {
        Some(CachedScheme::Dict) => {
          if let Some(candidate) = self.cached_dict
            && candidate.dict_len <= 1
          {
            let d0 = candidate.dict[0];
            if (count == 1 || data[1].is_exact_same(d0))
              && data.iter().all(|&v| v.is_exact_same(d0))
            {
              write_dict_chunk(count, &candidate, &[], dst);
              return;
            }
          }
          if let Some(candidate) = scan_dict(data, &mut self.dict_indices) {
            self.cached_dict = Some(candidate);
            write_dict_chunk(count, &candidate, &self.dict_indices, dst);
            return;
          }
          self.cached_scheme = None;
          self.cached_dict = None;
        }
        Some(CachedScheme::Rd) => {
          if let Some((right_bw, actual_dict_size, dict)) = self.cached_rd
            && let Some(rd) = encode_rd_fast(
              data,
              right_bw,
              actual_dict_size,
              dict,
              &mut self.rd_left_indices,
              &mut self.rd_right_parts,
              &mut self.rd_exceptions,
            )
          {
            write_rd_chunk::<F>(
              count,
              &rd,
              &self.rd_left_indices,
              &self.rd_right_parts,
              &self.rd_exceptions,
              dst,
            );
            return;
          }
          if let Some(rd) = try_encode_rd(
            data,
            &mut self.rd_left_indices,
            &mut self.rd_right_parts,
            &mut self.rd_exceptions,
          ) {
            self.cached_rd = Some((rd.right_bw, rd.actual_dict_size as usize, rd.dict));
            write_rd_chunk::<F>(
              count,
              &rd,
              &self.rd_left_indices,
              &self.rd_right_parts,
              &self.rd_exceptions,
              dst,
            );
            return;
          }
          self.cached_scheme = None;
          self.cached_rd = None;
        }
        Some(CachedScheme::Alp) => {
          self.compress_chunk_inner(data, dst, force_delta);
          return;
        }
        None => {}
      }
    }

    let dict_candidate = if !force_delta {
      scan_dict(data, &mut self.dict_indices)
    } else {
      None
    };

    if let Some(ref candidate) = dict_candidate
      && candidate.dict_len <= 1
    {
      if !force_delta {
        self.cached_scheme = Some(CachedScheme::Dict);
        self.cached_dict = Some(*candidate);
      }
      write_dict_chunk(count, candidate, &self.dict_indices, dst);
      return;
    }

    let start_len = dst.len();
    self.compress_chunk_inner(data, dst, force_delta);
    let mut current_size = dst.len() - start_len;
    let mut chosen_scheme = CachedScheme::Alp;

    if let Some(ref candidate) = dict_candidate {
      let dict_size = dict_compressed_size::<F>(count, candidate.dict_len, candidate.bit_width);
      if dict_size < current_size {
        dst.truncate(start_len);
        write_dict_chunk(count, candidate, &self.dict_indices, dst);
        current_size = dict_size;
        chosen_scheme = CachedScheme::Dict;
        self.cached_dict = Some(*candidate);
      }
    }

    let raw_size = std::mem::size_of_val(data);
    if current_size * RD_SIZE_THRESHOLD_DEN > raw_size * RD_SIZE_THRESHOLD_NUM
      && !force_delta
      && let Some(rd) = try_encode_rd(
        data,
        &mut self.rd_left_indices,
        &mut self.rd_right_parts,
        &mut self.rd_exceptions,
      )
      && rd.total_size < current_size
    {
      dst.truncate(start_len);
      self.cached_rd = Some((rd.right_bw, rd.actual_dict_size as usize, rd.dict));
      write_rd_chunk::<F>(
        count,
        &rd,
        &self.rd_left_indices,
        &self.rd_right_parts,
        &self.rd_exceptions,
        dst,
      );
      chosen_scheme = CachedScheme::Rd;
    }

    if !force_delta {
      self.cached_scheme = Some(chosen_scheme);
      if chosen_scheme != CachedScheme::Rd {
        self.cached_rd = None;
      }
      if chosen_scheme != CachedScheme::Dict {
        self.cached_dict = None;
      }
    }
  }

  /// Compresses float data slice into destination buffer (adaptive FOR/Delta, reusing parameters and memory).
  /// 压缩浮点数据切片并写入目标缓冲区（自适应 FOR 或 Delta 模式，复用采样参数与工作内存）
  #[inline]
  pub fn compress_into(&mut self, data: &[F], dst: &mut Vec<u8>) {
    self.compress_chunk(data, dst, false);
  }

  /// Compresses float data slice using Delta differential mode into destination buffer.
  /// 强制使用 Delta 差分模式压缩浮点数据切片并写入目标缓冲区
  #[inline]
  pub fn compress_delta_into(&mut self, data: &[F], dst: &mut Vec<u8>) {
    self.compress_chunk(data, dst, true);
  }

  /// Compresses float data slice and returns newly allocated byte vector.
  /// 压缩浮点数据切片并返回新分配的字节向量
  #[inline]
  pub fn compress(&mut self, data: &[F]) -> Vec<u8> {
    let mut dst = Vec::new();
    self.compress_into(data, &mut dst);
    dst
  }

  /// Compresses float data slice using Delta differential mode and returns newly allocated byte vector.
  /// 强制使用 Delta 差分模式压缩浮点数据切片并返回新分配的字节向量
  #[inline]
  pub fn compress_delta(&mut self, data: &[F]) -> Vec<u8> {
    let mut dst = Vec::new();
    self.compress_delta_into(data, &mut dst);
    dst
  }

  /// Shrinks the capacity of the internal work buffer as much as possible.
  /// 释放内部工作缓冲区多余分配的容量
  #[inline]
  pub fn shrink_to_fit(&mut self) {
    self.dict_indices.shrink_to_fit();
    self.encoded_buf.shrink_to_fit();
    self.exceptions.shrink_to_fit();
    self.non_repeated_buf.shrink_to_fit();
    self.rd_left_indices.shrink_to_fit();
    self.rd_right_parts.shrink_to_fit();
    self.rd_exceptions.shrink_to_fit();
    self.repeat_bitmap.shrink_to_fit();
    self.scratch_dst.shrink_to_fit();
    self.scratch_encoded_buf.shrink_to_fit();
    self.scratch_exceptions.shrink_to_fit();
  }

  /// Returns the current capacity of the internal work buffer.
  /// 获取内部工作缓冲区的当前容量
  #[inline]
  pub fn capacity(&self) -> usize {
    self.encoded_buf.capacity()
  }
}
