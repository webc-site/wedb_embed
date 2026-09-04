### Ubuntu CI (GitHub Actions Runner)

#### 硬件与测试环境

CPU: AMD EPYC 7763 64-Core Processor (4核)<br>
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
| **实际物理落盘大小** | **1053 MB** | **7558 MB** | **节省 86%** |
| **进程常驻内存 (RSS)** | **283 MB** | **4821 MB** | **节省 94%** |

#### wedb_embed vs Redis 核心指令性能对比

| 指令 | wedb_embed P95延迟 | Redis P95延迟 | 性能领先 |
| :--- | :--- | :--- | :--- |
| `SET` | 7.2 us | 27.4 us | **3.8x** |
| `GET` | 8.3 us | 25.9 us | **3.1x** |
| `MSET` | 49.0 us | 32.5 us | **0.7x** |
| `MGET` | 4.8 us | 23.0 us | **4.8x** |
| `INCRBY` | 0.68 us | 27.0 us | **39.9x** |
| `DECRBY` | 1.2 us | 28.9 us | **25.1x** |
| `APPEND` | 1.3 us | 27.8 us | **21.6x** |
| `STRLEN` | 0.32 us | 20.6 us | **65.2x** |
| `GETDEL` | 8.0 us | 55.6 us | **7.0x** |
| `GETRANGE` | 0.26 us | 23.3 us | **91.0x** |
| `SETRANGE` | 1.3 us | 27.7 us | **22.1x** |
| `HSET` | 2.7 us | 27.8 us | **10.4x** |
| `HGET` | 0.65 us | 20.4 us | **31.3x** |
| `HMGET` | 3.0 us | 19.3 us | **6.3x** |
| `HEXISTS` | 0.64 us | 20.9 us | **32.6x** |
| `HLEN` | 0.41 us | 20.1 us | **48.9x** |
| `HDEL` | 4.7 us | 25.5 us | **5.4x** |
| `HGETALL` | 3.3 us | 20.6 us | **6.2x** |
| `HKEYS` | 3.1 us | 20.6 us | **6.6x** |
| `HVALS` | 3.1 us | 20.6 us | **6.6x** |
| `HINCRBY` | 1.9 us | 27.4 us | **14.7x** |
| `LPUSH` | 3.1 us | 27.4 us | **8.7x** |
| `RPUSH` | 2.8 us | 26.2 us | **9.5x** |
| `LPOP` | 2.4 us | 27.3 us | **11.1x** |
| `RPOP` | 2.5 us | 27.2 us | **11.1x** |
| `LLEN` | 0.43 us | 20.1 us | **47.3x** |
| `LRANGE` | 3.4 us | 20.3 us | **5.9x** |
| `LINDEX` | 0.63 us | 20.2 us | **32.1x** |
| `LSET` | 1.0 us | 27.5 us | **26.6x** |
| `LREM` | 16.5 us | 55.0 us | **3.3x** |
| `LTRIM` | 1.1 us | 20.4 us | **19.0x** |
| `SADD` | 1.6 us | 44.1 us | **27.8x** |
| `SREM` | 4.3 us | 35.5 us | **8.3x** |
| `SISMEMBER` | 0.63 us | 24.5 us | **38.6x** |
| `SCARD` | 0.43 us | 25.0 us | **58.8x** |
| `SMEMBERS` | 3.0 us | 33.7 us | **11.1x** |
| `SPOP` | 7.2 us | 80.0 us | **11.1x** |
| `SRANDMEMBER` | 2.4 us | 24.2 us | **10.1x** |
| `ZADD` | 2.9 us | 27.0 us | **9.3x** |
| `ZSCORE` | 0.85 us | 20.4 us | **23.9x** |
| `ZRANGE` | 4.0 us | 19.3 us | **4.9x** |
| `ZCARD` | 0.49 us | 19.6 us | **40.2x** |
| `ZCOUNT` | 3.1 us | 20.4 us | **6.5x** |
| `ZINCRBY` | 3.0 us | 29.9 us | **9.9x** |
| `ZRANK` | 3.4 us | 20.1 us | **5.9x** |
| `ZREVRANGE` | 6.2 us | 20.1 us | **3.3x** |
| `ZPOPMIN` | 8.1 us | 57.4 us | **7.1x** |
| `ZREM` | 4.7 us | 22.4 us | **4.8x** |
| `SETBIT` | 11.2 us | 26.3 us | **2.3x** |
| `GETBIT` | 0.47 us | 19.1 us | **40.9x** |
| `BITCOUNT` | 0.39 us | 34.3 us | **88.4x** |
| `BITPOS` | 0.43 us | 19.1 us | **44.8x** |
| `PFADD` | 2.6 us | 24.4 us | **9.3x** |
| `PFCOUNT` | 8.1 us | 19.9 us | **2.5x** |
| `GEOADD` | 2.7 us | 27.9 us | **10.4x** |
| `GEODIST` | 1.1 us | 20.3 us | **18.4x** |
| `GEOPOS` | 1.1 us | 20.6 us | **18.2x** |
| `GEOHASH` | 1.2 us | 20.4 us | **17.3x** |
| `XADD` | 1.6 us | 29.1 us | **17.9x** |
| `XLEN` | 0.59 us | 20.1 us | **33.8x** |
| `XRANGE` | 3.9 us | 30.3 us | **7.7x** |
| `XREAD` | 3.9 us | 31.4 us | **8.0x** |
| `XDEL` | 3.8 us | 59.1 us | **15.6x** |
| `DEL` | 5.0 us | 20.4 us | **4.1x** |
| `EXISTS` | 0.25 us | 20.5 us | **83.1x** |
| `EXPIRE` | 1.2 us | 27.9 us | **24.1x** |
| `TTL` | 0.28 us | 20.4 us | **73.8x** |
| `JSON.SET` | 3.7 us | 20.0 us | **5.3x** |
| `JSON.GET` | 1.5 us | 20.4 us | **13.5x** |
| `JSON.DEL` | 7.0 us | 39.0 us | **5.6x** |
| `JSON.NUMINCRBY` | 3.7 us | 20.0 us | **5.4x** |
| `JSON.ARRLEN` | 1.3 us | 20.7 us | **15.5x** |
| `JSON.TYPE` | 1.4 us | 20.4 us | **14.4x** |
| `BF.ADD` | 32.1 us | 19.2 us | **0.6x** |
| `BF.EXISTS` | 0.64 us | 18.5 us | **28.7x** |
| `BF.INFO` | 0.36 us | 18.9 us | **52.0x** |
| `CF.ADD` | 2.5 us | 20.4 us | **8.1x** |
| `CF.EXISTS` | 0.64 us | 20.3 us | **31.6x** |
| `CF.DEL` | 9.5 us | 38.7 us | **4.1x** |
| `TDIGEST.ADD` | 2.6 us | 20.1 us | **7.6x** |
| `TDIGEST.QUANTILE` | 1.1 us | 18.9 us | **17.5x** |
| `TDIGEST.BYRANK` | 1.1 us | 20.8 us | **19.2x** |
| `TDIGEST.CDF` | 1.2 us | 18.6 us | **15.4x** |
| `TS.ADD` | 7.5 us | 19.1 us | **2.6x** |
| `TS.GET` | 1.5 us | 19.0 us | **13.0x** |
| `TS.RANGE` | 20.5 us | 18.8 us | **0.9x** |
| `TS.INCRBY` | 10.0 us | 18.8 us | **1.9x** |
| `FT.SEARCH` | 32.7 us | 20.0 us | **0.6x** |
| `FT.TAG` | 29.2 us | 36.6 us | **1.3x** |
| `VECTOR.KNN` | 4.1 us | 19.0 us | **4.6x** |

