### Ubuntu CI (GitHub Actions Runner)

#### Hardware & Test Environment

CPU: Intel(R) Xeon(R) 6973P-C (4 cores)<br>
Memory: 15.6 GB<br>
Disk: Azure Managed Virtual Disk (Cloud Standard SSD)<br>
OS: Ubuntu 24.04.4 LTS (Linux 6.17.0-1022-azure)<br>
Rust: 1.98.0 (88d9e12ae 2026-08-18)<br>
Redis: v8.10.1

#### Physical Footprint & Memory Benchmark (4.3 GB Dataset Scale)

| Resource Metric | wedb_embed (Embedded LSM+LZ4) | Redis (v8.10.1 AOF Mode) | Resource Savings |
| :--- | :--- | :--- | :--- |
| **Dataset Scale** | 5,000,000 Structured Items | 5,000,000 Structured Items | All 14 Data Formats |
| **Raw Uncompressed Payload** | 4377 MB | 4377 MB | Structured Payload |
| **Physical Disk Footprint** | **1053 MB** | **7980 MB** | **Saves 87%** |
| **Resident Memory (RSS)** | **282 MB** | **4836 MB** | **Saves 94%** |

#### wedb_embed vs Redis Core Command Benchmark

| Command | wedb_embed P95 Latency | Redis P95 Latency | Speedup |
| :--- | :--- | :--- | :--- |
| `SET` | 6.9 us | 22.3 us | **3.2x** |
| `GET` | 4.4 us | 21.9 us | **4.9x** |
| `MSET` | 45.6 us | 29.9 us | **0.7x** |
| `MGET` | 4.3 us | 21.3 us | **5.0x** |
| `INCRBY` | 0.54 us | 21.8 us | **40.4x** |
| `DECRBY` | 0.69 us | 22.0 us | **31.8x** |
| `APPEND` | 0.80 us | 38.3 us | **47.6x** |
| `STRLEN` | 0.25 us | 21.5 us | **85.9x** |
| `GETDEL` | 7.4 us | 42.6 us | **5.8x** |
| `GETRANGE` | 0.22 us | 21.4 us | **98.4x** |
| `SETRANGE` | 0.73 us | 21.3 us | **29.3x** |
| `HSET` | 2.0 us | 28.0 us | **13.9x** |
| `HGET` | 0.57 us | 19.1 us | **33.3x** |
| `HMGET` | 2.4 us | 27.5 us | **11.3x** |
| `HEXISTS` | 0.53 us | 19.1 us | **35.8x** |
| `HLEN` | 0.36 us | 18.4 us | **50.7x** |
| `HDEL` | 3.9 us | 22.5 us | **5.8x** |
| `HGETALL` | 2.6 us | 24.2 us | **9.4x** |
| `HKEYS` | 2.4 us | 20.7 us | **8.5x** |
| `HVALS` | 2.5 us | 21.6 us | **8.8x** |
| `HINCRBY` | 1.5 us | 28.3 us | **19.3x** |
| `LPUSH` | 1.7 us | 47.7 us | **27.6x** |
| `RPUSH` | 1.8 us | 37.8 us | **21.1x** |
| `LPOP` | 1.9 us | 51.4 us | **27.1x** |
| `RPOP` | 1.9 us | 41.8 us | **21.5x** |
| `LLEN` | 0.38 us | 48.3 us | **127.4x** |
| `LRANGE` | 2.6 us | 49.1 us | **18.9x** |
| `LINDEX` | 0.59 us | 49.2 us | **83.2x** |
| `LSET` | 0.87 us | 49.2 us | **56.3x** |
| `LREM` | 13.8 us | 98.6 us | **7.2x** |
| `LTRIM` | 0.91 us | 48.4 us | **53.3x** |
| `SADD` | 1.1 us | 37.9 us | **34.7x** |
| `SREM` | 3.5 us | 37.8 us | **10.7x** |
| `SISMEMBER` | 0.56 us | 37.8 us | **67.3x** |
| `SCARD` | 0.38 us | 37.3 us | **98.6x** |
| `SMEMBERS` | 2.5 us | 37.9 us | **15.3x** |
| `SPOP` | 4.7 us | 75.4 us | **16.2x** |
| `SRANDMEMBER` | 2.0 us | 37.5 us | **18.9x** |
| `ZADD` | 2.3 us | 21.6 us | **9.6x** |
| `ZSCORE` | 0.70 us | 21.7 us | **31.2x** |
| `ZRANGE` | 2.9 us | 21.4 us | **7.5x** |
| `ZCARD` | 0.41 us | 20.3 us | **49.5x** |
| `ZCOUNT` | 2.4 us | 21.1 us | **8.8x** |
| `ZINCRBY` | 2.3 us | 22.0 us | **9.6x** |
| `ZRANK` | 2.5 us | 20.9 us | **8.3x** |
| `ZREVRANGE` | 4.0 us | 21.6 us | **5.4x** |
| `ZPOPMIN` | 6.0 us | 43.1 us | **7.2x** |
| `ZREM` | 4.0 us | 21.7 us | **5.5x** |
| `SETBIT` | 11.8 us | 27.2 us | **2.3x** |
| `GETBIT` | 0.39 us | 20.0 us | **51.3x** |
| `BITCOUNT` | 0.35 us | 21.2 us | **60.5x** |
| `BITPOS` | 0.41 us | 21.8 us | **53.9x** |
| `PFADD` | 2.4 us | 21.6 us | **8.9x** |
| `PFCOUNT` | 30.3 us | 19.0 us | **0.6x** |
| `GEOADD` | 2.1 us | 27.7 us | **13.0x** |
| `GEODIST` | 0.82 us | 20.6 us | **25.1x** |
| `GEOPOS` | 0.60 us | 19.6 us | **32.5x** |
| `GEOHASH` | 0.63 us | 19.5 us | **30.9x** |
| `XADD` | 1.3 us | 21.8 us | **16.3x** |
| `XLEN` | 0.48 us | 18.5 us | **38.6x** |
| `XRANGE` | 3.0 us | 29.3 us | **9.9x** |
| `XREAD` | 2.9 us | 29.7 us | **10.2x** |
| `XDEL` | 3.1 us | 43.1 us | **13.7x** |
| `DEL` | 3.1 us | 19.5 us | **6.3x** |
| `EXISTS` | 0.21 us | 18.9 us | **92.2x** |
| `EXPIRE` | 0.71 us | 29.6 us | **41.9x** |
| `TTL` | 0.22 us | 19.3 us | **87.5x** |
| `JSON.SET` | 2.8 us | 48.7 us | **17.4x** |
| `JSON.GET` | 1.1 us | 50.1 us | **44.3x** |
| `JSON.DEL` | 6.0 us | 44.4 us | **7.4x** |
| `JSON.NUMINCRBY` | 2.8 us | 48.0 us | **17.2x** |
| `JSON.ARRLEN` | 1.0 us | 21.6 us | **21.4x** |
| `JSON.TYPE` | 1.0 us | 48.2 us | **47.6x** |
| `BF.ADD` | 11.3 us | 21.7 us | **1.9x** |
| `BF.EXISTS` | 0.55 us | 21.6 us | **39.1x** |
| `BF.INFO` | 0.34 us | 21.1 us | **61.7x** |
| `CF.ADD` | 2.3 us | 20.4 us | **8.8x** |
| `CF.EXISTS` | 0.57 us | 21.7 us | **37.8x** |
| `CF.DEL` | 6.5 us | 41.5 us | **6.4x** |
| `TDIGEST.ADD` | 2.0 us | 20.9 us | **10.3x** |
| `TDIGEST.QUANTILE` | 0.80 us | 20.1 us | **25.2x** |
| `TDIGEST.BYRANK` | 0.84 us | 20.5 us | **24.4x** |
| `TDIGEST.CDF` | 0.90 us | 19.8 us | **22.1x** |
| `TS.ADD` | 5.0 us | 21.6 us | **4.4x** |
| `TS.GET` | 1.0 us | 21.1 us | **20.7x** |
| `TS.RANGE` | 21.2 us | 21.6 us | **1.0x** |
| `TS.INCRBY` | 6.6 us | 19.7 us | **3.0x** |
| `FT.SEARCH` | 17.7 us | 37.8 us | **2.1x** |
| `FT.TAG` | 17.7 us | 37.9 us | **2.1x** |
| `VECTOR.KNN` | 6.7 us | 21.1 us | **3.2x** |

