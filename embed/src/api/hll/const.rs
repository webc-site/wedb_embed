pub use crate::error::ERR_WRONG_TYPE;

/// Total HyperLogLog registers (2^14 = 16384, aligned with Kvrocks kHyperLogLogRegisterCount).
/// HyperLogLog 寄存器总数（2^14 = 16384，对标 Apache Kvrocks kHyperLogLogRegisterCount）
pub const HLL_REGISTERS: usize = 16384;
/// Dense storage bytes (16384 * 6 bits / 8 = 12288 bytes, aligned with Kvrocks kHyperLogLogRegisterBytes).
/// 密集存储字节数（16384 * 6 位 / 8 = 12288 字节，对标 Apache Kvrocks kHyperLogLogRegisterBytes）
pub const HLL_DENSE_SIZE: usize = 12288;
/// Bucket index bits (14 bits, aligned with Kvrocks kHyperLogLogRegisterCountPow).
/// 桶索引位数（14 位，对标 Apache Kvrocks kHyperLogLogRegisterCountPow）
pub const HLL_REGISTER_COUNT_POW: usize = 14;
/// Bucket index mask (0x3FFF, aligned with Kvrocks kHyperLogLogRegisterCountMask).
/// 桶索引掩码（0x3FFF，对标 Apache Kvrocks kHyperLogLogRegisterCountMask）
pub const HLL_REGISTER_COUNT_MASK: u64 = (1 << HLL_REGISTER_COUNT_POW) - 1;
/// Remaining hash bits (50 bits, aligned with Kvrocks kHyperLogLogHashBitCount).
/// 剩余哈希位数（50 位，对标 Apache Kvrocks kHyperLogLogHashBitCount）
pub const HLL_HASH_BIT_COUNT: usize = 50;
/// Single register bit width (6 bits, aligned with Kvrocks kHyperLogLogRegisterBits).
/// 单个寄存器位宽（6 位，对标 Apache Kvrocks kHyperLogLogRegisterBits）
pub const HLL_REGISTER_BITS: usize = 6;
/// 6-bit register max value (63, aligned with Kvrocks kHyperLogLogRegisterMax).
/// 6-bit 寄存器最大值（63，对标 Apache Kvrocks kHyperLogLogRegisterMax）
pub const HLL_REGISTER_MAX: u8 = (1 << HLL_REGISTER_BITS) - 1;
/// Asymptotic constant alpha_infinity = 0.5 / ln(2) (aligned with Kvrocks kHyperLogLogAlpha).
/// 渐近常数 alpha_infinity = 0.5 / ln(2)（对标 Apache Kvrocks kHyperLogLogAlpha）
pub const HLL_ALPHA_INF: f64 = 0.721_347_520_444_481_7;
/// Register count floating-point constant m = 16384.0.
/// 寄存器数量浮点常数 m = 16384.0
pub const HLL_M_F64: f64 = HLL_REGISTERS as f64;
/// Reciprocal of register count 1.0 / m.
/// 寄存器数量倒数 1.0 / m
pub const HLL_INV_M: f64 = 1.0 / HLL_M_F64;
/// Precomputed constant alpha * m^2 to eliminate runtime multiplications.
/// 预计算常量 alpha * m^2（编译期计算，避免运行时重复乘法）
pub const HLL_ALPHA_M_SQ: f64 = HLL_ALPHA_INF * HLL_M_F64 * HLL_M_F64;
/// Total segment count (16 segments, aligned with Kvrocks kHyperLogLogSegmentCount).
/// 分段总数（16 段，对标 Apache Kvrocks kHyperLogLogSegmentCount）
pub const HLL_SEGMENT_COUNT: usize = 16;
/// Registers per segment (1024, aligned with Kvrocks kHyperLogLogSegmentRegisters).
/// 每段寄存器数（1024，对标 Apache Kvrocks kHyperLogLogSegmentRegisters）
pub const HLL_SEGMENT_REGISTERS: usize = 1024;
/// Bytes per segment (768 bytes, aligned with Kvrocks kHyperLogLogSegmentBytes).
/// 每段字节数（768 字节，对标 Apache Kvrocks kHyperLogLogSegmentBytes）
pub const HLL_SEGMENT_BYTES: usize = 768;
/// Hash seed constant (aligned with Kvrocks kHyperLogLogHashSeed).
/// 哈希种子常量（对标 Apache Kvrocks kHyperLogLogHashSeed）
pub const HLL_HASH_SEED: u32 = 0xadc8_3b19;
