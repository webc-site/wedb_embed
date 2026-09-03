use std::{cmp::Ordering, collections::BinaryHeap};

use hipstr::HipStr;
use rapidhash::{HashSetExt, RapidHashMap as HashMap, RapidHashSet as HashSet};

use crate::{
  error::{Error, Result},
  search::{
    encoding::{compute_sq8_distance, compute_vector_distance},
    meta::DistanceMetric,
    node_pack::{NodePackRef, Sq8Vector},
  },
};

/// Candidate node distance score and ID pair (16-byte Copy primitive).
/// 候选节点得分与 ID 紧凑结构（纯 16 字节 Copy 原语，零堆内存分配）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Candidate {
  pub dist: f64,
  pub node_id: u64,
}

impl Eq for Candidate {}

impl PartialOrd for Candidate {
  #[inline]
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    Some(self.cmp(other))
  }
}

impl Ord for Candidate {
  #[inline]
  fn cmp(&self, other: &Self) -> Ordering {
    self.dist.total_cmp(&other.dist)
  }
}

/// Min-heap element wrapper for nearest-neighbor priority queues.
/// 最小堆包装器（用于优先队列弹出最近邻居，16 字节 Copy 原语）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MinCandidate(pub Candidate);

impl PartialOrd for MinCandidate {
  #[inline]
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    Some(self.cmp(other))
  }
}

impl Ord for MinCandidate {
  #[inline]
  fn cmp(&self, other: &Self) -> Ordering {
    other.0.cmp(&self.0)
  }
}

/// HNSW graph node supporting full precision and SQ8 scalar quantization.
/// HNSW 图节点（对标 Apache Kvrocks HnswNode，支持全精度与 SQ8 标量量化）
#[derive(Debug, Clone)]
pub struct HnswNode {
  pub doc_id: HipStr<'static>,
  pub node_id: u64,
  pub vector: Vec<f64>,
  pub sq8: Sq8Vector,
  pub level: usize,
  /// List of neighbor nodes per layer indexed by 0..=level.
  /// 每一层的邻居列表，索引为层号 0..=level
  pub neighbors: Vec<Vec<u64>>,
}

impl HnswNode {
  /// Composes storage key or prefix.
  /// 构造新节点（自动完成 SQ8 标量量化）
  #[inline]
  pub fn new(doc_id: HipStr<'static>, node_id: u64, vector: Vec<f64>, level: usize) -> Self {
    let sq8 = Sq8Vector::encode(&vector);
    let neighbors = vec![Vec::new(); level + 1];
    Self {
      doc_id,
      node_id,
      vector,
      sq8,
      level,
      neighbors,
    }
  }

  /// Encodes data into binary format.
  /// 编码指定层级的节点数据为 NodePack 字节流（供写入 Fjall LSM-Tree）
  #[inline]
  pub fn encode_level_pack(&self, level: usize, out: &mut Vec<u8>) {
    let neighbors = self.neighbors.get(level).map_or(&[][..], Vec::as_slice);
    NodePackRef::encode_sq8(
      self.sq8.scale,
      self.sq8.offset,
      &self.sq8.data,
      neighbors,
      out,
    );
  }

  /// Composes storage key or prefix.
  /// 从 NodePack 字节流反序列化构造节点
  #[inline]
  pub fn decode_level_pack(
    doc_id: HipStr<'static>,
    node_id: u64,
    level: usize,
    payload: &[u8],
    dim: usize,
  ) -> Result<Self> {
    let pack = NodePackRef::decode(payload, dim)?;
    let mut neighbors = vec![Vec::new(); level + 1];
    neighbors[level] = pack.to_neighbor_vec();
    let (vector, sq8) = if let Some(q) = pack.sq8_vector {
      let sq8 = Sq8Vector {
        scale: pack.sq8_scale,
        offset: pack.sq8_offset,
        data: q.to_vec(),
      };
      let vector = sq8.decode();
      (vector, sq8)
    } else {
      let vector = pack.to_f64_vec();
      let sq8 = Sq8Vector::encode(&vector);
      (vector, sq8)
    };

    Ok(Self {
      doc_id,
      node_id,
      vector,
      sq8,
      level,
      neighbors,
    })
  }
}

/// In-memory HNSW vector index graph (aligned with Apache Kvrocks HnswIndex and RediSearch).
/// 内存 HNSW 向量索引图（对标 Apache Kvrocks HnswIndex 与 RediSearch HNSW 实现）
#[derive(Debug, Clone)]
pub struct HnswGraph {
  pub dim: usize,
  pub distance_metric: DistanceMetric,
  pub m: usize,
  pub ef_construction: usize,
  pub ef_runtime: usize,
  pub epsilon: f64,
  pub max_level: usize,
  pub entry_point: Option<u64>,
  pub nodes: HashMap<u64, HnswNode>,
  pub doc_to_node: HashMap<HipStr<'static>, u64>,
  next_node_id: u64,
  level_mult: f64,
}

impl Default for HnswGraph {
  fn default() -> Self {
    Self::new(0, DistanceMetric::Cosine, 16, 200, 10, 0.01)
  }
}

impl HnswGraph {
  pub fn new(
    dim: usize,
    distance_metric: DistanceMetric,
    m: usize,
    ef_construction: usize,
    ef_runtime: usize,
    epsilon: f64,
  ) -> Self {
    let m_val = m.max(2);
    let level_mult = 1.0 / (m_val as f64).ln();
    Self {
      dim,
      distance_metric,
      m: m_val,
      ef_construction: ef_construction.max(1),
      ef_runtime: ef_runtime.max(1),
      epsilon,
      max_level: 0,
      entry_point: None,
      nodes: HashMap::default(),
      doc_to_node: HashMap::default(),
      next_node_id: 1,
      level_mult,
    }
  }

  /// Returns the number of levels in the HNSW graph aligned with Kvrocks.
  /// 返回当前图的层级数（对标 Apache Kvrocks metadata.num_levels）
  #[inline]
  pub fn num_levels(&self) -> u16 {
    if self.nodes.is_empty() {
      0
    } else {
      (self.max_level + 1) as u16
    }
  }

  /// Randomly selects layer for a newly inserted node aligned with Kvrocks.
  /// 随机生成新插入节点的层数（对标 Apache Kvrocks HnswIndex::RandomizeLayer）
  #[inline]
  pub fn random_level(&self) -> usize {
    let r: f64 = fastrand::f64();
    let r = r.max(f64::MIN_POSITIVE);
    ((-r.ln()) * self.level_mult).floor() as usize
  }

  /// Computes full-precision distance between two vectors.
  /// 计算两向量间距离（全精度）
  #[inline]
  pub fn dist(&self, v1: &[f64], v2: &[f64]) -> Result<f64> {
    compute_vector_distance(v1, v2, self.distance_metric)
  }

  /// Computes distance between two SQ8 quantized vectors using SIMD acceleration.
  /// 计算两 SQ8 量化向量间距离（硬件级 SIMD 极速计算）
  #[inline]
  pub fn dist_sq8(&self, q: &[i8], v: &[i8]) -> Result<f64> {
    compute_sq8_distance(q, v, self.distance_metric)
  }

  /// Inserts a new vector into the HNSW graph aligned with Kvrocks.
  /// 插入新向量（对标 Apache Kvrocks HnswIndex::InsertVectorEntry）
  pub fn insert(&mut self, doc_id: HipStr<'static>, vector: Vec<f64>) -> Result<u64> {
    if self.dim != 0 && vector.len() != self.dim {
      let dim = self.dim;
      let len = vector.len();
      return Err(Error::invalid_data(format!(
        "vector dimension mismatch: expected {dim}, got {len}"
      )));
    }
    if self.dim == 0 {
      self.dim = vector.len();
    }

    // 如果节点已存在，先删除旧节点
    if self.doc_to_node.contains_key(&doc_id) {
      self.delete(doc_id.as_str());
    }

    let node_id = self.next_node_id;
    self.next_node_id += 1;

    let node_level = self.random_level();
    let new_node = HnswNode::new(doc_id.clone(), node_id, vector.clone(), node_level);

    self.nodes.insert(node_id, new_node);
    self.doc_to_node.insert(doc_id, node_id);

    let Some(mut curr_ep) = self.entry_point else {
      self.entry_point = Some(node_id);
      self.max_level = node_level;
      return Ok(node_id);
    };
    let max_lvl = self.max_level;

    // 1. 从最高层下行到 node_level + 1 贪心搜索最近入口点
    if max_lvl > node_level {
      for lvl in (node_level + 1..=max_lvl).rev() {
        let mut changed = true;
        while changed {
          changed = false;
          let ep_node = match self.nodes.get(&curr_ep) {
            Some(n) => n,
            None => break,
          };
          let curr_dist = self.dist(&vector, &ep_node.vector)?;
          let mut closest_ep = None;
          let mut min_d = curr_dist;

          if lvl < ep_node.neighbors.len() {
            for &neighbor_id in &ep_node.neighbors[lvl] {
              if let Some(neighbor_node) = self.nodes.get(&neighbor_id) {
                let d = self.dist(&vector, &neighbor_node.vector)?;
                if d < min_d {
                  min_d = d;
                  closest_ep = Some(neighbor_id);
                }
              }
            }
          }
          if let Some(next_ep) = closest_ep {
            curr_ep = next_ep;
            changed = true;
          }
        }
      }
    }

    // 2. 从 min(node_level, max_lvl) 到 0 层逐层连边
    let mut curr_eps = vec![curr_ep];
    let insert_top_level = node_level.min(max_lvl);

    for lvl in (0..=insert_top_level).rev() {
      let candidates = self.search_layer_internal(&vector, &curr_eps, self.ef_construction, lvl)?;
      let m_max = if lvl == 0 { self.m * 2 } else { self.m };
      let selected = self.select_neighbors(&candidates, m_max);

      // 连接新节点与选取的邻居
      if let Some(node) = self.nodes.get_mut(&node_id) {
        node.neighbors[lvl] = selected.clone();
      }

      // 双向连边与邻居截断
      for &neighbor_id in &selected {
        let mut need_prune = false;
        let mut candidates_to_prune = Vec::new();
        if let Some(neighbor_node) = self.nodes.get_mut(&neighbor_id)
          && lvl < neighbor_node.neighbors.len()
        {
          if !neighbor_node.neighbors[lvl].contains(&node_id) {
            neighbor_node.neighbors[lvl].push(node_id);
          }
          if neighbor_node.neighbors[lvl].len() > m_max {
            need_prune = true;
            candidates_to_prune = neighbor_node.neighbors[lvl].clone();
          }
        }
        if need_prune && let Some(n_node) = self.nodes.get(&neighbor_id) {
          let pruned = self.select_neighbors_from_ids(&n_node.vector, &candidates_to_prune, m_max);
          if let Some(n_node_re) = self.nodes.get_mut(&neighbor_id) {
            n_node_re.neighbors[lvl] = pruned;
          }
        }
      }

      curr_eps = candidates.into_iter().map(|c| c.node_id).collect();
    }

    if node_level > self.max_level {
      self.max_level = node_level;
      self.entry_point = Some(node_id);
    }

    Ok(node_id)
  }

  /// Deletes a node from the HNSW graph aligned with Kvrocks.
  /// 删除节点（对标 Apache Kvrocks HnswIndex::DeleteVectorEntry）
  pub fn delete(&mut self, doc_id: &str) -> bool {
    let node_id = match self.doc_to_node.remove(doc_id) {
      Some(id) => id,
      None => return false,
    };
    let removed = match self.nodes.remove(&node_id) {
      Some(n) => n,
      None => return false,
    };

    // 清理所有邻居的反向连边
    for (lvl, n_list) in removed.neighbors.iter().enumerate() {
      for &neighbor_id in n_list {
        if let Some(neighbor_node) = self.nodes.get_mut(&neighbor_id)
          && lvl < neighbor_node.neighbors.len()
        {
          neighbor_node.neighbors[lvl].retain(|&id| id != node_id);
        }
      }
    }

    // 若删除的是入口点，更新入口点为剩余节点中层数最高者
    if self.entry_point == Some(node_id) {
      if self.nodes.is_empty() {
        self.entry_point = None;
        self.max_level = 0;
      } else {
        let mut best_ep = None;
        let mut best_lvl = 0;
        for (&id, node) in &self.nodes {
          if best_ep.is_none() || node.level >= best_lvl {
            best_ep = Some(id);
            best_lvl = node.level;
          }
        }
        self.entry_point = best_ep;
        self.max_level = best_lvl;
      }
    }

    true
  }

  /// Performs beam search in a single layer aligned with Apache Kvrocks.
  /// 单层 Beam Search 搜索邻近候选集（对标 Apache Kvrocks HnswIndex::SearchLayerInternal）
  pub fn search_layer_internal(
    &self,
    query: &[f64],
    entry_points: &[u64],
    ef: usize,
    level: usize,
  ) -> Result<Vec<Candidate>> {
    let mut visited: HashSet<u64> = HashSet::with_capacity(ef * 4);
    let mut explore_heap = BinaryHeap::with_capacity(ef * 2); // 小顶堆 MinCandidate
    let mut result_heap = BinaryHeap::with_capacity(ef + 1); // 大顶堆 Candidate (保持最近的 ef 个)

    for &ep in entry_points {
      if let Some(ep_node) = self.nodes.get(&ep) {
        let dist = self.dist(query, &ep_node.vector)?;
        let cand = Candidate { dist, node_id: ep };
        explore_heap.push(MinCandidate(cand));
        result_heap.push(cand);
        visited.insert(ep);
      }
    }

    while let Some(MinCandidate(curr)) = explore_heap.pop() {
      if let Some(furthest) = result_heap.peek()
        && curr.dist > furthest.dist
      {
        break;
      }

      if let Some(curr_node) = self.nodes.get(&curr.node_id)
        && level < curr_node.neighbors.len()
      {
        for &neighbor_id in &curr_node.neighbors[level] {
          if !visited.insert(neighbor_id) {
            continue;
          }
          if let Some(neighbor_node) = self.nodes.get(&neighbor_id) {
            let dist = self.dist(query, &neighbor_node.vector)?;
            let furthest_dist = result_heap.peek().map(|f| f.dist).unwrap_or(f64::INFINITY);

            if result_heap.len() < ef || dist < furthest_dist {
              let cand = Candidate {
                dist,
                node_id: neighbor_id,
              };
              explore_heap.push(MinCandidate(cand));
              result_heap.push(cand);
              if result_heap.len() > ef {
                result_heap.pop();
              }
            }
          }
        }
      }
    }

    let mut res: Vec<Candidate> = result_heap.into_vec();
    res.sort();
    Ok(res)
  }

  /// Selects neighbor candidates using heuristics aligned with Apache Kvrocks.
  /// 邻居启发式筛选（对标 Apache Kvrocks HnswIndex::SelectNeighbors）
  #[inline]
  pub fn select_neighbors(&self, candidates: &[Candidate], m_max: usize) -> Vec<u64> {
    candidates.iter().take(m_max).map(|c| c.node_id).collect()
  }

  pub fn select_neighbors_from_ids(
    &self,
    base_vec: &[f64],
    candidates: &[u64],
    m_max: usize,
  ) -> Vec<u64> {
    let mut scored: Vec<Candidate> = Vec::with_capacity(candidates.len());
    for &id in candidates {
      if let Some(node) = self.nodes.get(&id)
        && let Ok(dist) = self.dist(base_vec, &node.vector)
      {
        scored.push(Candidate { dist, node_id: id });
      }
    }
    scored.sort();
    scored.into_iter().take(m_max).map(|c| c.node_id).collect()
  }

  /// Robust neighbor pruning algorithm (RobustPrune) based on Vamana / DiskANN alpha-RNG with zero heap allocations.
  /// 基于 Vamana / DiskANN 的 $\alpha$-RNG 鲁棒邻居修剪算法（RobustPrune，零堆内存分配）
  ///
  /// The parameter alpha >= 1.0 controls long skip edges and directional diversity (default recommended 1.2).
  /// $\alpha \ge 1.0$ 控制长跳跃边与方向多样性（默认推荐 1.2）
  pub fn robust_prune(
    &self,
    base_vec: &[f64],
    candidates: &[u64],
    m_max: usize,
    alpha: f64,
  ) -> Vec<u64> {
    let mut scored: Vec<(f64, u64, &[f64])> = Vec::with_capacity(candidates.len());
    for &id in candidates {
      if let Some(node) = self.nodes.get(&id)
        && let Ok(d) = self.dist(base_vec, &node.vector)
      {
        scored.push((d, id, node.vector.as_slice()));
      }
    }
    scored.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(Ordering::Equal));

    let mut pruned: Vec<u64> = Vec::with_capacity(m_max);
    let mut pruned_vecs: Vec<&[f64]> = Vec::with_capacity(m_max);

    for (d_p_c, id_c, vec_c) in &scored {
      if pruned.len() >= m_max {
        break;
      }
      let mut keep = true;
      for &vec_r in &pruned_vecs {
        if let Ok(d_r_c) = self.dist(vec_r, vec_c)
          && alpha * d_r_c <= *d_p_c
        {
          keep = false;
          break;
        }
      }
      if keep {
        pruned.push(*id_c);
        pruned_vecs.push(vec_c);
      }
    }

    if pruned.len() < m_max {
      for (_, id_c, _) in scored {
        if !pruned.contains(&id_c) {
          pruned.push(id_c);
          if pruned.len() >= m_max {
            break;
          }
        }
      }
    }

    pruned
  }

  /// Performs K-Nearest-Neighbor search aligned with Apache Kvrocks HnswIndex::KnnSearch.
  /// 执行 KNN 近邻检索（对标 Apache Kvrocks HnswIndex::KnnSearch）
  pub fn search_knn(
    &self,
    query: &[f64],
    k: usize,
    ef_runtime: Option<usize>,
  ) -> Result<Vec<(f64, HipStr<'static>)>> {
    if k == 0 || self.nodes.is_empty() {
      return Ok(Vec::new());
    }
    let Some(mut curr_ep) = self.entry_point else {
      return Ok(Vec::new());
    };

    let max_lvl = self.max_level;

    // 1. 上层贪心搜索入口点
    for lvl in (1..=max_lvl).rev() {
      let mut changed = true;
      while changed {
        changed = false;
        let Some(ep_node) = self.nodes.get(&curr_ep) else {
          break;
        };
        let curr_dist = self.dist(query, &ep_node.vector)?;
        let mut closest_ep = None;
        let mut min_d = curr_dist;

        if lvl < ep_node.neighbors.len() {
          for &neighbor_id in &ep_node.neighbors[lvl] {
            if let Some(neighbor_node) = self.nodes.get(&neighbor_id) {
              let d = self.dist(query, &neighbor_node.vector)?;
              if d < min_d {
                min_d = d;
                closest_ep = Some(neighbor_id);
              }
            }
          }
        }
        if let Some(next_ep) = closest_ep {
          curr_ep = next_ep;
          changed = true;
        }
      }
    }

    // 2. 底层 0 层 Beam Search
    let ef = ef_runtime.unwrap_or(self.ef_runtime).max(k);
    let candidates = self.search_layer_internal(query, &[curr_ep], ef, 0)?;

    let results = candidates
      .into_iter()
      .take(k)
      .filter_map(|c| {
        self
          .nodes
          .get(&c.node_id)
          .map(|n| (c.dist, n.doc_id.clone()))
      })
      .collect();
    Ok(results)
  }

  /// Performs vector range query aligned with Apache Kvrocks VECTOR_RANGE.
  /// 执行范围检索（对标 Apache Kvrocks VECTOR_RANGE 检索）
  pub fn search_range(
    &self,
    query: &[f64],
    radius: f64,
    epsilon: Option<f64>,
  ) -> Result<Vec<(f64, HipStr<'static>)>> {
    let eps = epsilon.unwrap_or(self.epsilon);
    let effective_radius = radius * (1.0 + eps);
    let knn_candidates = self.search_knn(query, self.nodes.len(), Some(self.ef_runtime * 2))?;
    let filtered: Vec<(f64, HipStr<'static>)> = knn_candidates
      .into_iter()
      .filter(|(d, _)| *d <= effective_radius)
      .collect();
    Ok(filtered)
  }

  /// Expands search scope to locate candidate entry points aligned with Kvrocks.
  /// 扩展搜索范围（对标 Apache Kvrocks HnswIndex::ExpandSearchScope）
  pub fn expand_search_scope(
    &self,
    query: &[f64],
    initial_keys: &[(f64, HipStr<'static>)],
    visited: &mut HashSet<HipStr<'static>>,
  ) -> Result<Vec<(f64, HipStr<'static>)>> {
    let mut result = Vec::new();
    for (_, key) in initial_keys {
      if let Some(&node_id) = self.doc_to_node.get(key)
        && let Some(node) = self.nodes.get(&node_id)
        && !node.neighbors.is_empty()
      {
        for &neighbor_id in &node.neighbors[0] {
          if let Some(neighbor_node) = self.nodes.get(&neighbor_id) {
            if !visited.insert(neighbor_node.doc_id.clone()) {
              continue;
            }
            let dist = self.dist(query, &neighbor_node.vector)?;
            result.push((dist, neighbor_node.doc_id.clone()));
          }
        }
      }
    }
    result.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(Ordering::Equal));
    Ok(result)
  }

  /// Clears all nodes and edges from the index.
  /// 清空索引
  #[inline]
  pub fn clear(&mut self) {
    self.nodes.clear();
    self.doc_to_node.clear();
    self.entry_point = None;
    self.max_level = 0;
    self.next_node_id = 1;
  }
}
