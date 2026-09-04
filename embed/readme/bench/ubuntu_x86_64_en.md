### Ubuntu CI (GitHub Actions Runner)

#### Hardware & Test Environment

CPU: AMD EPYC 7763 64-Core Processor (4 cores)<br>
Memory: 15.6 GB<br>
Disk: Azure Managed Virtual Disk (Cloud Standard SSD)<br>
OS: Ubuntu 24.04.4 LTS (Linux 6.17.0-1022-azure)<br>
Rust: 1.98.1 (48a229cea 2026-09-01)<br>
Redis: v8.10.1

#### Physical Footprint & Memory Benchmark (4.3 GB Dataset Scale)

| Resource Metric | wedb_embed (Embedded LSM+LZ4) | Redis (v8.10.1 AOF Mode) | Resource Savings |
| :--- | :--- | :--- | :--- |
| **Dataset Scale** | 5,000,000 Structured Items | 5,000,000 Structured Items | All 14 Data Formats |
| **Raw Uncompressed Payload** | 4377 MB | 4377 MB | Structured Payload |
| **Physical Disk Footprint** | **1053 MB** | **7567 MB** | **Saves 86%** |
| **Resident Memory (RSS)** | **271 MB** | **4802 MB** | **Saves 94%** |

#### wedb_embed vs Redis Core Command Benchmark

| Command | wedb_embed P95 Latency | Redis P95 Latency | Speedup |
| :--- | :--- | :--- | :--- |
| `SET` | 7.5 us | 29.1 us | **3.9x** |
| `GET` | 5.1 us | 20.3 us | **4.0x** |
| `MSET` | 51.3 us | 34.1 us | **0.7x** |
| `MGET` | 5.5 us | 24.2 us | **4.4x** |
| `INCRBY` | 0.77 us | 27.2 us | **35.2x** |
| `DECRBY` | 1.1 us | 27.1 us | **24.0x** |
| `APPEND` | 1.3 us | 27.0 us | **20.8x** |
| `STRLEN` | 0.29 us | 19.9 us | **69.3x** |
| `GETDEL` | 8.2 us | 57.6 us | **7.0x** |
| `GETRANGE` | 0.45 us | 20.0 us | **45.0x** |
| `SETRANGE` | 1.4 us | 27.4 us | **19.9x** |
| `HSET` | 2.6 us | 41.7 us | **16.3x** |
| `HGET` | 0.74 us | 25.1 us | **34.0x** |
| `HMGET` | 3.4 us | 37.3 us | **10.8x** |
| `HEXISTS` | 0.67 us | 25.0 us | **37.2x** |
| `HLEN` | 0.45 us | 25.5 us | **56.9x** |
| `HDEL` | 4.0 us | 40.5 us | **10.1x** |
| `HGETALL` | 3.6 us | 36.2 us | **9.9x** |
| `HKEYS` | 3.2 us | 32.1 us | **9.9x** |
| `HVALS` | 3.3 us | 32.0 us | **9.8x** |
| `HINCRBY` | 1.8 us | 42.5 us | **23.3x** |
| `LPUSH` | 3.2 us | 27.0 us | **8.6x** |
| `RPUSH` | 3.3 us | 28.5 us | **8.6x** |
| `LPOP` | 2.5 us | 24.8 us | **10.0x** |
| `RPOP` | 2.4 us | 28.7 us | **11.9x** |
| `LLEN` | 0.48 us | 21.1 us | **44.0x** |
| `LRANGE` | 3.5 us | 19.1 us | **5.4x** |
| `LINDEX` | 0.71 us | 20.7 us | **29.0x** |
| `LSET` | 1.2 us | 27.1 us | **22.9x** |
| `LREM` | 17.6 us | 61.7 us | **3.5x** |
| `LTRIM` | 1.1 us | 19.8 us | **18.0x** |
| `SADD` | 1.5 us | 24.5 us | **16.8x** |
| `SREM` | 4.0 us | 22.6 us | **5.7x** |
| `SISMEMBER` | 0.77 us | 19.6 us | **25.5x** |
| `SCARD` | 0.47 us | 19.9 us | **42.7x** |
| `SMEMBERS` | 3.3 us | 19.8 us | **6.0x** |
| `SPOP` | 6.3 us | 60.3 us | **9.6x** |
| `SRANDMEMBER` | 2.2 us | 20.1 us | **9.1x** |
| `ZADD` | 3.1 us | 30.0 us | **9.6x** |
| `ZSCORE` | 0.93 us | 26.1 us | **28.1x** |
| `ZRANGE` | 4.1 us | 18.9 us | **4.7x** |
| `ZCARD` | 0.49 us | 19.7 us | **40.0x** |
| `ZCOUNT` | 3.3 us | 19.0 us | **5.7x** |
| `ZINCRBY` | 3.3 us | 30.6 us | **9.3x** |
| `ZRANK` | 3.5 us | 20.0 us | **5.8x** |
| `ZREVRANGE` | 5.9 us | 33.0 us | **5.6x** |
| `ZPOPMIN` | 7.8 us | 62.7 us | **8.1x** |
| `ZREM` | 4.5 us | 22.5 us | **5.0x** |
| `SETBIT` | 11.5 us | 39.9 us | **3.5x** |
| `GETBIT` | 0.47 us | 25.5 us | **54.3x** |
| `BITCOUNT` | 0.38 us | 38.8 us | **101.6x** |
| `BITPOS` | 0.47 us | 27.2 us | **57.9x** |
| `PFADD` | 2.8 us | 34.5 us | **12.2x** |
| `PFCOUNT` | 8.0 us | 25.1 us | **3.1x** |
| `GEOADD` | 2.6 us | 42.9 us | **16.4x** |
| `GEODIST` | 0.97 us | 31.1 us | **32.2x** |
| `GEOPOS` | 0.74 us | 31.3 us | **42.4x** |
| `GEOHASH` | 0.78 us | 27.2 us | **35.0x** |
| `XADD` | 1.7 us | 31.5 us | **18.4x** |
| `XLEN` | 0.58 us | 19.6 us | **33.6x** |
| `XRANGE` | 4.3 us | 31.5 us | **7.4x** |
| `XREAD` | 4.2 us | 33.8 us | **8.1x** |
| `XDEL` | 3.7 us | 63.0 us | **17.0x** |
| `DEL` | 3.0 us | 25.1 us | **8.2x** |
| `EXISTS` | 0.26 us | 24.9 us | **96.2x** |
| `EXPIRE` | 0.71 us | 41.4 us | **58.1x** |
| `TTL` | 0.30 us | 26.7 us | **89.7x** |
| `JSON.SET` | 3.7 us | 22.1 us | **6.0x** |
| `JSON.GET` | 1.6 us | 35.6 us | **23.0x** |
| `JSON.DEL` | 8.3 us | 66.3 us | **8.0x** |
| `JSON.NUMINCRBY` | 3.8 us | 31.9 us | **8.3x** |
| `JSON.ARRLEN` | 1.3 us | 32.1 us | **24.0x** |
| `JSON.TYPE` | 1.5 us | 18.9 us | **12.9x** |
| `BF.ADD` | 34.4 us | 35.3 us | **1.0x** |
| `BF.EXISTS` | 0.64 us | 35.5 us | **55.3x** |
| `BF.INFO` | 0.36 us | 32.8 us | **89.9x** |
| `CF.ADD` | 2.5 us | 31.9 us | **12.6x** |
| `CF.EXISTS` | 0.67 us | 31.4 us | **46.6x** |
| `CF.DEL` | 9.5 us | 64.8 us | **6.8x** |
| `TDIGEST.ADD` | 2.6 us | 19.0 us | **7.5x** |
| `TDIGEST.QUANTILE` | 1.1 us | 19.0 us | **17.8x** |
| `TDIGEST.BYRANK` | 1.3 us | 19.3 us | **15.4x** |
| `TDIGEST.CDF` | 1.3 us | 19.9 us | **14.9x** |
| `TS.ADD` | 6.8 us | 19.3 us | **2.8x** |
| `TS.GET` | 1.5 us | 19.0 us | **12.4x** |
| `TS.RANGE` | 20.8 us | 18.9 us | **0.9x** |
| `TS.INCRBY` | 9.7 us | 18.7 us | **1.9x** |
| `FT.SEARCH` | 27.6 us | 19.4 us | **0.7x** |
| `FT.TAG` | 28.2 us | 19.5 us | **0.7x** |
| `VECTOR.KNN` | 4.9 us | 24.7 us | **5.1x** |

