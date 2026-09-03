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
| **Physical Disk Footprint** | **1053 MB** | **7676 MB** | **Saves 86%** |
| **Resident Memory (RSS)** | **281 MB** | **4817 MB** | **Saves 94%** |

#### wedb_embed vs Redis Core Command Benchmark

| Command | wedb_embed P95 Latency | Redis P95 Latency | Speedup |
| :--- | :--- | :--- | :--- |
| `SET` | 7.4 us | 28.7 us | **3.8x** |
| `GET` | 5.1 us | 20.4 us | **4.0x** |
| `MSET` | 50.0 us | 32.9 us | **0.7x** |
| `MGET` | 5.2 us | 23.6 us | **4.6x** |
| `INCRBY` | 1.2 us | 27.1 us | **23.5x** |
| `DECRBY` | 1.0 us | 27.6 us | **27.1x** |
| `APPEND` | 1.2 us | 27.1 us | **23.5x** |
| `STRLEN` | 0.28 us | 19.8 us | **71.8x** |
| `GETDEL` | 8.1 us | 57.9 us | **7.2x** |
| `GETRANGE` | 0.37 us | 19.5 us | **52.1x** |
| `SETRANGE` | 1.4 us | 27.6 us | **20.0x** |
| `HSET` | 2.0 us | 39.2 us | **19.7x** |
| `HGET` | 0.70 us | 34.0 us | **48.4x** |
| `HMGET` | 3.2 us | 38.2 us | **11.8x** |
| `HEXISTS` | 0.63 us | 25.1 us | **40.1x** |
| `HLEN` | 0.43 us | 24.3 us | **55.9x** |
| `HDEL` | 4.2 us | 33.8 us | **8.1x** |
| `HGETALL` | 3.5 us | 35.0 us | **10.1x** |
| `HKEYS` | 3.5 us | 33.6 us | **9.6x** |
| `HVALS` | 3.3 us | 34.3 us | **10.4x** |
| `HINCRBY` | 1.6 us | 39.6 us | **24.7x** |
| `LPUSH` | 2.6 us | 27.6 us | **10.6x** |
| `RPUSH` | 2.5 us | 27.4 us | **10.8x** |
| `LPOP` | 2.5 us | 27.3 us | **10.9x** |
| `RPOP` | 2.4 us | 27.8 us | **11.4x** |
| `LLEN` | 0.46 us | 20.1 us | **43.7x** |
| `LRANGE` | 3.5 us | 20.2 us | **5.7x** |
| `LINDEX` | 0.67 us | 20.3 us | **30.1x** |
| `LSET` | 1.2 us | 27.9 us | **23.8x** |
| `LREM` | 18.6 us | 58.7 us | **3.1x** |
| `LTRIM` | 1.1 us | 20.3 us | **18.2x** |
| `SADD` | 1.4 us | 24.0 us | **16.6x** |
| `SREM` | 4.7 us | 21.8 us | **4.6x** |
| `SISMEMBER` | 1.1 us | 20.4 us | **17.9x** |
| `SCARD` | 0.46 us | 20.5 us | **44.2x** |
| `SMEMBERS` | 3.3 us | 20.6 us | **6.2x** |
| `SPOP` | 8.3 us | 58.6 us | **7.0x** |
| `SRANDMEMBER` | 2.8 us | 20.2 us | **7.2x** |
| `ZADD` | 3.2 us | 28.0 us | **8.9x** |
| `ZSCORE` | 0.88 us | 19.8 us | **22.4x** |
| `ZRANGE` | 4.0 us | 20.4 us | **5.1x** |
| `ZCARD` | 0.49 us | 20.2 us | **41.5x** |
| `ZCOUNT` | 3.2 us | 20.1 us | **6.2x** |
| `ZINCRBY` | 3.1 us | 30.1 us | **9.7x** |
| `ZRANK` | 3.3 us | 20.2 us | **6.1x** |
| `ZREVRANGE` | 5.8 us | 20.3 us | **3.5x** |
| `ZPOPMIN` | 14.5 us | 57.2 us | **4.0x** |
| `ZREM` | 4.6 us | 22.5 us | **4.9x** |
| `SETBIT` | 11.4 us | 40.2 us | **3.5x** |
| `GETBIT` | 0.44 us | 32.4 us | **74.4x** |
| `BITCOUNT` | 0.37 us | 33.5 us | **91.6x** |
| `BITPOS` | 0.43 us | 26.7 us | **62.7x** |
| `PFADD` | 2.8 us | 36.0 us | **12.9x** |
| `PFCOUNT` | 8.1 us | 25.0 us | **3.1x** |
| `GEOADD` | 2.5 us | 40.5 us | **15.9x** |
| `GEODIST` | 0.93 us | 30.9 us | **33.4x** |
| `GEOPOS` | 0.69 us | 30.9 us | **44.6x** |
| `GEOHASH` | 0.73 us | 30.7 us | **41.8x** |
| `XADD` | 1.7 us | 30.0 us | **17.7x** |
| `XLEN` | 0.57 us | 19.8 us | **35.0x** |
| `XRANGE` | 4.2 us | 43.4 us | **10.4x** |
| `XREAD` | 4.2 us | 42.1 us | **10.0x** |
| `XDEL` | 3.7 us | 59.8 us | **16.1x** |
| `DEL` | 3.0 us | 26.0 us | **8.7x** |
| `EXISTS` | 0.24 us | 23.8 us | **99.8x** |
| `EXPIRE` | 0.73 us | 39.2 us | **54.0x** |
| `TTL` | 0.29 us | 24.7 us | **86.6x** |
| `JSON.SET` | 3.5 us | 33.7 us | **9.6x** |
| `JSON.GET` | 1.5 us | 30.4 us | **20.0x** |
| `JSON.DEL` | 8.2 us | 63.9 us | **7.7x** |
| `JSON.NUMINCRBY` | 3.9 us | 31.5 us | **8.2x** |
| `JSON.ARRLEN` | 1.3 us | 30.8 us | **22.9x** |
| `JSON.TYPE` | 1.4 us | 20.9 us | **14.4x** |
| `BF.ADD` | 34.2 us | 30.6 us | **0.9x** |
| `BF.EXISTS` | 0.62 us | 30.5 us | **49.3x** |
| `BF.INFO` | 0.36 us | 30.9 us | **85.9x** |
| `CF.ADD` | 2.8 us | 30.3 us | **10.7x** |
| `CF.EXISTS` | 0.64 us | 31.1 us | **48.4x** |
| `CF.DEL` | 9.9 us | 62.6 us | **6.3x** |
| `TDIGEST.ADD` | 2.7 us | 29.9 us | **11.0x** |
| `TDIGEST.QUANTILE` | 1.0 us | 29.8 us | **29.3x** |
| `TDIGEST.BYRANK` | 1.2 us | 29.9 us | **24.8x** |
| `TDIGEST.CDF` | 1.2 us | 29.6 us | **23.9x** |
| `TS.ADD` | 6.6 us | 30.8 us | **4.6x** |
| `TS.GET` | 1.6 us | 30.0 us | **19.1x** |
| `TS.RANGE` | 21.1 us | 30.0 us | **1.4x** |
| `TS.INCRBY` | 9.1 us | 29.8 us | **3.3x** |
| `FT.SEARCH` | 29.3 us | 19.8 us | **0.7x** |
| `FT.TAG` | 29.7 us | 20.7 us | **0.7x** |
| `VECTOR.KNN` | 5.0 us | 34.1 us | **6.9x** |

