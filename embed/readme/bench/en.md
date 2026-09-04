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
| **Physical Disk Footprint** | **1053 MB** | **7545 MB** | **Saves 86%** |
| **Resident Memory (RSS)** | **275 MB** | **4818 MB** | **Saves 94%** |

#### wedb_embed vs Redis Core Command Benchmark

| Command | wedb_embed P95 Latency | Redis P95 Latency | Speedup |
| :--- | :--- | :--- | :--- |
| `SET` | 7.7 us | 28.2 us | **3.7x** |
| `GET` | 5.0 us | 20.6 us | **4.1x** |
| `MSET` | 52.2 us | 31.6 us | **0.6x** |
| `MGET` | 5.8 us | 19.1 us | **3.3x** |
| `INCRBY` | 0.86 us | 19.9 us | **23.3x** |
| `DECRBY` | 0.72 us | 19.8 us | **27.4x** |
| `APPEND` | 0.96 us | 22.3 us | **23.2x** |
| `STRLEN` | 0.30 us | 20.1 us | **67.3x** |
| `GETDEL` | 8.7 us | 49.1 us | **5.6x** |
| `GETRANGE` | 0.30 us | 20.5 us | **68.5x** |
| `SETRANGE` | 0.75 us | 24.4 us | **32.5x** |
| `HSET` | 2.6 us | 36.7 us | **14.3x** |
| `HGET` | 0.76 us | 30.5 us | **39.9x** |
| `HMGET` | 3.6 us | 34.5 us | **9.5x** |
| `HEXISTS` | 0.69 us | 30.2 us | **44.0x** |
| `HLEN` | 0.47 us | 30.1 us | **64.6x** |
| `HDEL` | 5.9 us | 35.2 us | **5.9x** |
| `HGETALL` | 3.8 us | 33.5 us | **8.7x** |
| `HKEYS` | 3.6 us | 31.3 us | **8.8x** |
| `HVALS` | 3.7 us | 33.3 us | **9.0x** |
| `HINCRBY` | 1.7 us | 40.6 us | **24.4x** |
| `LPUSH` | 2.3 us | 36.8 us | **15.7x** |
| `RPUSH` | 2.4 us | 35.1 us | **14.6x** |
| `LPOP` | 2.6 us | 40.9 us | **15.4x** |
| `RPOP` | 2.7 us | 35.0 us | **13.1x** |
| `LLEN` | 0.50 us | 28.9 us | **57.2x** |
| `LRANGE` | 3.9 us | 29.7 us | **7.6x** |
| `LINDEX` | 0.74 us | 30.0 us | **40.5x** |
| `LSET` | 1.3 us | 37.1 us | **29.5x** |
| `LREM` | 18.1 us | 73.7 us | **4.1x** |
| `LTRIM` | 1.2 us | 29.2 us | **25.4x** |
| `SADD` | 1.5 us | 24.0 us | **16.2x** |
| `SREM` | 5.1 us | 21.4 us | **4.2x** |
| `SISMEMBER` | 0.75 us | 19.9 us | **26.4x** |
| `SCARD` | 0.48 us | 19.8 us | **40.9x** |
| `SMEMBERS` | 3.7 us | 19.9 us | **5.4x** |
| `SPOP` | 8.8 us | 50.8 us | **5.8x** |
| `SRANDMEMBER` | 2.8 us | 20.3 us | **7.3x** |
| `ZADD` | 3.0 us | 24.3 us | **8.2x** |
| `ZSCORE` | 0.83 us | 20.0 us | **24.1x** |
| `ZRANGE` | 4.5 us | 20.0 us | **4.5x** |
| `ZCARD` | 0.48 us | 19.9 us | **41.2x** |
| `ZCOUNT` | 3.8 us | 20.2 us | **5.4x** |
| `ZINCRBY` | 3.0 us | 26.4 us | **8.8x** |
| `ZRANK` | 3.9 us | 19.7 us | **5.1x** |
| `ZREVRANGE` | 5.8 us | 19.8 us | **3.4x** |
| `ZPOPMIN` | 8.4 us | 50.9 us | **6.0x** |
| `ZREM` | 5.3 us | 20.5 us | **3.9x** |
| `SETBIT` | 12.3 us | 37.5 us | **3.0x** |
| `GETBIT` | 0.49 us | 30.3 us | **61.6x** |
| `BITCOUNT` | 0.41 us | 26.1 us | **63.1x** |
| `BITPOS` | 0.47 us | 25.8 us | **55.0x** |
| `PFADD` | 2.8 us | 35.4 us | **12.5x** |
| `PFCOUNT` | 8.4 us | 29.4 us | **3.5x** |
| `GEOADD` | 2.4 us | 22.4 us | **9.2x** |
| `GEODIST` | 1.0 us | 33.9 us | **33.9x** |
| `GEOPOS` | 0.74 us | 30.8 us | **41.4x** |
| `GEOHASH` | 0.79 us | 33.2 us | **41.9x** |
| `XADD` | 1.5 us | 26.8 us | **17.4x** |
| `XLEN` | 0.57 us | 19.4 us | **33.9x** |
| `XRANGE` | 4.2 us | 27.4 us | **6.5x** |
| `XREAD` | 4.2 us | 29.1 us | **6.9x** |
| `XDEL` | 3.5 us | 54.2 us | **15.4x** |
| `DEL` | 3.2 us | 21.2 us | **6.7x** |
| `EXISTS` | 0.28 us | 20.9 us | **75.1x** |
| `EXPIRE` | 0.78 us | 29.1 us | **37.3x** |
| `TTL` | 0.33 us | 20.6 us | **62.8x** |
| `JSON.SET` | 3.5 us | 33.8 us | **9.6x** |
| `JSON.GET` | 1.6 us | 30.2 us | **18.3x** |
| `JSON.DEL` | 8.4 us | 64.2 us | **7.6x** |
| `JSON.NUMINCRBY` | 4.1 us | 33.0 us | **8.0x** |
| `JSON.ARRLEN` | 1.4 us | 30.7 us | **22.2x** |
| `JSON.TYPE` | 1.5 us | 30.7 us | **20.8x** |
| `BF.ADD` | 36.4 us | 33.5 us | **0.9x** |
| `BF.EXISTS` | 0.68 us | 31.7 us | **46.4x** |
| `BF.INFO` | 0.39 us | 28.7 us | **74.1x** |
| `CF.ADD` | 2.7 us | 33.6 us | **12.3x** |
| `CF.EXISTS` | 0.72 us | 20.8 us | **28.9x** |
| `CF.DEL` | 10.3 us | 37.0 us | **3.6x** |
| `TDIGEST.ADD` | 2.8 us | 19.0 us | **6.8x** |
| `TDIGEST.QUANTILE` | 1.3 us | 19.7 us | **15.1x** |
| `TDIGEST.BYRANK` | 1.2 us | 19.7 us | **16.2x** |
| `TDIGEST.CDF` | 1.3 us | 19.3 us | **14.7x** |
| `TS.ADD` | 7.6 us | 18.9 us | **2.5x** |
| `TS.GET` | 1.5 us | 18.2 us | **12.2x** |
| `TS.RANGE` | 23.3 us | 19.1 us | **0.8x** |
| `TS.INCRBY` | 9.4 us | 21.1 us | **2.3x** |
| `FT.SEARCH` | 28.2 us | 21.7 us | **0.8x** |
| `FT.TAG` | 27.6 us | 19.8 us | **0.7x** |
| `VECTOR.KNN` | 3.9 us | 19.1 us | **4.9x** |

