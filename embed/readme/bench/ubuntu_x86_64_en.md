### Ubuntu CI (GitHub Actions Runner)

#### Hardware & Test Environment

CPU: AMD EPYC 9V74 80-Core Processor (4 cores)<br>
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
| **Physical Disk Footprint** | **1053 MB** | **7476 MB** | **Saves 86%** |
| **Resident Memory (RSS)** | **281 MB** | **4805 MB** | **Saves 94%** |

#### wedb_embed vs Redis Core Command Benchmark

| Command | wedb_embed P95 Latency | Redis P95 Latency | Speedup |
| :--- | :--- | :--- | :--- |
| `SET` | 8.0 us | 28.5 us | **3.6x** |
| `GET` | 5.3 us | 19.9 us | **3.8x** |
| `MSET` | 55.9 us | 31.9 us | **0.6x** |
| `MGET` | 5.8 us | 19.8 us | **3.4x** |
| `INCRBY` | 0.74 us | 28.5 us | **38.4x** |
| `DECRBY` | 0.69 us | 26.8 us | **38.7x** |
| `APPEND` | 0.98 us | 24.9 us | **25.3x** |
| `STRLEN` | 0.28 us | 20.2 us | **71.6x** |
| `GETDEL` | 9.0 us | 57.0 us | **6.3x** |
| `GETRANGE` | 0.30 us | 20.4 us | **67.0x** |
| `SETRANGE` | 0.76 us | 27.6 us | **36.5x** |
| `HSET` | 2.8 us | 27.9 us | **10.1x** |
| `HGET` | 0.75 us | 20.2 us | **26.8x** |
| `HMGET` | 3.8 us | 19.8 us | **5.3x** |
| `HEXISTS` | 0.99 us | 20.4 us | **20.6x** |
| `HLEN` | 0.46 us | 20.5 us | **45.0x** |
| `HDEL` | 6.2 us | 21.1 us | **3.4x** |
| `HGETALL` | 3.8 us | 20.2 us | **5.3x** |
| `HKEYS` | 3.5 us | 20.0 us | **5.7x** |
| `HVALS` | 3.7 us | 19.9 us | **5.4x** |
| `HINCRBY` | 1.7 us | 29.7 us | **17.7x** |
| `LPUSH` | 2.1 us | 23.5 us | **11.1x** |
| `RPUSH` | 2.2 us | 28.8 us | **13.0x** |
| `LPOP` | 2.7 us | 26.4 us | **10.0x** |
| `RPOP` | 2.7 us | 27.9 us | **10.2x** |
| `LLEN` | 0.47 us | 20.1 us | **43.3x** |
| `LRANGE` | 4.0 us | 21.3 us | **5.4x** |
| `LINDEX` | 0.71 us | 20.0 us | **28.2x** |
| `LSET` | 1.2 us | 25.0 us | **20.6x** |
| `LREM` | 18.0 us | 53.4 us | **3.0x** |
| `LTRIM` | 1.1 us | 19.6 us | **17.4x** |
| `SADD` | 1.4 us | 23.1 us | **16.3x** |
| `SREM` | 5.2 us | 21.1 us | **4.0x** |
| `SISMEMBER` | 0.74 us | 20.1 us | **27.2x** |
| `SCARD` | 0.47 us | 20.3 us | **43.4x** |
| `SMEMBERS` | 3.6 us | 20.1 us | **5.5x** |
| `SPOP` | 6.3 us | 50.3 us | **8.0x** |
| `SRANDMEMBER` | 2.7 us | 20.6 us | **7.7x** |
| `ZADD` | 3.0 us | 26.7 us | **8.9x** |
| `ZSCORE` | 0.85 us | 19.9 us | **23.4x** |
| `ZRANGE` | 4.1 us | 19.4 us | **4.7x** |
| `ZCARD` | 0.47 us | 20.1 us | **42.3x** |
| `ZCOUNT` | 3.8 us | 19.3 us | **5.1x** |
| `ZINCRBY` | 3.0 us | 26.7 us | **8.8x** |
| `ZRANK` | 3.5 us | 19.9 us | **5.7x** |
| `ZREVRANGE` | 6.7 us | 19.6 us | **2.9x** |
| `ZPOPMIN` | 10.2 us | 51.6 us | **5.0x** |
| `ZREM` | 4.8 us | 20.8 us | **4.3x** |
| `SETBIT` | 12.3 us | 38.5 us | **3.1x** |
| `GETBIT` | 0.49 us | 28.1 us | **57.9x** |
| `BITCOUNT` | 0.41 us | 27.1 us | **66.3x** |
| `BITPOS` | 0.46 us | 19.6 us | **42.5x** |
| `PFADD` | 2.8 us | 21.0 us | **7.4x** |
| `PFCOUNT` | 8.4 us | 20.2 us | **2.4x** |
| `GEOADD` | 2.6 us | 22.9 us | **8.9x** |
| `GEODIST` | 1.0 us | 20.1 us | **20.1x** |
| `GEOPOS` | 0.79 us | 20.6 us | **26.2x** |
| `GEOHASH` | 0.81 us | 19.9 us | **24.8x** |
| `XADD` | 1.6 us | 27.8 us | **17.8x** |
| `XLEN` | 0.55 us | 20.4 us | **37.2x** |
| `XRANGE` | 4.3 us | 27.2 us | **6.4x** |
| `XREAD` | 4.3 us | 29.8 us | **7.0x** |
| `XDEL` | 3.4 us | 55.7 us | **16.3x** |
| `DEL` | 3.3 us | 20.4 us | **6.3x** |
| `EXISTS` | 0.27 us | 20.4 us | **76.2x** |
| `EXPIRE` | 0.79 us | 29.3 us | **37.0x** |
| `TTL` | 0.33 us | 20.6 us | **62.0x** |
| `JSON.SET` | 3.7 us | 20.0 us | **5.4x** |
| `JSON.GET` | 1.6 us | 19.8 us | **12.2x** |
| `JSON.DEL` | 8.5 us | 38.2 us | **4.5x** |
| `JSON.NUMINCRBY` | 4.0 us | 19.6 us | **4.8x** |
| `JSON.ARRLEN` | 1.4 us | 19.8 us | **14.1x** |
| `JSON.TYPE` | 1.5 us | 20.7 us | **14.0x** |
| `BF.ADD` | 34.4 us | 34.1 us | **1.0x** |
| `BF.EXISTS` | 0.68 us | 33.2 us | **49.0x** |
| `BF.INFO` | 0.38 us | 33.6 us | **87.4x** |
| `CF.ADD` | 2.7 us | 33.7 us | **12.6x** |
| `CF.EXISTS` | 0.71 us | 20.2 us | **28.3x** |
| `CF.DEL` | 10.0 us | 62.2 us | **6.2x** |
| `TDIGEST.ADD` | 2.6 us | 19.7 us | **7.7x** |
| `TDIGEST.QUANTILE` | 1.1 us | 19.2 us | **16.9x** |
| `TDIGEST.BYRANK` | 1.2 us | 19.5 us | **15.8x** |
| `TDIGEST.CDF` | 1.4 us | 19.7 us | **13.9x** |
| `TS.ADD` | 7.9 us | 19.3 us | **2.4x** |
| `TS.GET` | 1.5 us | 19.3 us | **13.1x** |
| `TS.RANGE` | 22.7 us | 19.9 us | **0.9x** |
| `TS.INCRBY` | 10.9 us | 19.6 us | **1.8x** |
| `FT.SEARCH` | 29.5 us | 19.8 us | **0.7x** |
| `FT.TAG` | 28.8 us | 19.5 us | **0.7x** |
| `VECTOR.KNN` | 4.3 us | 19.3 us | **4.5x** |

