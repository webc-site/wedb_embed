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
| **实际物理落盘大小** | **1053 MB** | **7526 MB** | **节省 86%** |
| **进程常驻内存 (RSS)** | **277 MB** | **4809 MB** | **节省 94%** |

#### wedb_embed vs Redis 核心指令性能对比

| 指令 | wedb_embed P95延迟 | Redis P95延迟 | 性能领先 |
| :--- | :--- | :--- | :--- |
| `SET` | 10.7 us | 37.8 us | **3.5x** |
| `GET` | 5.5 us | 25.2 us | **4.6x** |
| `MSET` | 51.5 us | 42.0 us | **0.8x** |
| `MGET` | 5.4 us | 34.5 us | **6.4x** |
| `INCRBY` | 0.78 us | 35.8 us | **46.2x** |
| `DECRBY` | 0.66 us | 36.9 us | **56.4x** |
| `APPEND` | 0.87 us | 37.2 us | **43.0x** |
| `STRLEN` | 0.30 us | 27.9 us | **93.3x** |
| `GETDEL` | 8.6 us | 75.0 us | **8.7x** |
| `GETRANGE` | 0.36 us | 29.0 us | **80.4x** |
| `SETRANGE` | 0.94 us | 37.1 us | **39.5x** |
| `HSET` | 2.6 us | 35.9 us | **13.9x** |
| `HGET` | 0.74 us | 29.5 us | **40.2x** |
| `HMGET` | 3.4 us | 34.7 us | **10.2x** |
| `HEXISTS` | 0.65 us | 30.4 us | **46.6x** |
| `HLEN` | 0.46 us | 28.8 us | **62.2x** |
| `HDEL` | 4.4 us | 35.8 us | **8.1x** |
| `HGETALL` | 3.5 us | 32.9 us | **9.3x** |
| `HKEYS` | 3.5 us | 32.1 us | **9.1x** |
| `HVALS` | 3.6 us | 32.2 us | **9.0x** |
| `HINCRBY` | 1.7 us | 37.7 us | **22.7x** |
| `LPUSH` | 2.2 us | 27.7 us | **12.4x** |
| `RPUSH` | 2.1 us | 28.2 us | **13.2x** |
| `LPOP` | 2.6 us | 26.8 us | **10.2x** |
| `RPOP` | 2.6 us | 27.6 us | **10.5x** |
| `LLEN` | 0.52 us | 19.7 us | **38.2x** |
| `LRANGE` | 3.7 us | 19.9 us | **5.4x** |
| `LINDEX` | 0.72 us | 19.9 us | **27.8x** |
| `LSET` | 1.2 us | 27.8 us | **22.3x** |
| `LREM` | 18.3 us | 55.6 us | **3.0x** |
| `LTRIM` | 1.2 us | 19.8 us | **16.8x** |
| `SADD` | 1.5 us | 35.5 us | **23.0x** |
| `SREM` | 4.1 us | 35.8 us | **8.7x** |
| `SISMEMBER` | 0.72 us | 29.6 us | **41.2x** |
| `SCARD` | 0.48 us | 29.7 us | **61.8x** |
| `SMEMBERS` | 3.7 us | 30.8 us | **8.3x** |
| `SPOP` | 6.6 us | 73.8 us | **11.2x** |
| `SRANDMEMBER` | 2.3 us | 29.6 us | **12.8x** |
| `ZADD` | 3.1 us | 35.7 us | **11.5x** |
| `ZSCORE` | 0.95 us | 28.1 us | **29.7x** |
| `ZRANGE` | 4.4 us | 32.1 us | **7.3x** |
| `ZCARD` | 0.51 us | 28.2 us | **55.5x** |
| `ZCOUNT` | 3.9 us | 28.6 us | **7.4x** |
| `ZINCRBY` | 3.2 us | 36.0 us | **11.4x** |
| `ZRANK` | 3.5 us | 28.6 us | **8.1x** |
| `ZREVRANGE` | 6.1 us | 32.0 us | **5.3x** |
| `ZPOPMIN` | 15.3 us | 69.8 us | **4.5x** |
| `ZREM` | 5.4 us | 34.4 us | **6.4x** |
| `SETBIT` | 12.7 us | 37.6 us | **3.0x** |
| `GETBIT` | 0.49 us | 31.9 us | **64.9x** |
| `BITCOUNT` | 0.41 us | 29.4 us | **71.7x** |
| `BITPOS` | 0.48 us | 31.6 us | **65.5x** |
| `PFADD` | 2.8 us | 34.5 us | **12.2x** |
| `PFCOUNT` | 8.4 us | 28.6 us | **3.4x** |
| `GEOADD` | 2.6 us | 38.2 us | **14.6x** |
| `GEODIST` | 1.0 us | 29.5 us | **29.0x** |
| `GEOPOS` | 0.77 us | 29.9 us | **39.1x** |
| `GEOHASH` | 0.82 us | 30.0 us | **36.5x** |
| `XADD` | 1.6 us | 38.1 us | **23.4x** |
| `XLEN` | 0.61 us | 28.8 us | **47.6x** |
| `XRANGE` | 4.5 us | 39.0 us | **8.6x** |
| `XREAD` | 4.5 us | 40.5 us | **9.0x** |
| `XDEL` | 3.8 us | 76.1 us | **20.1x** |
| `DEL` | 3.2 us | 20.8 us | **6.6x** |
| `EXISTS` | 0.26 us | 20.1 us | **77.6x** |
| `EXPIRE` | 0.81 us | 27.8 us | **34.2x** |
| `TTL` | 0.31 us | 28.4 us | **90.5x** |
| `JSON.SET` | 3.7 us | 20.1 us | **5.5x** |
| `JSON.GET` | 1.5 us | 28.7 us | **18.6x** |
| `JSON.DEL` | 8.6 us | 61.8 us | **7.2x** |
| `JSON.NUMINCRBY` | 4.1 us | 31.9 us | **7.9x** |
| `JSON.ARRLEN` | 1.4 us | 31.4 us | **22.9x** |
| `JSON.TYPE` | 1.4 us | 19.7 us | **13.7x** |
| `BF.ADD` | 34.4 us | 33.3 us | **1.0x** |
| `BF.EXISTS` | 0.65 us | 33.0 us | **50.6x** |
| `BF.INFO` | 0.38 us | 24.7 us | **64.8x** |
| `CF.ADD` | 3.4 us | 22.6 us | **6.6x** |
| `CF.EXISTS` | 0.71 us | 20.6 us | **29.0x** |
| `CF.DEL` | 12.2 us | 40.5 us | **3.3x** |
| `TDIGEST.ADD` | 2.5 us | 29.4 us | **11.6x** |
| `TDIGEST.QUANTILE` | 1.2 us | 30.9 us | **26.3x** |
| `TDIGEST.BYRANK` | 1.3 us | 29.0 us | **22.8x** |
| `TDIGEST.CDF` | 1.4 us | 28.2 us | **20.8x** |
| `TS.ADD` | 6.9 us | 30.6 us | **4.4x** |
| `TS.GET` | 1.6 us | 28.4 us | **17.9x** |
| `TS.RANGE` | 23.5 us | 31.4 us | **1.3x** |
| `TS.INCRBY` | 9.3 us | 31.5 us | **3.4x** |
| `FT.SEARCH` | 29.4 us | 22.9 us | **0.8x** |
| `FT.TAG` | 29.3 us | 31.1 us | **1.1x** |
| `VECTOR.KNN` | 4.1 us | 33.0 us | **8.1x** |

