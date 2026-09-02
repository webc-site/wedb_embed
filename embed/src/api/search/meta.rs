/// Target data structure type for indexing aligned with Apache Kvrocks IndexOnDataType.
/// 索引针对的数据类型（对标 Apache Kvrocks IndexOnDataType）
#[derive(
  Debug,
  Clone,
  Copy,
  PartialEq,
  Eq,
  bitcode::Encode,
  bitcode::Decode,
  strum::Display,
  strum::EnumString,
  strum::FromRepr,
)]
#[strum(ascii_case_insensitive)]
#[repr(u8)]
pub enum IndexOnDataType {
  #[strum(serialize = "HASH")]
  Hash = 2,
  #[strum(serialize = "JSON")]
  Json = 10,
}

impl IndexOnDataType {
  #[inline]
  pub const fn as_str(&self) -> &'static str {
    match self {
      Self::Hash => "HASH",
      Self::Json => "JSON",
    }
  }

  #[inline]
  pub fn parse(s: &str) -> Option<Self> {
    s.parse().ok()
  }
}

/// Index field type enumeration aligned with Apache Kvrocks IndexFieldType.
/// 索引字段类型（对标 Apache Kvrocks IndexFieldType）
#[derive(
  Debug,
  Clone,
  Copy,
  PartialEq,
  Eq,
  bitcode::Encode,
  bitcode::Decode,
  strum::Display,
  strum::EnumString,
  strum::FromRepr,
)]
#[strum(ascii_case_insensitive)]
#[repr(u8)]
pub enum IndexFieldType {
  #[strum(serialize = "TEXT")]
  Text = 0,
  #[strum(serialize = "TAG")]
  Tag = 1,
  #[strum(serialize = "NUMERIC")]
  Numeric = 2,
  #[strum(serialize = "VECTOR")]
  Vector = 3,
  #[strum(serialize = "GEO")]
  Geo = 4,
}

impl IndexFieldType {
  #[inline]
  pub const fn as_str(&self) -> &'static str {
    match self {
      Self::Text => "TEXT",
      Self::Tag => "TAG",
      Self::Numeric => "NUMERIC",
      Self::Vector => "VECTOR",
      Self::Geo => "GEO",
    }
  }

  #[inline]
  pub fn parse(s: &str) -> Option<Self> {
    s.parse().ok()
  }
}

/// Vector numeric data type aligned with Apache Kvrocks VectorType.
/// 向量数据类型（对标 Apache Kvrocks VectorType）
#[derive(
  Debug,
  Clone,
  Copy,
  PartialEq,
  Eq,
  Default,
  bitcode::Encode,
  bitcode::Decode,
  strum::Display,
  strum::EnumString,
  strum::FromRepr,
)]
#[strum(ascii_case_insensitive)]
#[repr(u8)]
pub enum VectorType {
  #[default]
  #[strum(serialize = "FLOAT64")]
  Float64 = 1,
  #[strum(serialize = "FLOAT32")]
  Float32 = 2,
}

impl VectorType {
  #[inline]
  pub const fn as_str(&self) -> &'static str {
    match self {
      Self::Float64 => "FLOAT64",
      Self::Float32 => "FLOAT32",
    }
  }

  #[inline]
  pub fn parse(s: &str) -> Option<Self> {
    s.parse().ok()
  }

  #[inline]
  pub const fn byte_size(&self) -> usize {
    match self {
      Self::Float64 => 8,
      Self::Float32 => 4,
    }
  }
}

/// Vector distance metric aligned with Apache Kvrocks DistanceMetric.
/// 向量距离度量方式（对标 Apache Kvrocks DistanceMetric）
#[derive(
  Debug,
  Clone,
  Copy,
  PartialEq,
  Eq,
  Default,
  bitcode::Encode,
  bitcode::Decode,
  strum::Display,
  strum::EnumString,
  strum::FromRepr,
)]
#[strum(ascii_case_insensitive)]
#[repr(u8)]
pub enum DistanceMetric {
  #[default]
  #[strum(serialize = "L2")]
  L2 = 0,
  #[strum(serialize = "IP")]
  IP = 1,
  #[strum(serialize = "COSINE")]
  Cosine = 2,
}

impl DistanceMetric {
  #[inline]
  pub const fn as_str(&self) -> &'static str {
    match self {
      Self::L2 => "L2",
      Self::IP => "IP",
      Self::Cosine => "COSINE",
    }
  }

  #[inline]
  pub fn parse(s: &str) -> Option<Self> {
    s.parse().ok()
  }
}

/// Vector index construction algorithm type (Flat, HNSW).
/// 向量索引构建算法
#[derive(
  Debug,
  Clone,
  Copy,
  PartialEq,
  Eq,
  Default,
  bitcode::Encode,
  bitcode::Decode,
  strum::Display,
  strum::EnumString,
  strum::FromRepr,
)]
#[strum(ascii_case_insensitive)]
#[repr(u8)]
pub enum VectorAlgorithm {
  #[default]
  #[strum(serialize = "HNSW")]
  Hnsw = 1,
  #[strum(serialize = "FLAT")]
  Flat = 2,
}

impl VectorAlgorithm {
  #[inline]
  pub const fn as_str(&self) -> &'static str {
    match self {
      Self::Hnsw => "HNSW",
      Self::Flat => "FLAT",
    }
  }

  #[inline]
  pub fn parse(s: &str) -> Option<Self> {
    s.parse().ok()
  }
}

/// HNSW vector field metadata options aligned with Apache Kvrocks HnswVectorFieldMetadata.
/// 向量字段元数据（对标 Apache Kvrocks HnswVectorFieldMetadata）
#[derive(Debug, Clone, PartialEq, bitcode::Encode, bitcode::Decode)]
pub struct VectorFieldMetadata {
  pub vector_type: VectorType,
  pub dim: usize,
  pub distance_metric: DistanceMetric,
  pub algorithm: VectorAlgorithm,
  pub initial_cap: usize,
  pub m: usize,
  pub ef_construction: usize,
  pub ef_runtime: usize,
  pub epsilon: f64,
  pub num_levels: u16,
}

impl Default for VectorFieldMetadata {
  fn default() -> Self {
    Self {
      vector_type: VectorType::Float64,
      dim: 0,
      distance_metric: DistanceMetric::Cosine,
      algorithm: VectorAlgorithm::Hnsw,
      initial_cap: 500_000,
      m: 16,
      ef_construction: 200,
      ef_runtime: 10,
      epsilon: 0.01,
      num_levels: 0,
    }
  }
}

impl VectorFieldMetadata {
  pub fn new(dim: usize, distance_metric: DistanceMetric) -> Self {
    Self {
      dim,
      distance_metric,
      ..Default::default()
    }
  }
}

/// Index field definition (aligned with Apache Kvrocks FieldInfo and IndexFieldMetadata).
/// 索引字段定义（对标 Apache Kvrocks FieldInfo 与 IndexFieldMetadata）
#[derive(Debug, Clone, PartialEq, bitcode::Encode, bitcode::Decode)]
pub struct IndexField {
  pub name: String,
  pub field_type: IndexFieldType,
  pub alias: Option<String>,
  pub separator: Option<char>,
  pub case_sensitive: bool,
  pub weight: f64,
  pub sortable: bool,
  pub noindex: bool,
  pub unf: bool, // Un-Normalized Form
  pub vector_meta: Option<VectorFieldMetadata>,
}

impl IndexField {
  pub fn new(name: impl Into<String>, field_type: IndexFieldType) -> Self {
    Self {
      name: name.into(),
      field_type,
      alias: None,
      separator: None,
      case_sensitive: false,
      weight: 1.0,
      sortable: false,
      noindex: false,
      unf: false,
      vector_meta: None,
    }
  }

  pub fn with_alias(mut self, alias: impl Into<String>) -> Self {
    self.alias = Some(alias.into());
    self
  }

  pub fn with_text(name: impl Into<String>, weight: f64, sortable: bool) -> Self {
    Self {
      name: name.into(),
      field_type: IndexFieldType::Text,
      alias: None,
      separator: None,
      case_sensitive: false,
      weight,
      sortable,
      noindex: false,
      unf: false,
      vector_meta: None,
    }
  }

  pub fn with_tag(name: impl Into<String>, separator: Option<char>, case_sensitive: bool) -> Self {
    Self {
      name: name.into(),
      field_type: IndexFieldType::Tag,
      alias: None,
      separator,
      case_sensitive,
      weight: 1.0,
      sortable: false,
      noindex: false,
      unf: false,
      vector_meta: None,
    }
  }

  pub fn with_numeric(name: impl Into<String>, sortable: bool) -> Self {
    Self {
      name: name.into(),
      field_type: IndexFieldType::Numeric,
      alias: None,
      separator: None,
      case_sensitive: false,
      weight: 1.0,
      sortable,
      noindex: false,
      unf: false,
      vector_meta: None,
    }
  }

  pub fn with_geo(name: impl Into<String>, sortable: bool) -> Self {
    Self {
      name: name.into(),
      field_type: IndexFieldType::Geo,
      alias: None,
      separator: None,
      case_sensitive: false,
      weight: 1.0,
      sortable,
      noindex: false,
      unf: false,
      vector_meta: None,
    }
  }

  pub fn with_vector(name: impl Into<String>, dim: usize, metric: DistanceMetric) -> Self {
    Self {
      name: name.into(),
      field_type: IndexFieldType::Vector,
      alias: None,
      separator: None,
      case_sensitive: false,
      weight: 1.0,
      sortable: true,
      noindex: false,
      unf: false,
      vector_meta: Some(VectorFieldMetadata::new(dim, metric)),
    }
  }
}

/// Full-text search engine schema definition aligned with Apache Kvrocks kqir::IndexInfo.
/// 全文检索引擎模式定义（对标 Apache Kvrocks kqir::IndexInfo）
#[derive(Debug, Clone, PartialEq, bitcode::Encode, bitcode::Decode)]
pub struct SearchIndexSchema {
  pub name: String,
  pub on_data_type: IndexOnDataType,
  pub prefixes: Vec<String>,
  pub filter: Option<String>,
  pub default_score: f64,
  pub score_field: Option<String>,
  pub payload_field: Option<String>,
  pub language: Option<String>,
  pub language_field: Option<String>,
  pub max_text_fields: bool,
  pub no_offsets: bool,
  pub no_hl: bool,
  pub no_fields: bool,
  pub no_freqs: bool,
  pub stop_words: Vec<String>,
  pub fields: Vec<IndexField>,
}

impl SearchIndexSchema {
  pub fn new(name: impl Into<String>, prefixes: Vec<String>, fields: Vec<String>) -> Self {
    let field_structs = fields
      .into_iter()
      .map(|f| IndexField::new(f, IndexFieldType::Text))
      .collect();
    Self {
      name: name.into(),
      on_data_type: IndexOnDataType::Hash,
      prefixes,
      filter: None,
      default_score: 1.0,
      score_field: None,
      payload_field: None,
      language: None,
      language_field: None,
      max_text_fields: false,
      no_offsets: false,
      no_hl: false,
      no_fields: false,
      no_freqs: false,
      stop_words: Vec::new(),
      fields: field_structs,
    }
  }

  pub fn with_full_spec(
    name: impl Into<String>,
    on_data_type: IndexOnDataType,
    prefixes: Vec<String>,
    fields: Vec<IndexField>,
  ) -> Self {
    Self {
      name: name.into(),
      on_data_type,
      prefixes,
      filter: None,
      default_score: 1.0,
      score_field: None,
      payload_field: None,
      language: None,
      language_field: None,
      max_text_fields: false,
      no_offsets: false,
      no_hl: false,
      no_fields: false,
      no_freqs: false,
      stop_words: Vec::new(),
      fields,
    }
  }

  #[inline]
  pub fn add_field(&mut self, field: IndexField) {
    self.fields.push(field);
  }

  #[inline]
  pub fn get_field(&self, name: &str) -> Option<&IndexField> {
    self
      .fields
      .iter()
      .find(|f| f.name.as_str() == name || f.alias.as_deref() == Some(name))
  }

  #[inline]
  pub fn matches_key(&self, key: &str) -> bool {
    if self.prefixes.is_empty() {
      return true;
    }
    self.prefixes.iter().any(|p| key.starts_with(p.as_str()))
  }
}
