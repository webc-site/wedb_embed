### Ubuntu CI (GitHub Actions Runner)

#### Hardware & Test Environment

CPU: AMD EPYC 7763 64-Core Processor (4 cores)<br>
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
| **Physical Disk Footprint** | **1051 MB** | **7650 MB** | **Saves 86%** |
| **Resident Memory (RSS)** | **272 MB** | **4809 MB** | **Saves 94%** |

#### wedb_embed vs Redis Core Command Benchmark

| Command | wedb_embed P95 Latency | Redis P95 Latency | Speedup |
| :--- | :--- | :--- | :--- |
| `SET` | 7.4 us | 29.9 us | **4.0x** |
| `GET` | 5.1 us | 20.2 us | **3.9x** |
| `MSET` | 49.2 us | 34.9 us | **0.7x** |
| `MGET` | 5.3 us | 28.4 us | **5.4x** |
| `INCRBY` | 1.2 us | 28.4 us | **23.4x** |
| `DECRBY` | 1.1 us | 27.4 us | **25.3x** |
| `APPEND` | 1.3 us | 27.6 us | **21.9x** |
| `STRLEN` | 0.28 us | 19.5 us | **69.7x** |
| `GETDEL` | 8.2 us | 58.5 us | **7.1x** |
| `GETRANGE` | 0.35 us | 19.4 us | **55.9x** |
| `SETRANGE` | 1.4 us | 27.8 us | **19.7x** |
| `HSET` | 2.5 us | 30.0 us | **12.1x** |
| `HGET` | 1.3 us | 18.8 us | **14.5x** |
| `HMGET` | 3.4 us | 28.1 us | **8.2x** |
| `HEXISTS` | 1.1 us | 19.5 us | **17.5x** |
| `HLEN` | 0.45 us | 19.8 us | **44.2x** |
| `HDEL` | 5.0 us | 25.0 us | **5.0x** |
| `HGETALL` | 3.5 us | 20.9 us | **5.9x** |
| `HKEYS` | 3.3 us | 19.4 us | **5.8x** |
| `HVALS` | 3.4 us | 19.6 us | **5.8x** |
| `HINCRBY` | 1.9 us | 29.9 us | **15.9x** |
| `LPUSH` | 2.6 us | 29.3 us | **11.1x** |
| `RPUSH` | 2.9 us | 28.9 us | **10.0x** |
| `LPOP` | 2.5 us | 24.2 us | **9.9x** |
| `RPOP` | 2.5 us | 23.7 us | **9.6x** |
| `LLEN` | 0.47 us | 19.0 us | **40.1x** |
| `LRANGE` | 3.4 us | 19.0 us | **5.5x** |
| `LINDEX` | 0.73 us | 19.1 us | **26.1x** |
| `LSET` | 1.2 us | 28.6 us | **24.0x** |
| `LREM` | 17.2 us | 49.8 us | **2.9x** |
| `LTRIM` | 1.1 us | 21.1 us | **19.0x** |
| `SADD` | 1.4 us | 28.1 us | **19.6x** |
| `SREM` | 3.7 us | 23.5 us | **6.3x** |
| `SISMEMBER` | 0.74 us | 19.6 us | **26.6x** |
| `SCARD` | 0.48 us | 20.2 us | **42.6x** |
| `SMEMBERS` | 3.8 us | 19.6 us | **5.2x** |
| `SPOP` | 7.6 us | 63.4 us | **8.3x** |
| `SRANDMEMBER` | 3.2 us | 19.9 us | **6.3x** |
| `ZADD` | 3.1 us | 29.6 us | **9.5x** |
| `ZSCORE` | 0.93 us | 19.1 us | **20.6x** |
| `ZRANGE` | 3.8 us | 19.8 us | **5.2x** |
| `ZCARD` | 0.50 us | 19.7 us | **39.6x** |
| `ZCOUNT` | 3.3 us | 18.9 us | **5.8x** |
| `ZINCRBY` | 3.4 us | 30.9 us | **9.2x** |
| `ZRANK` | 3.3 us | 18.8 us | **5.7x** |
| `ZREVRANGE` | 5.7 us | 19.4 us | **3.4x** |
| `ZPOPMIN` | 14.9 us | 61.9 us | **4.1x** |
| `ZREM` | 4.4 us | 24.3 us | **5.5x** |
| `SETBIT` | 16.6 us | 25.7 us | **1.5x** |
| `GETBIT` | 0.64 us | 20.4 us | **32.1x** |
| `BITCOUNT` | 0.54 us | 33.1 us | **61.6x** |
| `BITPOS` | 0.68 us | 23.8 us | **35.0x** |
| `PFADD` | 3.0 us | 26.4 us | **8.9x** |
| `PFCOUNT` | 8.3 us | 19.4 us | **2.4x** |
| `GEOADD` | 2.6 us | 44.5 us | **17.3x** |
| `GEODIST` | 0.95 us | 37.0 us | **39.2x** |
| `GEOPOS` | 1.2 us | 37.6 us | **31.1x** |
| `GEOHASH` | 0.80 us | 36.3 us | **45.6x** |
| `XADD` | 1.7 us | 32.2 us | **19.2x** |
| `XLEN` | 0.58 us | 19.2 us | **32.9x** |
| `XRANGE` | 3.8 us | 31.1 us | **8.1x** |
| `XREAD` | 4.1 us | 32.8 us | **8.0x** |
| `XDEL` | 3.7 us | 63.3 us | **17.2x** |
| `DEL` | 3.0 us | 34.0 us | **11.2x** |
| `EXISTS` | 0.25 us | 36.0 us | **143.7x** |
| `EXPIRE` | 0.73 us | 44.8 us | **61.3x** |
| `TTL` | 0.29 us | 36.3 us | **124.2x** |
| `JSON.SET` | 3.6 us | 19.5 us | **5.4x** |
| `JSON.GET` | 1.5 us | 19.1 us | **12.6x** |
| `JSON.DEL` | 8.4 us | 39.0 us | **4.7x** |
| `JSON.NUMINCRBY` | 3.9 us | 19.0 us | **4.9x** |
| `JSON.ARRLEN` | 1.4 us | 19.3 us | **14.2x** |
| `JSON.TYPE` | 1.4 us | 19.3 us | **13.9x** |
| `BF.ADD` | 54.1 us | 22.7 us | **0.4x** |
| `BF.EXISTS` | 0.63 us | 22.5 us | **35.7x** |
| `BF.INFO` | 0.36 us | 37.3 us | **102.8x** |
| `CF.ADD` | 2.5 us | 36.7 us | **14.5x** |
| `CF.EXISTS` | 0.67 us | 38.0 us | **57.0x** |
| `CF.DEL` | 10.3 us | 75.0 us | **7.3x** |
| `TDIGEST.ADD` | 2.5 us | 19.4 us | **7.7x** |
| `TDIGEST.QUANTILE` | 1.1 us | 19.0 us | **17.8x** |
| `TDIGEST.BYRANK` | 1.2 us | 19.0 us | **16.1x** |
| `TDIGEST.CDF` | 1.3 us | 19.2 us | **15.0x** |
| `TS.ADD` | 6.7 us | 19.8 us | **3.0x** |
| `TS.GET` | 1.5 us | 18.9 us | **12.6x** |
| `TS.RANGE` | 20.7 us | 19.2 us | **0.9x** |
| `TS.INCRBY` | 9.8 us | 18.6 us | **1.9x** |
| `FT.SEARCH` | 28.2 us | 19.1 us | **0.7x** |
| `FT.TAG` | 27.5 us | 19.3 us | **0.7x** |
| `VECTOR.KNN` | 7.7 us | 20.8 us | **2.7x** |

