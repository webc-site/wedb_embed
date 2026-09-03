use crate::{
  encoder::{
    engine::compress_into_engine,
    exception::{DEFAULT_EXCEPTIONS_CAP, Exception},
  },
  float::AlpFloat,
  sampler::BestParams,
};

/// Stateful encoder that caches optimal parameters and scratch buffers across adjacent chunks.
/// 状态化编码器：在连续数据块编码时复用已探测的最优参数与内部工作缓冲区，消除重复采样开销与内存分配。
#[derive(Debug, Clone)]
pub struct Encoder<F: AlpFloat> {
  pub cached_params: Option<BestParams>,
  pub(crate) encoded_buf: Vec<F::Int>,
  pub(crate) exceptions: Vec<Exception<F::RawBits>>,
}

impl<F: AlpFloat> Default for Encoder<F> {
  #[inline]
  fn default() -> Self {
    Self::new()
  }
}

impl<F: AlpFloat> Encoder<F> {
  /// 创建新的编码器（空缓冲区，按需动态分配并长久复用）
  #[inline]
  pub const fn new() -> Self {
    Self {
      cached_params: None,
      encoded_buf: Vec::new(),
      exceptions: Vec::new(),
    }
  }

  /// 创建具有指定初始容量的编码器（预分配缓冲区，实现全程零堆分配）
  #[inline]
  pub fn with_capacity(capacity: usize) -> Self {
    Self {
      cached_params: None,
      encoded_buf: Vec::with_capacity(capacity),
      exceptions: Vec::with_capacity(DEFAULT_EXCEPTIONS_CAP),
    }
  }

  /// 重置已缓存的最优参数与内部缓冲区
  #[inline]
  pub fn reset(&mut self) {
    self.cached_params = None;
    self.encoded_buf.clear();
    self.exceptions.clear();
  }

  /// 压缩浮点数据切片并写入目标缓冲区（自适应 FOR 或 Delta 模式，复用采样参数与工作内存）
  #[inline]
  pub fn compress_into(&mut self, data: &[F], dst: &mut Vec<u8>) {
    self.cached_params = compress_into_engine(
      data,
      dst,
      false,
      self.cached_params,
      &mut self.encoded_buf,
      &mut self.exceptions,
    );
  }

  /// 强制使用 Delta 差分模式压缩浮点数据切片并写入目标缓冲区
  #[inline]
  pub fn compress_delta_into(&mut self, data: &[F], dst: &mut Vec<u8>) {
    self.cached_params = compress_into_engine(
      data,
      dst,
      true,
      self.cached_params,
      &mut self.encoded_buf,
      &mut self.exceptions,
    );
  }

  /// 压缩浮点数据切片并返回新分配的字节向量
  #[inline]
  pub fn compress(&mut self, data: &[F]) -> Vec<u8> {
    let mut dst = Vec::new();
    self.compress_into(data, &mut dst);
    dst
  }

  /// 强制使用 Delta 差分模式压缩浮点数据切片并返回新分配的字节向量
  #[inline]
  pub fn compress_delta(&mut self, data: &[F]) -> Vec<u8> {
    let mut dst = Vec::new();
    self.compress_delta_into(data, &mut dst);
    dst
  }

  /// 释放内部工作缓冲区多余分配的容量
  #[inline]
  pub fn shrink_to_fit(&mut self) {
    self.encoded_buf.shrink_to_fit();
    self.exceptions.shrink_to_fit();
  }

  /// 获取内部工作缓冲区的当前容量
  #[inline]
  pub fn capacity(&self) -> usize {
    self.encoded_buf.capacity()
  }
}
