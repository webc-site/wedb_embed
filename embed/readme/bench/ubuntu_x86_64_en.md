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
| **Physical Disk Footprint** | **1053 MB** | **7582 MB** | **Saves 86%** |
| **Resident Memory (RSS)** | **279 MB** | **4798 MB** | **Saves 94%** |

#### wedb_embed vs Redis Core Command Benchmark

| Command | wedb_embed P95 Latency | Redis P95 Latency | Speedup |
| :--- | :--- | :--- | :--- |
| `SET` | 7.8 us | 36.1 us | **4.7x** |
| `GET` | 5.2 us | 27.8 us | **5.3x** |
| `MSET` | 52.7 us | 40.7 us | **0.8x** |
| `MGET` | 5.6 us | 34.2 us | **6.1x** |
| `INCRBY` | 0.76 us | 35.0 us | **46.3x** |
| `DECRBY` | 0.69 us | 35.7 us | **51.8x** |
| `APPEND` | 0.95 us | 35.0 us | **36.8x** |
| `STRLEN` | 0.27 us | 28.4 us | **104.5x** |
| `GETDEL` | 8.9 us | 70.8 us | **7.9x** |
| `GETRANGE` | 0.30 us | 28.6 us | **94.1x** |
| `SETRANGE` | 0.74 us | 35.9 us | **48.4x** |
| `HSET` | 2.0 us | 37.7 us | **18.7x** |
| `HGET` | 0.73 us | 29.5 us | **40.5x** |
| `HMGET` | 3.7 us | 34.8 us | **9.5x** |
| `HEXISTS` | 0.65 us | 29.2 us | **45.2x** |
| `HLEN` | 0.45 us | 26.6 us | **58.8x** |
| `HDEL` | 4.4 us | 32.4 us | **7.3x** |
| `HGETALL` | 3.6 us | 32.9 us | **9.0x** |
| `HKEYS` | 3.5 us | 31.4 us | **8.9x** |
| `HVALS` | 3.7 us | 32.6 us | **8.9x** |
| `HINCRBY` | 1.7 us | 37.7 us | **22.8x** |
| `LPUSH` | 2.2 us | 35.6 us | **16.5x** |
| `RPUSH` | 2.1 us | 34.9 us | **16.6x** |
| `LPOP` | 2.6 us | 37.6 us | **14.4x** |
| `RPOP` | 2.7 us | 34.8 us | **12.9x** |
| `LLEN` | 0.48 us | 24.4 us | **50.9x** |
| `LRANGE` | 3.9 us | 31.7 us | **8.2x** |
| `LINDEX` | 0.70 us | 29.2 us | **41.9x** |
| `LSET` | 1.2 us | 35.4 us | **28.4x** |
| `LREM` | 18.4 us | 71.2 us | **3.9x** |
| `LTRIM` | 1.1 us | 28.4 us | **25.0x** |
| `SADD` | 1.5 us | 35.4 us | **23.0x** |
| `SREM` | 4.6 us | 34.1 us | **7.5x** |
| `SISMEMBER` | 0.72 us | 28.4 us | **39.4x** |
| `SCARD` | 0.49 us | 28.2 us | **57.4x** |
| `SMEMBERS` | 4.2 us | 28.6 us | **6.9x** |
| `SPOP` | 8.9 us | 71.2 us | **8.0x** |
| `SRANDMEMBER` | 3.5 us | 28.3 us | **8.1x** |
| `ZADD` | 3.1 us | 35.3 us | **11.5x** |
| `ZSCORE` | 0.81 us | 28.6 us | **35.3x** |
| `ZRANGE` | 4.0 us | 32.5 us | **8.2x** |
| `ZCARD` | 0.48 us | 28.3 us | **58.5x** |
| `ZCOUNT` | 3.8 us | 28.6 us | **7.6x** |
| `ZINCRBY` | 3.0 us | 36.8 us | **12.2x** |
| `ZRANK` | 3.6 us | 29.0 us | **8.1x** |
| `ZREVRANGE` | 6.5 us | 31.8 us | **4.9x** |
| `ZPOPMIN` | 8.1 us | 72.1 us | **8.9x** |
| `ZREM` | 4.8 us | 34.1 us | **7.1x** |
| `SETBIT` | 12.2 us | 36.2 us | **3.0x** |
| `GETBIT` | 0.51 us | 32.3 us | **63.2x** |
| `BITCOUNT` | 0.39 us | 68.2 us | **172.7x** |
| `BITPOS` | 0.46 us | 32.9 us | **71.4x** |
| `PFADD` | 2.9 us | 35.6 us | **12.3x** |
| `PFCOUNT` | 8.5 us | 26.9 us | **3.2x** |
| `GEOADD` | 2.6 us | 38.3 us | **14.9x** |
| `GEODIST` | 1.0 us | 31.5 us | **30.8x** |
| `GEOPOS` | 0.77 us | 29.9 us | **39.1x** |
| `GEOHASH` | 0.80 us | 29.5 us | **36.7x** |
| `XADD` | 1.6 us | 36.8 us | **23.1x** |
| `XLEN` | 0.56 us | 28.4 us | **50.9x** |
| `XRANGE` | 4.4 us | 38.4 us | **8.7x** |
| `XREAD` | 4.4 us | 39.7 us | **9.1x** |
| `XDEL` | 3.4 us | 73.4 us | **21.4x** |
| `DEL` | 3.2 us | 24.5 us | **7.7x** |
| `EXISTS` | 0.26 us | 29.6 us | **115.3x** |
| `EXPIRE` | 0.74 us | 38.0 us | **51.4x** |
| `TTL` | 0.31 us | 25.7 us | **82.7x** |
| `JSON.SET` | 3.5 us | 32.6 us | **9.4x** |
| `JSON.GET` | 1.5 us | 32.7 us | **21.2x** |
| `JSON.DEL` | 9.5 us | 63.6 us | **6.7x** |
| `JSON.NUMINCRBY` | 3.8 us | 31.2 us | **8.1x** |
| `JSON.ARRLEN` | 1.3 us | 32.2 us | **24.1x** |
| `JSON.TYPE` | 1.3 us | 29.8 us | **22.9x** |
| `BF.ADD` | 34.5 us | 33.0 us | **1.0x** |
| `BF.EXISTS` | 0.67 us | 24.9 us | **36.9x** |
| `BF.INFO` | 0.38 us | 33.7 us | **89.6x** |
| `CF.ADD` | 2.7 us | 30.5 us | **11.2x** |
| `CF.EXISTS` | 0.73 us | 33.1 us | **45.4x** |
| `CF.DEL` | 10.4 us | 63.9 us | **6.2x** |
| `TDIGEST.ADD` | 2.6 us | 31.3 us | **12.2x** |
| `TDIGEST.QUANTILE` | 1.1 us | 31.2 us | **28.0x** |
| `TDIGEST.BYRANK` | 1.3 us | 29.1 us | **21.8x** |
| `TDIGEST.CDF` | 1.3 us | 28.9 us | **22.5x** |
| `TS.ADD` | 7.8 us | 32.0 us | **4.1x** |
| `TS.GET` | 1.4 us | 28.6 us | **20.1x** |
| `TS.RANGE` | 23.4 us | 32.1 us | **1.4x** |
| `TS.INCRBY` | 10.1 us | 31.9 us | **3.2x** |
| `FT.SEARCH` | 31.9 us | 31.9 us | **1.0x** |
| `FT.TAG` | 31.7 us | 28.7 us | **0.9x** |
| `VECTOR.KNN` | 6.4 us | 30.6 us | **4.8x** |

