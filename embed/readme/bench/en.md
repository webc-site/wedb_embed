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
| **Physical Disk Footprint** | **1053 MB** | **7710 MB** | **Saves 86%** |
| **Resident Memory (RSS)** | **290 MB** | **4832 MB** | **Saves 94%** |

#### wedb_embed vs Redis Core Command Benchmark

| Command | wedb_embed P95 Latency | Redis P95 Latency | Speedup |
| :--- | :--- | :--- | :--- |
| `SET` | 7.4 us | 28.9 us | **3.9x** |
| `GET` | 4.9 us | 19.5 us | **3.9x** |
| `MSET` | 49.2 us | 34.6 us | **0.7x** |
| `MGET` | 4.8 us | 24.8 us | **5.1x** |
| `INCRBY` | 1.1 us | 27.1 us | **24.8x** |
| `DECRBY` | 1.1 us | 27.1 us | **25.0x** |
| `APPEND` | 0.86 us | 27.2 us | **31.8x** |
| `STRLEN` | 0.31 us | 20.1 us | **64.7x** |
| `GETDEL` | 8.0 us | 57.0 us | **7.1x** |
| `GETRANGE` | 0.26 us | 19.2 us | **73.4x** |
| `SETRANGE` | 1.3 us | 26.0 us | **19.9x** |
| `HSET` | 3.0 us | 28.9 us | **9.8x** |
| `HGET` | 0.73 us | 19.6 us | **27.0x** |
| `HMGET` | 3.0 us | 20.6 us | **6.8x** |
| `HEXISTS` | 0.61 us | 19.6 us | **31.9x** |
| `HLEN` | 0.41 us | 20.3 us | **49.6x** |
| `HDEL` | 4.8 us | 23.2 us | **4.9x** |
| `HGETALL` | 3.5 us | 19.7 us | **5.7x** |
| `HKEYS` | 3.3 us | 19.2 us | **5.7x** |
| `HVALS` | 3.2 us | 19.5 us | **6.1x** |
| `HINCRBY` | 3.1 us | 29.3 us | **9.5x** |
| `LPUSH` | 2.9 us | 28.4 us | **9.7x** |
| `RPUSH` | 3.1 us | 27.0 us | **8.9x** |
| `LPOP` | 2.5 us | 26.9 us | **10.7x** |
| `RPOP` | 2.5 us | 27.5 us | **11.1x** |
| `LLEN` | 0.44 us | 19.8 us | **45.6x** |
| `LRANGE` | 3.3 us | 19.8 us | **5.9x** |
| `LINDEX` | 0.67 us | 19.9 us | **29.6x** |
| `LSET` | 1.1 us | 27.1 us | **25.6x** |
| `LREM` | 16.8 us | 60.3 us | **3.6x** |
| `LTRIM` | 1.1 us | 19.8 us | **17.6x** |
| `SADD` | 1.6 us | 24.2 us | **14.7x** |
| `SREM` | 3.7 us | 23.7 us | **6.5x** |
| `SISMEMBER` | 0.67 us | 19.1 us | **28.6x** |
| `SCARD` | 0.43 us | 19.5 us | **45.8x** |
| `SMEMBERS` | 3.4 us | 19.0 us | **5.6x** |
| `SPOP` | 5.8 us | 57.7 us | **10.0x** |
| `SRANDMEMBER` | 2.5 us | 19.8 us | **8.1x** |
| `ZADD` | 3.0 us | 25.9 us | **8.7x** |
| `ZSCORE` | 0.85 us | 19.9 us | **23.4x** |
| `ZRANGE` | 4.0 us | 19.6 us | **4.9x** |
| `ZCARD` | 0.49 us | 19.6 us | **39.9x** |
| `ZCOUNT` | 3.2 us | 19.3 us | **6.1x** |
| `ZINCRBY` | 3.1 us | 29.8 us | **9.5x** |
| `ZRANK` | 3.5 us | 19.2 us | **5.5x** |
| `ZREVRANGE` | 6.6 us | 19.8 us | **3.0x** |
| `ZPOPMIN` | 8.4 us | 62.1 us | **7.4x** |
| `ZREM` | 4.4 us | 22.7 us | **5.1x** |
| `SETBIT` | 15.8 us | 28.8 us | **1.8x** |
| `GETBIT` | 0.63 us | 22.3 us | **35.7x** |
| `BITCOUNT` | 0.54 us | 26.6 us | **49.4x** |
| `BITPOS` | 0.63 us | 23.1 us | **36.9x** |
| `PFADD` | 2.7 us | 26.9 us | **9.9x** |
| `PFCOUNT` | 10.1 us | 19.6 us | **1.9x** |
| `GEOADD` | 2.7 us | 32.0 us | **11.9x** |
| `GEODIST` | 1.6 us | 19.9 us | **12.4x** |
| `GEOPOS` | 0.73 us | 19.3 us | **26.5x** |
| `GEOHASH` | 0.77 us | 19.6 us | **25.6x** |
| `XADD` | 1.8 us | 30.7 us | **17.4x** |
| `XLEN` | 0.58 us | 19.6 us | **33.9x** |
| `XRANGE` | 4.3 us | 31.0 us | **7.1x** |
| `XREAD` | 4.2 us | 32.5 us | **7.7x** |
| `XDEL` | 3.9 us | 61.4 us | **15.6x** |
| `DEL` | 3.1 us | 21.3 us | **6.9x** |
| `EXISTS` | 0.24 us | 21.4 us | **89.7x** |
| `EXPIRE` | 0.86 us | 29.4 us | **34.1x** |
| `TTL` | 0.33 us | 21.1 us | **63.9x** |
| `JSON.SET` | 3.8 us | 20.5 us | **5.4x** |
| `JSON.GET` | 1.5 us | 19.3 us | **12.7x** |
| `JSON.DEL` | 7.1 us | 38.9 us | **5.5x** |
| `JSON.NUMINCRBY` | 3.9 us | 19.4 us | **5.0x** |
| `JSON.ARRLEN` | 1.3 us | 19.4 us | **14.7x** |
| `JSON.TYPE` | 1.4 us | 19.3 us | **13.8x** |
| `BF.ADD` | 60.1 us | 23.6 us | **0.4x** |
| `BF.EXISTS` | 1.0 us | 23.1 us | **22.4x** |
| `BF.INFO` | 0.57 us | 22.2 us | **39.3x** |
| `CF.ADD` | 4.0 us | 22.6 us | **5.6x** |
| `CF.EXISTS` | 0.67 us | 18.8 us | **28.1x** |
| `CF.DEL` | 10.7 us | 72.6 us | **6.8x** |
| `TDIGEST.ADD` | 2.6 us | 19.1 us | **7.3x** |
| `TDIGEST.QUANTILE` | 1.1 us | 19.2 us | **18.1x** |
| `TDIGEST.BYRANK` | 1.1 us | 19.4 us | **17.4x** |
| `TDIGEST.CDF` | 1.2 us | 19.1 us | **15.3x** |
| `TS.ADD` | 7.2 us | 19.1 us | **2.7x** |
| `TS.GET` | 1.5 us | 18.9 us | **13.0x** |
| `TS.RANGE` | 21.2 us | 19.1 us | **0.9x** |
| `TS.INCRBY` | 10.7 us | 19.0 us | **1.8x** |
| `FT.SEARCH` | 27.3 us | 19.6 us | **0.7x** |
| `FT.TAG` | 26.9 us | 19.2 us | **0.7x** |
| `VECTOR.KNN` | 4.3 us | 20.1 us | **4.7x** |

