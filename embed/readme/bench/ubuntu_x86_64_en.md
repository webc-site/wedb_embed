### Ubuntu CI (GitHub Actions Runner)

#### Hardware & Test Environment

CPU: AMD EPYC 9V74 80-Core Processor (4 cores)<br>
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
| **Physical Disk Footprint** | **1053 MB** | **7428 MB** | **Saves 86%** |
| **Resident Memory (RSS)** | **277 MB** | **4826 MB** | **Saves 94%** |

#### wedb_embed vs Redis Core Command Benchmark

| Command | wedb_embed P95 Latency | Redis P95 Latency | Speedup |
| :--- | :--- | :--- | :--- |
| `SET` | 7.8 us | 28.5 us | **3.7x** |
| `GET` | 5.2 us | 19.8 us | **3.8x** |
| `MSET` | 51.9 us | 31.6 us | **0.6x** |
| `MGET` | 5.5 us | 19.5 us | **3.5x** |
| `INCRBY` | 0.98 us | 27.9 us | **28.5x** |
| `DECRBY` | 0.70 us | 23.9 us | **34.2x** |
| `APPEND` | 0.96 us | 21.1 us | **22.1x** |
| `STRLEN` | 0.36 us | 19.4 us | **54.0x** |
| `GETDEL` | 8.5 us | 53.1 us | **6.2x** |
| `GETRANGE` | 0.30 us | 20.3 us | **67.4x** |
| `SETRANGE` | 1.1 us | 26.9 us | **24.0x** |
| `HSET` | 2.4 us | 36.2 us | **14.9x** |
| `HGET` | 0.67 us | 35.8 us | **53.7x** |
| `HMGET` | 3.2 us | 34.7 us | **10.7x** |
| `HEXISTS` | 0.64 us | 35.1 us | **54.6x** |
| `HLEN` | 0.42 us | 26.9 us | **63.9x** |
| `HDEL` | 4.2 us | 35.6 us | **8.5x** |
| `HGETALL` | 3.7 us | 33.4 us | **9.1x** |
| `HKEYS` | 3.6 us | 34.8 us | **9.7x** |
| `HVALS` | 3.5 us | 31.9 us | **9.0x** |
| `HINCRBY` | 1.9 us | 40.0 us | **21.5x** |
| `LPUSH` | 2.3 us | 22.9 us | **10.1x** |
| `RPUSH` | 2.1 us | 27.9 us | **13.1x** |
| `LPOP` | 2.6 us | 20.4 us | **8.0x** |
| `RPOP` | 2.7 us | 20.6 us | **7.5x** |
| `LLEN` | 0.45 us | 20.0 us | **44.4x** |
| `LRANGE` | 3.7 us | 20.7 us | **5.6x** |
| `LINDEX` | 0.67 us | 20.5 us | **30.5x** |
| `LSET` | 1.1 us | 27.4 us | **26.1x** |
| `LREM` | 17.4 us | 55.4 us | **3.2x** |
| `LTRIM` | 1.1 us | 20.1 us | **18.0x** |
| `SADD` | 1.3 us | 22.9 us | **17.2x** |
| `SREM` | 4.6 us | 22.2 us | **4.8x** |
| `SISMEMBER` | 0.64 us | 20.2 us | **31.5x** |
| `SCARD` | 0.45 us | 20.3 us | **45.3x** |
| `SMEMBERS` | 3.5 us | 19.9 us | **5.7x** |
| `SPOP` | 5.8 us | 53.1 us | **9.2x** |
| `SRANDMEMBER` | 2.7 us | 19.9 us | **7.5x** |
| `ZADD` | 2.9 us | 24.7 us | **8.4x** |
| `ZSCORE` | 0.95 us | 20.4 us | **21.5x** |
| `ZRANGE` | 4.2 us | 21.2 us | **5.0x** |
| `ZCARD` | 0.53 us | 20.1 us | **37.6x** |
| `ZCOUNT` | 3.8 us | 20.8 us | **5.5x** |
| `ZINCRBY` | 3.2 us | 29.2 us | **9.2x** |
| `ZRANK` | 4.0 us | 20.6 us | **5.2x** |
| `ZREVRANGE` | 6.6 us | 20.9 us | **3.2x** |
| `ZPOPMIN` | 8.6 us | 54.3 us | **6.3x** |
| `ZREM` | 5.1 us | 20.4 us | **4.0x** |
| `SETBIT` | 16.4 us | 23.4 us | **1.4x** |
| `GETBIT` | 0.48 us | 20.4 us | **42.5x** |
| `BITCOUNT` | 0.41 us | 30.4 us | **73.8x** |
| `BITPOS` | 0.72 us | 22.9 us | **31.7x** |
| `PFADD` | 2.7 us | 34.6 us | **12.7x** |
| `PFCOUNT` | 8.3 us | 31.8 us | **3.8x** |
| `GEOADD` | 2.5 us | 39.5 us | **15.7x** |
| `GEODIST` | 1.1 us | 30.8 us | **29.2x** |
| `GEOPOS` | 0.83 us | 31.1 us | **37.5x** |
| `GEOHASH` | 0.79 us | 30.9 us | **39.0x** |
| `XADD` | 1.6 us | 27.0 us | **17.1x** |
| `XLEN` | 0.62 us | 19.9 us | **32.2x** |
| `XRANGE` | 4.5 us | 28.4 us | **6.3x** |
| `XREAD` | 4.4 us | 31.7 us | **7.2x** |
| `XDEL` | 4.0 us | 55.5 us | **14.0x** |
| `DEL` | 3.2 us | 25.2 us | **7.8x** |
| `EXISTS` | 0.27 us | 26.3 us | **95.9x** |
| `EXPIRE` | 0.90 us | 39.2 us | **43.5x** |
| `TTL` | 0.30 us | 32.2 us | **107.3x** |
| `JSON.SET` | 3.7 us | 25.5 us | **6.9x** |
| `JSON.GET` | 1.6 us | 31.2 us | **19.4x** |
| `JSON.DEL` | 8.4 us | 63.7 us | **7.6x** |
| `JSON.NUMINCRBY` | 4.1 us | 31.8 us | **7.8x** |
| `JSON.ARRLEN` | 1.5 us | 32.0 us | **22.0x** |
| `JSON.TYPE` | 1.5 us | 20.6 us | **13.8x** |
| `BF.ADD` | 59.8 us | 22.7 us | **0.4x** |
| `BF.EXISTS` | 1.1 us | 25.2 us | **22.8x** |
| `BF.INFO` | 0.38 us | 21.4 us | **56.9x** |
| `CF.ADD` | 2.6 us | 33.9 us | **13.3x** |
| `CF.EXISTS` | 0.71 us | 31.4 us | **44.1x** |
| `CF.DEL` | 10.5 us | 64.7 us | **6.2x** |
| `TDIGEST.ADD` | 2.6 us | 20.8 us | **7.9x** |
| `TDIGEST.QUANTILE` | 1.2 us | 20.7 us | **16.8x** |
| `TDIGEST.BYRANK` | 1.2 us | 20.7 us | **16.7x** |
| `TDIGEST.CDF` | 1.3 us | 20.9 us | **16.0x** |
| `TS.ADD` | 7.3 us | 20.7 us | **2.9x** |
| `TS.GET` | 1.5 us | 20.5 us | **13.7x** |
| `TS.RANGE` | 23.3 us | 19.0 us | **0.8x** |
| `TS.INCRBY` | 9.4 us | 21.0 us | **2.2x** |
| `FT.SEARCH` | 29.3 us | 19.8 us | **0.7x** |
| `FT.TAG` | 30.4 us | 20.1 us | **0.7x** |
| `VECTOR.KNN` | 4.3 us | 19.3 us | **4.5x** |

