use std::path::Path;

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

const WEDB_BENCH_5GB_DIR: &str = "/tmp/wedb_bench_data_5gb";

fn setup_db() -> (Option<tempfile::TempDir>, wedb_embed::Db<Fjall>) {
  let (dir, path) = if Path::new(WEDB_BENCH_5GB_DIR).exists() {
    (None, WEDB_BENCH_5GB_DIR.to_string())
  } else {
    let dir = tempdir().expect("tempdir");
    let p = dir.path().join("data").to_string_lossy().to_string();
    (Some(dir), p)
  };
  let engine = Fjall::open(&path).expect("open engine");
  let db = WeDb::new(engine).ns(0).expect("ns 0").db(0).expect("db 0");
  (dir, db)
}

// ───────────────────────────────────────────────
// 静态零分配 Key 与字段池
// ───────────────────────────────────────────────

const STR_KEYS: [&[u8]; 10] = [
  b"bench:str:0",
  b"bench:str:1",
  b"bench:str:2",
  b"bench:str:3",
  b"bench:str:4",
  b"bench:str:5",
  b"bench:str:6",
  b"bench:str:7",
  b"bench:str:8",
  b"bench:str:9",
];

const MSET_KEYS: [&[u8]; 10] = [
  b"bench:mset:0",
  b"bench:mset:1",
  b"bench:mset:2",
  b"bench:mset:3",
  b"bench:mset:4",
  b"bench:mset:5",
  b"bench:mset:6",
  b"bench:mset:7",
  b"bench:mset:8",
  b"bench:mset:9",
];

const SAMPLE_FIELDS: [&[u8]; 10] = [
  b"f0", b"f1", b"f2", b"f3", b"f4", b"f5", b"f6", b"f7", b"f8", b"f9",
];

const SAMPLE_MEMBERS: [&[u8]; 10] = [
  b"m0", b"m1", b"m2", b"m3", b"m4", b"m5", b"m6", b"m7", b"m8", b"m9",
];

const DEL_KEYS: [&[u8]; 10] = [
  b"bench:del:0",
  b"bench:del:1",
  b"bench:del:2",
  b"bench:del:3",
  b"bench:del:4",
  b"bench:del:5",
  b"bench:del:6",
  b"bench:del:7",
  b"bench:del:8",
  b"bench:del:9",
];

const JSON_KEYS: [&str; 10] = [
  "bench:json:0",
  "bench:json:1",
  "bench:json:2",
  "bench:json:3",
  "bench:json:4",
  "bench:json:5",
  "bench:json:6",
  "bench:json:7",
  "bench:json:8",
  "bench:json:9",
];

const SAMPLE_JSON_STR: &str = r#"{"name":"EnterpriseNode","tier":"cluster_l3","tags":["rust","database","lsm"],"score":99.5,"counter":100,"list":[1,2,3,4,5,6,7,8,9,10]}"#;

// ───────────────────────────────────────────────
// 1. String 模块单命令基准测试
// ───────────────────────────────────────────────

#[divan::bench]
fn bench_str_set(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let mut i = 0usize;
  let val = b"value_128_bytes_payload_for_benchmarking_lsm_tree_storage_engine_write_path_efficiency_and_throughput_check_zero_allocation";
  bencher.bench_local(|| {
    let key = STR_KEYS[i % STR_KEYS.len()];
    i += 1;
    db.set(key, val, []).unwrap();
  });
}

#[divan::bench]
fn bench_str_get(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let val = b"value_128_bytes_payload_for_benchmarking_lsm_tree_storage_engine_write_path_efficiency_and_throughput_check_zero_allocation";
  for &k in &STR_KEYS {
    db.set(k, val, []).unwrap();
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let key = STR_KEYS[i % STR_KEYS.len()];
    i += 1;
    let res = db.get(key).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_str_mset(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let pairs: [(&[u8], &[u8]); 10] = [
    (MSET_KEYS[0], b"v0"),
    (MSET_KEYS[1], b"v1"),
    (MSET_KEYS[2], b"v2"),
    (MSET_KEYS[3], b"v3"),
    (MSET_KEYS[4], b"v4"),
    (MSET_KEYS[5], b"v5"),
    (MSET_KEYS[6], b"v6"),
    (MSET_KEYS[7], b"v7"),
    (MSET_KEYS[8], b"v8"),
    (MSET_KEYS[9], b"v9"),
  ];
  bencher.bench_local(|| {
    db.mset(&pairs).unwrap();
  });
}

#[divan::bench]
fn bench_str_mget(bencher: Bencher) {
  let (_dir, db) = setup_db();
  for &k in &MSET_KEYS {
    db.set(k, b"val", []).unwrap();
  }
  bencher.bench_local(|| {
    let res = db.mget(&MSET_KEYS).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_str_incrby(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:str:incrby";
  let _ = db.set(key, b"0", []);
  bencher.bench_local(|| {
    let val = db.incrby(key, 1).unwrap();
    black_box(val);
  });
}

#[divan::bench]
fn bench_str_decrby(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:str:decrby";
  db.set(key, b"1000000000", []).unwrap();
  bencher.bench_local(|| {
    let val = db.decrby(key, 1).unwrap();
    black_box(val);
  });
}

#[divan::bench]
fn bench_str_append(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:str:append";
  let _ = db.set(key, b"base_", []);
  bencher.bench_local(|| {
    let len = db.append(key, b"x").unwrap();
    black_box(len);
  });
}

#[divan::bench]
fn bench_str_strlen(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:str:strlen";
  db.set(key, b"hello_world_wedb_embed_benchmark_payload", [])
    .unwrap();
  bencher.bench_local(|| {
    let len = db.strlen(key).unwrap();
    black_box(len);
  });
}

#[divan::bench]
fn bench_str_getdel(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let mut i = 0usize;
  bencher.bench_local(|| {
    let key = STR_KEYS[i % STR_KEYS.len()];
    i += 1;
    let _ = db.set(key, b"val", []);
    let res = db.getdel(key).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_str_getrange(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:str:getrange";
  db.set(key, b"abcdefghijklmnopqrstuvwxyz0123456789", [])
    .unwrap();
  bencher.bench_local(|| {
    let range = db.getrange(key, (5, 20)).unwrap();
    black_box(range);
  });
}

#[divan::bench]
fn bench_str_setrange(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:str:setrange";
  db.set(key, b"abcdefghijklmnopqrstuvwxyz0123456789", [])
    .unwrap();
  bencher.bench_local(|| {
    let len = db.setrange(key, 6, b"redis").unwrap();
    black_box(len);
  });
}

// ───────────────────────────────────────────────
// 2. Hash 模块单命令基准测试
// ───────────────────────────────────────────────

#[divan::bench]
fn bench_hash_hset(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:hash:hset";
  let mut i = 0usize;
  let val: &[u8] = b"hash_val_128_bytes_payload_benchmarking_lsm_tree_storage_engine_write_path_efficiency_and_throughput_check_zero_alloc";
  bencher.bench_local(|| {
    let f = SAMPLE_FIELDS[i % SAMPLE_FIELDS.len()];
    i += 1;
    db.hset(key, &[(f, val)]).unwrap();
  });
}

#[divan::bench]
fn bench_hash_hget(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:hash:hget";
  let val: &[u8] = b"hash_val_128_bytes_payload_benchmarking_lsm_tree_storage_engine_write_path_efficiency_and_throughput_check_zero_alloc";
  for &f in &SAMPLE_FIELDS {
    db.hset(key, &[(f, val)]).unwrap();
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let f = SAMPLE_FIELDS[i % SAMPLE_FIELDS.len()];
    i += 1;
    let res = db.hget(key, f).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_hash_hdel(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:hash:hdel";
  for &f in &SAMPLE_FIELDS {
    db.hset(key, &[(f, b"val" as &[u8])]).unwrap();
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let f = SAMPLE_FIELDS[i % SAMPLE_FIELDS.len()];
    i += 1;
    let deleted = db.hdel(key, &[f]).unwrap();
    black_box(deleted);
  });
}

#[divan::bench]
fn bench_hash_hexists(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:hash:hexists";
  db.hset(key, &[(b"target_field" as &[u8], b"val" as &[u8])])
    .unwrap();
  bencher.bench_local(|| {
    let exists = db.hexists(key, b"target_field").unwrap();
    black_box(exists);
  });
}

#[divan::bench]
fn bench_hash_hlen(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:hash:hlen";
  for &f in &SAMPLE_FIELDS {
    db.hset(key, &[(f, b"val" as &[u8])]).unwrap();
  }
  bencher.bench_local(|| {
    let len = db.hlen(key).unwrap();
    black_box(len);
  });
}

#[divan::bench]
fn bench_hash_hmget(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:hash:hmget";
  for &f in &SAMPLE_FIELDS {
    db.hset(key, &[(f, b"val" as &[u8])]).unwrap();
  }
  bencher.bench_local(|| {
    let res = db.hmget(key, &SAMPLE_FIELDS).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_hash_hgetall(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:hash:hgetall";
  for &f in &SAMPLE_FIELDS {
    db.hset(key, &[(f, b"val" as &[u8])]).unwrap();
  }
  bencher.bench_local(|| {
    let all = db.hgetall(key).unwrap();
    black_box(all);
  });
}

#[divan::bench]
fn bench_hash_hkeys(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:hash:hkeys";
  for &f in &SAMPLE_FIELDS {
    db.hset(key, &[(f, b"val" as &[u8])]).unwrap();
  }
  bencher.bench_local(|| {
    let keys = db.hkeys(key).unwrap();
    black_box(keys);
  });
}

#[divan::bench]
fn bench_hash_hvals(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:hash:hvals";
  for &f in &SAMPLE_FIELDS {
    db.hset(key, &[(f, b"val" as &[u8])]).unwrap();
  }
  bencher.bench_local(|| {
    let vals = db.hvals(key).unwrap();
    black_box(vals);
  });
}

#[divan::bench]
fn bench_hash_hincrby(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:hash:hincrby";
  let field = b"counter";
  bencher.bench_local(|| {
    let val = db.hincrby(key, field, 1).unwrap();
    black_box(val);
  });
}

// ───────────────────────────────────────────────
// 3. List 模块单命令基准测试
// ───────────────────────────────────────────────

#[divan::bench]
fn bench_list_lpush(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:list:lpush";
  bencher.bench_local(|| {
    db.lpush(key, &[b"item"]).unwrap();
  });
}

#[divan::bench]
fn bench_list_rpush(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:list:rpush";
  bencher.bench_local(|| {
    db.rpush(key, &[b"item"]).unwrap();
  });
}

#[divan::bench]
fn bench_list_lpop(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:list:lpop";
  for _ in 0..50 {
    db.lpush(key, &[b"item"]).unwrap();
  }
  bencher.bench_local(|| {
    let _ = db.lpush(key, &[b"item"]);
    let popped = db.lpop(key, 1).unwrap();
    black_box(popped);
  });
}

#[divan::bench]
fn bench_list_rpop(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:list:rpop";
  for _ in 0..50 {
    db.rpush(key, &[b"item"]).unwrap();
  }
  bencher.bench_local(|| {
    let _ = db.rpush(key, &[b"item"]);
    let popped = db.rpop(key, 1).unwrap();
    black_box(popped);
  });
}

#[divan::bench]
fn bench_list_llen(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:list:llen";
  for _ in 0..10 {
    db.lpush(key, &[b"item"]).unwrap();
  }
  bencher.bench_local(|| {
    let len = db.llen(key).unwrap();
    black_box(len);
  });
}

#[divan::bench]
fn bench_list_lrange(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:list:lrange";
  for _ in 0..20 {
    db.lpush(key, &[b"item"]).unwrap();
  }
  bencher.bench_local(|| {
    let items = db.lrange(key, (0, 10)).unwrap();
    black_box(items);
  });
}

#[divan::bench]
fn bench_list_lindex(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:list:lindex";
  for _ in 0..20 {
    db.lpush(key, &[b"item"]).unwrap();
  }
  bencher.bench_local(|| {
    let item = db.lindex(key, 0).unwrap();
    black_box(item);
  });
}

#[divan::bench]
fn bench_list_lset(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:list:lset";
  for _ in 0..20 {
    db.lpush(key, &[b"item"]).unwrap();
  }
  bencher.bench_local(|| {
    db.lset(key, 0, b"updated_val").unwrap();
  });
}

#[divan::bench]
fn bench_list_lrem(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:list:lrem";
  bencher.bench_local(|| {
    let _ = db.lpush(key, &[b"elem"]);
    let rem = db.lrem(key, 1, b"elem").unwrap();
    black_box(rem);
  });
}

#[divan::bench]
fn bench_list_ltrim(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:list:ltrim";
  for _ in 0..20 {
    db.lpush(key, &[b"item"]).unwrap();
  }
  bencher.bench_local(|| {
    db.ltrim(key, (0, 9)).unwrap();
  });
}

// ───────────────────────────────────────────────
// 4. Set 模块单命令基准测试
// ───────────────────────────────────────────────

#[divan::bench]
fn bench_set_sadd(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:set:sadd";
  let mut i = 0usize;
  bencher.bench_local(|| {
    let member = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    db.sadd(key, &[member]).unwrap();
  });
}

#[divan::bench]
fn bench_set_srem(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:set:srem";
  for &m in &SAMPLE_MEMBERS {
    db.sadd(key, &[m]).unwrap();
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let member = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    let rem = db.srem(key, &[member]).unwrap();
    black_box(rem);
  });
}

#[divan::bench]
fn bench_set_sismember(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:set:sismember";
  for &m in &SAMPLE_MEMBERS {
    db.sadd(key, &[m]).unwrap();
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let member = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    let is_member = db.sismember(key, member).unwrap();
    black_box(is_member);
  });
}

#[divan::bench]
fn bench_set_smembers(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:set:smembers";
  for &m in &SAMPLE_MEMBERS {
    db.sadd(key, &[m]).unwrap();
  }
  bencher.bench_local(|| {
    let members = db.smembers(key).unwrap();
    black_box(members);
  });
}

#[divan::bench]
fn bench_set_scard(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:set:scard";
  for &m in &SAMPLE_MEMBERS {
    db.sadd(key, &[m]).unwrap();
  }
  bencher.bench_local(|| {
    let card = db.scard(key).unwrap();
    black_box(card);
  });
}

#[divan::bench]
fn bench_set_spop(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:set:spop";
  for &m in &SAMPLE_MEMBERS {
    let _ = db.sadd(key, &[m]);
  }
  bencher.bench_local(|| {
    let _ = db.sadd(key, &[b"item" as &[u8]]);
    let popped = db.spop(key, 1).unwrap();
    black_box(popped);
  });
}

#[divan::bench]
fn bench_set_srandmember(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:set:srandmember";
  for &m in &SAMPLE_MEMBERS {
    let _ = db.sadd(key, &[m]);
  }
  bencher.bench_local(|| {
    let res = db.srandmember(key, 5).unwrap();
    black_box(res);
  });
}

// ───────────────────────────────────────────────
// 5. ZSet 模块单命令基准测试
// ───────────────────────────────────────────────

#[divan::bench]
fn bench_zset_zadd(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:zset:zadd";
  let mut i = 0usize;
  bencher.bench_local(|| {
    let member = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    db.zadd(key, &[(i as f64, member)], []).unwrap();
  });
}

#[divan::bench]
fn bench_zset_zrem(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:zset:zrem";
  for (idx, &m) in SAMPLE_MEMBERS.iter().enumerate() {
    db.zadd(key, &[(idx as f64, m)], []).unwrap();
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let member = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    let rem = db.zrem(key, &[member]).unwrap();
    black_box(rem);
  });
}

#[divan::bench]
fn bench_zset_zscore(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:zset:zscore";
  for (idx, &m) in SAMPLE_MEMBERS.iter().enumerate() {
    db.zadd(key, &[(idx as f64, m)], []).unwrap();
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let member = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    let score = db.zscore(key, member).unwrap();
    black_box(score);
  });
}

#[divan::bench]
fn bench_zset_zcard(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:zset:zcard";
  for (idx, &m) in SAMPLE_MEMBERS.iter().enumerate() {
    db.zadd(key, &[(idx as f64, m)], []).unwrap();
  }
  bencher.bench_local(|| {
    let card = db.zcard(key).unwrap();
    black_box(card);
  });
}

#[divan::bench]
fn bench_zset_zcount(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:zset:zcount";
  for (idx, &m) in SAMPLE_MEMBERS.iter().enumerate() {
    db.zadd(key, &[(idx as f64, m)], []).unwrap();
  }
  let spec = RangeScore::new(0.0, 100.0);
  bencher.bench_local(|| {
    let count = db.zcount(key, spec).unwrap();
    black_box(count);
  });
}

#[divan::bench]
fn bench_zset_zincrby(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:zset:zincrby";
  let member = b"target_member";
  bencher.bench_local(|| {
    let score = db.zincrby(key, 1.5, member).unwrap();
    black_box(score);
  });
}

#[divan::bench]
fn bench_zset_zrank(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:zset:zrank";
  for (idx, &m) in SAMPLE_MEMBERS.iter().enumerate() {
    db.zadd(key, &[(idx as f64, m)], []).unwrap();
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let member = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    let rank = db.zrank(key, member).unwrap();
    black_box(rank);
  });
}

#[divan::bench]
fn bench_zset_zrange(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:zset:zrange";
  for (idx, &m) in SAMPLE_MEMBERS.iter().enumerate() {
    db.zadd(key, &[(idx as f64, m)], []).unwrap();
  }
  bencher.bench_local(|| {
    let range = db.zrange(key, b"0", b"10", []).unwrap();
    black_box(range);
  });
}

#[divan::bench]
fn bench_zset_zrevrange(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:zset:zrevrange";
  for (idx, &m) in SAMPLE_MEMBERS.iter().enumerate() {
    db.zadd(key, &[(idx as f64, m)], []).unwrap();
  }
  bencher.bench_local(|| {
    let range = db.zrevrange(key, (0, 10)).unwrap();
    black_box(range);
  });
}

#[divan::bench]
fn bench_zset_zpopmin(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:zset:zpopmin";
  for (idx, &m) in SAMPLE_MEMBERS.iter().enumerate() {
    db.zadd(key, &[(idx as f64, m)], []).unwrap();
  }
  bencher.bench_local(|| {
    let _ = db.zadd(key, &[(100.0, b"temp_m" as &[u8])], []);
    let popped = db.zpopmin(key, 1).unwrap();
    black_box(popped);
  });
}

// ───────────────────────────────────────────────
// 6. Bitmap 模块单命令基准测试
// ───────────────────────────────────────────────

#[divan::bench]
fn bench_bitmap_setbit(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = "bench:bitmap:setbit";
  let mut i = 0u64;
  bencher.bench_local(|| {
    i = (i + 1) % 100_000;
    db.setbit(key, i, 1).unwrap();
  });
}

#[divan::bench]
fn bench_bitmap_getbit(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = "bench:bitmap:getbit";
  for i in 0..100 {
    db.setbit(key, i * 8, 1).unwrap();
  }
  let mut i = 0u64;
  bencher.bench_local(|| {
    i = (i + 1) % 100;
    let bit = db.getbit(key, i * 8).unwrap();
    black_box(bit);
  });
}

fn populate_bitmap(db: &wedb_embed::Db<wedb_embed::Fjall>, key: &str) {
  for i in 0..100 {
    let _ = db.setbit(key, i * 8, 1);
  }
}

#[divan::bench]
fn bench_bitmap_bitcount(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = "bench:bitmap:bitcount";
  populate_bitmap(&db, key);
  bencher.bench_local(|| {
    let count = db.bitcount(key, []).unwrap();
    black_box(count);
  });
}

#[divan::bench]
fn bench_bitmap_bitpos(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = "bench:bitmap:bitpos";
  populate_bitmap(&db, key);
  bencher.bench_local(|| {
    let pos = db.bitpos(key, 1, []).unwrap();
    black_box(pos);
  });
}

// ───────────────────────────────────────────────
// 7. JSON 模块单命令基准测试
// ───────────────────────────────────────────────

#[divan::bench]
fn bench_json_set(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let mut i = 0usize;
  bencher.bench_local(|| {
    let key = JSON_KEYS[i % JSON_KEYS.len()];
    i += 1;
    db.json_set_one(key, "$", SAMPLE_JSON_STR).unwrap();
  });
}

#[divan::bench]
fn bench_json_get(bencher: Bencher) {
  let (_dir, db) = setup_db();
  for &k in &JSON_KEYS {
    db.json_set_one(k, "$", SAMPLE_JSON_STR).unwrap();
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let key = JSON_KEYS[i % JSON_KEYS.len()];
    i += 1;
    let val = db.json_get_one(key, "$.tags").unwrap();
    black_box(val);
  });
}

#[divan::bench]
fn bench_json_del(bencher: Bencher) {
  let (_dir, db) = setup_db();
  for &k in &JSON_KEYS {
    db.json_set_one(k, "$", SAMPLE_JSON_STR).unwrap();
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let key = JSON_KEYS[i % JSON_KEYS.len()];
    i += 1;
    let _ = db.json_set_one(key, "$", SAMPLE_JSON_STR);
    let deleted = db.json_del(key, Some("$")).unwrap();
    black_box(deleted);
  });
}

#[divan::bench]
fn bench_json_type(bencher: Bencher) {
  let (_dir, db) = setup_db();
  for &k in &JSON_KEYS {
    db.json_set_one(k, "$", SAMPLE_JSON_STR).unwrap();
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let key = JSON_KEYS[i % JSON_KEYS.len()];
    i += 1;
    let typ = db.json_type(key, Some("$.score")).unwrap();
    black_box(typ);
  });
}

#[divan::bench]
fn bench_json_numincrby(bencher: Bencher) {
  let (_dir, db) = setup_db();
  for &k in &JSON_KEYS {
    db.json_set_one(k, "$", SAMPLE_JSON_STR).unwrap();
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let key = JSON_KEYS[i % JSON_KEYS.len()];
    i += 1;
    let val = db.json_numincrby(key, "$.counter", "1").unwrap();
    black_box(val);
  });
}

#[divan::bench]
fn bench_json_arrlen(bencher: Bencher) {
  let (_dir, db) = setup_db();
  for &k in &JSON_KEYS {
    db.json_set_one(k, "$", SAMPLE_JSON_STR).unwrap();
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let key = JSON_KEYS[i % JSON_KEYS.len()];
    i += 1;
    let len = db.json_arrlen(key, Some("$.list")).unwrap();
    black_box(len);
  });
}

// ───────────────────────────────────────────────
// 8. Bloom & Cuckoo 模块单命令基准测试
// ───────────────────────────────────────────────

#[divan::bench]
fn bench_bloom_bf_add(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:bloom:bf_add";
  db.bf_reserve(key, 0.01, 100_000, [BfReserve::Expansion(2)])
    .unwrap();
  let mut i = 0usize;
  bencher.bench_local(|| {
    let item = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    db.bf_add(key, item).unwrap();
  });
}

#[divan::bench]
fn bench_bloom_bf_exists(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:bloom:bf_exists";
  db.bf_reserve(key, 0.01, 100_000, [BfReserve::Expansion(2)])
    .unwrap();
  for &item in &SAMPLE_MEMBERS {
    db.bf_add(key, item).unwrap();
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let item = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    let exists = db.bf_exists(key, item).unwrap();
    black_box(exists);
  });
}

#[divan::bench]
fn bench_bloom_bf_info(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:bloom:bf_info";
  db.bf_reserve(key, 0.01, 100_000, [BfReserve::Expansion(2)])
    .unwrap();
  bencher.bench_local(|| {
    let info = db.bf_info(key).unwrap();
    black_box(info);
  });
}

#[divan::bench]
fn bench_bloom_cf_add(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:bloom:cf_add";
  db.cf_reserve(
    key,
    100_000,
    [
      CfReserve::BucketSize(2),
      CfReserve::MaxIterations(20),
      CfReserve::Expansion(1),
    ],
  )
  .unwrap();
  let mut i = 0usize;
  bencher.bench_local(|| {
    let item = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    db.cf_add(key, item).unwrap();
  });
}

#[divan::bench]
fn bench_bloom_cf_exists(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:bloom:cf_exists";
  db.cf_reserve(
    key,
    100_000,
    [
      CfReserve::BucketSize(2),
      CfReserve::MaxIterations(20),
      CfReserve::Expansion(1),
    ],
  )
  .unwrap();
  for &item in &SAMPLE_MEMBERS {
    db.cf_add(key, item).unwrap();
  }
  let mut i = 0usize;
  bencher.bench_local(|| {
    let item = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    let exists = db.cf_exists(key, item).unwrap();
    black_box(exists);
  });
}

#[divan::bench]
fn bench_bloom_cf_del(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:bloom:cf_del";
  db.cf_reserve(
    key,
    100_000,
    [
      CfReserve::BucketSize(2),
      CfReserve::MaxIterations(20),
      CfReserve::Expansion(1),
    ],
  )
  .unwrap();
  let mut i = 0usize;
  bencher.bench_local(|| {
    let item = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    let _ = db.cf_add(key, item);
    let deleted = db.cf_del(key, item).unwrap();
    black_box(deleted);
  });
}

// ───────────────────────────────────────────────
// 9. TimeSeries 模块单命令基准测试
// ───────────────────────────────────────────────

#[divan::bench]
fn bench_timeseries_ts_add(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:ts:add";
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
    db.ts_add(key, ts, val, None, None).unwrap();
  });
}

#[divan::bench]
fn bench_timeseries_ts_get(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:ts:get";
  let _ = db.del(&[key]);
  db.ts_create(
    key,
    [TsCreate::RetentionTime(86400000), TsCreate::ChunkSize(4096)],
  )
  .unwrap();
  let _ = db.ts_add(key, 5000000001000, 25.5, None, None);
  bencher.bench_local(|| {
    let sample = db.ts_get(key).unwrap();
    black_box(sample);
  });
}

#[divan::bench]
fn bench_timeseries_ts_range(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:ts:range";
  let _ = db.del(&[key]);
  db.ts_create(
    key,
    [TsCreate::RetentionTime(86400000), TsCreate::ChunkSize(4096)],
  )
  .unwrap();
  let base_ts = 5000000000000u64;
  for i in 1..=500 {
    let _ = db.ts_add(key, base_ts + i * 1000, i as f64, None, None);
  }
  bencher.bench_local(|| {
    let samples = db.ts_range(key, (base_ts, base_ts + 500_000), []).unwrap();
    black_box(samples);
  });
}

#[divan::bench]
fn bench_timeseries_ts_incrby(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:ts:incrby";
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
    let val = db.ts_incrby(key, 1.0, Some(ts), None).unwrap();
    black_box(val);
  });
}

// ───────────────────────────────────────────────
// 10. Geo 模块单命令基准测试
// ───────────────────────────────────────────────

#[divan::bench]
fn bench_geo_geoadd(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:geo:geoadd";
  let mut i = 0usize;
  bencher.bench_local(|| {
    let member = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    let lon = 116.40 + (i as f64) * 0.0001;
    let lat = 39.90 + (i as f64) * 0.0001;
    i += 1;
    db.geoadd(key, &[(lon, lat, member)], []).unwrap();
  });
}

#[divan::bench]
fn bench_geo_geodist(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:geo:geodist";
  db.geoadd(
    key,
    &[
      (116.4074, 39.9042, b"Beijing" as &[u8]),
      (121.4737, 31.2304, b"Shanghai"),
    ],
    [],
  )
  .unwrap();
  bencher.bench_local(|| {
    let dist = db.geodist(key, b"Beijing", b"Shanghai", None).unwrap();
    black_box(dist);
  });
}

#[divan::bench]
fn bench_geo_geopos(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:geo:geopos";
  db.geoadd(key, &[(116.4074, 39.9042, b"Beijing" as &[u8])], [])
    .unwrap();
  bencher.bench_local(|| {
    let pos = db.geopos(key, &[b"Beijing" as &[u8]]).unwrap();
    black_box(pos);
  });
}

#[divan::bench]
fn bench_geo_geohash(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:geo:geohash";
  db.geoadd(key, &[(116.4074, 39.9042, b"Beijing" as &[u8])], [])
    .unwrap();
  bencher.bench_local(|| {
    let hash = db.geohash(key, &[b"Beijing" as &[u8]]).unwrap();
    black_box(hash);
  });
}

// ───────────────────────────────────────────────
// 11. HyperLogLog 模块单命令基准测试
// ───────────────────────────────────────────────

#[divan::bench]
fn bench_hll_pfadd(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:hll:pfadd";
  let mut i = 0usize;
  bencher.bench_local(|| {
    let elem = SAMPLE_MEMBERS[i % SAMPLE_MEMBERS.len()];
    i += 1;
    db.pfadd(key, &[elem]).unwrap();
  });
}

#[divan::bench]
fn bench_hll_pfcount(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:hll:pfcount";
  for &elem in &SAMPLE_MEMBERS {
    db.pfadd(key, &[elem]).unwrap();
  }
  bencher.bench_local(|| {
    let count = db.pfcount(&[key]).unwrap();
    black_box(count);
  });
}

#[divan::bench]
fn bench_hll_pfmerge(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let k1: &[u8] = b"bench:hll:k1";
  let k2: &[u8] = b"bench:hll:k2";
  let dest: &[u8] = b"bench:hll:dest";
  for &m in &SAMPLE_MEMBERS {
    db.pfadd(k1, &[m]).unwrap();
    db.pfadd(k2, &[m]).unwrap();
  }
  let sources: [&[u8]; 2] = [k1, k2];
  bencher.bench_local(|| {
    db.pfmerge(dest, &sources).unwrap();
  });
}

// ───────────────────────────────────────────────
// 12. TDigest 模块单命令基准测试
// ───────────────────────────────────────────────

#[divan::bench]
fn bench_tdigest_add(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:tdigest:add";
  let _ = db.tdigest_create(key, 100.0);
  let mut i = 0u64;
  bencher.bench_local(|| {
    i += 1;
    let val = (i % 1000) as f64;
    db.tdigest_add(key, &[val]).unwrap();
  });
}

#[divan::bench]
fn bench_tdigest_quantile(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:tdigest:quantile";
  let _ = db.tdigest_create(key, 100.0);
  let vals: Vec<f64> = (0..1000).map(|i| i as f64).collect();
  let _ = db.tdigest_add(key, &vals);
  bencher.bench_local(|| {
    let q = db.tdigest_quantile(key, &[0.5, 0.95, 0.99]).unwrap();
    black_box(q);
  });
}

#[divan::bench]
fn bench_tdigest_byrank(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:tdigest:byrank";
  let _ = db.tdigest_create(key, 100.0);
  let vals: Vec<f64> = (0..1000).map(|i| i as f64).collect();
  let _ = db.tdigest_add(key, &vals);
  bencher.bench_local(|| {
    let r = db.tdigest_byrank(key, &[100, 500, 900]).unwrap();
    black_box(r);
  });
}

#[divan::bench]
fn bench_tdigest_cdf(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:tdigest:cdf";
  let _ = db.tdigest_create(key, 100.0);
  let vals: Vec<f64> = (0..1000).map(|i| i as f64).collect();
  let _ = db.tdigest_add(key, &vals);
  bencher.bench_local(|| {
    let cdf = db.tdigest_cdf(key, &[250.0, 500.0, 750.0]).unwrap();
    black_box(cdf);
  });
}

// ───────────────────────────────────────────────
// 13. SortedInt 模块单命令基准测试
// ───────────────────────────────────────────────

#[divan::bench]
fn bench_sortedint_si_add(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:si:add";
  let mut i = 0u64;
  bencher.bench_local(|| {
    i += 1;
    db.si_add(key, &[i]).unwrap();
  });
}

#[divan::bench]
fn bench_sortedint_si_card(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:si:card";
  let ids: Vec<u64> = (0..1000).collect();
  db.si_add(key, &ids).unwrap();
  bencher.bench_local(|| {
    let card = db.si_card(key).unwrap();
    black_box(card);
  });
}

#[divan::bench]
fn bench_sortedint_si_exists(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:si:exists";
  let ids: Vec<u64> = (0..1000).collect();
  db.si_add(key, &ids).unwrap();
  let mut i = 0u64;
  bencher.bench_local(|| {
    i = (i + 1) % 1000;
    let exists = db.si_exists(key, i).unwrap();
    black_box(exists);
  });
}

#[divan::bench]
fn bench_sortedint_si_range(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:si:range";
  let ids: Vec<u64> = (0..1000).collect();
  db.si_add(key, &ids).unwrap();
  bencher.bench_local(|| {
    let range = db.si_range(key, 0, 0, 50, false).unwrap();
    black_box(range);
  });
}

#[divan::bench]
fn bench_sortedint_si_rem(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:si:rem";
  let mut i = 0u64;
  bencher.bench_local(|| {
    i += 1;
    db.si_add(key, &[i]).unwrap();
    let rem = db.si_rem(key, &[i]).unwrap();
    black_box(rem);
  });
}

// ───────────────────────────────────────────────
// 14. Stream 模块单命令基准测试
// ───────────────────────────────────────────────

#[divan::bench]
fn bench_stream_xadd(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:stream:xadd";
  bencher.bench_local(|| {
    let _id = db.xadd(key, (), &[(b"sensor", b"temp")]).unwrap();
  });
}

#[divan::bench]
fn bench_stream_xlen(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:stream:xlen";
  for _ in 1..=500 {
    db.xadd(key, (), &[(b"k", b"v")]).unwrap();
  }
  bencher.bench_local(|| {
    let len = db.xlen(key).unwrap();
    black_box(len);
  });
}

#[divan::bench]
fn bench_stream_xrange(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:stream:xrange";
  for _ in 1..=500 {
    db.xadd(key, (), &[(b"k", b"v")]).unwrap();
  }
  bencher.bench_local(|| {
    let entries = db
      .xrange(key, (StreamId::min(), StreamId::max(), 50))
      .unwrap();
    black_box(entries);
  });
}

#[divan::bench]
fn bench_stream_xread(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:stream:xread";
  for _ in 1..=500 {
    db.xadd(key, (), &[(b"k", b"v")]).unwrap();
  }
  bencher.bench_local(|| {
    let read = db.xread(key, StreamId::min(), Some(50)).unwrap();
    black_box(read);
  });
}

#[divan::bench]
fn bench_stream_xdel(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:stream:xdel";
  bencher.bench_local(|| {
    let id = db.xadd(key, (), &[(b"k", b"v")]).unwrap();
    let deleted = db.xdel(key, &[id]).unwrap();
    black_box(deleted);
  });
}

// ───────────────────────────────────────────────
// 15. DB / 命名空间 / 事务操作基准测试
// ───────────────────────────────────────────────

#[divan::bench]
fn bench_db_exists(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:db:exists";
  db.set(key, b"val", []).unwrap();
  bencher.bench_local(|| {
    let exists = db.exists(&[key]).unwrap();
    black_box(exists);
  });
}

#[divan::bench]
fn bench_db_del(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let mut i = 0usize;
  bencher.bench_local(|| {
    let key = DEL_KEYS[i % DEL_KEYS.len()];
    i += 1;
    db.set(key, b"val", []).unwrap();
    let deleted = db.del(&[key]).unwrap();
    black_box(deleted);
  });
}

#[divan::bench]
fn bench_db_expire(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:db:expire";
  db.set(key, b"val", []).unwrap();
  bencher.bench_local(|| {
    let res = db.expire(key, 3600).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_db_ttl(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let key = b"bench:db:ttl";
  db.set(key, b"val", []).unwrap();
  let _ = db.expire(key, 3600);
  bencher.bench_local(|| {
    let res = db.ttl(key).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_db_namespace(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let ns_db = db.wedb().ns(1).unwrap().db(0).unwrap();
  let mut i = 0usize;
  bencher.bench_local(|| {
    let key = STR_KEYS[i % STR_KEYS.len()];
    i += 1;
    ns_db.set(key, b"payload", []).unwrap();
    let val = ns_db.get(key).unwrap();
    black_box(val);
  });
}

#[divan::bench]
fn bench_db_batch_commit(bencher: Bencher) {
  let (_dir, db) = setup_db();
  let batch_keys: [&[u8]; 10] = [
    b"bench:batch:0",
    b"bench:batch:1",
    b"bench:batch:2",
    b"bench:batch:3",
    b"bench:batch:4",
    b"bench:batch:5",
    b"bench:batch:6",
    b"bench:batch:7",
    b"bench:batch:8",
    b"bench:batch:9",
  ];
  bencher.bench_local(|| {
    let mut batch = db.batch();
    for &key in &batch_keys {
      batch.insert_data(key, b"batch_value_payload");
    }
    batch.commit().unwrap();
  });
}

// ───────────────────────────────────────────────
// 16. Search & Vector 检索模块基准测试
// ───────────────────────────────────────────────

fn setup_search_mgr() -> SearchIndexManager {
  let mut mgr = SearchIndexManager::new();
  let schema_opts = FtCreate {
    index_name: "bench_vec_idx".to_string(),
    on_data_type: IndexOnDataType::Hash,
    prefixes: vec!["doc:".to_string()],
    fields: vec![
      IndexField::new("title", IndexFieldType::Text),
      IndexField::with_tag("tag", Some(','), false),
      IndexField::with_vector("vec", 4, DistanceMetric::Cosine),
    ],
    ..Default::default()
  };
  mgr.create_index(schema_opts).unwrap();
  let (schema, idx) = mgr.indexes.get_mut("bench_vec_idx").unwrap();
  let mut itoa_buf = itoa::Buffer::new();
  for i in 0..100 {
    let doc_id = format!("doc:{}", itoa_buf.format(i));
    let doc = sonic_rs::json!({
      "title": "Distributed LSM-tree database storage engine indexing and search",
      "tag": "rust,database,vector,search",
      "vec": [0.1 + (i as f64) * 0.001, 0.2, 0.3, 0.4]
    });
    let raw = sonic_rs::to_vec(&doc).unwrap();
    idx
      .index_doc(schema, &doc_id, &raw, Some(1.0), None)
      .unwrap();
  }
  mgr
}

#[divan::bench]
fn bench_search_ft_search(bencher: Bencher) {
  let mgr = setup_search_mgr();
  let opts = FtSearch::default();
  bencher.bench_local(|| {
    let res = mgr
      .search("bench_vec_idx", "@title:database", &opts)
      .unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_search_tag(bencher: Bencher) {
  let mgr = setup_search_mgr();
  let opts = FtSearch::default();
  bencher.bench_local(|| {
    let res = mgr.search("bench_vec_idx", "@tag:{rust}", &opts).unwrap();
    black_box(res);
  });
}

#[divan::bench]
fn bench_vector_knn(bencher: Bencher) {
  let mgr = setup_search_mgr();
  let mut params = RapidHashMap::default();
  params.insert("BLOB".to_string(), "[0.1, 0.2, 0.3, 0.4]".to_string());
  let opts = FtSearch {
    params,
    ..Default::default()
  };
  bencher.bench_local(|| {
    let res = mgr
      .search("bench_vec_idx", "*=>[KNN 5 @vec $BLOB]", &opts)
      .unwrap();
    black_box(res);
  });
}
