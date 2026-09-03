### Ubuntu CI (GitHub Actions Runner)

#### 硬件与测试环境

CPU: AMD EPYC 9V74 80-Core Processor (4核)<br>
内存: 15.6 GB<br>
硬盘: Azure Managed Virtual Disk (Cloud Standard SSD)<br>
系统: Ubuntu 24.04.4 LTS (Linux 6.17.0-1022-azure)<br>
Rust: 1.98.1 (48a229cea 2026-09-01)<br>
Redis: v8.10.1

#### 真实物理落盘与内存占用实测 (4.3 GB 数据规模)

| 资源维度 | wedb_embed (嵌入式 LSM+LZ4) | Redis (v8.10.1 AOF持久化) | 资源节省比例 |
| :--- | :--- | :--- | :--- |
| **测试数据规模** | 5,000,000 条全格式结构化数据 | 5,000,000 条全格式结构化数据 | 14 种数据格式等比实测 |
| **原始数据载荷** | 4377 MB | 4377 MB | 真实结构化载荷 |
| **实际物理落盘大小** | **1053 MB** | **7429 MB** | **节省 86%** |
| **进程常驻内存 (RSS)** | **256 MB** | **4828 MB** | **节省 95%** |

#### wedb_embed vs Redis 核心指令性能对比

| 指令 | wedb_embed P95延迟 | Redis P95延迟 | 性能领先 |
| :--- | :--- | :--- | :--- |
| `SET` | 8.2 us | 38.3 us | **4.7x** |
| `GET` | 5.1 us | 25.5 us | **5.0x** |
| `MSET` | 54.0 us | 42.5 us | **0.8x** |
| `MGET` | 5.7 us | 33.9 us | **5.9x** |
| `INCRBY` | 0.80 us | 35.7 us | **44.4x** |
| `DECRBY` | 0.73 us | 36.1 us | **49.7x** |
| `APPEND` | 0.91 us | 35.7 us | **39.4x** |
| `STRLEN` | 0.33 us | 24.4 us | **73.6x** |
| `GETDEL` | 8.9 us | 73.0 us | **8.2x** |
| `GETRANGE` | 0.28 us | 24.6 us | **86.9x** |
| `SETRANGE` | 0.75 us | 34.6 us | **45.9x** |
| `HSET` | 2.6 us | 21.0 us | **8.0x** |
| `HGET` | 0.73 us | 20.8 us | **28.4x** |
| `HMGET` | 3.5 us | 19.2 us | **5.5x** |
| `HEXISTS` | 0.90 us | 21.0 us | **23.4x** |
| `HLEN` | 0.47 us | 24.0 us | **51.6x** |
| `HDEL` | 6.0 us | 20.7 us | **3.5x** |
| `HGETALL` | 3.7 us | 20.4 us | **5.5x** |
| `HKEYS` | 3.6 us | 30.1 us | **8.4x** |
| `HVALS` | 3.7 us | 21.2 us | **5.7x** |
| `HINCRBY` | 1.7 us | 29.5 us | **17.8x** |
| `LPUSH` | 2.4 us | 28.5 us | **12.0x** |
| `RPUSH` | 2.1 us | 36.2 us | **17.0x** |
| `LPOP` | 2.7 us | 22.5 us | **8.4x** |
| `RPOP` | 2.8 us | 35.5 us | **12.7x** |
| `LLEN` | 0.48 us | 20.9 us | **43.5x** |
| `LRANGE` | 3.8 us | 21.3 us | **5.6x** |
| `LINDEX` | 0.72 us | 20.8 us | **28.9x** |
| `LSET` | 1.2 us | 28.4 us | **23.1x** |
| `LREM` | 19.1 us | 57.3 us | **3.0x** |
| `LTRIM` | 1.1 us | 20.9 us | **18.5x** |
| `SADD` | 1.5 us | 33.0 us | **22.3x** |
| `SREM` | 5.1 us | 32.3 us | **6.3x** |
| `SISMEMBER` | 0.74 us | 25.2 us | **33.9x** |
| `SCARD` | 0.47 us | 29.2 us | **62.3x** |
| `SMEMBERS` | 3.6 us | 30.0 us | **8.4x** |
| `SPOP` | 8.3 us | 73.5 us | **8.8x** |
| `SRANDMEMBER` | 2.6 us | 25.5 us | **10.0x** |
| `ZADD` | 3.3 us | 35.4 us | **10.9x** |
| `ZSCORE` | 0.93 us | 28.6 us | **30.7x** |
| `ZRANGE` | 4.4 us | 32.2 us | **7.3x** |
| `ZCARD` | 0.56 us | 26.9 us | **47.6x** |
| `ZCOUNT` | 3.7 us | 28.9 us | **7.9x** |
| `ZINCRBY` | 3.3 us | 37.1 us | **11.3x** |
| `ZRANK` | 3.7 us | 28.8 us | **7.8x** |
| `ZREVRANGE` | 6.0 us | 32.5 us | **5.4x** |
| `ZPOPMIN` | 14.8 us | 73.0 us | **4.9x** |
| `ZREM` | 4.9 us | 32.9 us | **6.7x** |
| `SETBIT` | 16.4 us | 22.3 us | **1.4x** |
| `GETBIT` | 0.66 us | 22.0 us | **33.4x** |
| `BITCOUNT` | 0.58 us | 20.9 us | **36.4x** |
| `BITPOS` | 0.64 us | 22.5 us | **35.1x** |
| `PFADD` | 2.8 us | 21.4 us | **7.7x** |
| `PFCOUNT` | 8.7 us | 20.9 us | **2.4x** |
| `GEOADD` | 2.5 us | 25.3 us | **10.0x** |
| `GEODIST` | 1.3 us | 21.1 us | **16.5x** |
| `GEOPOS` | 0.75 us | 21.0 us | **27.9x** |
| `GEOHASH` | 0.77 us | 21.1 us | **27.2x** |
| `XADD` | 1.6 us | 38.3 us | **23.8x** |
| `XLEN` | 0.62 us | 24.9 us | **40.0x** |
| `XRANGE` | 4.2 us | 40.7 us | **9.7x** |
| `XREAD` | 4.3 us | 41.3 us | **9.7x** |
| `XDEL` | 3.7 us | 77.9 us | **21.1x** |
| `DEL` | 3.2 us | 20.6 us | **6.4x** |
| `EXISTS` | 0.27 us | 20.8 us | **77.9x** |
| `EXPIRE` | 0.80 us | 28.8 us | **35.8x** |
| `TTL` | 0.32 us | 20.7 us | **65.5x** |
| `JSON.SET` | 3.5 us | 21.8 us | **6.2x** |
| `JSON.GET` | 1.6 us | 21.3 us | **13.7x** |
| `JSON.DEL` | 8.3 us | 39.8 us | **4.8x** |
| `JSON.NUMINCRBY` | 4.1 us | 19.2 us | **4.6x** |
| `JSON.ARRLEN` | 1.4 us | 21.4 us | **15.5x** |
| `JSON.TYPE` | 1.5 us | 21.2 us | **14.2x** |
| `BF.ADD` | 59.5 us | 22.3 us | **0.4x** |
| `BF.EXISTS` | 1.0 us | 23.3 us | **23.0x** |
| `BF.INFO` | 0.56 us | 22.5 us | **39.8x** |
| `CF.ADD` | 2.6 us | 29.0 us | **11.1x** |
| `CF.EXISTS` | 0.71 us | 20.8 us | **29.4x** |
| `CF.DEL` | 9.9 us | 59.6 us | **6.0x** |
| `TDIGEST.ADD` | 2.7 us | 30.6 us | **11.3x** |
| `TDIGEST.QUANTILE` | 1.3 us | 32.3 us | **24.9x** |
| `TDIGEST.BYRANK` | 1.3 us | 29.7 us | **22.3x** |
| `TDIGEST.CDF` | 1.3 us | 30.4 us | **23.0x** |
| `TS.ADD` | 7.5 us | 29.3 us | **3.9x** |
| `TS.GET` | 1.7 us | 28.9 us | **16.8x** |
| `TS.RANGE` | 23.1 us | 31.7 us | **1.4x** |
| `TS.INCRBY` | 10.3 us | 29.0 us | **2.8x** |
| `FT.SEARCH` | 27.5 us | 32.7 us | **1.2x** |
| `FT.TAG` | 27.6 us | 31.1 us | **1.1x** |
| `VECTOR.KNN` | 4.4 us | 32.1 us | **7.3x** |

