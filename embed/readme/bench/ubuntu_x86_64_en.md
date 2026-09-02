### Ubuntu CI (GitHub Actions Runner)

#### Hardware & Test Environment

CPU: AMD EPYC 9V74 80-Core Processor (4 cores)<br>
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
| **Physical Disk Footprint** | **1053 MB** | **7432 MB** | **Saves 86%** |
| **Resident Memory (RSS)** | **288 MB** | **4819 MB** | **Saves 94%** |

#### wedb_embed vs Redis Core Command Benchmark

| Command | wedb_embed P95 Latency | Redis P95 Latency | Speedup |
| :--- | :--- | :--- | :--- |
| `SET` | 7.7 us | 27.9 us | **3.6x** |
| `GET` | 5.2 us | 19.9 us | **3.8x** |
| `MSET` | 53.8 us | 31.7 us | **0.6x** |
| `MGET` | 5.7 us | 19.6 us | **3.4x** |
| `INCRBY` | 0.97 us | 28.5 us | **29.5x** |
| `DECRBY` | 0.68 us | 28.3 us | **41.5x** |
| `APPEND` | 0.91 us | 27.8 us | **30.7x** |
| `STRLEN` | 0.33 us | 19.6 us | **59.3x** |
| `GETDEL` | 8.9 us | 57.1 us | **6.4x** |
| `GETRANGE` | 0.40 us | 19.9 us | **49.8x** |
| `SETRANGE` | 1.0 us | 27.8 us | **26.9x** |
| `HSET` | 2.1 us | 28.8 us | **13.7x** |
| `HGET` | 0.74 us | 30.2 us | **40.7x** |
| `HMGET` | 3.4 us | 34.4 us | **10.1x** |
| `HEXISTS` | 0.68 us | 29.9 us | **44.3x** |
| `HLEN` | 0.46 us | 26.5 us | **57.5x** |
| `HDEL` | 4.5 us | 35.6 us | **7.9x** |
| `HGETALL` | 3.7 us | 33.2 us | **8.9x** |
| `HKEYS` | 3.6 us | 33.0 us | **9.0x** |
| `HVALS` | 3.8 us | 20.7 us | **5.5x** |
| `HINCRBY` | 1.6 us | 37.9 us | **23.0x** |
| `LPUSH` | 2.2 us | 27.7 us | **12.8x** |
| `RPUSH` | 2.3 us | 28.4 us | **12.5x** |
| `LPOP` | 2.6 us | 21.7 us | **8.3x** |
| `RPOP` | 2.6 us | 27.7 us | **10.5x** |
| `LLEN` | 0.47 us | 20.3 us | **43.2x** |
| `LRANGE` | 4.4 us | 19.5 us | **4.4x** |
| `LINDEX` | 0.69 us | 20.4 us | **29.6x** |
| `LSET` | 1.2 us | 28.1 us | **22.8x** |
| `LREM` | 11.8 us | 55.6 us | **4.7x** |
| `LTRIM` | 1.1 us | 19.9 us | **17.6x** |
| `SADD` | 1.5 us | 23.9 us | **16.4x** |
| `SREM` | 5.3 us | 23.0 us | **4.4x** |
| `SISMEMBER` | 0.74 us | 19.8 us | **26.7x** |
| `SCARD` | 0.47 us | 20.1 us | **42.5x** |
| `SMEMBERS` | 4.2 us | 19.7 us | **4.7x** |
| `SPOP` | 8.9 us | 52.2 us | **5.8x** |
| `SRANDMEMBER` | 4.3 us | 19.9 us | **4.6x** |
| `ZADD` | 3.0 us | 22.0 us | **7.2x** |
| `ZSCORE` | 0.89 us | 18.8 us | **21.1x** |
| `ZRANGE` | 4.3 us | 18.7 us | **4.3x** |
| `ZCARD` | 0.49 us | 19.3 us | **39.2x** |
| `ZCOUNT` | 3.5 us | 19.3 us | **5.4x** |
| `ZINCRBY` | 3.1 us | 26.5 us | **8.6x** |
| `ZRANK` | 3.7 us | 19.0 us | **5.2x** |
| `ZREVRANGE` | 6.0 us | 18.9 us | **3.1x** |
| `ZPOPMIN` | 8.6 us | 51.3 us | **6.0x** |
| `ZREM` | 5.6 us | 21.7 us | **3.9x** |
| `SETBIT` | 12.5 us | 37.0 us | **3.0x** |
| `GETBIT` | 0.49 us | 30.4 us | **61.6x** |
| `BITCOUNT` | 0.40 us | 27.3 us | **68.5x** |
| `BITPOS` | 0.46 us | 30.1 us | **65.4x** |
| `PFADD` | 4.7 us | 24.2 us | **5.2x** |
| `PFCOUNT` | 8.4 us | 20.3 us | **2.4x** |
| `GEOADD` | 2.6 us | 39.0 us | **14.9x** |
| `GEODIST` | 1.0 us | 29.9 us | **29.9x** |
| `GEOPOS` | 0.72 us | 29.8 us | **41.4x** |
| `GEOHASH` | 0.77 us | 31.1 us | **40.2x** |
| `XADD` | 1.6 us | 26.6 us | **17.1x** |
| `XLEN` | 0.58 us | 19.6 us | **34.0x** |
| `XRANGE` | 4.5 us | 28.1 us | **6.2x** |
| `XREAD` | 4.6 us | 29.8 us | **6.5x** |
| `XDEL` | 3.7 us | 54.6 us | **14.8x** |
| `DEL` | 3.1 us | 29.1 us | **9.3x** |
| `EXISTS` | 0.27 us | 28.4 us | **106.8x** |
| `EXPIRE` | 0.79 us | 37.3 us | **47.4x** |
| `TTL` | 0.32 us | 25.2 us | **79.9x** |
| `JSON.SET` | 3.7 us | 19.6 us | **5.3x** |
| `JSON.GET` | 1.9 us | 20.7 us | **11.0x** |
| `JSON.DEL` | 9.2 us | 39.9 us | **4.3x** |
| `JSON.NUMINCRBY` | 3.9 us | 19.3 us | **5.0x** |
| `JSON.ARRLEN` | 1.3 us | 20.3 us | **15.2x** |
| `JSON.TYPE` | 1.4 us | 19.4 us | **13.5x** |
| `BF.ADD` | 34.4 us | 29.9 us | **0.9x** |
| `BF.EXISTS` | 0.70 us | 33.0 us | **46.9x** |
| `BF.INFO` | 0.40 us | 32.8 us | **82.0x** |
| `CF.ADD` | 2.7 us | 30.2 us | **11.3x** |
| `CF.EXISTS` | 0.85 us | 32.8 us | **38.7x** |
| `CF.DEL` | 10.3 us | 63.1 us | **6.2x** |
| `TDIGEST.ADD` | 2.7 us | 18.9 us | **7.1x** |
| `TDIGEST.QUANTILE` | 1.3 us | 18.8 us | **14.7x** |
| `TDIGEST.BYRANK` | 1.3 us | 18.6 us | **14.3x** |
| `TDIGEST.CDF` | 1.4 us | 18.9 us | **13.9x** |
| `TS.ADD` | 12.2 us | 18.6 us | **1.5x** |
| `TS.GET` | 1.5 us | 18.7 us | **12.2x** |
| `TS.RANGE` | 23.2 us | 18.5 us | **0.8x** |
| `TS.INCRBY` | 9.8 us | 18.6 us | **1.9x** |
| `FT.SEARCH` | 28.9 us | 19.4 us | **0.7x** |
| `FT.TAG` | 29.1 us | 19.4 us | **0.7x** |
| `VECTOR.KNN` | 4.9 us | 18.6 us | **3.8x** |

