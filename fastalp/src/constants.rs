use std::mem::size_of;

/// Bits per byte constant.
/// 每字节比特数常量
pub(crate) const BITS_PER_BYTE: usize = 8;

/// Bits in u64 constant.
/// u64 比特数常量
pub(crate) const BITS_U64: usize = 64;

/// Byte size of u64.
/// u64 字节大小
pub(crate) const BYTES_U64: usize = size_of::<u64>();

/// Format type identifier for f64 ALP compressed chunk.
/// f64 ALP 压缩数据块类型标识
pub const TYPE_F64: u8 = 1;

/// Format type identifier for f32 ALP compressed chunk.
/// f32 ALP 压缩数据块类型标识
pub const TYPE_F32: u8 = 2;

/// Format type identifier for f64 raw uncompressed fallback chunk.
/// f64 未压缩原始保底回退数据块类型标识
pub const TYPE_F64_RAW: u8 = 3;

/// Format type identifier for f32 raw uncompressed fallback chunk.
/// f32 未压缩原始保底回退数据块类型标识
pub const TYPE_F32_RAW: u8 = 4;

/// Format type identifier for f64 ALP Delta differential compressed chunk.
/// f64 ALP 时序差分压缩数据块类型标识
pub const TYPE_F64_DELTA: u8 = 5;

/// Format type identifier for f32 ALP Delta differential compressed chunk.
/// f32 ALP 时序差分压缩数据块类型标识
pub const TYPE_F32_DELTA: u8 = 6;

/// Format type identifier for f64 Decimal Division exact compressed chunk.
/// f64 十进制精确除法重构压缩数据块类型标识
pub const TYPE_F64_DEC: u8 = 7;

/// Format type identifier for f32 Decimal Division exact compressed chunk.
/// f32 十进制精确除法重构压缩数据块类型标识
pub const TYPE_F32_DEC: u8 = 8;

/// Format type identifier for f64 Decimal Division Delta compressed chunk.
/// f64 十进制精确除法时序差分压缩数据块类型标识
pub const TYPE_F64_DEC_DELTA: u8 = 9;

/// Format type identifier for f32 Decimal Division Delta compressed chunk.
/// f32 十进制精确除法时序差分压缩数据块类型标识
pub const TYPE_F32_DEC_DELTA: u8 = 10;

/// Format type identifier for f64 Dictionary compressed chunk.
/// f64 低基数紧凑字典压缩数据块类型标识
pub const TYPE_F64_DICT: u8 = 11;

/// Format type identifier for f32 Dictionary compressed chunk.
/// f32 低基数紧凑字典压缩数据块类型标识
pub const TYPE_F32_DICT: u8 = 12;

/// Format type identifier for f64 Real Doubles (ALP-RD) compressed chunk.
/// f64 真实双精度浮点高低位解耦（ALP-RD）压缩数据块类型标识
pub const TYPE_F64_RD: u8 = 13;

/// Format type identifier for f32 Real Doubles (ALP-RD) compressed chunk.
/// f32 真实单精度浮点高低位解耦（ALP-RD）压缩数据块类型标识
pub const TYPE_F32_RD: u8 = 14;

/// Maximum valid format type identifier.
/// 最大有效编码格式类型标识
pub const MAX_TYPE_BYTE: u8 = TYPE_F32_RD;

/// Maximum number of distinct entries in low-cardinality dictionary mode.
/// 低基数紧凑字典模式下支持的最大字典项数
pub const MAX_DICT_ENTRIES: usize = 64;

/// Size ratio threshold numerator for attempting ALP-RD bit decoupling (4/5 = 80%).
/// 尝试高低位解耦（ALP-RD）压缩的数据大小比例阈值分子 (4/5 = 80%)
pub(crate) const RD_SIZE_THRESHOLD_NUM: usize = 4;

/// Size ratio threshold denominator for attempting ALP-RD bit decoupling (4/5 = 80%).
/// 尝试高低位解耦（ALP-RD）压缩的数据大小比例阈值分母 (4/5 = 80%)
pub(crate) const RD_SIZE_THRESHOLD_DEN: usize = 5;

/// Maximum decimal exponent for double precision f64.
/// f64 最大十进制指数
pub(crate) const MAX_EXPONENT_F64: u8 = 18;

/// Maximum decimal exponent for single precision f32.
/// f32 最大十进制指数
pub(crate) const MAX_EXPONENT_F32: u8 = 10;

/// Maximum factor exponent for f64.
/// f64 最大因子指数
pub(crate) const MAX_FAC_F64: u8 = 8;

/// Maximum factor exponent for f32.
/// f32 最大因子指数
pub(crate) const MAX_FAC_F32: u8 = 4;

/// Maximum encodable finite threshold for f64.
/// f64 编码上限阈值
pub(crate) const ENCODING_UPPER_LIMIT_F64: f64 = 9223372036854774784.0;

/// Maximum encodable finite threshold for f32.
/// f32 编码上限阈值
pub(crate) const ENCODING_UPPER_LIMIT_F32: f32 = 2147483520.0;

/// Standard chunk size for typical ALP blocks.
/// 典型 ALP 标准块大小 (1024)
pub const CHUNK_SIZE: usize = 1024;
pub const CHUNK_SIZE_1024: usize = CHUNK_SIZE;

/// Type ID mask (lower 4 bits of descriptor byte: 0..=15).
/// 描述符字节低 4 位：编码类型掩码
pub const TYPE_MASK: u8 = 0x0F;

/// Repeat value run-length bitmask flag (bit 6 of descriptor byte).
/// 重复值时序行程掩码标志位 (描述符字节第 6 位)
pub const FLAG_REPEAT: u8 = 0x40;

/// Length tag bit shift (bits 4..=5 of descriptor byte).
/// 长度档位位移偏移 (第 4~5 位)
pub const LEN_TAG_SHIFT: u8 = 4;

/// Length tag bitmask (2 bits).
/// 长度档位掩码
pub const LEN_TAG_MASK: u8 = 0x03;

/// Length tag: 1-byte count (0..=255).
/// 长度档位：1 字节长度 (0..=255)
pub const LEN_TAG_U8: u8 = 0b00;

/// Length tag: 2-byte count (256..=65535, except 1024).
/// 长度档位：2 字节长度 (256..=65535, 排除 1024)
pub const LEN_TAG_U16: u8 = 0b01;

/// Length tag: 4-byte count (65536..=u32::MAX).
/// 长度档位：4 字节长度 (65536..=42亿，突破 65535 限制)
pub const LEN_TAG_U32: u8 = 0b10;

/// Length tag: Preset 1024 count (0 bytes for count field).
/// 长度档位：预设 1024 满块 (0 字节存储长度，极限精简)
pub const LEN_TAG_1024: u8 = 0b11;

/// Maximum acceptable number of exceptions per block before fallback to RAW uncompressed encoding.
/// 单块允许的最大异常值数量上限（超过此门限直接回退至 RAW 未压缩模式）
pub(crate) const MAX_EXCEPTIONS: usize = 128;

/// Exception count field length in bytes (u16).
/// 异常总数字段长度 (字节, u16)
pub(crate) const EXC_COUNT_LEN: usize = size_of::<u16>();

/// Exception position index field length in bytes (u16).
/// 异常位置索引字段长度 (字节, u16)
pub(crate) const EXC_POS_LEN: usize = size_of::<u16>();

/// Exception count field length in bytes for large arrays (u32).
/// 大数组异常总数字段长度 (字节, u32)
pub(crate) const EXC_COUNT_LEN_U32: usize = size_of::<u32>();

/// Exception position index field length in bytes for large arrays (u32).
/// 大数组异常位置索引字段长度 (字节, u32)
pub(crate) const EXC_POS_LEN_U32: usize = size_of::<u32>();

/// Number of sample points drawn during parameter search.
/// 参数推导搜索采样点数量
pub(crate) const SAMPLES_COUNT: usize = 32;

/// Early exit Bit-width threshold for early exit when 0 exceptions are found in parameter sampling.
/// 采样时当 0 异常且位宽不超过此门限时提前终止探测
pub(crate) const EARLY_EXIT_BIT_WIDTH: usize = 8;

/// Lookup table size for 1-bit unpacking.
/// 1 比特解包查找表大小
pub(crate) const LUT_SIZE_1BIT: usize = 2;

/// Lookup table size for 2-bit unpacking.
/// 2 比特解包查找表大小
pub(crate) const LUT_SIZE_2BIT: usize = 4;

/// Lookup table size for 4-bit unpacking.
/// 4 比特解包查找表大小
pub(crate) const LUT_SIZE_4BIT: usize = 16;

/// Static positive power table for f64 (10^0 .. 10^18).
/// f64 静态正幂表 10^0 .. 10^18
pub(crate) const EXP_ARR_F64: [f64; 19] = [
  1.0,
  10.0,
  100.0,
  1_000.0,
  10_000.0,
  100_000.0,
  1_000_000.0,
  10_000_000.0,
  100_000_000.0,
  1_000_000_000.0,
  10_000_000_000.0,
  100_000_000_000.0,
  1_000_000_000_000.0,
  10_000_000_000_000.0,
  100_000_000_000_000.0,
  1_000_000_000_000_000.0,
  10_000_000_000_000_000.0,
  100_000_000_000_000_000.0,
  1_000_000_000_000_000_000.0,
];

/// Static negative power table for f64 (10^-0 .. 10^-18).
/// f64 静态负幂表 10^-0 .. 10^-18
pub(crate) const FRAC_ARR_F64: [f64; 19] = [
  1.0,
  0.1,
  0.01,
  0.001,
  0.0001,
  0.00001,
  0.000001,
  0.0000001,
  0.00000001,
  0.000000001,
  0.0000000001,
  0.00000000001,
  0.000000000001,
  0.0000000000001,
  0.00000000000001,
  0.000000000000001,
  0.0000000000000001,
  0.00000000000000001,
  0.000000000000000001,
];

/// Static integer factor table for f64 (10^0 .. 10^18).
/// f64 静态整型因子表 10^0 .. 10^18
pub(crate) const FACT_ARR_F64: [i64; 19] = [
  1,
  10,
  100,
  1_000,
  10_000,
  100_000,
  1_000_000,
  10_000_000,
  100_000_000,
  1_000_000_000,
  10_000_000_000,
  100_000_000_000,
  1_000_000_000_000,
  10_000_000_000_000,
  100_000_000_000_000,
  1_000_000_000_000_000,
  10_000_000_000_000_000,
  100_000_000_000_000_000,
  1_000_000_000_000_000_000,
];

/// Static positive power table for f32 (10^0 .. 10^10).
/// f32 静态正幂表 10^0 .. 10^10
pub(crate) const EXP_ARR_F32: [f32; 11] = [
  1.0,
  10.0,
  100.0,
  1_000.0,
  10_000.0,
  100_000.0,
  1_000_000.0,
  10_000_000.0,
  100_000_000.0,
  1_000_000_000.0,
  10_000_000_000.0,
];

/// Static negative power table for f32 (10^-0 .. 10^-10).
/// f32 静态负幂表 10^-0 .. 10^-10
pub(crate) const FRAC_ARR_F32: [f32; 11] = [
  1.0,
  0.1,
  0.01,
  0.001,
  0.0001,
  0.00001,
  0.000001,
  0.0000001,
  0.00000001,
  0.000000001,
  0.0000000001,
];

/// Static integer factor table for f32 (10^0 .. 10^10).
/// f32 静态整型因子表 10^0 .. 10^10
pub(crate) const FACT_ARR_F32: [i64; 11] = [
  1,
  10,
  100,
  1_000,
  10_000,
  100_000,
  1_000_000,
  10_000_000,
  100_000_000,
  1_000_000_000,
  10_000_000_000,
];
