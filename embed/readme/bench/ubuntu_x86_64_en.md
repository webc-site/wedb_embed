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
| **Physical Disk Footprint** | **1053 MB** | **7480 MB** | **Saves 86%** |
| **Resident Memory (RSS)** | **293 MB** | **4830 MB** | **Saves 94%** |

#### wedb_embed vs Redis Core Command Benchmark

| Command | wedb_embed P95 Latency | Redis P95 Latency | Speedup |
| :--- | :--- | :--- | :--- |
| `SET` | 7.4 us | 29.2 us | **3.9x** |
| `GET` | 5.2 us | 19.1 us | **3.7x** |
| `MSET` | 50.1 us | 34.0 us | **0.7x** |
| `MGET` | 5.4 us | 26.2 us | **4.9x** |
| `INCRBY` | 1.2 us | 27.4 us | **23.1x** |
| `DECRBY` | 0.77 us | 28.2 us | **36.8x** |
| `APPEND` | 1.3 us | 27.4 us | **21.6x** |
| `STRLEN` | 0.28 us | 19.1 us | **68.3x** |
| `GETDEL` | 8.0 us | 58.5 us | **7.3x** |
| `GETRANGE` | 0.35 us | 19.1 us | **54.4x** |
| `SETRANGE` | 0.99 us | 28.0 us | **28.4x** |
| `HSET` | 2.0 us | 25.5 us | **12.8x** |
| `HGET` | 0.70 us | 35.8 us | **50.7x** |
| `HMGET` | 3.3 us | 42.4 us | **12.7x** |
| `HEXISTS` | 0.63 us | 34.9 us | **55.7x** |
| `HLEN` | 0.44 us | 36.4 us | **82.1x** |
| `HDEL` | 4.2 us | 42.7 us | **10.2x** |
| `HGETALL` | 3.5 us | 40.9 us | **11.5x** |
| `HKEYS` | 3.2 us | 37.1 us | **11.7x** |
| `HVALS` | 3.6 us | 19.0 us | **5.3x** |
| `HINCRBY` | 1.8 us | 44.3 us | **24.8x** |
| `LPUSH` | 2.7 us | 28.9 us | **10.5x** |
| `RPUSH` | 2.7 us | 27.7 us | **10.3x** |
| `LPOP` | 2.5 us | 27.8 us | **11.2x** |
| `RPOP` | 2.4 us | 27.9 us | **11.4x** |
| `LLEN` | 0.47 us | 18.7 us | **40.0x** |
| `LRANGE` | 3.4 us | 19.0 us | **5.5x** |
| `LINDEX` | 0.71 us | 18.0 us | **25.4x** |
| `LSET` | 1.2 us | 27.6 us | **22.7x** |
| `LREM` | 16.9 us | 60.8 us | **3.6x** |
| `LTRIM` | 1.1 us | 19.3 us | **17.2x** |
| `SADD` | 1.4 us | 24.3 us | **17.8x** |
| `SREM` | 3.7 us | 22.7 us | **6.2x** |
| `SISMEMBER` | 0.72 us | 19.1 us | **26.6x** |
| `SCARD` | 0.47 us | 19.2 us | **40.5x** |
| `SMEMBERS` | 4.1 us | 18.2 us | **4.4x** |
| `SPOP` | 8.7 us | 60.5 us | **7.0x** |
| `SRANDMEMBER` | 3.1 us | 19.3 us | **6.3x** |
| `ZADD` | 3.1 us | 29.1 us | **9.5x** |
| `ZSCORE` | 0.86 us | 18.9 us | **22.0x** |
| `ZRANGE` | 3.8 us | 19.2 us | **5.1x** |
| `ZCARD` | 0.50 us | 19.3 us | **38.6x** |
| `ZCOUNT` | 3.3 us | 18.9 us | **5.8x** |
| `ZINCRBY` | 3.1 us | 30.9 us | **10.0x** |
| `ZRANK` | 3.3 us | 18.3 us | **5.5x** |
| `ZREVRANGE` | 5.8 us | 19.3 us | **3.3x** |
| `ZPOPMIN` | 14.5 us | 60.8 us | **4.2x** |
| `ZREM` | 4.4 us | 28.0 us | **6.3x** |
| `SETBIT` | 11.4 us | 43.9 us | **3.9x** |
| `GETBIT` | 0.46 us | 37.2 us | **80.8x** |
| `BITCOUNT` | 0.37 us | 31.0 us | **83.1x** |
| `BITPOS` | 0.46 us | 20.5 us | **44.8x** |
| `PFADD` | 2.8 us | 25.8 us | **9.1x** |
| `PFCOUNT` | 8.3 us | 18.9 us | **2.3x** |
| `GEOADD` | 2.6 us | 44.9 us | **17.4x** |
| `GEODIST` | 0.95 us | 38.5 us | **40.7x** |
| `GEOPOS` | 0.70 us | 36.2 us | **51.6x** |
| `GEOHASH` | 0.77 us | 36.5 us | **47.6x** |
| `XADD` | 1.7 us | 30.0 us | **17.7x** |
| `XLEN` | 0.57 us | 18.8 us | **33.0x** |
| `XRANGE` | 4.3 us | 30.5 us | **7.2x** |
| `XREAD` | 4.1 us | 32.4 us | **7.8x** |
| `XDEL` | 3.7 us | 61.0 us | **16.5x** |
| `DEL` | 3.0 us | 36.0 us | **12.0x** |
| `EXISTS` | 0.25 us | 33.9 us | **135.2x** |
| `EXPIRE` | 0.74 us | 44.8 us | **60.2x** |
| `TTL` | 0.29 us | 34.4 us | **118.6x** |
| `JSON.SET` | 3.5 us | 19.1 us | **5.5x** |
| `JSON.GET` | 1.5 us | 18.9 us | **12.7x** |
| `JSON.DEL` | 9.2 us | 36.9 us | **4.0x** |
| `JSON.NUMINCRBY` | 3.8 us | 18.6 us | **4.9x** |
| `JSON.ARRLEN` | 1.3 us | 18.1 us | **13.6x** |
| `JSON.TYPE` | 1.4 us | 18.5 us | **13.3x** |
| `BF.ADD` | 32.7 us | 30.6 us | **0.9x** |
| `BF.EXISTS` | 0.61 us | 38.1 us | **62.0x** |
| `BF.INFO` | 0.37 us | 36.2 us | **96.8x** |
| `CF.ADD` | 2.5 us | 38.0 us | **15.1x** |
| `CF.EXISTS` | 0.64 us | 37.0 us | **57.4x** |
| `CF.DEL` | 10.3 us | 72.1 us | **7.0x** |
| `TDIGEST.ADD` | 2.7 us | 18.7 us | **6.9x** |
| `TDIGEST.QUANTILE` | 1.1 us | 18.2 us | **16.8x** |
| `TDIGEST.BYRANK` | 1.3 us | 18.1 us | **14.0x** |
| `TDIGEST.CDF` | 1.3 us | 18.0 us | **13.7x** |
| `TS.ADD` | 6.6 us | 18.5 us | **2.8x** |
| `TS.GET` | 1.5 us | 17.8 us | **12.1x** |
| `TS.RANGE` | 20.7 us | 18.4 us | **0.9x** |
| `TS.INCRBY` | 10.0 us | 18.3 us | **1.8x** |
| `FT.SEARCH` | 27.8 us | 19.3 us | **0.7x** |
| `FT.TAG` | 27.8 us | 18.8 us | **0.7x** |
| `VECTOR.KNN` | 4.4 us | 25.7 us | **5.9x** |

