use strum::FromRepr;

/// Compact 1-byte data structure and metadata type tag enumeration.
/// 紧凑 1 字节数据结构与元数据类型标签枚举（单字节紧凑编码，单指令分支分派）
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, FromRepr)]
pub enum KeyTag {
  // 基础字符串键正交标签 (0x00)
  RawString = 0x00,
  // 复合结构元数据与数据标签 (0x01 ~ 0x22)
  HashMeta = 0x01,
  HashData = 0x02,
  ListMeta = 0x03,
  ListData = 0x04,
  SetMeta = 0x05,
  SetData = 0x06,
  ZSetMeta = 0x07,
  ZSetScore = 0x08,
  ZSetData = 0x09,
  BloomMeta = 0x0A,
  BloomData = 0x0B,
  CuckooMeta = 0x0C,
  CuckooData = 0x0D,
  BitmapMeta = 0x0E,
  BitmapData = 0x0F,
  HllMeta = 0x10,
  HllRaw = 0x11,
  JsonMeta = 0x12,
  JsonData = 0x13,
  SortedIntMeta = 0x14,
  SortedIntData = 0x15,
  StreamMeta = 0x16,
  StreamData = 0x17,
  StreamGroup = 0x18,
  StreamConsumer = 0x19,
  StreamPel = 0x1A,
  TDigestMeta = 0x1B,
  TDigestData = 0x1C,
  TimeSeriesMeta = 0x1D,
  TimeSeriesData = 0x1E,
  FtSchema = 0x1F,
  FtAlias = 0x20,
  FtIndex = 0x21,
  FtData = 0x22,
}

static TAG_SLICES: [[u8; 1]; 35] = {
  let mut arr = [[0u8; 1]; 35];
  let mut i = 0u8;
  while i < 35 {
    arr[i as usize] = [i];
    i += 1;
  }
  arr
};

impl KeyTag {
  /// Converts tag to a 1-byte static byte slice reference.
  /// 转换为 1 字节字节切片常量（编译期静态 LUT，零内存分配）
  #[inline(always)]
  pub const fn as_slice(&self) -> &'static [u8] {
    &TAG_SLICES[*self as usize]
  }

  /// Decodes tag from a single byte value.
  /// 单字节快速反解析（过程宏派生 `from_repr` 别名）
  #[inline(always)]
  pub fn from_u8(b: u8) -> Option<Self> {
    Self::from_repr(b)
  }

  #[inline(always)]
  pub const fn as_u8(&self) -> u8 {
    *self as u8
  }

  #[inline(always)]
  pub fn from_slice(slice: &[u8]) -> Option<Self> {
    if slice.len() == 1 {
      Self::from_u8(slice[0])
    } else {
      None
    }
  }

  #[inline]
  pub const fn type_name(&self) -> &'static str {
    match self {
      Self::RawString => "string",
      Self::HashMeta | Self::HashData => "hash",
      Self::ListMeta | Self::ListData => "list",
      Self::SetMeta | Self::SetData => "set",
      Self::ZSetMeta | Self::ZSetScore | Self::ZSetData => "zset",
      Self::BloomMeta | Self::BloomData => "MBbloom--",
      Self::CuckooMeta | Self::CuckooData => "MBbloomCF",
      Self::BitmapMeta | Self::BitmapData => "bitmap",
      Self::HllMeta | Self::HllRaw => "hyperloglog",
      Self::JsonMeta | Self::JsonData => "ReJSON-RL",
      Self::SortedIntMeta | Self::SortedIntData => "sortedint",
      Self::StreamMeta
      | Self::StreamData
      | Self::StreamGroup
      | Self::StreamConsumer
      | Self::StreamPel => "stream",
      Self::TDigestMeta | Self::TDigestData => "TDIS-TYPE",
      Self::TimeSeriesMeta | Self::TimeSeriesData => "timeseries",
      Self::FtSchema | Self::FtAlias | Self::FtIndex | Self::FtData => "ft",
    }
  }

  /// Checks whether the tag represents a metadata key.
  /// 判断当前 Tag 是否为元数据键标签
  #[inline(always)]
  pub const fn is_meta(&self) -> bool {
    matches!(
      self,
      Self::HashMeta
        | Self::ListMeta
        | Self::SetMeta
        | Self::ZSetMeta
        | Self::BloomMeta
        | Self::CuckooMeta
        | Self::BitmapMeta
        | Self::HllMeta
        | Self::JsonMeta
        | Self::SortedIntMeta
        | Self::StreamMeta
        | Self::TDigestMeta
        | Self::TimeSeriesMeta
        | Self::FtSchema
        | Self::FtAlias
    )
  }

  /// Checks whether the tag represents a subkey data item.
  /// 判断当前 Tag 是否为子键/数据键标签
  #[inline(always)]
  pub const fn is_data(&self) -> bool {
    matches!(
      self,
      Self::HashData
        | Self::ListData
        | Self::SetData
        | Self::ZSetScore
        | Self::ZSetData
        | Self::BloomData
        | Self::CuckooData
        | Self::BitmapData
        | Self::HllRaw
        | Self::JsonData
        | Self::SortedIntData
        | Self::StreamData
        | Self::StreamGroup
        | Self::StreamConsumer
        | Self::StreamPel
        | Self::TDigestData
        | Self::TimeSeriesData
        | Self::FtIndex
        | Self::FtData
    )
  }
}

/// List of all composite data structure metadata tags.
/// 所有复合类型的元数据 Tag 列表
pub const ALL_COMPOSITE_META_TAGS: &[&[u8]] = &[
  KeyTag::HashMeta.as_slice(),
  KeyTag::ListMeta.as_slice(),
  KeyTag::SetMeta.as_slice(),
  KeyTag::ZSetMeta.as_slice(),
  KeyTag::BloomMeta.as_slice(),
  KeyTag::CuckooMeta.as_slice(),
  KeyTag::BitmapMeta.as_slice(),
  KeyTag::HllMeta.as_slice(),
  KeyTag::JsonMeta.as_slice(),
  KeyTag::SortedIntMeta.as_slice(),
  KeyTag::StreamMeta.as_slice(),
  KeyTag::TDigestMeta.as_slice(),
  KeyTag::TimeSeriesMeta.as_slice(),
];

/// System internal domain tag following leading 0xFF byte.
/// 系统内部管理域标签（紧随首字节 0xFF 后的第 2 个字节）
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, FromRepr)]
pub enum SystemDomainTag {
  /// System tenant and authentication token metadata.
  /// 系统租户与 Token 元数据
  SysMeta = 0x01,
  /// Multi-database catalog directory.
  /// 多数据库 Catalog 目录
  Catalog = 0x02,
  /// Global TTL expiration index.
  /// 全局 TTL 时间索引
  TtlIndex = 0x03,
  /// Raft consensus log and state machine metadata.
  /// Raft 日志与状态机元数据
  RaftState = 0x04,
  /// Cluster topology and node configuration metadata.
  /// 集群拓扑元数据
  Cluster = 0x05,
}

impl SystemDomainTag {
  #[inline(always)]
  pub fn from_u8(b: u8) -> Option<Self> {
    Self::from_repr(b)
  }

  #[inline(always)]
  pub const fn as_u8(&self) -> u8 {
    *self as u8
  }
}

/// System tenant metadata subtag following SystemDomainTag::SysMeta.
/// 系统租户元数据子标签（紧随 SystemDomainTag::SysMeta 后的第 3 个字节）
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, FromRepr)]
pub enum SysMetaTag {
  /// Namespace name lookup index (\xFF\x01\x01).
  /// 命名空间名称索引 (\xFF\x01\x01)
  NsName = 0x01,
  /// Namespace numeric ID reverse lookup index (\xFF\x01\x02).
  /// 命名空间数字 ID 反查索引 (\xFF\x01\x02)
  NsId = 0x02,
  /// Namespace global auto-increment ID generator (\xFF\x01\x03).
  /// 命名空间全局自增 ID 发号器 (\xFF\x01\x03)
  NsNextId = 0x03,
  /// Namespace token authentication index (\xFF\x01\x04).
  /// 命名空间 Token 认证索引 (\xFF\x01\x04)
  NsToken = 0x04,
  /// Database auto-increment ID generator within namespace (\xFF\x01\x05).
  /// 命名空间内 DB 自增 ID 发号器 (\xFF\x01\x05)
  DbNextId = 0x05,
}

impl SysMetaTag {
  #[inline(always)]
  pub fn from_u8(b: u8) -> Option<Self> {
    Self::from_repr(b)
  }

  #[inline(always)]
  pub const fn as_u8(&self) -> u8 {
    *self as u8
  }
}
