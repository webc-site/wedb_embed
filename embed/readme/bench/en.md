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
| **Physical Disk Footprint** | **1053 MB** | **7425 MB** | **Saves 86%** |
| **Resident Memory (RSS)** | **288 MB** | **4844 MB** | **Saves 94%** |

#### wedb_embed vs Redis Core Command Benchmark

| Command | wedb_embed P95 Latency | Redis P95 Latency | Speedup |
| :--- | :--- | :--- | :--- |
| `SET` | 8.0 us | 28.1 us | **3.5x** |
| `GET` | 5.2 us | 19.9 us | **3.8x** |
| `MSET` | 52.0 us | 32.1 us | **0.6x** |
| `MGET` | 5.4 us | 19.3 us | **3.5x** |
| `INCRBY` | 0.73 us | 27.6 us | **37.6x** |
| `DECRBY` | 0.68 us | 28.1 us | **41.3x** |
| `APPEND` | 1.2 us | 27.9 us | **23.9x** |
| `STRLEN` | 0.33 us | 19.7 us | **59.0x** |
| `GETDEL` | 8.6 us | 54.2 us | **6.3x** |
| `GETRANGE` | 0.41 us | 19.8 us | **48.1x** |
| `SETRANGE` | 0.75 us | 27.7 us | **37.0x** |
| `HSET` | 2.0 us | 36.2 us | **18.0x** |
| `HGET` | 0.72 us | 30.0 us | **41.5x** |
| `HMGET` | 3.3 us | 34.9 us | **10.5x** |
| `HEXISTS` | 0.66 us | 27.7 us | **41.8x** |
| `HLEN` | 0.45 us | 29.2 us | **65.4x** |
| `HDEL` | 5.9 us | 32.9 us | **5.6x** |
| `HGETALL` | 4.0 us | 33.4 us | **8.3x** |
| `HKEYS` | 3.6 us | 30.4 us | **8.5x** |
| `HVALS` | 3.7 us | 32.4 us | **8.8x** |
| `HINCRBY` | 1.7 us | 38.2 us | **22.1x** |
| `LPUSH` | 2.3 us | 27.1 us | **11.8x** |
| `RPUSH` | 2.2 us | 28.1 us | **12.7x** |
| `LPOP` | 2.6 us | 25.2 us | **9.8x** |
| `RPOP` | 2.6 us | 20.1 us | **7.8x** |
| `LLEN` | 0.47 us | 19.9 us | **42.4x** |
| `LRANGE` | 4.2 us | 19.4 us | **4.6x** |
| `LINDEX` | 1.0 us | 19.9 us | **19.5x** |
| `LSET` | 1.2 us | 26.4 us | **21.9x** |
| `LREM` | 11.8 us | 52.5 us | **4.4x** |
| `LTRIM` | 1.1 us | 19.7 us | **17.6x** |
| `SADD` | 1.3 us | 25.2 us | **19.2x** |
| `SREM` | 4.0 us | 26.1 us | **6.5x** |
| `SISMEMBER` | 0.75 us | 19.3 us | **25.6x** |
| `SCARD` | 0.47 us | 19.8 us | **42.4x** |
| `SMEMBERS` | 4.3 us | 20.0 us | **4.7x** |
| `SPOP` | 8.4 us | 50.4 us | **6.0x** |
| `SRANDMEMBER` | 4.4 us | 20.0 us | **4.6x** |
| `ZADD` | 3.0 us | 24.5 us | **8.2x** |
| `ZSCORE` | 0.91 us | 19.7 us | **21.5x** |
| `ZRANGE` | 4.5 us | 19.0 us | **4.2x** |
| `ZCARD` | 0.53 us | 19.3 us | **36.5x** |
| `ZCOUNT` | 3.7 us | 19.3 us | **5.2x** |
| `ZINCRBY` | 3.1 us | 26.6 us | **8.5x** |
| `ZRANK` | 3.9 us | 19.5 us | **5.0x** |
| `ZREVRANGE` | 6.3 us | 19.2 us | **3.1x** |
| `ZPOPMIN` | 16.0 us | 53.5 us | **3.3x** |
| `ZREM` | 4.8 us | 22.2 us | **4.6x** |
| `SETBIT` | 12.5 us | 34.2 us | **2.7x** |
| `GETBIT` | 0.51 us | 24.7 us | **48.8x** |
| `BITCOUNT` | 0.41 us | 40.1 us | **98.6x** |
| `BITPOS` | 0.46 us | 32.9 us | **71.7x** |
| `PFADD` | 2.9 us | 32.7 us | **11.4x** |
| `PFCOUNT` | 8.4 us | 29.3 us | **3.5x** |
| `GEOADD` | 2.6 us | 38.4 us | **14.7x** |
| `GEODIST` | 0.98 us | 30.3 us | **30.9x** |
| `GEOPOS` | 0.70 us | 31.0 us | **44.2x** |
| `GEOHASH` | 0.77 us | 30.4 us | **39.6x** |
| `XADD` | 1.6 us | 26.6 us | **16.6x** |
| `XLEN` | 0.61 us | 19.7 us | **32.5x** |
| `XRANGE` | 4.5 us | 28.1 us | **6.3x** |
| `XREAD` | 4.3 us | 30.8 us | **7.1x** |
| `XDEL` | 3.8 us | 54.2 us | **14.3x** |
| `DEL` | 3.3 us | 24.6 us | **7.5x** |
| `EXISTS` | 0.30 us | 28.8 us | **96.4x** |
| `EXPIRE` | 0.77 us | 38.2 us | **49.9x** |
| `TTL` | 0.32 us | 26.5 us | **83.6x** |
| `JSON.SET` | 3.8 us | 25.1 us | **6.7x** |
| `JSON.GET` | 1.6 us | 30.3 us | **19.1x** |
| `JSON.DEL` | 9.4 us | 62.2 us | **6.6x** |
| `JSON.NUMINCRBY` | 3.9 us | 32.5 us | **8.4x** |
| `JSON.ARRLEN` | 1.4 us | 30.1 us | **21.7x** |
| `JSON.TYPE` | 1.4 us | 19.1 us | **13.5x** |
| `BF.ADD` | 34.9 us | 32.4 us | **0.9x** |
| `BF.EXISTS` | 0.66 us | 31.5 us | **48.0x** |
| `BF.INFO` | 0.56 us | 31.3 us | **56.1x** |
| `CF.ADD` | 2.7 us | 19.8 us | **7.4x** |
| `CF.EXISTS` | 0.86 us | 30.9 us | **36.1x** |
| `CF.DEL` | 10.3 us | 64.6 us | **6.3x** |
| `TDIGEST.ADD` | 2.6 us | 19.5 us | **7.5x** |
| `TDIGEST.QUANTILE` | 1.2 us | 19.7 us | **16.3x** |
| `TDIGEST.BYRANK` | 1.3 us | 20.0 us | **15.9x** |
| `TDIGEST.CDF` | 1.3 us | 20.0 us | **15.2x** |
| `TS.ADD` | 12.7 us | 19.4 us | **1.5x** |
| `TS.GET` | 1.6 us | 19.0 us | **12.2x** |
| `TS.RANGE` | 23.1 us | 19.0 us | **0.8x** |
| `TS.INCRBY` | 9.4 us | 19.2 us | **2.0x** |
| `FT.SEARCH` | 30.1 us | 19.2 us | **0.6x** |
| `FT.TAG` | 30.1 us | 19.8 us | **0.7x** |
| `VECTOR.KNN` | 5.3 us | 18.9 us | **3.5x** |

