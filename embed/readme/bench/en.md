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
| **Physical Disk Footprint** | **1053 MB** | **7663 MB** | **Saves 86%** |
| **Resident Memory (RSS)** | **274 MB** | **4825 MB** | **Saves 94%** |

#### wedb_embed vs Redis Core Command Benchmark

| Command | wedb_embed P95 Latency | Redis P95 Latency | Speedup |
| :--- | :--- | :--- | :--- |
| `SET` | 7.4 us | 27.3 us | **3.7x** |
| `GET` | 5.0 us | 19.5 us | **3.9x** |
| `MSET` | 50.9 us | 34.2 us | **0.7x** |
| `MGET` | 4.9 us | 25.5 us | **5.2x** |
| `INCRBY` | 0.90 us | 27.0 us | **30.0x** |
| `DECRBY` | 1.1 us | 28.4 us | **26.1x** |
| `APPEND` | 0.80 us | 26.9 us | **33.8x** |
| `STRLEN` | 0.36 us | 19.3 us | **53.0x** |
| `GETDEL` | 8.2 us | 57.4 us | **7.0x** |
| `GETRANGE` | 0.26 us | 19.2 us | **73.9x** |
| `SETRANGE` | 1.3 us | 27.8 us | **21.4x** |
| `HSET` | 2.6 us | 28.6 us | **11.2x** |
| `HGET` | 0.66 us | 20.9 us | **31.7x** |
| `HMGET` | 3.1 us | 24.5 us | **7.9x** |
| `HEXISTS` | 1.1 us | 20.4 us | **19.3x** |
| `HLEN` | 0.41 us | 21.1 us | **51.8x** |
| `HDEL` | 4.3 us | 23.4 us | **5.4x** |
| `HGETALL` | 3.3 us | 19.6 us | **6.0x** |
| `HKEYS` | 3.2 us | 19.4 us | **6.2x** |
| `HVALS` | 3.3 us | 19.4 us | **5.9x** |
| `HINCRBY` | 1.8 us | 28.6 us | **15.5x** |
| `LPUSH` | 2.6 us | 28.6 us | **11.1x** |
| `RPUSH` | 3.0 us | 27.4 us | **9.2x** |
| `LPOP` | 2.5 us | 23.8 us | **9.4x** |
| `RPOP` | 2.7 us | 25.4 us | **9.5x** |
| `LLEN` | 0.43 us | 21.3 us | **49.9x** |
| `LRANGE` | 3.5 us | 19.2 us | **5.4x** |
| `LINDEX` | 0.64 us | 20.8 us | **32.6x** |
| `LSET` | 1.1 us | 28.5 us | **26.9x** |
| `LREM` | 16.6 us | 58.8 us | **3.5x** |
| `LTRIM` | 1.1 us | 19.2 us | **17.1x** |
| `SADD` | 1.4 us | 24.1 us | **17.7x** |
| `SREM` | 3.7 us | 24.0 us | **6.5x** |
| `SISMEMBER` | 0.67 us | 19.4 us | **28.8x** |
| `SCARD` | 0.43 us | 19.1 us | **44.5x** |
| `SMEMBERS` | 3.7 us | 18.7 us | **5.0x** |
| `SPOP` | 8.8 us | 57.8 us | **6.5x** |
| `SRANDMEMBER` | 4.0 us | 19.1 us | **4.7x** |
| `ZADD` | 2.9 us | 29.1 us | **9.9x** |
| `ZSCORE` | 0.94 us | 19.3 us | **20.5x** |
| `ZRANGE` | 3.9 us | 19.7 us | **5.0x** |
| `ZCARD` | 0.52 us | 20.0 us | **38.8x** |
| `ZCOUNT` | 3.4 us | 20.5 us | **6.0x** |
| `ZINCRBY` | 3.2 us | 31.0 us | **9.7x** |
| `ZRANK` | 3.4 us | 19.1 us | **5.7x** |
| `ZREVRANGE` | 5.9 us | 19.6 us | **3.3x** |
| `ZPOPMIN` | 8.3 us | 61.5 us | **7.5x** |
| `ZREM` | 5.2 us | 28.8 us | **5.6x** |
| `SETBIT` | 11.4 us | 45.4 us | **4.0x** |
| `GETBIT` | 0.44 us | 29.4 us | **67.0x** |
| `BITCOUNT` | 0.37 us | 27.3 us | **74.4x** |
| `BITPOS` | 0.45 us | 30.7 us | **67.6x** |
| `PFADD` | 2.7 us | 22.8 us | **8.6x** |
| `PFCOUNT` | 8.1 us | 21.4 us | **2.6x** |
| `GEOADD` | 2.5 us | 31.4 us | **12.4x** |
| `GEODIST` | 0.94 us | 19.3 us | **20.6x** |
| `GEOPOS` | 0.78 us | 19.7 us | **25.3x** |
| `GEOHASH` | 0.81 us | 21.1 us | **26.2x** |
| `XADD` | 1.6 us | 30.1 us | **18.5x** |
| `XLEN` | 0.62 us | 19.2 us | **31.1x** |
| `XRANGE` | 4.6 us | 30.7 us | **6.7x** |
| `XREAD` | 4.2 us | 32.9 us | **7.8x** |
| `XDEL` | 3.7 us | 62.8 us | **17.0x** |
| `DEL` | 3.0 us | 20.8 us | **7.0x** |
| `EXISTS` | 0.24 us | 21.4 us | **88.9x** |
| `EXPIRE` | 0.89 us | 29.8 us | **33.7x** |
| `TTL` | 0.27 us | 20.9 us | **76.0x** |
| `JSON.SET` | 3.5 us | 19.4 us | **5.5x** |
| `JSON.GET` | 1.5 us | 18.9 us | **12.4x** |
| `JSON.DEL` | 8.1 us | 38.5 us | **4.7x** |
| `JSON.NUMINCRBY` | 3.9 us | 19.4 us | **4.9x** |
| `JSON.ARRLEN` | 1.3 us | 19.3 us | **14.7x** |
| `JSON.TYPE` | 1.4 us | 19.0 us | **13.3x** |
| `BF.ADD` | 32.7 us | 37.3 us | **1.1x** |
| `BF.EXISTS` | 0.63 us | 30.3 us | **47.8x** |
| `BF.INFO` | 0.36 us | 21.5 us | **59.3x** |
| `CF.ADD` | 2.5 us | 19.9 us | **8.0x** |
| `CF.EXISTS` | 0.80 us | 18.7 us | **23.4x** |
| `CF.DEL` | 9.8 us | 40.0 us | **4.1x** |
| `TDIGEST.ADD` | 2.6 us | 19.4 us | **7.4x** |
| `TDIGEST.QUANTILE` | 1.1 us | 19.1 us | **16.8x** |
| `TDIGEST.BYRANK` | 1.1 us | 19.3 us | **17.0x** |
| `TDIGEST.CDF` | 1.2 us | 18.9 us | **15.5x** |
| `TS.ADD` | 11.5 us | 19.8 us | **1.7x** |
| `TS.GET` | 1.4 us | 19.1 us | **13.5x** |
| `TS.RANGE` | 21.0 us | 19.3 us | **0.9x** |
| `TS.INCRBY` | 8.9 us | 18.9 us | **2.1x** |
| `FT.SEARCH` | 26.9 us | 18.9 us | **0.7x** |
| `FT.TAG` | 26.9 us | 19.0 us | **0.7x** |
| `VECTOR.KNN` | 4.1 us | 19.2 us | **4.7x** |

