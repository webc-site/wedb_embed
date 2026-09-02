/// Default Bloom Filter configuration constants.
/// 默认 Bloom Filter 配置常量
pub const DEFAULT_BF_CAPACITY: u32 = 100;
pub const DEFAULT_BF_ERROR_RATE: f64 = 0.01;
pub const DEFAULT_BF_EXPANSION: u16 = 2;

/// Default Cuckoo Filter configuration constants.
/// 默认 Cuckoo Filter 配置常量
pub const DEFAULT_CF_CAPACITY: u64 = 1024;
pub const DEFAULT_CF_BUCKET_SIZE: u8 = 2;
pub const DEFAULT_CF_MAX_ITERATIONS: u16 = 20;
pub const DEFAULT_CF_EXPANSION: u16 = 1;
pub const DEFAULT_CF_PAGE_SIZE: u32 = 2048;
pub const MAX_CF_EXPANSION: u16 = 32768;
