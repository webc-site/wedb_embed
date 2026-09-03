# wedb_embed_engine : 嵌入式键值存储引擎通用 Trait 抽象定义

`wedb_embed_engine` 为嵌入式键值存储引擎提供统一的零抽象开销 Trait 定义规范。

通过将底层物理分区、键值生命周期、双向区间迭代以及跨分区原子批量写入抽象为标准 Rust Trait，<br>
上层多模型数据库（包括 Redis 兼容数据结构、向量索引与全文检索等）能够与底层物理存储引擎（例如 LSM-Tree、B+Tree 或内存引擎）解耦。

---

## 功能特性

在嵌入式数据库架构设计中，将高层数据结构与底层存储引擎解耦，<br>
能够提升模块独立性、测试便利性与存储后端切换灵活性。

`wedb_embed_engine` 核心能力包括：

- **标准化存储接口**：<br>
  统一分区生命周期、点查、范围扫描与持久化操作规范。

- **零拷贝数据访问**：<br>
  利用 `Deref<Target = [u8]>` 关联类型实现直接借用底层存储缓冲区，避免堆内存分配。

- **跨分区原子写入**：<br>
  协调跨越多个分区/键空间的写批次，保障多分区事务原子提交。

- **双向范围扫描**：<br>
  统一抽象前缀匹配与正反向区间扫描，提供标准双向迭代器支持。

---

## 使用示例

以下代码演示如何基于 `wedb_embed_engine` 导出的 Trait 进行存储分区操作与跨分区原子批处理：

```rust
use std::ops::Bound;
use wedb_embed_engine::{Batch, Engine, KvEntry, Partition};

fn demo<E: Engine>(engine: &E) -> Result<(), E::Error> {
  // 打开或创建存储分区
  let data_part = engine.partition("data")?;
  let index_part = engine.partition("index")?;

  // 键值点查与写入
  data_part.insert(b"user:001", b"Alice")?;
  if let Some(val) = data_part.get(b"user:001")? {
    assert_eq!(&*val, b"Alice");
  }

  // 前缀双向扫描
  data_part.insert(b"user:002", b"Bob")?;
  data_part.insert(b"user:003", b"Charlie")?;

  let mut iter = data_part.prefix(b"user:");
  while let Some(res) = iter.next() {
    let entry = res?;
    println!("Key: {:?}, Val: {:?}", &*entry.key(), &*entry.value());
  }

  // 有界区间范围扫描
  let range_iter = data_part.range((
    Bound::Included(b"user:001"),
    Bound::Excluded(b"user:003"),
  ));
  for res in range_iter {
    let entry = res?;
    println!("Range Entry: {:?} => {:?}", &*entry.key(), &*entry.value());
  }

  // 跨分区原子批量写入
  let mut batch = engine.batch();
  batch.insert(&data_part, b"user:004", b"David");
  batch.insert(&index_part, b"idx:david", b"user:004");
  batch.rm(&data_part, b"user:001");
  batch.commit()?;

  // 持久化刷盘与磁盘空间查询
  engine.persist()?;
  let _bytes = engine.disk_space()?;

  Ok(())
}
```

---

## 核心特性

- **零拷贝借用**：<br>
  键与值均通过 `Deref<Target = [u8]>` 暴露字节切片，自底层页缓存或内存映射区域读取时无需堆内存分配。

- **元组通用实现**：<br>
  为任意满足 `Deref<Target = [u8]>` 的标准二元组 `(K, V)` 自动实现 `KvEntry` Trait。

- **物理分区隔离**：<br>
  原生支持命名分区（Keyspace），提供元数据与业务数据的物理隔离能力。

- **跨分区原子批处理**：<br>
  `Batch` Trait 抽象跨分区原子写入，确保崩溃恢复过程中的数据一致性。

- **双向范围检索**：<br>
  `Partition::iter`、`Partition::prefix` 与 `Partition::range` 均返回 `DoubleEndedIterator`，支持正向与反向双向遍历。

- **元数据与空间计量**：<br>
  内置分区非空校验、条目计数、SST/Blob 文件统计以及物理磁盘空间统计方法。

---

## 架构设计

`wedb_embed_engine` 确立了上层数据库模型与底层物理存储介质之间的标准抽象边界：

```mermaid
graph TD
  HighLevel["上层数据模型 / 多模型数据库层"] --> EngineTrait["Engine Trait"]
  HighLevel --> PartitionTrait["Partition Trait"]
  HighLevel --> BatchTrait["Batch Trait"]

  subgraph AbstractionLayer["wedb_embed_engine 抽象层"]
    EngineTrait --> PartitionTrait
    EngineTrait --> BatchTrait
    PartitionTrait --> KvEntryTrait["KvEntry Trait"]
    PartitionTrait --> IterTrait["DoubleEndedIterator"]
  end

  subgraph BackendEngines["具体底层存储引擎实现"]
    FjallEngine["Fjall LSM-Tree 存储引擎"]
    MemEngine["内存键值存储引擎"]
    CustomEngine["自定义存储后端"]
  end

  EngineTrait -.->|实现| FjallEngine
  EngineTrait -.->|实现| MemEngine
  EngineTrait -.->|实现| CustomEngine
```

### 模块调用流程

- **引擎初始化**：<br>
  调用方构建并持有具体 `Engine` 实现实例。

- **获取分区句柄**：<br>
  通过 `engine.partition(name)` 获取对应分区的 `Partition` 句柄。

- **点查与扫描**：<br>
  调用方直接在分区句柄上执行点查（`get`、`contains_key`、`size_of`）或范围遍历（`prefix`、`range`、`iter`）。

- **批量事务写入**：<br>
  通过 `engine.batch()` 构建批次，向各分区注册写入/删除操作（`insert`、`rm`、`rm_weak`），调用 `batch.commit()` 完成跨分区原子提交。

- **持久化刷盘**：<br>
  调用 `engine.persist()` 将内存表与预写日志同步刷盘至持久化存储。

---

## 技术栈

- **开发语言**：Rust Edition 2024
- **依赖说明**：Rust 标准库（`std::ops::{Bound, Deref}`, `std::error::Error`）
- **运行时开销**：零第三方运行时依赖

---

## 目录结构

```
wedb_embed_engine/
├── Cargo.toml
├── README.md
├── README.mdt
├── readme/
│   ├── en.md
│   └── zh.md
├── src/
│   ├── lib.rs
│   └── traits.rs
├── test.sh
└── tests/
    └── main.rs
```

---

## 核心接口

### `KvEntry`

存储迭代器产出的键值条目抽象。

- **泛型通用实现**：<br>
  `impl<K, V> KvEntry for (K, V) where K: Deref<Target = [u8]>, V: Deref<Target = [u8]>`
- **关联类型**：<br>
  - `type Key: Deref<Target = [u8]>`：实现字节切片解引用的键类型。<br>
  - `type Value: Deref<Target = [u8]>`：实现字节切片解引用的值类型。
- **接口方法**：<br>
  - `fn key(&self) -> &Self::Key`：获取条目的键引用。<br>
  - `fn value(&self) -> &Self::Value`：获取条目的值引用。

### `Partition`

具备读写能力的独立键空间/分区接口抽象。

- **Trait 约束**：`Clone + Send + Sync + 'static`
- **关联类型**：<br>
  - `type Error: StdError + Send + Sync + 'static`：分区操作关联错误类型。<br>
  - `type Value: Deref<Target = [u8]>`：查询返回的值类型。<br>
  - `type Entry<'a>: KvEntry where Self: 'a`：迭代器产出的条目类型。<br>
  - `type Iter<'a>: Iterator<Item = Result<Self::Entry<'a>, Self::Error>> + DoubleEndedIterator where Self: 'a`：双向迭代器类型。
- **接口方法**：<br>
  - `fn get(&self, key: &[u8]) -> Result<Option<Self::Value>, Self::Error>`：获取指定键对应的值。<br>
  - `fn size_of(&self, key: &[u8]) -> Result<Option<usize>, Self::Error>`：获取指定键对应值的字节大小。<br>
  - `fn contains_key(&self, key: &[u8]) -> Result<bool, Self::Error>`：检查分区中是否存在指定键。<br>
  - `fn is_empty(&self) -> Result<bool, Self::Error>`：检查分区是否为空。<br>
  - `fn len(&self) -> Result<usize, Self::Error>`：返回分区中的条目总数。<br>
  - `fn approximate_len(&self) -> Result<usize, Self::Error>`：以 O(1) 复杂度获取分区的近似条目数。<br>
  - `fn insert(&self, key: &[u8], value: &[u8]) -> Result<(), Self::Error>`：向分区中插入键值对。<br>
  - `fn rm(&self, key: &[u8]) -> Result<(), Self::Error>`：从分区中删除指定键。<br>
  - `fn rm_weak(&self, key: &[u8]) -> Result<(), Self::Error>`：从分区中弱墓碑删除指定键。<br>
  - `fn clear(&self) -> Result<(), Self::Error>`：清空当前分区中的全部物理条目。<br>
  - `fn iter(&self) -> Self::Iter<'_>`：返回遍历分区中所有条目的双向迭代器。<br>
  - `fn prefix(&self, prefix: &[u8]) -> Self::Iter<'_>`：返回遍历指定前缀匹配条目的双向迭代器。<br>
  - `fn range(&self, range: (Bound<&[u8]>, Bound<&[u8]>)) -> Self::Iter<'_>`：返回遍历指定键区间内条目的双向迭代器。<br>
  - `fn first_entry(&self) -> Result<Option<Self::Entry<'_>>, Self::Error>`：获取分区首个键值对条目。<br>
  - `fn last_entry(&self) -> Result<Option<Self::Entry<'_>>, Self::Error>`：获取分区末尾键值对条目。<br>
  - `fn is_kv_separated(&self) -> bool`：返回是否启用了大 Value 键值分离存储。<br>
  - `fn fragmented_blob_bytes(&self) -> u64`：返回未引用的陈旧 Blob 磁盘占用字节数。<br>
  - `fn disk_space(&self) -> Result<u64, Self::Error>`：获取当前分区的近似物理磁盘占用字节数。<br>
  - `fn table_count(&self) -> usize`：返回当前分区的 SST 表文件总数。<br>
  - `fn blob_file_count(&self) -> usize`：返回当前分区的 Blob 文件总数。<br>
  - `fn compact(&self) -> Result<(), Self::Error>`：触发当前分区的全量压缩与空间整理。

### `Batch`

跨分区的原子批量写入抽象。

- **Trait 约束**：`Send`
- **关联类型**：<br>
  - `type Error: StdError + Send + Sync + 'static`：批处理操作关联错误类型。<br>
  - `type Partition: Partition<Error = Self::Error>`：目标分区类型。
- **接口方法**：<br>
  - `fn insert(&mut self, partition: &Self::Partition, key: &[u8], value: &[u8])`：向批次中添加键值插入操作。<br>
  - `fn rm(&mut self, partition: &Self::Partition, key: &[u8])`：向批次中添加键删除操作。<br>
  - `fn rm_weak(&mut self, partition: &Self::Partition, key: &[u8])`：向批次中添加弱墓碑键删除操作。<br>
  - `fn len(&self) -> usize`：返回当前批次中已排队的写入操作数。<br>
  - `fn is_empty(&self) -> bool`：检查当前写入批次是否为空。<br>
  - `fn commit(self) -> Result<(), Self::Error>`：原子提交批次中的全部写入操作至底层存储引擎。

### `Engine`

提供分区管理与事务批处理的底层存储引擎通用抽象。

- **Trait 约束**：`Send + Sync + 'static`
- **关联类型**：<br>
  - `type Error: StdError + Send + Sync + 'static`：存储引擎关联错误类型。<br>
  - `type Partition: Partition<Error = Self::Error>`：引擎管理的分区类型。<br>
  - `type Batch: Batch<Partition = Self::Partition, Error = Self::Error>`：引擎生成的写批次类型。
- **接口方法**：<br>
  - `fn partition(&self, name: &str) -> Result<Self::Partition, Self::Error>`：打开或创建指定名称的分区。<br>
  - `fn partition_exists(&self, name: &str) -> bool`：检查指定名称的分区是否存在。<br>
  - `fn list_partitions(&self) -> Result<Vec<String>, Self::Error>`：列出存储引擎中的所有分区名称。<br>
  - `fn rm_partition(&self, partition: &Self::Partition) -> Result<(), Self::Error>`：删除指定分区并回收其物理存储资源。<br>
  - `fn write_buffer_size(&self) -> u64`：获取写入缓冲区总内存占用字节数。<br>
  - `fn cache_size(&self) -> u64`：获取当前块缓存占用的内存字节数。<br>
  - `fn cache_capacity(&self) -> u64`：获取配置的块缓存容量字节数。<br>
  - `fn outstanding_flushes(&self) -> usize`：获取排队等待落盘的 Memtable 刷盘任务数。<br>
  - `fn active_compactions(&self) -> usize`：获取当前正在运行的压缩任务数。<br>
  - `fn compactions_completed(&self) -> usize`：获取已完成的压缩任务总数。<br>
  - `fn journal_count(&self) -> usize`：获取磁盘上的 WAL 日志文件数量。<br>
  - `fn journal_disk_space(&self) -> Result<u64, Self::Error>`：获取 WAL 日志占用的磁盘字节数。<br>
  - `fn batch(&self) -> Self::Batch`：创建新的原子批量写入批次。<br>
  - `fn batch_with_capacity(&self, capacity: usize) -> Self::Batch`：创建具有预分配容量槽位的新原子批量写入批次。<br>
  - `fn persist(&self) -> Result<(), Self::Error>`：将内存数据与预写日志刷盘持久化到存储介质。<br>
  - `fn disk_space(&self) -> Result<u64, Self::Error>`：获取整个存储引擎的近似物理磁盘占用字节数。<br>
  - `fn compact(&self) -> Result<(), Self::Error>`：触发所有分区的全量压缩与空间整理。