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
| **Physical Disk Footprint** | **1053 MB** | **7720 MB** | **Saves 86%** |
| **Resident Memory (RSS)** | **262 MB** | **4813 MB** | **Saves 95%** |

#### wedb_embed vs Redis Core Command Benchmark

| Command | wedb_embed P95 Latency | Redis P95 Latency | Speedup |
| :--- | :--- | :--- | :--- |
| `SET` | 7.3 us | 41.3 us | **5.7x** |
| `GET` | 5.2 us | 31.3 us | **6.1x** |
| `MSET` | 50.1 us | 46.9 us | **0.9x** |
| `MGET` | 5.3 us | 39.3 us | **7.4x** |
| `INCRBY` | 0.73 us | 40.4 us | **55.1x** |
| `DECRBY` | 0.69 us | 40.6 us | **59.2x** |
| `APPEND` | 0.84 us | 40.3 us | **48.2x** |
| `STRLEN` | 0.27 us | 31.1 us | **116.9x** |
| `GETDEL` | 8.0 us | 82.7 us | **10.3x** |
| `GETRANGE` | 0.36 us | 32.0 us | **88.2x** |
| `SETRANGE` | 0.93 us | 40.6 us | **43.9x** |
| `HSET` | 2.5 us | 44.8 us | **17.6x** |
| `HGET` | 0.69 us | 34.4 us | **49.6x** |
| `HMGET` | 3.3 us | 43.4 us | **13.2x** |
| `HEXISTS` | 0.63 us | 34.0 us | **54.4x** |
| `HLEN` | 0.44 us | 33.6 us | **76.5x** |
| `HDEL` | 4.1 us | 40.4 us | **9.8x** |
| `HGETALL` | 3.3 us | 40.1 us | **12.2x** |
| `HKEYS` | 3.0 us | 38.4 us | **12.6x** |
| `HVALS` | 3.6 us | 39.9 us | **11.2x** |
| `HINCRBY` | 1.7 us | 44.8 us | **26.8x** |
| `LPUSH` | 2.0 us | 40.4 us | **19.9x** |
| `RPUSH` | 2.0 us | 40.5 us | **19.9x** |
| `LPOP` | 2.6 us | 54.9 us | **21.4x** |
| `RPOP` | 2.5 us | 52.7 us | **20.8x** |
| `LLEN` | 0.47 us | 31.5 us | **67.6x** |
| `LRANGE` | 3.2 us | 34.0 us | **10.7x** |
| `LINDEX` | 0.69 us | 34.2 us | **49.4x** |
| `LSET` | 1.2 us | 41.1 us | **34.8x** |
| `LREM` | 16.9 us | 83.0 us | **4.9x** |
| `LTRIM` | 1.1 us | 34.6 us | **30.6x** |
| `SADD` | 1.4 us | 39.3 us | **28.1x** |
| `SREM` | 3.6 us | 39.5 us | **11.0x** |
| `SISMEMBER` | 0.69 us | 34.0 us | **49.0x** |
| `SCARD` | 0.46 us | 32.4 us | **70.3x** |
| `SMEMBERS` | 3.4 us | 33.9 us | **10.1x** |
| `SPOP` | 5.8 us | 84.0 us | **14.4x** |
| `SRANDMEMBER` | 3.0 us | 31.4 us | **10.5x** |
| `ZADD` | 2.9 us | 41.8 us | **14.4x** |
| `ZSCORE` | 0.75 us | 34.6 us | **45.8x** |
| `ZRANGE` | 3.8 us | 37.2 us | **9.8x** |
| `ZCARD` | 0.49 us | 31.3 us | **64.5x** |
| `ZCOUNT` | 3.2 us | 34.3 us | **10.6x** |
| `ZINCRBY` | 3.0 us | 42.5 us | **14.3x** |
| `ZRANK` | 3.3 us | 33.9 us | **10.1x** |
| `ZREVRANGE` | 5.8 us | 36.2 us | **6.3x** |
| `ZPOPMIN` | 13.3 us | 84.4 us | **6.4x** |
| `ZREM` | 4.8 us | 37.9 us | **7.8x** |
| `SETBIT` | 11.4 us | 44.3 us | **3.9x** |
| `GETBIT` | 0.44 us | 33.7 us | **76.5x** |
| `BITCOUNT` | 0.36 us | 25.8 us | **71.2x** |
| `BITPOS` | 0.44 us | 22.2 us | **50.0x** |
| `PFADD` | 2.8 us | 40.7 us | **14.7x** |
| `PFCOUNT` | 8.5 us | 33.5 us | **4.0x** |
| `GEOADD` | 2.4 us | 45.9 us | **18.8x** |
| `GEODIST` | 0.94 us | 36.5 us | **39.0x** |
| `GEOPOS` | 0.71 us | 37.1 us | **52.4x** |
| `GEOHASH` | 0.74 us | 36.4 us | **49.0x** |
| `XADD` | 1.8 us | 43.1 us | **24.2x** |
| `XLEN` | 0.56 us | 30.9 us | **55.3x** |
| `XRANGE` | 3.7 us | 44.1 us | **11.9x** |
| `XREAD` | 3.8 us | 45.0 us | **11.9x** |
| `XDEL` | 3.7 us | 87.0 us | **23.6x** |
| `DEL` | 3.0 us | 36.3 us | **12.1x** |
| `EXISTS` | 0.25 us | 33.8 us | **133.2x** |
| `EXPIRE` | 0.76 us | 44.8 us | **59.2x** |
| `TTL` | 0.29 us | 33.1 us | **113.0x** |
| `JSON.SET` | 3.7 us | 37.1 us | **10.1x** |
| `JSON.GET` | 1.6 us | 33.2 us | **20.8x** |
| `JSON.DEL` | 8.2 us | 73.6 us | **9.0x** |
| `JSON.NUMINCRBY` | 3.9 us | 34.5 us | **8.8x** |
| `JSON.ARRLEN` | 1.4 us | 35.6 us | **25.7x** |
| `JSON.TYPE` | 1.4 us | 35.0 us | **24.3x** |
| `BF.ADD` | 31.6 us | 35.6 us | **1.1x** |
| `BF.EXISTS` | 1.0 us | 36.8 us | **36.8x** |
| `BF.INFO` | 0.36 us | 36.3 us | **100.4x** |
| `CF.ADD` | 2.4 us | 37.3 us | **15.5x** |
| `CF.EXISTS` | 0.66 us | 36.5 us | **55.7x** |
| `CF.DEL` | 9.8 us | 73.1 us | **7.4x** |
| `TDIGEST.ADD` | 2.6 us | 36.4 us | **14.3x** |
| `TDIGEST.QUANTILE` | 1.2 us | 34.0 us | **28.4x** |
| `TDIGEST.BYRANK` | 1.1 us | 33.9 us | **30.0x** |
| `TDIGEST.CDF` | 1.2 us | 34.0 us | **27.3x** |
| `TS.ADD` | 7.3 us | 36.1 us | **5.0x** |
| `TS.GET` | 1.5 us | 34.3 us | **22.2x** |
| `TS.RANGE` | 20.5 us | 34.5 us | **1.7x** |
| `TS.INCRBY` | 8.9 us | 34.6 us | **3.9x** |
| `FT.SEARCH` | 28.6 us | 34.4 us | **1.2x** |
| `FT.TAG` | 28.2 us | 33.9 us | **1.2x** |
| `VECTOR.KNN` | 8.4 us | 38.5 us | **4.6x** |

