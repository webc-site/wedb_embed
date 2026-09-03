### macOS (Apple M2 Max)

#### Hardware & Test Environment

CPU: Apple M2 Max (12 cores)<br>
Memory: 64.0 GB<br>
OS: macOS 26.5.1 (Darwin 25.5.0)<br>
Rust: 1.98.0 (88d9e12ae 2026-08-18)<br>
Redis: v8.10.1

#### Physical Footprint & Memory Benchmark (5GB Dataset Scale)

| Resource Metric | wedb_embed (Embedded LSM+LZ4) | Redis (v8.10.1 AOF Mode) | Resource Savings |
| :--- | :--- | :--- | :--- |
| **Dataset Scale** | 500,000 Structured Items | 500,000 Structured Items | All 14 Data Formats |
| **Raw Uncompressed Payload** | 437 MB | 437 MB | Structured Payload |
| **Physical Disk Footprint** | **287 MB** | **768 MB** | **Saves 63%** |
| **Resident Memory (RSS)** | **380 MB** | **517 MB** | **Saves 26%** |

#### wedb_embed vs Redis Core Command Benchmark

| Command | wedb_embed P95 Latency | Redis P95 Latency | Speedup |
| :--- | :--- | :--- | :--- |
| `SET` | 7.6 us | 49.3 us | **6.5x** |
| `GET` | 13.4 us | 85.9 us | **6.4x** |
| `MSET` | 59.0 us | 54.4 us | **0.9x** |
| `MGET` | 2.6 us | 43.1 us | **16.9x** |
| `INCRBY` | 0.58 us | 48.0 us | **82.6x** |
| `DECRBY` | 1.1 us | 47.5 us | **42.3x** |
| `APPEND` | 0.61 us | 49.8 us | **81.5x** |
| `STRLEN` | 0.23 us | 43.3 us | **189.3x** |
| `GETDEL` | 9.0 us | 189.6 us | **21.1x** |
| `GETRANGE` | 0.22 us | 44.9 us | **208.6x** |
| `SETRANGE` | 0.59 us | 50.0 us | **85.0x** |
| `HSET` | 1.8 us | 50.6 us | **28.4x** |
| `HGET` | 0.71 us | 40.8 us | **57.7x** |
| `HMGET` | 3.5 us | 45.4 us | **12.8x** |
| `HEXISTS` | 0.63 us | 42.8 us | **68.0x** |
| `HLEN` | 0.48 us | 40.7 us | **84.1x** |
| `HDEL` | 5.5 us | 45.1 us | **8.2x** |
| `HGETALL` | 2.2 us | 41.4 us | **18.9x** |
| `HKEYS` | 2.0 us | 41.1 us | **20.6x** |
| `HVALS` | 2.0 us | 45.4 us | **22.6x** |
| `HINCRBY` | 1.5 us | 47.4 us | **31.6x** |
| `LPUSH` | 1.5 us | 50.2 us | **33.9x** |
| `RPUSH` | 1.7 us | 45.0 us | **26.7x** |
| `LPOP` | 1.9 us | 43.7 us | **23.6x** |
| `RPOP` | 1.8 us | 47.9 us | **25.9x** |
| `LLEN` | 0.44 us | 37.2 us | **84.7x** |
| `LRANGE` | 2.6 us | 46.8 us | **17.7x** |
| `LINDEX` | 0.65 us | 35.4 us | **54.8x** |
| `LSET` | 0.90 us | 47.3 us | **52.7x** |
| `LREM` | 9.2 us | 91.7 us | **10.0x** |
| `LTRIM` | 0.96 us | 40.9 us | **42.7x** |
| `SADD` | 1.3 us | 40.3 us | **30.7x** |
| `SREM` | 4.9 us | 43.8 us | **9.0x** |
| `SISMEMBER` | 0.65 us | 35.1 us | **54.3x** |
| `SCARD` | 0.47 us | 35.7 us | **75.8x** |
| `SMEMBERS` | 2.0 us | 41.9 us | **20.7x** |
| `SPOP` | 5.2 us | 92.1 us | **17.7x** |
| `SRANDMEMBER` | 2.0 us | 40.8 us | **20.6x** |
| `ZADD` | 2.5 us | 38.4 us | **15.3x** |
| `ZSCORE` | 0.82 us | 43.7 us | **53.1x** |
| `ZRANGE` | 2.7 us | 45.0 us | **16.9x** |
| `ZCARD` | 0.54 us | 34.8 us | **65.0x** |
| `ZCOUNT` | 2.0 us | 41.0 us | **20.4x** |
| `ZINCRBY` | 2.3 us | 47.8 us | **20.5x** |
| `ZRANK` | 2.4 us | 41.4 us | **17.3x** |
| `ZREVRANGE` | 4.0 us | 41.1 us | **10.3x** |
| `ZPOPMIN` | 5.6 us | 91.3 us | **16.4x** |
| `ZREM` | 5.1 us | 43.7 us | **8.6x** |
| `SETBIT` | 9.0 us | 53.6 us | **5.9x** |
| `GETBIT` | 0.42 us | 41.7 us | **100.1x** |
| `BITCOUNT` | 0.35 us | 48.3 us | **137.9x** |
| `BITPOS` | 0.41 us | 36.8 us | **89.0x** |
| `PFADD` | 2.2 us | 47.5 us | **21.7x** |
| `PFCOUNT` | 32.8 us | 46.9 us | **1.4x** |
| `GEOADD` | 1.9 us | 48.5 us | **25.3x** |
| `GEODIST` | 0.93 us | 44.7 us | **48.0x** |
| `GEOPOS` | 0.67 us | 40.9 us | **61.3x** |
| `GEOHASH` | 0.79 us | 41.9 us | **52.9x** |
| `XADD` | 1.3 us | 47.9 us | **35.9x** |
| `XLEN` | 0.51 us | 45.9 us | **89.9x** |
| `XRANGE` | 2.6 us | 50.1 us | **19.6x** |
| `XREAD` | 2.5 us | 50.6 us | **19.9x** |
| `XDEL` | 3.1 us | 96.0 us | **30.5x** |
| `DEL` | 4.0 us | 38.9 us | **9.7x** |
| `EXISTS` | 0.20 us | 35.0 us | **176.7x** |
| `EXPIRE` | 0.56 us | 40.2 us | **71.6x** |
| `TTL` | 0.22 us | 36.8 us | **170.4x** |
| `JSON.SET` | 2.5 us | 41.2 us | **16.7x** |
| `JSON.GET` | 1.2 us | 36.7 us | **30.4x** |
| `JSON.DEL` | 6.5 us | 80.5 us | **12.4x** |
| `JSON.NUMINCRBY` | 2.6 us | 41.2 us | **16.0x** |
| `JSON.ARRLEN` | 1.2 us | 39.6 us | **34.0x** |
| `JSON.TYPE` | 1.2 us | 41.1 us | **35.1x** |
| `BF.ADD` | 14.8 us | 69.4 us | **4.7x** |
| `BF.EXISTS` | 0.63 us | 37.1 us | **59.2x** |
| `BF.INFO` | 0.44 us | 37.2 us | **84.7x** |
| `CF.ADD` | 2.3 us | 40.1 us | **17.4x** |
| `CF.EXISTS` | 0.80 us | 35.5 us | **44.3x** |
| `CF.DEL` | 6.8 us | 75.7 us | **11.1x** |
| `TDIGEST.ADD` | 2.2 us | 42.7 us | **19.0x** |
| `TDIGEST.QUANTILE` | 0.96 us | 46.8 us | **48.9x** |
| `TDIGEST.BYRANK` | 0.92 us | 41.2 us | **44.9x** |
| `TDIGEST.CDF` | 1.1 us | 40.9 us | **37.8x** |
| `TS.ADD` | 5.4 us | 48.6 us | **9.1x** |
| `TS.GET` | 1.2 us | 45.5 us | **39.2x** |
| `TS.RANGE` | 13.3 us | 77.5 us | **5.8x** |
| `TS.INCRBY` | 7.4 us | 44.5 us | **6.0x** |
| `FT.SEARCH` | 20.5 us | 67.8 us | **3.3x** |
| `FT.TAG` | 20.7 us | 58.7 us | **2.8x** |
| `VECTOR.KNN` | 2.4 us | 63.5 us | **26.3x** |

