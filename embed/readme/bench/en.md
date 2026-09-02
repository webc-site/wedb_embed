### Ubuntu CI (GitHub Actions Runner)

#### Hardware & Test Environment

CPU: AMD EPYC 7763 64-Core Processor (4 cores)<br>
Memory: 15.6 GB<br>
Disk: Azure Managed Virtual Disk (Cloud Standard SSD)<br>
OS: Ubuntu 24.04.4 LTS (Linux 6.17.0-1022-azure)<br>
Rust: 1.98.0 (88d9e12ae 2026-08-18)<br>
Redis: v8.10.1

#### Physical Footprint & Memory Benchmark (5GB Dataset Scale)

| Resource Metric | wedb_embed (Embedded LSM+LZ4) | Redis (v8.10.1 AOF Mode) | Resource Savings |
| :--- | :--- | :--- | :--- |
| **Dataset Scale** | 5,000,000 Structured Items | 5,000,000 Structured Items | All 14 Data Formats |
| **Raw Uncompressed Payload** | 4377 MB | 4377 MB | Structured Payload |
| **Physical Disk Footprint** | **1053 MB** | **7367 MB** | **Saves 86%** |
| **Resident Memory (RSS)** | **281 MB** | **4833 MB** | **Saves 94%** |

#### wedb_embed vs Redis Core Command Benchmark

| Command | wedb_embed P95 Latency | Redis P95 Latency | Speedup |
| :--- | :--- | :--- | :--- |
| `SET` | 7.4 us | 28.3 us | **3.8x** |
| `GET` | 5.3 us | 19.5 us | **3.7x** |
| `MSET` | 50.5 us | 33.4 us | **0.7x** |
| `MGET` | 5.3 us | 25.6 us | **4.8x** |
| `INCRBY` | 1.2 us | 27.3 us | **22.1x** |
| `DECRBY` | 1.0 us | 27.4 us | **26.7x** |
| `APPEND` | 1.3 us | 27.4 us | **20.9x** |
| `STRLEN` | 0.28 us | 18.4 us | **66.9x** |
| `GETDEL` | 8.5 us | 56.6 us | **6.7x** |
| `GETRANGE` | 0.36 us | 19.4 us | **53.6x** |
| `SETRANGE` | 1.4 us | 28.1 us | **19.9x** |
| `HSET` | 2.0 us | 38.3 us | **19.3x** |
| `HGET` | 0.70 us | 28.9 us | **41.3x** |
| `HMGET` | 3.3 us | 34.6 us | **10.6x** |
| `HEXISTS` | 0.64 us | 24.4 us | **38.4x** |
| `HLEN` | 0.44 us | 24.1 us | **54.6x** |
| `HDEL` | 4.2 us | 37.5 us | **9.0x** |
| `HGETALL` | 3.7 us | 33.4 us | **9.1x** |
| `HKEYS` | 3.2 us | 29.9 us | **9.3x** |
| `HVALS` | 3.3 us | 33.2 us | **10.0x** |
| `HINCRBY` | 1.7 us | 38.6 us | **22.3x** |
| `LPUSH` | 2.8 us | 27.5 us | **9.7x** |
| `RPUSH` | 2.9 us | 27.1 us | **9.5x** |
| `LPOP` | 2.5 us | 24.5 us | **9.9x** |
| `RPOP` | 2.6 us | 27.4 us | **10.7x** |
| `LLEN` | 0.46 us | 19.5 us | **42.2x** |
| `LRANGE` | 3.9 us | 19.4 us | **5.0x** |
| `LINDEX` | 0.68 us | 19.3 us | **28.5x** |
| `LSET` | 1.2 us | 28.1 us | **23.9x** |
| `LREM` | 11.3 us | 56.4 us | **5.0x** |
| `LTRIM` | 1.1 us | 19.4 us | **17.1x** |
| `SADD` | 1.5 us | 22.2 us | **15.1x** |
| `SREM` | 3.8 us | 22.1 us | **5.8x** |
| `SISMEMBER` | 0.74 us | 19.5 us | **26.3x** |
| `SCARD` | 0.46 us | 19.3 us | **41.9x** |
| `SMEMBERS` | 3.4 us | 19.1 us | **5.6x** |
| `SPOP` | 9.2 us | 55.1 us | **6.0x** |
| `SRANDMEMBER` | 3.6 us | 19.0 us | **5.3x** |
| `ZADD` | 3.1 us | 37.4 us | **12.1x** |
| `ZSCORE` | 0.89 us | 28.9 us | **32.5x** |
| `ZRANGE` | 3.8 us | 29.3 us | **7.7x** |
| `ZCARD` | 0.49 us | 23.4 us | **47.5x** |
| `ZCOUNT` | 3.3 us | 29.0 us | **8.9x** |
| `ZINCRBY` | 3.3 us | 38.0 us | **11.6x** |
| `ZRANK` | 3.5 us | 28.4 us | **8.1x** |
| `ZREVRANGE` | 6.2 us | 29.6 us | **4.8x** |
| `ZPOPMIN` | 14.5 us | 74.4 us | **5.1x** |
| `ZREM` | 4.5 us | 36.1 us | **8.0x** |
| `SETBIT` | 11.4 us | 32.2 us | **2.8x** |
| `GETBIT` | 0.64 us | 20.9 us | **32.6x** |
| `BITCOUNT` | 0.40 us | 27.6 us | **68.8x** |
| `BITPOS` | 0.60 us | 20.4 us | **34.3x** |
| `PFADD` | 2.7 us | 29.9 us | **11.2x** |
| `PFCOUNT` | 8.2 us | 24.5 us | **3.0x** |
| `GEOADD` | 2.6 us | 40.2 us | **15.5x** |
| `GEODIST` | 0.96 us | 29.3 us | **30.4x** |
| `GEOPOS` | 0.71 us | 29.8 us | **42.2x** |
| `GEOHASH` | 0.75 us | 25.5 us | **34.0x** |
| `XADD` | 1.6 us | 29.1 us | **18.1x** |
| `XLEN` | 0.57 us | 18.9 us | **33.0x** |
| `XRANGE` | 4.2 us | 44.0 us | **10.4x** |
| `XREAD` | 4.2 us | 41.7 us | **9.9x** |
| `XDEL` | 3.5 us | 59.6 us | **16.8x** |
| `DEL` | 3.0 us | 24.0 us | **8.1x** |
| `EXISTS` | 0.24 us | 23.9 us | **97.9x** |
| `EXPIRE` | 0.74 us | 39.0 us | **52.8x** |
| `TTL` | 0.29 us | 24.4 us | **83.9x** |
| `JSON.SET` | 4.1 us | 25.3 us | **6.1x** |
| `JSON.GET` | 1.6 us | 29.1 us | **18.8x** |
| `JSON.DEL` | 9.1 us | 59.1 us | **6.5x** |
| `JSON.NUMINCRBY` | 4.0 us | 33.0 us | **8.3x** |
| `JSON.ARRLEN` | 1.3 us | 29.1 us | **21.9x** |
| `JSON.TYPE` | 1.5 us | 18.5 us | **12.1x** |
| `BF.ADD` | 33.1 us | 33.4 us | **1.0x** |
| `BF.EXISTS` | 0.65 us | 34.7 us | **53.3x** |
| `BF.INFO` | 0.36 us | 32.4 us | **89.0x** |
| `CF.ADD` | 2.7 us | 34.8 us | **12.8x** |
| `CF.EXISTS` | 0.79 us | 30.2 us | **38.4x** |
| `CF.DEL` | 9.8 us | 64.0 us | **6.5x** |
| `TDIGEST.ADD` | 2.6 us | 30.3 us | **11.6x** |
| `TDIGEST.QUANTILE` | 1.0 us | 28.7 us | **27.7x** |
| `TDIGEST.BYRANK` | 1.2 us | 28.9 us | **24.1x** |
| `TDIGEST.CDF` | 1.3 us | 31.7 us | **24.8x** |
| `TS.ADD` | 11.5 us | 29.0 us | **2.5x** |
| `TS.GET` | 1.5 us | 28.5 us | **18.5x** |
| `TS.RANGE` | 21.1 us | 28.3 us | **1.3x** |
| `TS.INCRBY` | 10.8 us | 28.6 us | **2.7x** |
| `FT.SEARCH` | 30.2 us | 18.5 us | **0.6x** |
| `FT.TAG` | 29.7 us | 18.3 us | **0.6x** |
| `VECTOR.KNN` | 4.2 us | 33.6 us | **7.9x** |

