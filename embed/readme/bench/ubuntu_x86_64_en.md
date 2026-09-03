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
| **Physical Disk Footprint** | **1078 MB** | **7950 MB** | **Saves 86%** |
| **Resident Memory (RSS)** | **282 MB** | **4843 MB** | **Saves 94%** |

#### wedb_embed vs Redis Core Command Benchmark

| Command | wedb_embed P95 Latency | Redis P95 Latency | Speedup |
| :--- | :--- | :--- | :--- |
| `SET` | 6.0 us | 20.9 us | **3.5x** |
| `GET` | 3.9 us | 17.5 us | **4.5x** |
| `MSET` | 40.1 us | 25.8 us | **0.6x** |
| `MGET` | 4.3 us | 15.7 us | **3.7x** |
| `INCRBY` | 0.58 us | 17.1 us | **29.3x** |
| `DECRBY` | 0.53 us | 16.7 us | **31.3x** |
| `APPEND` | 0.67 us | 17.7 us | **26.4x** |
| `STRLEN` | 0.25 us | 17.0 us | **68.8x** |
| `GETDEL` | 6.6 us | 41.2 us | **6.2x** |
| `GETRANGE` | 0.23 us | 17.4 us | **77.1x** |
| `SETRANGE` | 0.67 us | 21.5 us | **32.0x** |
| `HSET` | 1.6 us | 16.7 us | **10.6x** |
| `HGET` | 0.54 us | 17.5 us | **32.4x** |
| `HMGET` | 2.4 us | 16.8 us | **7.0x** |
| `HEXISTS` | 0.49 us | 17.4 us | **35.4x** |
| `HLEN` | 0.34 us | 17.6 us | **51.0x** |
| `HDEL` | 3.4 us | 16.9 us | **4.9x** |
| `HGETALL` | 2.9 us | 16.9 us | **5.9x** |
| `HKEYS` | 2.7 us | 16.9 us | **6.3x** |
| `HVALS` | 2.8 us | 16.9 us | **6.0x** |
| `HINCRBY` | 1.2 us | 21.6 us | **17.3x** |
| `LPUSH` | 2.1 us | 19.6 us | **9.3x** |
| `RPUSH` | 1.7 us | 23.3 us | **13.9x** |
| `LPOP` | 1.8 us | 23.3 us | **12.9x** |
| `RPOP` | 1.8 us | 15.1 us | **8.2x** |
| `LLEN` | 0.36 us | 17.3 us | **48.2x** |
| `LRANGE` | 3.0 us | 16.2 us | **5.4x** |
| `LINDEX` | 0.54 us | 17.4 us | **32.1x** |
| `LSET` | 0.85 us | 18.3 us | **21.4x** |
| `LREM` | 14.4 us | 42.6 us | **3.0x** |
| `LTRIM` | 1.1 us | 13.9 us | **12.8x** |
| `SADD` | 1.1 us | 20.4 us | **18.1x** |
| `SREM` | 3.9 us | 19.4 us | **5.0x** |
| `SISMEMBER` | 0.56 us | 17.6 us | **31.3x** |
| `SCARD` | 0.36 us | 17.6 us | **49.5x** |
| `SMEMBERS` | 2.8 us | 17.6 us | **6.3x** |
| `SPOP` | 5.7 us | 44.1 us | **7.8x** |
| `SRANDMEMBER` | 2.6 us | 17.0 us | **6.6x** |
| `ZADD` | 2.2 us | 18.9 us | **8.8x** |
| `ZSCORE` | 0.63 us | 16.3 us | **25.8x** |
| `ZRANGE` | 3.3 us | 15.6 us | **4.7x** |
| `ZCARD` | 0.40 us | 16.5 us | **41.4x** |
| `ZCOUNT` | 2.8 us | 15.5 us | **5.5x** |
| `ZINCRBY` | 2.2 us | 22.3 us | **10.2x** |
| `ZRANK` | 3.0 us | 16.4 us | **5.4x** |
| `ZREVRANGE` | 4.7 us | 15.8 us | **3.4x** |
| `ZPOPMIN` | 6.2 us | 42.6 us | **6.9x** |
| `ZREM` | 3.4 us | 16.2 us | **4.8x** |
| `SETBIT` | 13.0 us | 25.5 us | **2.0x** |
| `GETBIT` | 0.34 us | 18.9 us | **55.3x** |
| `BITCOUNT` | 0.30 us | 17.2 us | **58.3x** |
| `BITPOS` | 0.34 us | 21.0 us | **62.5x** |
| `PFADD` | 2.2 us | 18.2 us | **8.1x** |
| `PFCOUNT` | 6.5 us | 17.1 us | **2.6x** |
| `GEOADD` | 1.9 us | 19.2 us | **10.2x** |
| `GEODIST` | 0.75 us | 17.9 us | **23.9x** |
| `GEOPOS` | 0.54 us | 17.2 us | **31.8x** |
| `GEOHASH` | 0.59 us | 16.8 us | **28.4x** |
| `XADD` | 1.3 us | 23.3 us | **18.6x** |
| `XLEN` | 0.46 us | 17.8 us | **38.8x** |
| `XRANGE` | 3.3 us | 23.4 us | **7.0x** |
| `XREAD` | 3.3 us | 24.0 us | **7.4x** |
| `XDEL` | 2.8 us | 46.4 us | **16.4x** |
| `DEL` | 2.5 us | 17.8 us | **7.0x** |
| `EXISTS` | 0.21 us | 17.1 us | **83.3x** |
| `EXPIRE` | 0.60 us | 18.8 us | **31.3x** |
| `TTL` | 0.25 us | 17.1 us | **69.7x** |
| `JSON.SET` | 2.6 us | 18.6 us | **7.1x** |
| `JSON.GET` | 1.2 us | 17.5 us | **15.1x** |
| `JSON.DEL` | 6.3 us | 33.1 us | **5.3x** |
| `JSON.NUMINCRBY` | 3.0 us | 17.7 us | **5.9x** |
| `JSON.ARRLEN` | 1.0 us | 16.9 us | **16.4x** |
| `JSON.TYPE` | 1.0 us | 16.8 us | **16.0x** |
| `BF.ADD` | 26.8 us | 16.8 us | **0.6x** |
| `BF.EXISTS` | 0.47 us | 17.8 us | **38.0x** |
| `BF.INFO` | 0.29 us | 17.7 us | **61.4x** |
| `CF.ADD` | 2.0 us | 17.5 us | **8.7x** |
| `CF.EXISTS` | 0.49 us | 17.2 us | **34.8x** |
| `CF.DEL` | 8.5 us | 33.7 us | **3.9x** |
| `TDIGEST.ADD` | 2.1 us | 15.3 us | **7.3x** |
| `TDIGEST.QUANTILE` | 0.81 us | 17.2 us | **21.2x** |
| `TDIGEST.BYRANK` | 0.91 us | 17.3 us | **19.1x** |
| `TDIGEST.CDF` | 1.0 us | 17.4 us | **16.8x** |
| `TS.ADD` | 5.3 us | 15.4 us | **2.9x** |
| `TS.GET` | 1.1 us | 17.0 us | **15.1x** |
| `TS.RANGE` | 18.0 us | 17.7 us | **1.0x** |
| `TS.INCRBY` | 7.3 us | 16.7 us | **2.3x** |
| `FT.SEARCH` | 19.8 us | 17.5 us | **0.9x** |
| `FT.TAG` | 19.4 us | 17.9 us | **0.9x** |
| `VECTOR.KNN` | 4.0 us | 15.8 us | **3.9x** |

