use std::{
  io::{Read, Write},
  mem::forget,
  os::unix::net::UnixStream,
  path::Path,
  str,
  sync::OnceLock,
  time::Duration,
};

use divan::{Bencher, black_box};
use rapidhash::RapidHashMap;
use tempfile::tempdir;
use wedb_embed::{
  Fjall, WeDb,
  bloom::{BfReserve, CfReserve},
  search::{
    DistanceMetric, FtCreate, FtSearch, IndexField, IndexFieldType, IndexOnDataType,
    SearchIndexManager,
  },
  stream::StreamId,
  timeseries::TsCreate,
  zset::RangeScore,
};

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() {
  divan::main();
}

const REDIS_SOCK: &str = "/tmp/wedb_redis_bench.sock";

struct FastRedisClient {
  stream: UnixStream,
  buf: [u8; 8192],
  read_len: usize,
  msg: Vec<u8>,
  itoa_buf: itoa::Buffer,
}

impl FastRedisClient {
  fn new() -> Option<Self> {
    let stream = UnixStream::connect(REDIS_SOCK).ok()?;
    let _ = stream.set_read_timeout(Some(Duration::from_millis(1000)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(1000)));
    Some(Self {
      stream,
      buf: [0u8; 8192],
      read_len: 0,
      msg: Vec::with_capacity(512),
      itoa_buf: itoa::Buffer::new(),
    })
  }

  #[inline]
  fn send_args(&mut self, args: &[&[u8]]) {
    self.msg.clear();
    self.msg.push(b'*');
    self
      .msg
      .extend_from_slice(self.itoa_buf.format(args.len()).as_bytes());
    self.msg.extend_from_slice(b"\r\n");
    for &arg in args {
      self.msg.push(b'$');
      self
        .msg
        .extend_from_slice(self.itoa_buf.format(arg.len()).as_bytes());
      self.msg.extend_from_slice(b"\r\n");
      self.msg.extend_from_slice(arg);
      self.msg.extend_from_slice(b"\r\n");
    }
    if self.stream.write_all(&self.msg).is_err()
      && let Ok(stream) = UnixStream::connect(REDIS_SOCK)
    {
      let _ = stream.set_read_timeout(Some(Duration::from_millis(1000)));
      let _ = stream.set_write_timeout(Some(Duration::from_millis(1000)));
      self.stream = stream;
      let _ = self.stream.write_all(&self.msg);
    }
    self.read_len = self.stream.read(&mut self.buf).unwrap_or(0);
  }
}

const WEDB_BENCH_5GB_DIR: &str = "/tmp/wedb_bench_data_5gb";
static WEDB_INSTANCE: OnceLock<wedb_embed::Db<Fjall>> = OnceLock::new();

fn get_wedb() -> &'static wedb_embed::Db<Fjall> {
  WEDB_INSTANCE.get_or_init(|| {
    let path = if Path::new(WEDB_BENCH_5GB_DIR).exists() {
      WEDB_BENCH_5GB_DIR.to_string()
    } else {
      let dir = tempdir().expect("tempdir");
      let p = dir.path().to_string_lossy().to_string();
      forget(dir);
      p
    };
    let engine = Fjall::open(&path).expect("open engine");
    WeDb::new(engine).ns(0).expect("ns 0").db(0).expect("db 0")
  })
}

// ───────────────────────────────────────────────
// 静态零分配 Key 与字段池 (独立命名空间隔离)
// ───────────────────────────────────────────────

const STR_KEYS: [&[u8]; 10] = [
  b"cmp:str:0",
  b"cmp:str:1",
  b"cmp:str:2",
  b"cmp:str:3",
  b"cmp:str:4",
  b"cmp:str:5",
  b"cmp:str:6",
  b"cmp:str:7",
  b"cmp:str:8",
  b"cmp:str:9",
];

const MSET_KEYS: [&[u8]; 10] = [
  b"cmp:mset:0",
  b"cmp:mset:1",
  b"cmp:mset:2",
  b"cmp:mset:3",
  b"cmp:mset:4",
  b"cmp:mset:5",
  b"cmp:mset:6",
  b"cmp:mset:7",
  b"cmp:mset:8",
  b"cmp:mset:9",
];

const LIST_KEYS: [&[u8]; 10] = [
  b"cmp:list:0",
  b"cmp:list:1",
  b"cmp:list:2",
  b"cmp:list:3",
  b"cmp:list:4",
  b"cmp:list:5",
  b"cmp:list:6",
  b"cmp:list:7",
  b"cmp:list:8",
  b"cmp:list:9",
];

const JSON_KEYS: [&[u8]; 10] = [
  b"cmp:json:0",
  b"cmp:json:1",
  b"cmp:json:2",
  b"cmp:json:3",
  b"cmp:json:4",
  b"cmp:json:5",
  b"cmp:json:6",
  b"cmp:json:7",
  b"cmp:json:8",
  b"cmp:json:9",
];

const SAMPLE_JSON_STR: &str = r#"{"name":"EnterpriseNode","tier":"cluster_l3","tags":["rust","database","lsm"],"score":99.5,"counter":100,"list":[1,2,3,4,5,6,7,8,9,10]}"#;
const SAMPLE_JSON_DOC: &[u8] = SAMPLE_JSON_STR.as_bytes();

const DEL_KEYS: [&[u8]; 10] = [
  b"cmp:del:0",
  b"cmp:del:1",
  b"cmp:del:2",
  b"cmp:del:3",
  b"cmp:del:4",
  b"cmp:del:5",
  b"cmp:del:6",
  b"cmp:del:7",
  b"cmp:del:8",
  b"cmp:del:9",
];

const EXISTS_KEYS: [&[u8]; 10] = [
  b"cmp:exists:0",
  b"cmp:exists:1",
  b"cmp:exists:2",
  b"cmp:exists:3",
  b"cmp:exists:4",
  b"cmp:exists:5",
  b"cmp:exists:6",
  b"cmp:exists:7",
  b"cmp:exists:8",
  b"cmp:exists:9",
];

const SAMPLE_FIELDS: [&[u8]; 10] = [
  b"f0", b"f1", b"f2", b"f3", b"f4", b"f5", b"f6", b"f7", b"f8", b"f9",
];

const SAMPLE_MEMBERS: [&[u8]; 10] = [
  b"m0", b"m1", b"m2", b"m3", b"m4", b"m5", b"m6", b"m7", b"m8", b"m9",
];

const SAMPLE_5GB_KEYS: [&[u8]; 10] = [
  b"user:account:00000001",
  b"user:account:00000100",
  b"user:account:00000500",
  b"user:account:00001000",
  b"user:account:00005000",
  b"user:account:00010000",
  b"user:account:00050000",
  b"user:account:00075000",
  b"user:account:00100000",
  b"user:account:00200000",
];

// ═══════════════════════════════════════════════
// 1. String 模块 (SET, GET, MSET, MGET, INCRBY, DECRBY, APPEND, STRLEN)
// ═══════════════════════════════════════════════

#[divan::bench]
fn bench_cmp_str_set_wedb(bencher: Bencher) {
  let db = get_wedb();
  let mut i = 0usize;
  let val = b"value_128_bytes_payload_for_benchmarking_lsm_tree_storage_engine_write_path_efficiency_and_throughput_check_zero_allocation";
  bencher.bench_local(|| {
    let key = STR_KEYS[i % STR_KEYS.len()];
    i += 1;
    db.set(key, val, []).unwrap();
  });
}

#[divan::bench]
fn bench_cmp_str_set_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let mut i = 0usize;
  let val = b"value_128_bytes_payload_for_benchmarking_lsm_tree_storage_engine_write_path_efficiency_and_throughput_check_zero_allocation";
  bencher.bench_local(|| {
    let key = STR_KEYS[i % STR_KEYS.len()];
    i += 1;
    client.send_args(&[b"SET", key, val]);
  });
}

#[divan::bench]
fn bench_cmp_str_get_wedb(bencher: Bencher) {
  let db = get_wedb();
  for &k in &SAMPLE_5GB_KEYS {
    let _ = db.get(k);
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let key = SAMPLE_5GB_KEYS[i % SAMPLE_5GB_KEYS.len()];
    i += 1;
    let res = db.get(key).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_str_get_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let mut i = 0usize;
  bencher.bench_local(|| {
    let key = SAMPLE_5GB_KEYS[i % SAMPLE_5GB_KEYS.len()];
    i += 1;
    client.send_args(&[b"GET", key]);
  });
}

#[divan::bench]
fn bench_cmp_str_mset_wedb(bencher: Bencher) {
  let db = get_wedb();
  let pairs: [(&[u8], &[u8]); 10] = [
    (MSET_KEYS[0], b"val0"),
    (MSET_KEYS[1], b"val1"),
    (MSET_KEYS[2], b"val2"),
    (MSET_KEYS[3], b"val3"),
    (MSET_KEYS[4], b"val4"),
    (MSET_KEYS[5], b"val5"),
    (MSET_KEYS[6], b"val6"),
    (MSET_KEYS[7], b"val7"),
    (MSET_KEYS[8], b"val8"),
    (MSET_KEYS[9], b"val9"),
  ];
  bencher.bench_local(|| {
    db.mset(&pairs).unwrap();
  });
}

#[divan::bench]
fn bench_cmp_str_mset_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  bencher.bench_local(|| {
    client.send_args(&[
      b"MSET",
      MSET_KEYS[0],
      b"val0",
      MSET_KEYS[1],
      b"val1",
      MSET_KEYS[2],
      b"val2",
      MSET_KEYS[3],
      b"val3",
      MSET_KEYS[4],
      b"val4",
      MSET_KEYS[5],
      b"val5",
      MSET_KEYS[6],
      b"val6",
      MSET_KEYS[7],
      b"val7",
      MSET_KEYS[8],
      b"val8",
      MSET_KEYS[9],
      b"val9",
    ]);
  });
}

#[divan::bench]
fn bench_cmp_str_mget_wedb(bencher: Bencher) {
  let db = get_wedb();
  for &k in &MSET_KEYS {
    db.set(k, b"val", []).unwrap();
  }
  bencher.bench_local(|| {
    let res = db.mget(&MSET_KEYS).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_str_mget_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  for &k in &MSET_KEYS {
    client.send_args(&[b"SET", k, b"val"]);
  }
  bencher.bench_local(|| {
    client.send_args(&[
      b"MGET",
      MSET_KEYS[0],
      MSET_KEYS[1],
      MSET_KEYS[2],
      MSET_KEYS[3],
      MSET_KEYS[4],
      MSET_KEYS[5],
      MSET_KEYS[6],
      MSET_KEYS[7],
      MSET_KEYS[8],
      MSET_KEYS[9],
    ]);
  });
}

#[divan::bench]
fn bench_cmp_str_incrby_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:counter:incr";
  let _ = db.set(key, b"0", []);
  bencher.bench_local(|| {
    let res = db.incrby(key, 1).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_str_incrby_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:counter:incr";
  client.send_args(&[b"SET", key, b"0"]);
  bencher.bench_local(|| {
    client.send_args(&[b"INCRBY", key, b"1"]);
  });
}

#[divan::bench]
fn bench_cmp_str_decrby_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:counter:decr";
  let _ = db.set(key, b"1000000", []);
  bencher.bench_local(|| {
    let res = db.decrby(key, 1).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_str_decrby_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:counter:decr";
  client.send_args(&[b"SET", key, b"1000000"]);
  bencher.bench_local(|| {
    client.send_args(&[b"DECRBY", key, b"1"]);
  });
}

#[divan::bench]
fn bench_cmp_str_append_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:str:append";
  let _ = db.set(key, b"base_", []);
  bencher.bench_local(|| {
    let res = db.append(key, b"x").unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_str_append_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:str:append";
  client.send_args(&[b"SET", key, b"base_"]);
  bencher.bench_local(|| {
    client.send_args(&[b"APPEND", key, b"x"]);
  });
}

#[divan::bench]
fn bench_cmp_str_strlen_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:str:strlen";
  let _ = db.set(key, b"sample_payload_of_moderate_length", []);
  bencher.bench_local(|| {
    let res = db.strlen(key).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_str_strlen_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:str:strlen";
  client.send_args(&[b"SET", key, b"sample_payload_of_moderate_length"]);
  bencher.bench_local(|| {
    client.send_args(&[b"STRLEN", key]);
  });
}

#[divan::bench]
fn bench_cmp_str_getdel_wedb(bencher: Bencher) {
  let db = get_wedb();
  let mut i = 0usize;
  bencher.bench_local(|| {
    let key = STR_KEYS[i % STR_KEYS.len()];
    i += 1;
    let _ = db.set(key, b"temp_val", []);
    let res = db.getdel(key).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_str_getdel_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let mut i = 0usize;
  bencher.bench_local(|| {
    let key = STR_KEYS[i % STR_KEYS.len()];
    i += 1;
    client.send_args(&[b"SET", key, b"temp_val"]);
    client.send_args(&[b"GETDEL", key]);
  });
}

#[divan::bench]
fn bench_cmp_str_getrange_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:str:getrange";
  let _ = db.set(key, b"sample_payload_of_moderate_length", []);
  bencher.bench_local(|| {
    let res = db.getrange(key, (0, 10)).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_str_getrange_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:str:getrange";
  client.send_args(&[b"SET", key, b"sample_payload_of_moderate_length"]);
  bencher.bench_local(|| {
    client.send_args(&[b"GETRANGE", key, b"0", b"10"]);
  });
}

#[divan::bench]
fn bench_cmp_str_setrange_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:str:setrange";
  let _ = db.set(key, b"sample_payload_of_moderate_length", []);
  bencher.bench_local(|| {
    let res = db.setrange(key, 5, b"update").unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_str_setrange_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:str:setrange";
  client.send_args(&[b"SET", key, b"sample_payload_of_moderate_length"]);
  bencher.bench_local(|| {
    client.send_args(&[b"SETRANGE", key, b"5", b"update"]);
  });
}

// ═══════════════════════════════════════════════
// 2. Hash 模块 (HSET, HGET, HMGET, HEXISTS, HLEN, HDEL, HGETALL, HKEYS, HVALS, HINCRBY)
// ═══════════════════════════════════════════════

#[divan::bench]
fn bench_cmp_hash_hset_wedb(bencher: Bencher) {
  let db = get_wedb();
  let mut i = 0usize;
  bencher.bench_local(|| {
    let field = SAMPLE_FIELDS[i % SAMPLE_FIELDS.len()];
    i += 1;
    db.hset(b"cmp:hash:key", &[(field, b"val")]).unwrap();
  });
}

#[divan::bench]
fn bench_cmp_hash_hset_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let mut i = 0usize;
  bencher.bench_local(|| {
    let field = SAMPLE_FIELDS[i % SAMPLE_FIELDS.len()];
    i += 1;
    client.send_args(&[b"HSET", b"cmp:hash:key", field, b"val"]);
  });
}

#[divan::bench]
fn bench_cmp_hash_hget_wedb(bencher: Bencher) {
  let db = get_wedb();
  for &f in &SAMPLE_FIELDS {
    db.hset(b"cmp:hash:key", &[(f, b"val")]).unwrap();
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let field = SAMPLE_FIELDS[i % SAMPLE_FIELDS.len()];
    i += 1;
    let res = db.hget(b"cmp:hash:key", field).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_hash_hget_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  for &f in &SAMPLE_FIELDS {
    client.send_args(&[b"HSET", b"cmp:hash:key", f, b"val"]);
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let field = SAMPLE_FIELDS[i % SAMPLE_FIELDS.len()];
    i += 1;
    client.send_args(&[b"HGET", b"cmp:hash:key", field]);
  });
}

#[divan::bench]
fn bench_cmp_hash_hmget_wedb(bencher: Bencher) {
  let db = get_wedb();
  for &f in &SAMPLE_FIELDS {
    db.hset(b"cmp:hash:hmget", &[(f, b"val")]).unwrap();
  }
  bencher.bench_local(|| {
    let res = db.hmget(b"cmp:hash:hmget", &SAMPLE_FIELDS).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_hash_hmget_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  for &f in &SAMPLE_FIELDS {
    client.send_args(&[b"HSET", b"cmp:hash:hmget", f, b"val"]);
  }
  bencher.bench_local(|| {
    client.send_args(&[
      b"HMGET",
      b"cmp:hash:hmget",
      SAMPLE_FIELDS[0],
      SAMPLE_FIELDS[1],
      SAMPLE_FIELDS[2],
      SAMPLE_FIELDS[3],
      SAMPLE_FIELDS[4],
      SAMPLE_FIELDS[5],
      SAMPLE_FIELDS[6],
      SAMPLE_FIELDS[7],
      SAMPLE_FIELDS[8],
      SAMPLE_FIELDS[9],
    ]);
  });
}

#[divan::bench]
fn bench_cmp_hash_hexists_wedb(bencher: Bencher) {
  let db = get_wedb();
  let _ = db.hset(
    b"cmp:hash:hexists",
    &[(b"field0" as &[u8], b"val" as &[u8])],
  );
  bencher.bench_local(|| {
    let res = db.hexists(b"cmp:hash:hexists", b"field0").unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_hash_hexists_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  client.send_args(&[b"HSET", b"cmp:hash:hexists", b"field0", b"val"]);
  bencher.bench_local(|| {
    client.send_args(&[b"HEXISTS", b"cmp:hash:hexists", b"field0"]);
  });
}

#[divan::bench]
fn bench_cmp_hash_hlen_wedb(bencher: Bencher) {
  let db = get_wedb();
  for &f in &SAMPLE_FIELDS {
    db.hset(b"cmp:hash:hlen", &[(f, b"val")]).unwrap();
  }
  bencher.bench_local(|| {
    let res = db.hlen(b"cmp:hash:hlen").unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_hash_hlen_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  for &f in &SAMPLE_FIELDS {
    client.send_args(&[b"HSET", b"cmp:hash:hlen", f, b"val"]);
  }
  bencher.bench_local(|| {
    client.send_args(&[b"HLEN", b"cmp:hash:hlen"]);
  });
}

#[divan::bench]
fn bench_cmp_hash_hdel_wedb(bencher: Bencher) {
  let db = get_wedb();
  for &f in &SAMPLE_FIELDS {
    db.hset(b"cmp:hash:hdel", &[(f, b"val")]).unwrap();
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let field = SAMPLE_FIELDS[i % SAMPLE_FIELDS.len()];
    i += 1;
    let res = db.hdel(b"cmp:hash:hdel", &[field]).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_hash_hdel_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  for &f in &SAMPLE_FIELDS {
    client.send_args(&[b"HSET", b"cmp:hash:hdel", f, b"val"]);
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let field = SAMPLE_FIELDS[i % SAMPLE_FIELDS.len()];
    i += 1;
    client.send_args(&[b"HDEL", b"cmp:hash:hdel", field]);
  });
}

#[divan::bench]
fn bench_cmp_hash_hgetall_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:hash:hgetall";
  for &f in &SAMPLE_FIELDS {
    db.hset(key, &[(f, b"val")]).unwrap();
  }
  bencher.bench_local(|| {
    let res = db.hgetall(key).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_hash_hgetall_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:hash:hgetall";
  for &f in &SAMPLE_FIELDS {
    client.send_args(&[b"HSET", key, f, b"val"]);
  }
  bencher.bench_local(|| {
    client.send_args(&[b"HGETALL", key]);
  });
}

#[divan::bench]
fn bench_cmp_hash_hkeys_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:hash:hkeys";
  for &f in &SAMPLE_FIELDS {
    db.hset(key, &[(f, b"val")]).unwrap();
  }
  bencher.bench_local(|| {
    let res = db.hkeys(key).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_hash_hkeys_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:hash:hkeys";
  for &f in &SAMPLE_FIELDS {
    client.send_args(&[b"HSET", key, f, b"val"]);
  }
  bencher.bench_local(|| {
    client.send_args(&[b"HKEYS", key]);
  });
}

#[divan::bench]
fn bench_cmp_hash_hvals_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:hash:hvals";
  for &f in &SAMPLE_FIELDS {
    db.hset(key, &[(f, b"val")]).unwrap();
  }
  bencher.bench_local(|| {
    let res = db.hvals(key).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_hash_hvals_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:hash:hvals";
  for &f in &SAMPLE_FIELDS {
    client.send_args(&[b"HSET", key, f, b"val"]);
  }
  bencher.bench_local(|| {
    client.send_args(&[b"HVALS", key]);
  });
}

#[divan::bench]
fn bench_cmp_hash_hincrby_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:hash:hincrby";
  bencher.bench_local(|| {
    let res = db.hincrby(key, b"f0", 1).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_hash_hincrby_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:hash:hincrby";
  bencher.bench_local(|| {
    client.send_args(&[b"HINCRBY", key, b"f0", b"1"]);
  });
}

// ═══════════════════════════════════════════════
// 3. List 模块 (LPUSH, RPUSH, LPOP, RPOP, LLEN, LRANGE, LINDEX, LSET, LREM, LTRIM)
// ═══════════════════════════════════════════════

#[divan::bench]
fn bench_cmp_list_lpush_wedb(bencher: Bencher) {
  let db = get_wedb();
  let mut i = 0usize;
  bencher.bench_local(|| {
    let key = LIST_KEYS[i % LIST_KEYS.len()];
    i += 1;
    db.lpush(key, &[b"item"]).unwrap();
  });
}

#[divan::bench]
fn bench_cmp_list_lpush_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let mut i = 0usize;
  bencher.bench_local(|| {
    let key = LIST_KEYS[i % LIST_KEYS.len()];
    i += 1;
    client.send_args(&[b"LPUSH", key, b"item"]);
  });
}

#[divan::bench]
fn bench_cmp_list_rpush_wedb(bencher: Bencher) {
  let db = get_wedb();
  let mut i = 0usize;
  bencher.bench_local(|| {
    let key = LIST_KEYS[i % LIST_KEYS.len()];
    i += 1;
    db.rpush(key, &[b"item"]).unwrap();
  });
}

#[divan::bench]
fn bench_cmp_list_rpush_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let mut i = 0usize;
  bencher.bench_local(|| {
    let key = LIST_KEYS[i % LIST_KEYS.len()];
    i += 1;
    client.send_args(&[b"RPUSH", key, b"item"]);
  });
}

#[divan::bench]
fn bench_cmp_list_lpop_wedb(bencher: Bencher) {
  let db = get_wedb();
  for &k in &LIST_KEYS {
    for _ in 0..50 {
      db.lpush(k, &[b"item"]).unwrap();
    }
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let key = LIST_KEYS[i % LIST_KEYS.len()];
    i += 1;
    let res = db.lpop(key, 1).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_list_lpop_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  for &k in &LIST_KEYS {
    for _ in 0..50 {
      client.send_args(&[b"LPUSH", k, b"item"]);
    }
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let key = LIST_KEYS[i % LIST_KEYS.len()];
    i += 1;
    client.send_args(&[b"LPOP", key]);
  });
}

#[divan::bench]
fn bench_cmp_list_rpop_wedb(bencher: Bencher) {
  let db = get_wedb();
  for &k in &LIST_KEYS {
    for _ in 0..50 {
      db.rpush(k, &[b"item"]).unwrap();
    }
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let key = LIST_KEYS[i % LIST_KEYS.len()];
    i += 1;
    let res = db.rpop(key, 1).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_list_rpop_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  for &k in &LIST_KEYS {
    for _ in 0..50 {
      client.send_args(&[b"RPUSH", k, b"item"]);
    }
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let key = LIST_KEYS[i % LIST_KEYS.len()];
    i += 1;
    client.send_args(&[b"RPOP", key]);
  });
}

#[divan::bench]
fn bench_cmp_list_llen_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:list:llen";
  for _ in 0..10 {
    db.lpush(key, &[b"item"]).unwrap();
  }
  bencher.bench_local(|| {
    let res = db.llen(key).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_list_llen_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:list:llen";
  for _ in 0..10 {
    client.send_args(&[b"LPUSH", key, b"item"]);
  }
  bencher.bench_local(|| {
    client.send_args(&[b"LLEN", key]);
  });
}

#[divan::bench]
fn bench_cmp_list_lrange_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:list:lrange";
  for _ in 0..20 {
    db.lpush(key, &[b"item"]).unwrap();
  }
  bencher.bench_local(|| {
    let res = db.lrange(key, (0, 10)).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_list_lrange_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:list:lrange";
  for _ in 0..20 {
    client.send_args(&[b"LPUSH", key, b"item"]);
  }
  bencher.bench_local(|| {
    client.send_args(&[b"LRANGE", key, b"0", b"10"]);
  });
}

#[divan::bench]
fn bench_cmp_list_lindex_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:list:lindex";
  for _ in 0..20 {
    db.lpush(key, &[b"item"]).unwrap();
  }
  bencher.bench_local(|| {
    let res = db.lindex(key, 0).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_list_lindex_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:list:lindex";
  for _ in 0..20 {
    client.send_args(&[b"LPUSH", key, b"item"]);
  }
  bencher.bench_local(|| {
    client.send_args(&[b"LINDEX", key, b"0"]);
  });
}

#[divan::bench]
fn bench_cmp_list_lset_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:list:lset";
  for _ in 0..20 {
    db.lpush(key, &[b"item"]).unwrap();
  }
  bencher.bench_local(|| {
    db.lset(key, 0, b"updated_item").unwrap();
  });
}

#[divan::bench]
fn bench_cmp_list_lset_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:list:lset";
  for _ in 0..20 {
    client.send_args(&[b"LPUSH", key, b"item"]);
  }
  bencher.bench_local(|| {
    client.send_args(&[b"LSET", key, b"0", b"updated_item"]);
  });
}

#[divan::bench]
fn bench_cmp_list_lrem_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:list:lrem";
  bencher.bench_local(|| {
    let _ = db.lpush(key, &[b"elem"]);
    let res = db.lrem(key, 1, b"elem").unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_list_lrem_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:list:lrem";
  bencher.bench_local(|| {
    client.send_args(&[b"LPUSH", key, b"elem"]);
    client.send_args(&[b"LREM", key, b"1", b"elem"]);
  });
}

#[divan::bench]
fn bench_cmp_list_ltrim_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:list:ltrim";
  for _ in 0..20 {
    db.lpush(key, &[b"item"]).unwrap();
  }
  bencher.bench_local(|| {
    db.ltrim(key, (0, 9)).unwrap();
  });
}

#[divan::bench]
fn bench_cmp_list_ltrim_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:list:ltrim";
  for _ in 0..20 {
    client.send_args(&[b"LPUSH", key, b"item"]);
  }
  bencher.bench_local(|| {
    client.send_args(&[b"LTRIM", key, b"0", b"9"]);
  });
}

// ═══════════════════════════════════════════════
// 4. Set 模块 (SADD, SREM, SISMEMBER, SCARD, SMEMBERS, SPOP, SRANDMEMBER)
// ═══════════════════════════════════════════════

#[divan::bench]
fn bench_cmp_set_sadd_wedb(bencher: Bencher) {
  let db = get_wedb();
  let mut i = 0usize;
  bencher.bench_local(|| {
    let member = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    db.sadd(b"cmp:set:key", &[member]).unwrap();
  });
}

#[divan::bench]
fn bench_cmp_set_sadd_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let mut i = 0usize;
  bencher.bench_local(|| {
    let member = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    client.send_args(&[b"SADD", b"cmp:set:key", member]);
  });
}

#[divan::bench]
fn bench_cmp_set_sismember_wedb(bencher: Bencher) {
  let db = get_wedb();
  for &m in &SAMPLE_MEMBERS {
    db.sadd(b"cmp:set:key", &[m]).unwrap();
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let member = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    let res = db.sismember(b"cmp:set:key", member).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_set_sismember_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  for &m in &SAMPLE_MEMBERS {
    client.send_args(&[b"SADD", b"cmp:set:key", m]);
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let member = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    client.send_args(&[b"SISMEMBER", b"cmp:set:key", member]);
  });
}

#[divan::bench]
fn bench_cmp_set_scard_wedb(bencher: Bencher) {
  let db = get_wedb();
  for &m in &SAMPLE_MEMBERS {
    db.sadd(b"cmp:set:scard", &[m]).unwrap();
  }
  bencher.bench_local(|| {
    let res = db.scard(b"cmp:set:scard").unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_set_scard_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  for &m in &SAMPLE_MEMBERS {
    client.send_args(&[b"SADD", b"cmp:set:scard", m]);
  }
  bencher.bench_local(|| {
    client.send_args(&[b"SCARD", b"cmp:set:scard"]);
  });
}

#[divan::bench]
fn bench_cmp_set_smembers_wedb(bencher: Bencher) {
  let db = get_wedb();
  for &m in &SAMPLE_MEMBERS {
    db.sadd(b"cmp:set:smembers", &[m]).unwrap();
  }
  bencher.bench_local(|| {
    let res = db.smembers(b"cmp:set:smembers").unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_set_smembers_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  for &m in &SAMPLE_MEMBERS {
    client.send_args(&[b"SADD", b"cmp:set:smembers", m]);
  }
  bencher.bench_local(|| {
    client.send_args(&[b"SMEMBERS", b"cmp:set:smembers"]);
  });
}

#[divan::bench]
fn bench_cmp_set_srem_wedb(bencher: Bencher) {
  let db = get_wedb();
  for &m in &SAMPLE_MEMBERS {
    db.sadd(b"cmp:set:srem", &[m]).unwrap();
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let member = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    let res = db.srem(b"cmp:set:srem", &[member]).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_set_srem_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  for &m in &SAMPLE_MEMBERS {
    client.send_args(&[b"SADD", b"cmp:set:srem", m]);
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let member = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    client.send_args(&[b"SREM", b"cmp:set:srem", member]);
  });
}

#[divan::bench]
fn bench_cmp_set_spop_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:set:spop";
  for &m in &SAMPLE_MEMBERS {
    let _ = db.sadd(key, &[m]);
  }
  bencher.bench_local(|| {
    let _ = db.sadd(key, &[b"item" as &[u8]]);
    let res = db.spop(key, 1).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_set_spop_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:set:spop";
  for &m in &SAMPLE_MEMBERS {
    client.send_args(&[b"SADD", key, m]);
  }
  bencher.bench_local(|| {
    client.send_args(&[b"SADD", key, b"item"]);
    client.send_args(&[b"SPOP", key, b"1"]);
  });
}

#[divan::bench]
fn bench_cmp_set_srandmember_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:set:srandmember";
  for &m in &SAMPLE_MEMBERS {
    let _ = db.sadd(key, &[m]);
  }
  bencher.bench_local(|| {
    let res = db.srandmember(key, 1).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_set_srandmember_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:set:srandmember";
  for &m in &SAMPLE_MEMBERS {
    client.send_args(&[b"SADD", key, m]);
  }
  bencher.bench_local(|| {
    client.send_args(&[b"SRANDMEMBER", key, b"1"]);
  });
}

// ═══════════════════════════════════════════════
// 5. ZSet 模块 (ZADD, ZSCORE, ZRANGE, ZCARD, ZCOUNT, ZINCRBY, ZRANK, ZREVRANGE, ZPOPMIN, ZREM)
// ═══════════════════════════════════════════════

#[divan::bench]
fn bench_cmp_zset_zadd_wedb(bencher: Bencher) {
  let db = get_wedb();
  let mut i = 0usize;
  bencher.bench_local(|| {
    let member = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    db.zadd(b"cmp:zset:key", &[(i as f64, member)], []).unwrap();
  });
}

#[divan::bench]
fn bench_cmp_zset_zadd_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let mut i = 0usize;
  bencher.bench_local(|| {
    let member = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    client.send_args(&[b"ZADD", b"cmp:zset:key", b"10.0", member]);
  });
}

#[divan::bench]
fn bench_cmp_zset_zscore_wedb(bencher: Bencher) {
  let db = get_wedb();
  for (idx, &m) in SAMPLE_MEMBERS.iter().enumerate() {
    db.zadd(b"cmp:zset:zscore", &[(idx as f64, m)], []).unwrap();
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let member = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    let res = db.zscore(b"cmp:zset:zscore", member).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_zset_zscore_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  for (idx, &m) in SAMPLE_MEMBERS.iter().enumerate() {
    let score_bytes = [b'0' + (idx as u8)];
    client.send_args(&[b"ZADD", b"cmp:zset:zscore", &score_bytes, m]);
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let member = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    client.send_args(&[b"ZSCORE", b"cmp:zset:zscore", member]);
  });
}

#[divan::bench]
fn bench_cmp_zset_zrange_wedb(bencher: Bencher) {
  let db = get_wedb();
  for (idx, &m) in SAMPLE_MEMBERS.iter().enumerate() {
    db.zadd(b"cmp:zset:zrange", &[(idx as f64, m)], []).unwrap();
  }
  bencher.bench_local(|| {
    let res = db.zrange(b"cmp:zset:zrange", b"0", b"10", []).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_zset_zrange_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  for (idx, &m) in SAMPLE_MEMBERS.iter().enumerate() {
    let score_bytes = [b'0' + (idx as u8)];
    client.send_args(&[b"ZADD", b"cmp:zset:zrange", &score_bytes, m]);
  }
  bencher.bench_local(|| {
    client.send_args(&[b"ZRANGE", b"cmp:zset:zrange", b"0", b"10"]);
  });
}

#[divan::bench]
fn bench_cmp_zset_zcard_wedb(bencher: Bencher) {
  let db = get_wedb();
  for (idx, &m) in SAMPLE_MEMBERS.iter().enumerate() {
    db.zadd(b"cmp:zset:zcard", &[(idx as f64, m)], []).unwrap();
  }
  bencher.bench_local(|| {
    let res = db.zcard(b"cmp:zset:zcard").unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_zset_zcard_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  for (idx, &m) in SAMPLE_MEMBERS.iter().enumerate() {
    let score_bytes = [b'0' + (idx as u8)];
    client.send_args(&[b"ZADD", b"cmp:zset:zcard", &score_bytes, m]);
  }
  bencher.bench_local(|| {
    client.send_args(&[b"ZCARD", b"cmp:zset:zcard"]);
  });
}

#[divan::bench]
fn bench_cmp_zset_zrem_wedb(bencher: Bencher) {
  let db = get_wedb();
  for (idx, &m) in SAMPLE_MEMBERS.iter().enumerate() {
    db.zadd(b"cmp:zset:zrem", &[(idx as f64, m)], []).unwrap();
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let member = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    let res = db.zrem(b"cmp:zset:zrem", &[member]).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_zset_zrem_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  for (idx, &m) in SAMPLE_MEMBERS.iter().enumerate() {
    let score_bytes = [b'0' + (idx as u8)];
    client.send_args(&[b"ZADD", b"cmp:zset:zrem", &score_bytes, m]);
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let member = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    client.send_args(&[b"ZREM", b"cmp:zset:zrem", member]);
  });
}

#[divan::bench]
fn bench_cmp_zset_zcount_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:zset:zcount";
  for (idx, &m) in SAMPLE_MEMBERS.iter().enumerate() {
    db.zadd(key, &[(idx as f64, m)], []).unwrap();
  }
  let spec = RangeScore::default();
  bencher.bench_local(|| {
    let res = db.zcount(key, spec).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_zset_zcount_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:zset:zcount";
  for (idx, &m) in SAMPLE_MEMBERS.iter().enumerate() {
    let score_bytes = [b'0' + (idx as u8)];
    client.send_args(&[b"ZADD", key, &score_bytes, m]);
  }
  bencher.bench_local(|| {
    client.send_args(&[b"ZCOUNT", key, b"-inf", b"+inf"]);
  });
}

#[divan::bench]
fn bench_cmp_zset_zincrby_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:zset:zincrby";
  let mut i = 0usize;
  bencher.bench_local(|| {
    let member = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    let res = db.zincrby(key, 1.0, member).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_zset_zincrby_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:zset:zincrby";
  let mut i = 0usize;
  bencher.bench_local(|| {
    let member = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    client.send_args(&[b"ZINCRBY", key, b"1", member]);
  });
}

#[divan::bench]
fn bench_cmp_zset_zrank_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:zset:zrank";
  for (idx, &m) in SAMPLE_MEMBERS.iter().enumerate() {
    db.zadd(key, &[(idx as f64, m)], []).unwrap();
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let member = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    let res = db.zrank(key, member).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_zset_zrank_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:zset:zrank";
  for (idx, &m) in SAMPLE_MEMBERS.iter().enumerate() {
    let score_bytes = [b'0' + (idx as u8)];
    client.send_args(&[b"ZADD", key, &score_bytes, m]);
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let member = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    client.send_args(&[b"ZRANK", key, member]);
  });
}

#[divan::bench]
fn bench_cmp_zset_zrevrange_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:zset:zrevrange";
  for (idx, &m) in SAMPLE_MEMBERS.iter().enumerate() {
    db.zadd(key, &[(idx as f64, m)], []).unwrap();
  }
  bencher.bench_local(|| {
    let res = db.zrevrange(key, (0, 10)).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_zset_zrevrange_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:zset:zrevrange";
  for (idx, &m) in SAMPLE_MEMBERS.iter().enumerate() {
    let score_bytes = [b'0' + (idx as u8)];
    client.send_args(&[b"ZADD", key, &score_bytes, m]);
  }
  bencher.bench_local(|| {
    client.send_args(&[b"ZREVRANGE", key, b"0", b"10"]);
  });
}

#[divan::bench]
fn bench_cmp_zset_zpopmin_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:zset:zpopmin";
  for (idx, &m) in SAMPLE_MEMBERS.iter().enumerate() {
    db.zadd(key, &[(idx as f64, m)], []).unwrap();
  }
  bencher.bench_local(|| {
    let _ = db.zadd(key, &[(100.0, b"temp_m" as &[u8])], []);
    let res = db.zpopmin(key, 1).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_zset_zpopmin_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:zset:zpopmin";
  for (idx, &m) in SAMPLE_MEMBERS.iter().enumerate() {
    let score_bytes = [b'0' + (idx as u8)];
    client.send_args(&[b"ZADD", key, &score_bytes, m]);
  }
  bencher.bench_local(|| {
    client.send_args(&[b"ZADD", key, b"100.0", b"temp_m"]);
    client.send_args(&[b"ZPOPMIN", key, b"1"]);
  });
}

// ═══════════════════════════════════════════════
// 6. Bitmap 模块 (SETBIT, GETBIT, BITCOUNT, BITPOS)
// ═══════════════════════════════════════════════

#[divan::bench]
fn bench_cmp_bitmap_setbit_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:bitmap:setbit";
  let mut i = 0u64;
  bencher.bench_local(|| {
    i += 1;
    let res = db.setbit(key, i % 10000, 1).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_bitmap_setbit_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:bitmap:setbit";
  let mut itoa_buf = itoa::Buffer::new();
  let mut i = 0u64;
  bencher.bench_local(|| {
    i += 1;
    let offset = itoa_buf.format((i % 10000) as u32);
    client.send_args(&[b"SETBIT", key, offset.as_bytes(), b"1"]);
  });
}

#[divan::bench]
fn bench_cmp_bitmap_getbit_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:bitmap:getbit";
  let _ = db.setbit(key, 100, 1);
  let mut i = 0u64;
  bencher.bench_local(|| {
    i += 1;
    let res = db.getbit(key, i % 200).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_bitmap_getbit_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:bitmap:getbit";
  client.send_args(&[b"SETBIT", key, b"100", b"1"]);
  let mut itoa_buf = itoa::Buffer::new();
  let mut i = 0u64;
  bencher.bench_local(|| {
    i += 1;
    let offset = itoa_buf.format(i % 200);
    client.send_args(&[b"GETBIT", key, offset.as_bytes()]);
  });
}

#[divan::bench]
fn bench_cmp_bitmap_bitcount_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:bitmap:bitcount";
  for i in 0..100 {
    let _ = db.setbit(key, i * 8, 1);
  }
  bencher.bench_local(|| {
    let res = db.bitcount(key, []).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_bitmap_bitcount_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:bitmap:bitcount";
  let mut itoa_buf = itoa::Buffer::new();
  for i in 0..100 {
    let offset = itoa_buf.format(i * 8);
    client.send_args(&[b"SETBIT", key, offset.as_bytes(), b"1"]);
  }
  bencher.bench_local(|| {
    client.send_args(&[b"BITCOUNT", key]);
  });
}

#[divan::bench]
fn bench_cmp_bitmap_bitpos_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:bitmap:bitpos";
  let _ = db.setbit(key, 50, 1);
  bencher.bench_local(|| {
    let res = db.bitpos(key, 1, []).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_bitmap_bitpos_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:bitmap:bitpos";
  client.send_args(&[b"SETBIT", key, b"50", b"1"]);
  bencher.bench_local(|| {
    client.send_args(&[b"BITPOS", key, b"1"]);
  });
}

// ═══════════════════════════════════════════════
// 7. JSON 模块 (JSON.SET, JSON.GET, JSON.DEL, JSON.NUMINCRBY, JSON.ARRLEN, JSON.TYPE)
// ═══════════════════════════════════════════════

#[divan::bench]
fn bench_cmp_json_set_wedb(bencher: Bencher) {
  let db = get_wedb();
  let mut i = 0usize;
  bencher.bench_local(|| {
    let key = JSON_KEYS[i % JSON_KEYS.len()];
    i += 1;
    db.json_set_one(key, "$", SAMPLE_JSON_STR).unwrap();
  });
}

#[divan::bench]
fn bench_cmp_json_set_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let mut i = 0usize;
  bencher.bench_local(|| {
    let key = JSON_KEYS[i % JSON_KEYS.len()];
    i += 1;
    client.send_args(&[b"JSON.SET", key, b"$", SAMPLE_JSON_DOC]);
  });
}

#[divan::bench]
fn bench_cmp_json_get_wedb(bencher: Bencher) {
  let db = get_wedb();
  for &k in &JSON_KEYS {
    let _ = db.json_set_one(k, "$", SAMPLE_JSON_STR);
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let key = JSON_KEYS[i % JSON_KEYS.len()];
    i += 1;
    let res = db.json_get_one(key, "$.tags").unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_json_get_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  for &k in &JSON_KEYS {
    client.send_args(&[b"JSON.SET", k, b"$", SAMPLE_JSON_DOC]);
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let key = JSON_KEYS[i % JSON_KEYS.len()];
    i += 1;
    client.send_args(&[b"JSON.GET", key, b"$.tags"]);
  });
}

#[divan::bench]
fn bench_cmp_json_del_wedb(bencher: Bencher) {
  let db = get_wedb();
  for &k in &JSON_KEYS {
    let _ = db.json_set_one(k, "$", SAMPLE_JSON_STR);
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let key = JSON_KEYS[i % JSON_KEYS.len()];
    i += 1;
    let _ = db.json_set_one(key, "$", SAMPLE_JSON_STR);
    let res = db.json_del(key, Some("$")).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_json_del_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  for &k in &JSON_KEYS {
    client.send_args(&[b"JSON.SET", k, b"$", SAMPLE_JSON_DOC]);
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let key = JSON_KEYS[i % JSON_KEYS.len()];
    i += 1;
    client.send_args(&[b"JSON.SET", key, b"$", SAMPLE_JSON_DOC]);
    client.send_args(&[b"JSON.DEL", key, b"$"]);
  });
}

#[divan::bench]
fn bench_cmp_json_numincrby_wedb(bencher: Bencher) {
  let db = get_wedb();
  for &k in &JSON_KEYS {
    let _ = db.json_set_one(k, "$", SAMPLE_JSON_STR);
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let key = JSON_KEYS[i % JSON_KEYS.len()];
    i += 1;
    let res = db.json_numincrby(key, "$.counter", "1").unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_json_numincrby_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  for &k in &JSON_KEYS {
    client.send_args(&[b"JSON.SET", k, b"$", SAMPLE_JSON_DOC]);
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let key = JSON_KEYS[i % JSON_KEYS.len()];
    i += 1;
    client.send_args(&[b"JSON.NUMINCRBY", key, b"$.counter", b"1"]);
  });
}

#[divan::bench]
fn bench_cmp_json_arrlen_wedb(bencher: Bencher) {
  let db = get_wedb();
  for &k in &JSON_KEYS {
    let _ = db.json_set_one(k, "$", SAMPLE_JSON_STR);
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let key = JSON_KEYS[i % JSON_KEYS.len()];
    i += 1;
    let res = db.json_arrlen(key, Some("$.list")).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_json_arrlen_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  for &k in &JSON_KEYS {
    client.send_args(&[b"JSON.SET", k, b"$", SAMPLE_JSON_DOC]);
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let key = JSON_KEYS[i % JSON_KEYS.len()];
    i += 1;
    client.send_args(&[b"JSON.ARRLEN", key, b"$.list"]);
  });
}

#[divan::bench]
fn bench_cmp_json_type_wedb(bencher: Bencher) {
  let db = get_wedb();
  for &k in &JSON_KEYS {
    let _ = db.json_set_one(k, "$", SAMPLE_JSON_STR);
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let key = JSON_KEYS[i % JSON_KEYS.len()];
    i += 1;
    let res = db.json_type(key, Some("$.score")).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_json_type_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  for &k in &JSON_KEYS {
    client.send_args(&[b"JSON.SET", k, b"$", SAMPLE_JSON_DOC]);
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let key = JSON_KEYS[i % JSON_KEYS.len()];
    i += 1;
    client.send_args(&[b"JSON.TYPE", key, b"$.score"]);
  });
}

// ═══════════════════════════════════════════════
// 8. 向量与检索模块 (VECTOR.KNN, FT.SEARCH, FT.TAG)
// ═══════════════════════════════════════════════

static SEARCH_MGR_INSTANCE: OnceLock<SearchIndexManager> = OnceLock::new();
const SAMPLE_VEC_BYTES: [u8; 16] = [
  0xcd, 0xcc, 0xcc, 0x3d, // 0.1f32
  0xcd, 0xcc, 0x4c, 0x3e, // 0.2f32
  0x9a, 0x99, 0x99, 0x3e, // 0.3f32
  0xcd, 0xcc, 0xcc, 0x3e, // 0.4f32
];

fn get_search_manager() -> &'static SearchIndexManager {
  SEARCH_MGR_INSTANCE.get_or_init(|| {
    let mut mgr = SearchIndexManager::new();
    let schema_opts = FtCreate {
      index_name: "cmp_vec_idx".to_string(),
      on_data_type: IndexOnDataType::Hash,
      prefixes: vec!["cmp:vec:".to_string()],
      fields: vec![
        IndexField::new("title", IndexFieldType::Text),
        IndexField::with_tag("tag", Some(','), false),
        IndexField::with_vector("vec", 4, DistanceMetric::Cosine),
      ],
      ..Default::default()
    };
    mgr.create_index(schema_opts).expect("create search index");
    let (schema, idx) = mgr.indexes.get_mut("cmp_vec_idx").expect("get index");
    let mut itoa_buf = itoa::Buffer::new();
    for i in 0..100 {
      let doc_id = format!("cmp:vec:{}", itoa_buf.format(i));
      let doc = sonic_rs::json!({
        "title": "Distributed LSM-tree database storage engine indexing and search",
        "tag": "rust,database,vector,search",
        "vec": [0.1 + (i as f64) * 0.001, 0.2, 0.3, 0.4]
      });
      let raw = sonic_rs::to_vec(&doc).unwrap();
      idx
        .index_doc(schema, &doc_id, &raw, Some(1.0), None)
        .expect("index doc");
    }
    mgr
  })
}

fn init_redis_search(client: &mut FastRedisClient) {
  static REDIS_SEARCH_INIT: OnceLock<()> = OnceLock::new();
  REDIS_SEARCH_INIT.get_or_init(|| {
    client.send_args(&[
      b"FT.CREATE",
      b"cmp_vec_idx",
      b"ON",
      b"HASH",
      b"PREFIX",
      b"1",
      b"cmp:vec:",
      b"SCHEMA",
      b"title",
      b"TEXT",
      b"tag",
      b"TAG",
      b"vec",
      b"VECTOR",
      b"HNSW",
      b"6",
      b"TYPE",
      b"FLOAT32",
      b"DIM",
      b"4",
      b"DISTANCE_METRIC",
      b"COSINE",
    ]);
    let mut itoa_buf = itoa::Buffer::new();
    for i in 0..100 {
      let doc_id = format!("cmp:vec:{}", itoa_buf.format(i));
      let mut vec_bytes = SAMPLE_VEC_BYTES;
      let v0 = 0.1f32 + (i as f32) * 0.001;
      vec_bytes[..4].copy_from_slice(&v0.to_le_bytes());
      client.send_args(&[
        b"HSET",
        doc_id.as_bytes(),
        b"title",
        b"Distributed LSM-tree database storage engine indexing and search",
        b"tag",
        b"rust,database,vector,search",
        b"vec",
        &vec_bytes,
      ]);
    }
  });
}

#[divan::bench]
fn bench_cmp_vector_knn_wedb(bencher: Bencher) {
  let mgr = get_search_manager();
  let mut params = RapidHashMap::default();
  params.insert("BLOB".to_string(), "[0.1, 0.2, 0.3, 0.4]".to_string());
  let opts = FtSearch {
    params,
    ..Default::default()
  };
  bencher.bench_local(|| {
    let res = mgr
      .search("cmp_vec_idx", "*=>[KNN 5 @vec $BLOB]", &opts)
      .unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_vector_knn_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  init_redis_search(&mut client);
  bencher.bench_local(|| {
    client.send_args(&[
      b"FT.SEARCH",
      b"cmp_vec_idx",
      b"*=>[KNN 5 @vec $BLOB]",
      b"PARAMS",
      b"2",
      b"BLOB",
      &SAMPLE_VEC_BYTES,
      b"DIALECT",
      b"2",
    ]);
  });
}

#[divan::bench]
fn bench_cmp_search_ft_search_wedb(bencher: Bencher) {
  let mgr = get_search_manager();
  let opts = FtSearch::default();
  bencher.bench_local(|| {
    let res = mgr.search("cmp_vec_idx", "@title:database", &opts).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_search_ft_search_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  init_redis_search(&mut client);
  bencher.bench_local(|| {
    client.send_args(&[b"FT.SEARCH", b"cmp_vec_idx", b"@title:database"]);
  });
}

#[divan::bench]
fn bench_cmp_search_tag_wedb(bencher: Bencher) {
  let mgr = get_search_manager();
  let opts = FtSearch::default();
  bencher.bench_local(|| {
    let res = mgr.search("cmp_vec_idx", "@tag:{rust}", &opts).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_search_tag_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  init_redis_search(&mut client);
  bencher.bench_local(|| {
    client.send_args(&[b"FT.SEARCH", b"cmp_vec_idx", b"@tag:{rust}"]);
  });
}

// ═══════════════════════════════════════════════
// 9. Bloom 模块 (BF.ADD, BF.EXISTS)
// ═══════════════════════════════════════════════

#[divan::bench]
fn bench_cmp_bloom_bf_add_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:bloom:add";
  let _ = db.bf_reserve(key, 0.01, 100_000, [BfReserve::Expansion(2)]);
  let mut i = 0usize;
  bencher.bench_local(|| {
    let member = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    let res = db.bf_add(key, member).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_bloom_bf_add_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:bloom:add";
  let mut i = 0usize;
  bencher.bench_local(|| {
    let member = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    client.send_args(&[b"BF.ADD", key, member]);
  });
}

#[divan::bench]
fn bench_cmp_bloom_bf_exists_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:bloom:exists";
  let _ = db.bf_reserve(key, 0.01, 100_000, [BfReserve::Expansion(2)]);
  for &m in &SAMPLE_MEMBERS {
    let _ = db.bf_add(key, m);
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let member = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    let res = db.bf_exists(key, member).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_bloom_bf_exists_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:bloom:exists";
  for &m in &SAMPLE_MEMBERS {
    client.send_args(&[b"BF.ADD", key, m]);
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let member = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    client.send_args(&[b"BF.EXISTS", key, member]);
  });
}

#[divan::bench]
fn bench_cmp_bloom_bf_info_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:bloom:info";
  let _ = db.bf_reserve(key, 0.01, 100_000, [BfReserve::Expansion(2)]);
  bencher.bench_local(|| {
    let res = db.bf_info(key).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_bloom_bf_info_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:bloom:info";
  client.send_args(&[b"BF.RESERVE", key, b"0.01", b"100000"]);
  bencher.bench_local(|| {
    client.send_args(&[b"BF.INFO", key]);
  });
}

#[divan::bench]
fn bench_cmp_bloom_cf_add_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:bloom:cf_add";
  let _ = db.cf_reserve(
    key,
    100_000,
    [
      CfReserve::BucketSize(4),
      CfReserve::MaxIterations(500),
      CfReserve::Expansion(1),
    ],
  );
  let mut i = 0usize;
  bencher.bench_local(|| {
    let member = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    let res = db.cf_add(key, member).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_bloom_cf_add_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:bloom:cf_add";
  client.send_args(&[b"CF.RESERVE", key, b"100000"]);
  let mut i = 0usize;
  bencher.bench_local(|| {
    let member = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    client.send_args(&[b"CF.ADD", key, member]);
  });
}

#[divan::bench]
fn bench_cmp_bloom_cf_exists_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:bloom:cf_exists";
  let _ = db.cf_reserve(
    key,
    100_000,
    [
      CfReserve::BucketSize(4),
      CfReserve::MaxIterations(500),
      CfReserve::Expansion(1),
    ],
  );
  for &m in &SAMPLE_MEMBERS {
    let _ = db.cf_add(key, m);
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let member = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    let res = db.cf_exists(key, member).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_bloom_cf_exists_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:bloom:cf_exists";
  client.send_args(&[b"CF.RESERVE", key, b"100000"]);
  for &m in &SAMPLE_MEMBERS {
    client.send_args(&[b"CF.ADD", key, m]);
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let member = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    client.send_args(&[b"CF.EXISTS", key, member]);
  });
}

#[divan::bench]
fn bench_cmp_bloom_cf_del_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:bloom:cf_del";
  let _ = db.cf_reserve(
    key,
    100_000,
    [
      CfReserve::BucketSize(4),
      CfReserve::MaxIterations(500),
      CfReserve::Expansion(1),
    ],
  );
  let mut i = 0usize;
  bencher.bench_local(|| {
    let member = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    let _ = db.cf_add(key, member);
    let res = db.cf_del(key, member).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_bloom_cf_del_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:bloom:cf_del";
  client.send_args(&[b"CF.RESERVE", key, b"100000"]);
  let mut i = 0usize;
  bencher.bench_local(|| {
    let member = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    client.send_args(&[b"CF.ADD", key, member]);
    client.send_args(&[b"CF.DEL", key, member]);
  });
}

// ═══════════════════════════════════════════════
// 10. TDigest 模块 (TDIGEST.ADD, TDIGEST.QUANTILE, TDIGEST.BYRANK, TDIGEST.CDF)
// ═══════════════════════════════════════════════

#[divan::bench]
fn bench_cmp_tdigest_add_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:tdigest:add";
  let _ = db.tdigest_create(key, 100.0);
  let mut i = 0u64;
  bencher.bench_local(|| {
    i += 1;
    let val = (i % 1000) as f64;
    db.tdigest_add(key, &[val]).unwrap();
    black_box(());
  });
}

#[divan::bench]
fn bench_cmp_tdigest_add_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:tdigest:add";
  client.send_args(&[b"TDIGEST.CREATE", key, b"100"]);
  let mut itoa_buf = itoa::Buffer::new();
  let mut i = 0u64;
  bencher.bench_local(|| {
    i += 1;
    let val = itoa_buf.format(i % 1000);
    client.send_args(&[b"TDIGEST.ADD", key, val.as_bytes()]);
  });
}

#[divan::bench]
fn bench_cmp_tdigest_quantile_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:tdigest:quantile";
  let _ = db.tdigest_create(key, 100.0);
  for i in 1..=100 {
    let _ = db.tdigest_add(key, &[i as f64]);
  }
  bencher.bench_local(|| {
    let res = db.tdigest_quantile(key, &[0.5]).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_tdigest_quantile_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:tdigest:quantile";
  client.send_args(&[b"TDIGEST.CREATE", key, b"100"]);
  let mut itoa_buf = itoa::Buffer::new();
  for i in 1..=100 {
    let val = itoa_buf.format(i);
    client.send_args(&[b"TDIGEST.ADD", key, val.as_bytes()]);
  }
  bencher.bench_local(|| {
    client.send_args(&[b"TDIGEST.QUANTILE", key, b"0.5"]);
  });
}

#[divan::bench]
fn bench_cmp_tdigest_byrank_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:tdigest:byrank";
  let _ = db.tdigest_create(key, 100.0);
  for i in 1..=100 {
    let _ = db.tdigest_add(key, &[i as f64]);
  }
  bencher.bench_local(|| {
    let res = db.tdigest_byrank(key, &[50]).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_tdigest_byrank_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:tdigest:byrank";
  client.send_args(&[b"TDIGEST.CREATE", key, b"100"]);
  let mut itoa_buf = itoa::Buffer::new();
  for i in 1..=100 {
    let val = itoa_buf.format(i);
    client.send_args(&[b"TDIGEST.ADD", key, val.as_bytes()]);
  }
  bencher.bench_local(|| {
    client.send_args(&[b"TDIGEST.BYRANK", key, b"50"]);
  });
}

#[divan::bench]
fn bench_cmp_tdigest_cdf_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:tdigest:cdf";
  let _ = db.tdigest_create(key, 100.0);
  for i in 1..=100 {
    let _ = db.tdigest_add(key, &[i as f64]);
  }
  bencher.bench_local(|| {
    let res = db.tdigest_cdf(key, &[50.0]).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_tdigest_cdf_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:tdigest:cdf";
  client.send_args(&[b"TDIGEST.CREATE", key, b"100"]);
  let mut itoa_buf = itoa::Buffer::new();
  for i in 1..=100 {
    let val = itoa_buf.format(i);
    client.send_args(&[b"TDIGEST.ADD", key, val.as_bytes()]);
  }
  bencher.bench_local(|| {
    client.send_args(&[b"TDIGEST.CDF", key, b"50"]);
  });
}

// ═══════════════════════════════════════════════
// 11. TimeSeries 模块 (TS.ADD, TS.GET, TS.RANGE, TS.INCRBY)
// ═══════════════════════════════════════════════

#[divan::bench]
fn bench_cmp_timeseries_ts_add_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:ts:add";
  let _ = db.del(&[key]);
  db.ts_create(
    key,
    [TsCreate::RetentionTime(86400000), TsCreate::ChunkSize(4096)],
  )
  .unwrap();
  let mut i = 0u64;
  let base_ts = 5000000000000u64;
  bencher.bench_local(|| {
    i += 1;
    let ts = base_ts + i * 1000;
    let val = 20.0 + (i as f64) * 0.1;
    let res = db.ts_add(key, ts, val, None, None).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_timeseries_ts_add_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:ts:add";
  let mut itoa_buf = itoa::Buffer::new();
  let mut i = 0u64;
  let base_ts = 5000000000000u64;
  bencher.bench_local(|| {
    i += 1;
    let ts_str = itoa_buf.format(base_ts + i * 1000);
    client.send_args(&[b"TS.ADD", key, ts_str.as_bytes(), b"25.5"]);
  });
}

#[divan::bench]
fn bench_cmp_timeseries_ts_get_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:ts:get";
  let _ = db.del(&[key]);
  db.ts_create(
    key,
    [TsCreate::RetentionTime(86400000), TsCreate::ChunkSize(4096)],
  )
  .unwrap();
  let _ = db.ts_add(key, 5000000001000, 25.5, None, None);
  bencher.bench_local(|| {
    let res = db.ts_get(key).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_timeseries_ts_get_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:ts:get";
  client.send_args(&[b"TS.ADD", key, b"5000000001000", b"25.5"]);
  bencher.bench_local(|| {
    client.send_args(&[b"TS.GET", key]);
  });
}

#[divan::bench]
fn bench_cmp_timeseries_ts_range_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:ts:range";
  let _ = db.del(&[key]);
  db.ts_create(
    key,
    [TsCreate::RetentionTime(86400000), TsCreate::ChunkSize(4096)],
  )
  .unwrap();
  let base_ts = 5000000000000u64;
  for i in 1..=200 {
    let _ = db.ts_add(key, base_ts + i * 1000, i as f64, None, None);
  }
  bencher.bench_local(|| {
    let res = db.ts_range(key, (base_ts, base_ts + 200_000), []).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_timeseries_ts_range_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:ts:range";
  let mut itoa_buf = itoa::Buffer::new();
  let base_ts = 5000000000000u64;
  for i in 1..=200 {
    let ts_str = itoa_buf.format(base_ts + i * 1000);
    client.send_args(&[b"TS.ADD", key, ts_str.as_bytes(), b"1.0"]);
  }
  let mut itoa_buf2 = itoa::Buffer::new();
  let from_str = itoa_buf.format(base_ts);
  let to_str = itoa_buf2.format(base_ts + 200_000);
  bencher.bench_local(|| {
    client.send_args(&[b"TS.RANGE", key, from_str.as_bytes(), to_str.as_bytes()]);
  });
}

#[divan::bench]
fn bench_cmp_timeseries_ts_incrby_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:ts:incrby";
  let _ = db.del(&[key]);
  db.ts_create(
    key,
    [TsCreate::RetentionTime(86400000), TsCreate::ChunkSize(4096)],
  )
  .unwrap();
  bencher.bench_local(|| {
    let res = db.ts_incrby(key, 1.0, None, None).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_timeseries_ts_incrby_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:ts:incrby";
  client.send_args(&[b"TS.CREATE", key]);
  bencher.bench_local(|| {
    client.send_args(&[b"TS.INCRBY", key, b"1"]);
  });
}

// ═══════════════════════════════════════════════
// 12. HyperLogLog 模块 (PFADD, PFCOUNT)
// ═══════════════════════════════════════════════

#[divan::bench]
fn bench_cmp_hll_pfadd_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:hll:pfadd";
  let mut i = 0usize;
  bencher.bench_local(|| {
    let member = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    let res = db.pfadd(key, &[member]).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_hll_pfadd_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:hll:pfadd";
  let mut i = 0usize;
  bencher.bench_local(|| {
    let member = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    client.send_args(&[b"PFADD", key, member]);
  });
}

#[divan::bench]
fn bench_cmp_hll_pfcount_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:hll:pfcount";
  for &m in &SAMPLE_MEMBERS {
    let _ = db.pfadd(key, &[m]);
  }
  bencher.bench_local(|| {
    let res = db.pfcount(&[key]).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_hll_pfcount_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:hll:pfcount";
  for &m in &SAMPLE_MEMBERS {
    client.send_args(&[b"PFADD", key, m]);
  }
  bencher.bench_local(|| {
    client.send_args(&[b"PFCOUNT", key]);
  });
}

// ═══════════════════════════════════════════════
// 13. Geo 模块 (GEOADD, GEODIST, GEOPOS, GEOHASH)
// ═══════════════════════════════════════════════

#[divan::bench]
fn bench_cmp_geo_geoadd_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:geo:add";
  let mut i = 0usize;
  bencher.bench_local(|| {
    let member = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    let lon = 116.4 + (i % 100) as f64 * 0.001;
    let lat = 39.9 + (i % 100) as f64 * 0.001;
    i += 1;
    let res = db.geoadd(key, &[(lon, lat, member)], []).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_geo_geoadd_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:geo:add";
  let mut i = 0usize;
  bencher.bench_local(|| {
    let member = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    client.send_args(&[b"GEOADD", key, b"116.4074", b"39.9042", member]);
  });
}

#[divan::bench]
fn bench_cmp_geo_geodist_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:geo:dist";
  let _ = db.geoadd(
    key,
    &[
      (116.4074, 39.9042, b"beijing" as &[u8]),
      (121.4737, 31.2304, b"shanghai"),
    ],
    [],
  );
  bencher.bench_local(|| {
    let res = db.geodist(key, b"beijing", b"shanghai", None).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_geo_geodist_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:geo:dist";
  client.send_args(&[
    b"GEOADD",
    key,
    b"116.4074",
    b"39.9042",
    b"beijing",
    b"121.4737",
    b"31.2304",
    b"shanghai",
  ]);
  bencher.bench_local(|| {
    client.send_args(&[b"GEODIST", key, b"beijing", b"shanghai"]);
  });
}

#[divan::bench]
fn bench_cmp_geo_geopos_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:geo:geopos";
  let _ = db.geoadd(
    key,
    &[
      (116.4074, 39.9042, b"beijing" as &[u8]),
      (121.4737, 31.2304, b"shanghai"),
    ],
    [],
  );
  bencher.bench_local(|| {
    let res = db.geopos(key, &[b"beijing" as &[u8]]).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_geo_geopos_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:geo:geopos";
  client.send_args(&[
    b"GEOADD",
    key,
    b"116.4074",
    b"39.9042",
    b"beijing",
    b"121.4737",
    b"31.2304",
    b"shanghai",
  ]);
  bencher.bench_local(|| {
    client.send_args(&[b"GEOPOS", key, b"beijing"]);
  });
}

#[divan::bench]
fn bench_cmp_geo_geohash_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:geo:geohash";
  let _ = db.geoadd(
    key,
    &[
      (116.4074, 39.9042, b"beijing" as &[u8]),
      (121.4737, 31.2304, b"shanghai"),
    ],
    [],
  );
  bencher.bench_local(|| {
    let res = db.geohash(key, &[b"beijing" as &[u8]]).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_geo_geohash_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:geo:geohash";
  client.send_args(&[
    b"GEOADD",
    key,
    b"116.4074",
    b"39.9042",
    b"beijing",
    b"121.4737",
    b"31.2304",
    b"shanghai",
  ]);
  bencher.bench_local(|| {
    client.send_args(&[b"GEOHASH", key, b"beijing"]);
  });
}

// ═══════════════════════════════════════════════
// 14. Stream 模块 (XADD, XLEN, XRANGE, XREAD, XDEL)
// ═══════════════════════════════════════════════

#[divan::bench]
fn bench_cmp_stream_xadd_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:stream:xadd";
  bencher.bench_local(|| {
    let res = db.xadd(key, (), &[(b"sensor", b"temp")]).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_stream_xadd_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:stream:xadd";
  bencher.bench_local(|| {
    client.send_args(&[b"XADD", key, b"*", b"sensor", b"temp"]);
  });
}

#[divan::bench]
fn bench_cmp_stream_xlen_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:stream:xlen";
  for _ in 0..50 {
    let _ = db.xadd(key, (), &[(b"sensor", b"temp")]);
  }
  bencher.bench_local(|| {
    let res = db.xlen(key).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_stream_xlen_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:stream:xlen";
  for _ in 0..50 {
    client.send_args(&[b"XADD", key, b"*", b"sensor", b"temp"]);
  }
  bencher.bench_local(|| {
    client.send_args(&[b"XLEN", key]);
  });
}

#[divan::bench]
fn bench_cmp_stream_xrange_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:stream:xrange";
  for _ in 0..50 {
    let _ = db.xadd(key, (), &[(b"sensor", b"temp")]);
  }
  bencher.bench_local(|| {
    let res = db
      .xrange(key, (StreamId::min(), StreamId::max(), 10))
      .unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_stream_xrange_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:stream:xrange";
  for _ in 0..50 {
    client.send_args(&[b"XADD", key, b"*", b"sensor", b"temp"]);
  }
  bencher.bench_local(|| {
    client.send_args(&[b"XRANGE", key, b"-", b"+", b"COUNT", b"10"]);
  });
}

#[divan::bench]
fn bench_cmp_stream_xread_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:stream:xread";
  for _ in 0..50 {
    let _ = db.xadd(key, (), &[(b"sensor", b"temp")]);
  }
  bencher.bench_local(|| {
    let res = db.xread(key, StreamId::min(), Some(10)).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_stream_xread_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:stream:xread";
  for _ in 0..50 {
    client.send_args(&[b"XADD", key, b"*", b"sensor", b"temp"]);
  }
  bencher.bench_local(|| {
    client.send_args(&[b"XREAD", b"COUNT", b"10", b"STREAMS", key, b"0-0"]);
  });
}

#[divan::bench]
fn bench_cmp_stream_xdel_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:stream:xdel";
  bencher.bench_local(|| {
    let id = db.xadd(key, (), &[(b"sensor", b"temp")]).unwrap();
    let res = db.xdel(key, &[id]).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_stream_xdel_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:stream:xdel";
  let mut id_buf = [0u8; 64];
  bencher.bench_local(|| {
    client.send_args(&[b"XADD", key, b"*", b"k", b"v"]);
    let id_len = {
      let id_str = unsafe { str::from_utf8_unchecked(&client.buf[..client.read_len]) };
      let id = id_str.lines().nth(1).unwrap_or("0-0").trim();
      let len = id.len().min(64);
      id_buf[..len].copy_from_slice(&id.as_bytes()[..len]);
      len
    };
    client.send_args(&[b"XDEL", key, &id_buf[..id_len]]);
  });
}

// ═══════════════════════════════════════════════
// 15. Generic DB 模块 (DEL, EXISTS, EXPIRE, TTL)
// ═══════════════════════════════════════════════

#[divan::bench]
fn bench_cmp_db_del_wedb(bencher: Bencher) {
  let db = get_wedb();
  let mut i = 0usize;
  bencher.bench_local(|| {
    let key = DEL_KEYS[i % DEL_KEYS.len()];
    i += 1;
    let res = db.del(&[key]).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_db_del_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let mut i = 0usize;
  bencher.bench_local(|| {
    let key = DEL_KEYS[i % DEL_KEYS.len()];
    i += 1;
    client.send_args(&[b"DEL", key]);
  });
}

#[divan::bench]
fn bench_cmp_db_exists_wedb(bencher: Bencher) {
  let db = get_wedb();
  for &k in &EXISTS_KEYS {
    db.set(k, b"val", []).unwrap();
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let key = EXISTS_KEYS[i % EXISTS_KEYS.len()];
    i += 1;
    let res = db.exists(&[key]).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_db_exists_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  for &k in &EXISTS_KEYS {
    client.send_args(&[b"SET", k, b"val"]);
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let key = EXISTS_KEYS[i % EXISTS_KEYS.len()];
    i += 1;
    client.send_args(&[b"EXISTS", key]);
  });
}

#[divan::bench]
fn bench_cmp_db_expire_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:db:expire";
  db.set(key, b"val", []).unwrap();
  bencher.bench_local(|| {
    let res = db.expire(key, 3600).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_db_expire_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:db:expire";
  client.send_args(&[b"SET", key, b"val"]);
  bencher.bench_local(|| {
    client.send_args(&[b"EXPIRE", key, b"3600"]);
  });
}

#[divan::bench]
fn bench_cmp_db_ttl_wedb(bencher: Bencher) {
  let db = get_wedb();
  let key = b"cmp:db:ttl";
  db.set(key, b"val", []).unwrap();
  let _ = db.expire(key, 3600);
  bencher.bench_local(|| {
    let res = db.ttl(key).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_cmp_db_ttl_redis(bencher: Bencher) {
  let Some(mut client) = FastRedisClient::new() else {
    return;
  };
  let key = b"cmp:db:ttl";
  client.send_args(&[b"SET", key, b"val"]);
  client.send_args(&[b"EXPIRE", key, b"3600"]);
  bencher.bench_local(|| {
    client.send_args(&[b"TTL", key]);
  });
}
