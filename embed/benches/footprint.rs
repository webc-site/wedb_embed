use std::{
  env,
  fmt::Write as _,
  fs,
  io::{Read, Write},
  mem::MaybeUninit,
  os::unix::net::UnixStream,
  path::Path,
  process::Command,
  time::Duration,
};

use wedb_embed::{Fjall, WeDb};

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

const REDIS_SOCK: &str = "/tmp/wedb_redis_bench.sock";
const REDIS_DATA_DIR: &str = "/tmp/wedb_redis_bench_data";
pub const WEDB_BENCH_DIR: &str = "/tmp/wedb_bench_data_5gb";

fn get_data_scale() -> (usize, usize) {
  let target_mb = env::var("BENCH_DATA_MB")
    .ok()
    .and_then(|v| v.parse::<usize>().ok())
    .unwrap_or(5000);
  let scale = (target_mb / 100).max(1);
  let total_items = scale * 100_000;
  (scale, total_items)
}

// 常量定义：消除重复字符串常量分配
const F_NAME: &[u8] = b"name";
const F_DEPT: &[u8] = b"department";
const V_DEPT: &[u8] = b"Core Infrastructure Platform Engineering Service Group";
const F_STATUS: &[u8] = b"status";
const V_STATUS: &[u8] = b"active_enterprise_production_node";
const F_CONFIG: &[u8] = b"config";
const GEO_KEY: &[u8] = b"geo:datacenters";

/// 获取当前进程 Resident Set Size (RSS) 物理常驻内存
fn get_rss_bytes() -> u64 {
  let mut usage = MaybeUninit::<libc::rusage>::uninit();
  let ret = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
  if ret == 0 {
    let usage = unsafe { usage.assume_init() };
    #[cfg(target_os = "macos")]
    {
      usage.ru_maxrss as u64
    }
    #[cfg(target_os = "linux")]
    {
      if let Ok(s) = std::fs::read_to_string("/proc/self/statm") {
        if let Some(pages) = s.split_whitespace().nth(1) {
          if let Ok(p) = pages.parse::<u64>() {
            return p * 4096;
          }
        }
      }
      (usage.ru_maxrss as u64) * 1024
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
      usage.ru_maxrss as u64
    }
  } else {
    0
  }
}

/// 递归计算目录内全部文件实际物理磁盘占用总字节数
fn dir_size(path: &Path) -> u64 {
  let mut total = 0;
  if let Ok(entries) = fs::read_dir(path) {
    for entry in entries.flatten() {
      if let Ok(ft) = entry.file_type() {
        let p = entry.path();
        if ft.is_dir() {
          total += dir_size(&p);
        } else if let Ok(meta) = entry.metadata() {
          total += meta.len();
        }
      }
    }
  }
  total
}

/// 从 Redis INFO memory 输出中解析 used_memory 与 used_memory_rss
fn parse_redis_memory_from_str(s: &str) -> (u64, u64) {
  let mut mem = 0u64;
  let mut rss = 0u64;
  for line in s.lines() {
    let trimmed = line.trim();
    if let Some(val) = trimmed.strip_prefix("used_memory:") {
      if let Ok(v) = val.trim().parse::<u64>() {
        mem = v;
      }
    } else if let Some(val) = trimmed.strip_prefix("used_memory_rss:")
      && let Ok(v) = val.trim().parse::<u64>()
    {
      rss = v;
    }
  }
  (mem, rss)
}

/// 通过直连 Unix 套接字或 redis-cli 查询 Redis 内存占用
fn query_redis_memory() -> (u64, u64) {
  let mut mem = 0u64;
  let mut rss = 0u64;

  // 1. 优先使用直连 UnixStream（独立干净连接，带长超时）
  if let Ok(mut stream) = UnixStream::connect(REDIS_SOCK) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    if stream
      .write_all(b"*2\r\n$4\r\nINFO\r\n$6\r\nmemory\r\n")
      .is_ok()
    {
      let mut total_buf = Vec::with_capacity(8192);
      let mut chunk = [0u8; 4096];
      while let Ok(n) = stream.read(&mut chunk) {
        if n == 0 {
          break;
        }
        total_buf.extend_from_slice(&chunk[..n]);
        let s = String::from_utf8_lossy(&total_buf);
        let (m, r) = parse_redis_memory_from_str(&s);
        if m > 0 && r > 0 {
          mem = m;
          rss = r;
          break;
        }
      }
      if mem == 0 || rss == 0 {
        let s = String::from_utf8_lossy(&total_buf);
        let (m, r) = parse_redis_memory_from_str(&s);
        if m > 0 {
          mem = m;
        }
        if r > 0 {
          rss = r;
        }
      }
    }
  }

  // 2. 兜底回退：若直接通信异常或为 0，通过 redis-cli 获取
  if (mem == 0 || rss == 0)
    && let Ok(output) = Command::new("redis-cli")
      .args(["-s", REDIS_SOCK, "info", "memory"])
      .output()
    && output.status.success()
  {
    let s = String::from_utf8_lossy(&output.stdout);
    let (m, r) = parse_redis_memory_from_str(&s);
    if m > 0 {
      mem = m;
    }
    if r > 0 {
      rss = r;
    }
  }

  (mem, rss)
}

/// 触发 Redis 强制同步持久化落盘
fn force_redis_save() {
  if let Ok(mut stream) = UnixStream::connect(REDIS_SOCK) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(60)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(60)));
    let _ = stream.write_all(b"*1\r\n$4\r\nSAVE\r\n");
    let mut buf = [0u8; 128];
    let _ = stream.read(&mut buf);
  } else {
    let _ = Command::new("redis-cli")
      .args(["-s", REDIS_SOCK, "save"])
      .output();
  }
}

/// 针对极速灌入优化的 Redis 客户端（零堆分配指令编码 + 批量流水线发送）
struct FastRedisClient {
  stream: UnixStream,
  send_buf: Vec<u8>,
  recv_buf: [u8; 65536],
  pending_cmds: usize,
  itoa_buf: itoa::Buffer,
}

impl FastRedisClient {
  fn new() -> Option<Self> {
    let stream = UnixStream::connect(REDIS_SOCK).ok()?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(15)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(15)));
    Some(Self {
      stream,
      send_buf: Vec::with_capacity(256 * 1024),
      recv_buf: [0u8; 65536],
      pending_cmds: 0,
      itoa_buf: itoa::Buffer::new(),
    })
  }

  /// 发送单条命令并同步等待
  fn send_cmd(&mut self, args: &[&[u8]]) {
    self.push_cmd(args);
    self.flush();
  }

  /// 追加一条 RESP 编码指令到内部流水线缓冲区，达到阈值自动刷写
  fn push_cmd(&mut self, args: &[&[u8]]) {
    self.send_buf.push(b'*');
    self
      .send_buf
      .extend_from_slice(self.itoa_buf.format(args.len()).as_bytes());
    self.send_buf.extend_from_slice(b"\r\n");
    for &arg in args {
      self.send_buf.push(b'$');
      self
        .send_buf
        .extend_from_slice(self.itoa_buf.format(arg.len()).as_bytes());
      self.send_buf.extend_from_slice(b"\r\n");
      self.send_buf.extend_from_slice(arg);
      self.send_buf.extend_from_slice(b"\r\n");
    }
    self.pending_cmds += 1;
    if self.pending_cmds >= 1000 || self.send_buf.len() >= 128 * 1024 {
      self.flush();
    }
  }

  /// 刷写流水线中全部待发送指令并消费响应（使用 SIMD 换行计数加速）
  fn flush(&mut self) {
    if self.pending_cmds == 0 {
      return;
    }
    if self.stream.write_all(&self.send_buf).is_ok() {
      let mut remaining = self.pending_cmds;
      while remaining > 0 {
        match self.stream.read(&mut self.recv_buf) {
          Ok(n) if n > 0 => {
            let lines = memchr::memchr_iter(b'\n', &self.recv_buf[..n]).count();
            remaining = remaining.saturating_sub(lines.max(1));
          }
          _ => break,
        }
      }
    }
    self.send_buf.clear();
    self.pending_cmds = 0;
  }
}

/// 写入用户账号键名到复用缓冲区（零堆内存分配）
#[inline(always)]
fn write_user_account_key(i: usize, buf: &mut String) {
  buf.clear();
  let _ = write!(buf, "user:account:{:08}", i);
}

/// 写入完整 JSON 结构体到复用缓冲区（零堆内存分配）
#[inline(always)]
fn write_user_account_json(i: usize, buf: &mut String) {
  buf.clear();
  let _ = write!(
    buf,
    r#"{{"id":{},"uid":"usr_{:08x}","username":"EnterpriseUser_{}","email":"user_{}@corp.domain.internal","dept":"Distributed Systems & Infrastructure Platform Reliability Engineering Architecture Group","role":"Principal Database Reliability Architect and Storage Performance Lead","status":"active_verified_production_primary","tier":"enterprise_level_3_global_cluster","bio":"Lead architect for high-throughput distributed LSM-tree embedded database storage engines with Redis-compatible zero-allocation memory abstractions, LZ4 block compression, and sub-microsecond point lookup pipelines.","profile":{{"created_at":1710000000,"last_active":1725000000,"ip_address":"192.168.100.128","device":"macOS/AppleSilicon_ARM64_M2_Max_Workstation","auth_level":"multi_factor_hardware_fido2_authenticated","session_token":"sess_{:016x}","locale":"en_US.UTF-8","timezone":"UTC+8"}},"telemetry":{{"requests_today":142857,"cache_hit_rate":0.9991,"error_count":0,"p99_latency_ms":0.35,"bytes_transferred":104857600,"active_connections":64,"iops_read":125000,"iops_write":85000,"buffer_pool_hit_rate":0.9998,"wal_fsync_latency_us":12.5}},"features":{{"advanced_indexing":true,"wal_sync_immediate":false,"vector_search":true,"columnar_compression":"lz4_fast","tenant_isolation":true,"zero_copy_deserialization":true,"simd_accelerated_scanning":true,"lock_free_skip_list":true}},"security":{{"encryption_at_rest":"aes_256_gcm","tls_version":"1.3","audit_log_retention_days":365,"rbac_enforcement":true}},"tags":["rust","database","fjall","lsm-tree","embedded-engine","redis-api","zero-copy","distributed","in-process-db","high-throughput","sub-microsecond-latency","mimalloc","lock-free","columnar","simd"]}}"#,
    i, i, i, i, i
  );
}

fn main() {
  let (scale, total_items) = get_data_scale();
  let num_str = 50_000 * scale;
  let num_hash = 20_000 * scale;
  let num_list = 10_000 * scale;
  let num_set = 5_000 * scale;
  let num_zset = 5_000 * scale;
  let num_bitmap = 2_000 * scale;
  let num_ts = 3_000 * scale;
  let num_hll = 2_000 * scale;
  let num_geo = 2_000 * scale;
  let num_tdigest = 1_000 * scale;

  let mut raw_bytes = 0usize;

  // 清理并创建基准数据目录
  let wedb_data_path = Path::new(WEDB_BENCH_DIR);
  if wedb_data_path.exists() {
    let _ = fs::remove_dir_all(wedb_data_path);
  }
  let _ = fs::create_dir_all(wedb_data_path);

  // 1. 测试并灌入 WeDb (全 10 种格式数据灌入)
  let engine = Fjall::open(WEDB_BENCH_DIR).expect("open engine");
  let db = WeDb::new(engine).ns(0).expect("ns 0").db(0).expect("db 0");

  // 复用格式化缓冲区（彻底消除循环内堆内存分配）
  let mut k_buf = String::with_capacity(64);
  let mut v_buf = String::with_capacity(2048);
  let mut v1_buf = String::with_capacity(64);
  let mut v4_buf = String::with_capacity(128);

  // 格式 1: String 结构化实体 (~50% 权重)
  for i in 1..=num_str {
    write_user_account_key(i, &mut k_buf);
    write_user_account_json(i, &mut v_buf);
    raw_bytes += k_buf.len() + v_buf.len();
    let _ = db.set(k_buf.as_bytes(), v_buf.as_bytes(), []);
  }

  // 格式 2: Hash 多字段实体 (~20% 权重)
  for i in 1..=num_hash {
    k_buf.clear();
    let _ = write!(k_buf, "entity:hash:{:07}", i);
    v1_buf.clear();
    let _ = write!(v1_buf, "EntityName_{}", i);
    v4_buf.clear();
    let _ = write!(
      v4_buf,
      r#"{{"cluster_id":{},"shards":64,"replica":3,"lz4":true}}"#,
      i
    );
    raw_bytes += k_buf.len()
      + F_NAME.len()
      + v1_buf.len()
      + F_DEPT.len()
      + V_DEPT.len()
      + F_STATUS.len()
      + V_STATUS.len()
      + F_CONFIG.len()
      + v4_buf.len();
    let _ = db.hset(
      k_buf.as_bytes(),
      &[
        (F_NAME, v1_buf.as_bytes()),
        (F_DEPT, V_DEPT),
        (F_STATUS, V_STATUS),
        (F_CONFIG, v4_buf.as_bytes()),
      ],
    );
  }

  // 格式 3: List 消息队列与日志 (~10% 权重)
  for i in 1..=num_list {
    k_buf.clear();
    let _ = write!(k_buf, "queue:logs:{:04}", i % 500);
    v_buf.clear();
    let _ = write!(
      v_buf,
      "log_event_{:08}_system_metrics_heartbeat_payload_normal",
      i
    );
    raw_bytes += k_buf.len() + v_buf.len();
    let _ = db.lpush(k_buf.as_bytes(), &[v_buf.as_bytes()]);
  }

  // 格式 4: Set 标签集合 (~5% 权重)
  for i in 1..=num_set {
    k_buf.clear();
    let _ = write!(k_buf, "set:tags:{:04}", i % 200);
    v_buf.clear();
    let _ = write!(v_buf, "tag_item_{:07}_cluster_node", i);
    raw_bytes += k_buf.len() + v_buf.len();
    let _ = db.sadd(k_buf.as_bytes(), &[v_buf.as_bytes()]);
  }

  // 格式 5: ZSet 排序榜单 (~5% 权重)
  for i in 1..=num_zset {
    k_buf.clear();
    let _ = write!(k_buf, "zset:leaderboard:{:04}", i % 200);
    v_buf.clear();
    let _ = write!(v_buf, "player_rank_{:07}", i);
    raw_bytes += k_buf.len() + v_buf.len() + 8;
    let _ = db.zadd(k_buf.as_bytes(), &[(i as f64, v_buf.as_bytes())], []);
  }

  // 格式 6: Bitmap 位图 (~2% 权重)
  for i in 1..=num_bitmap {
    k_buf.clear();
    let _ = write!(k_buf, "bitmap:user_sign:{:04}", i % 100);
    raw_bytes += k_buf.len() + 8;
    let _ = db.setbit(k_buf.as_bytes(), i as u64, 1);
  }

  // 格式 7: TimeSeries 时序监控 (~3% 权重)
  for i in 1..=num_ts {
    k_buf.clear();
    let _ = write!(k_buf, "ts:cpu_load:{:03}", i % 50);
    raw_bytes += k_buf.len() + 16;
    let _ = db.ts_add(
      k_buf.as_bytes(),
      1710000000 + i as u64,
      (i % 100) as f64 + 0.5,
      None,
      None,
    );
  }

  // 格式 8: HyperLogLog 基数 UV (~2% 权重)
  for i in 1..=num_hll {
    k_buf.clear();
    let _ = write!(k_buf, "hll:visitor:{:03}", i % 50);
    v_buf.clear();
    let _ = write!(v_buf, "visitor_ip_uuid_{:08}", i);
    raw_bytes += k_buf.len() + v_buf.len();
    let _ = db.pfadd(k_buf.as_bytes(), &[v_buf.as_bytes()]);
  }

  // 格式 9: Geo 地理位置 (~2% 权重)
  for i in 1..=num_geo {
    v_buf.clear();
    let _ = write!(v_buf, "dc_node_{:06}", i);
    let lon = 116.4 + (i % 1000) as f64 * 0.001;
    let lat = 39.9 + (i % 1000) as f64 * 0.001;
    raw_bytes += GEO_KEY.len() + v_buf.len() + 16;
    let _ = db.geoadd(GEO_KEY, &[(lon, lat, v_buf.as_bytes())], []);
  }

  // 格式 10: TDigest 分位数 (~1% 权重)
  for i in 1..=num_tdigest {
    k_buf.clear();
    let _ = write!(k_buf, "td:latency:{:03}", i % 20);
    raw_bytes += k_buf.len() + 8;
    let _ = db.tdigest_add(k_buf.as_bytes(), &[(i % 1000) as f64]);
  }

  // 强制完整落盘
  db.wedb().persist().expect("wedb persist");

  // 实测 WeDb 落盘物理文件总大小 (完整持久化数据)
  let wedb_disk_bytes = dir_size(wedb_data_path);
  // 实测 WeDb 进程 Resident Set Size (RSS)
  let wedb_rss_bytes = get_rss_bytes();

  // 2. 测试并灌入 Redis (独立守护进程持久化与内存驻留)
  let mut redis_disk_bytes = 0u64;
  let mut redis_mem_bytes = 0u64;
  let mut redis_rss_bytes = 0u64;

  if let Some(mut redis) = FastRedisClient::new() {
    redis.send_cmd(&[b"FLUSHALL"]);

    let mut itoa_buf = itoa::Buffer::new();
    let mut zmij_lon = zmij::Buffer::new();
    let mut zmij_lat = zmij::Buffer::new();

    // String 50%
    for i in 1..=num_str {
      write_user_account_key(i, &mut k_buf);
      write_user_account_json(i, &mut v_buf);
      redis.push_cmd(&[b"SET", k_buf.as_bytes(), v_buf.as_bytes()]);
    }

    // Hash 20%
    for i in 1..=num_hash {
      k_buf.clear();
      let _ = write!(k_buf, "entity:hash:{:07}", i);
      v1_buf.clear();
      let _ = write!(v1_buf, "EntityName_{}", i);
      v4_buf.clear();
      let _ = write!(
        v4_buf,
        r#"{{"cluster_id":{},"shards":64,"replica":3,"lz4":true}}"#,
        i
      );
      redis.push_cmd(&[
        b"HSET",
        k_buf.as_bytes(),
        F_NAME,
        v1_buf.as_bytes(),
        F_DEPT,
        V_DEPT,
        F_STATUS,
        V_STATUS,
        F_CONFIG,
        v4_buf.as_bytes(),
      ]);
    }

    // List 10%
    for i in 1..=num_list {
      k_buf.clear();
      let _ = write!(k_buf, "queue:logs:{:04}", i % 500);
      v_buf.clear();
      let _ = write!(
        v_buf,
        "log_event_{:08}_system_metrics_heartbeat_payload_normal",
        i
      );
      redis.push_cmd(&[b"LPUSH", k_buf.as_bytes(), v_buf.as_bytes()]);
    }

    // Set 5%
    for i in 1..=num_set {
      k_buf.clear();
      let _ = write!(k_buf, "set:tags:{:04}", i % 200);
      v_buf.clear();
      let _ = write!(v_buf, "tag_item_{:07}_cluster_node", i);
      redis.push_cmd(&[b"SADD", k_buf.as_bytes(), v_buf.as_bytes()]);
    }

    // ZSet 5%
    for i in 1..=num_zset {
      k_buf.clear();
      let _ = write!(k_buf, "zset:leaderboard:{:04}", i % 200);
      let score_str = itoa_buf.format(i);
      v_buf.clear();
      let _ = write!(v_buf, "player_rank_{:07}", i);
      redis.push_cmd(&[
        b"ZADD",
        k_buf.as_bytes(),
        score_str.as_bytes(),
        v_buf.as_bytes(),
      ]);
    }

    // Bitmap 2%
    for i in 1..=num_bitmap {
      k_buf.clear();
      let _ = write!(k_buf, "bitmap:user_sign:{:04}", i % 100);
      let offset_str = itoa_buf.format(i);
      redis.push_cmd(&[b"SETBIT", k_buf.as_bytes(), offset_str.as_bytes(), b"1"]);
    }

    // HyperLogLog 2%
    for i in 1..=num_hll {
      k_buf.clear();
      let _ = write!(k_buf, "hll:visitor:{:03}", i % 50);
      v_buf.clear();
      let _ = write!(v_buf, "visitor_ip_uuid_{:08}", i);
      redis.push_cmd(&[b"PFADD", k_buf.as_bytes(), v_buf.as_bytes()]);
    }

    // Geo 2%
    for i in 1..=num_geo {
      let lon = 116.4 + (i % 1000) as f64 * 0.001;
      let lat = 39.9 + (i % 1000) as f64 * 0.001;
      let lon_str = zmij_lon.format(lon);
      let lat_str = zmij_lat.format(lat);
      v_buf.clear();
      let _ = write!(v_buf, "dc_node_{:06}", i);
      redis.push_cmd(&[
        b"GEOADD",
        GEO_KEY,
        lon_str.as_bytes(),
        lat_str.as_bytes(),
        v_buf.as_bytes(),
      ]);
    }

    // 刷写剩余指令
    redis.flush();

    // 显式释放批量灌入连接，避免管道遗留未读取响应污染后续指令
    drop(redis);

    // 在独立连接上精确测量 Redis 数据集常驻内存 (在落盘前测定以避免 SAVE 磁盘超时与阻塞)
    let (mem, rss) = query_redis_memory();
    redis_mem_bytes = mem;
    redis_rss_bytes = rss;

    // 强制完整同步持久化落盘
    force_redis_save();

    // 二次验证：若落盘前未采集到，则在落盘后重试
    if redis_mem_bytes == 0 || redis_rss_bytes == 0 {
      let (m2, r2) = query_redis_memory();
      if redis_mem_bytes == 0 {
        redis_mem_bytes = m2;
      }
      if redis_rss_bytes == 0 {
        redis_rss_bytes = r2;
      }
    }

    // 读取 Redis 磁盘持久化文件占用
    let redis_data_path = Path::new(REDIS_DATA_DIR);
    if redis_data_path.exists() {
      redis_disk_bytes = dir_size(redis_data_path);
    }
  }

  // 3. 输出标准化 JSON 结果 (MB 与 GB 计算)
  let raw_mb = raw_bytes as f64 / (1024.0 * 1024.0);
  let raw_gb = raw_mb / 1024.0;
  let wedb_disk_mb = wedb_disk_bytes as f64 / (1024.0 * 1024.0);
  let wedb_disk_gb = wedb_disk_mb / 1024.0;
  let wedb_rss_mb = wedb_rss_bytes as f64 / (1024.0 * 1024.0);
  let redis_disk_mb = redis_disk_bytes as f64 / (1024.0 * 1024.0);
  let redis_disk_gb = redis_disk_mb / 1024.0;
  let redis_mem_mb = redis_mem_bytes as f64 / (1024.0 * 1024.0);
  let redis_rss_mb = redis_rss_bytes as f64 / (1024.0 * 1024.0);

  println!("FOOTPRINT_RESULT_START");
  println!(
    r#"{{"items":{},"raw_payload_mb":{:.2},"raw_payload_gb":{:.2},"wedb":{{"disk_bytes":{},"disk_mb":{:.2},"disk_gb":{:.2},"rss_bytes":{},"rss_mb":{:.2}}},"redis":{{"disk_bytes":{},"disk_mb":{:.2},"disk_gb":{:.2},"mem_bytes":{},"mem_mb":{:.2},"rss_bytes":{},"rss_mb":{:.2}}}}}"#,
    total_items,
    raw_mb,
    raw_gb,
    wedb_disk_bytes,
    wedb_disk_mb,
    wedb_disk_gb,
    wedb_rss_bytes,
    wedb_rss_mb,
    redis_disk_bytes,
    redis_disk_mb,
    redis_disk_gb,
    redis_mem_bytes,
    redis_mem_mb,
    redis_rss_bytes,
    redis_rss_mb
  );
  println!("FOOTPRINT_RESULT_END");
}
