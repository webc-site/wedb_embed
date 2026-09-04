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
| **实际物理落盘大小** | **1053 MB** | **7703 MB** | **节省 86%** |
| **进程常驻内存 (RSS)** | **279 MB** | **4815 MB** | **节省 94%** |

#### wedb_embed vs Redis 核心指令性能对比

| 指令 | wedb_embed P95延迟 | Redis P95延迟 | 性能领先 |
| :--- | :--- | :--- | :--- |
| `SET` | 7.3 us | 40.9 us | **5.6x** |
| `GET` | 4.9 us | 33.4 us | **6.9x** |
| `MSET` | 49.7 us | 46.4 us | **0.9x** |
| `MGET` | 5.4 us | 39.7 us | **7.4x** |
| `INCRBY` | 0.81 us | 40.0 us | **49.3x** |
| `DECRBY` | 0.65 us | 39.9 us | **61.0x** |
| `APPEND` | 0.85 us | 39.9 us | **47.2x** |
| `STRLEN` | 0.30 us | 31.1 us | **104.9x** |
| `GETDEL` | 8.4 us | 81.2 us | **9.7x** |
| `GETRANGE` | 0.41 us | 31.0 us | **76.2x** |
| `SETRANGE` | 1.1 us | 40.6 us | **38.1x** |
| `HSET` | 2.7 us | 42.9 us | **15.9x** |
| `HGET` | 0.76 us | 35.2 us | **46.6x** |
| `HMGET` | 3.4 us | 40.5 us | **12.0x** |
| `HEXISTS` | 0.64 us | 35.4 us | **54.8x** |
| `HLEN` | 0.45 us | 31.8 us | **70.4x** |
| `HDEL` | 4.2 us | 39.8 us | **9.6x** |
| `HGETALL` | 3.4 us | 37.9 us | **11.2x** |
| `HKEYS` | 3.2 us | 34.8 us | **10.7x** |
| `HVALS` | 3.5 us | 36.0 us | **10.4x** |
| `HINCRBY` | 1.7 us | 41.8 us | **24.8x** |
| `LPUSH` | 2.1 us | 40.9 us | **19.4x** |
| `RPUSH` | 2.0 us | 40.2 us | **20.2x** |
| `LPOP` | 2.5 us | 55.3 us | **21.7x** |
| `RPOP` | 2.5 us | 50.5 us | **20.3x** |
| `LLEN` | 0.46 us | 32.0 us | **69.2x** |
| `LRANGE` | 3.6 us | 34.7 us | **9.7x** |
| `LINDEX` | 0.70 us | 32.2 us | **45.9x** |
| `LSET` | 1.2 us | 41.6 us | **34.7x** |
| `LREM` | 17.9 us | 82.4 us | **4.6x** |
| `LTRIM` | 1.1 us | 33.9 us | **30.3x** |
| `SADD` | 1.4 us | 38.5 us | **27.3x** |
| `SREM` | 3.5 us | 38.0 us | **10.9x** |
| `SISMEMBER` | 0.70 us | 30.9 us | **43.9x** |
| `SCARD` | 0.48 us | 30.9 us | **64.9x** |
| `SMEMBERS` | 3.4 us | 36.0 us | **10.6x** |
| `SPOP` | 5.9 us | 82.0 us | **14.0x** |
| `SRANDMEMBER` | 2.8 us | 30.9 us | **11.2x** |
| `ZADD` | 3.1 us | 28.3 us | **9.3x** |
| `ZSCORE` | 0.82 us | 19.5 us | **23.8x** |
| `ZRANGE` | 3.9 us | 18.8 us | **4.8x** |
| `ZCARD` | 0.50 us | 19.9 us | **39.7x** |
| `ZCOUNT` | 3.3 us | 19.4 us | **5.9x** |
| `ZINCRBY` | 3.0 us | 31.0 us | **10.2x** |
| `ZRANK` | 3.6 us | 19.8 us | **5.5x** |
| `ZREVRANGE` | 5.9 us | 19.7 us | **3.4x** |
| `ZPOPMIN` | 14.1 us | 60.4 us | **4.3x** |
| `ZREM` | 5.0 us | 25.4 us | **5.1x** |
| `SETBIT` | 11.4 us | 43.4 us | **3.8x** |
| `GETBIT` | 0.46 us | 20.8 us | **45.0x** |
| `BITCOUNT` | 0.55 us | 34.1 us | **61.8x** |
| `BITPOS` | 0.66 us | 22.6 us | **34.5x** |
| `PFADD` | 2.7 us | 39.2 us | **14.5x** |
| `PFCOUNT` | 8.2 us | 32.2 us | **3.9x** |
| `GEOADD` | 2.5 us | 45.4 us | **17.9x** |
| `GEODIST` | 0.97 us | 35.6 us | **36.8x** |
| `GEOPOS` | 0.71 us | 35.4 us | **49.7x** |
| `GEOHASH` | 0.75 us | 35.5 us | **47.1x** |
| `XADD` | 1.6 us | 42.2 us | **26.8x** |
| `XLEN` | 0.59 us | 30.5 us | **51.8x** |
| `XRANGE` | 3.8 us | 43.4 us | **11.6x** |
| `XREAD` | 3.9 us | 44.6 us | **11.5x** |
| `XDEL` | 3.9 us | 84.9 us | **21.8x** |
| `DEL` | 3.0 us | 35.1 us | **11.9x** |
| `EXISTS` | 0.25 us | 38.1 us | **154.9x** |
| `EXPIRE` | 0.73 us | 45.0 us | **61.7x** |
| `TTL` | 0.31 us | 33.6 us | **108.3x** |
| `JSON.SET` | 3.5 us | 37.3 us | **10.6x** |
| `JSON.GET` | 1.6 us | 34.5 us | **21.3x** |
| `JSON.DEL` | 8.5 us | 71.6 us | **8.5x** |
| `JSON.NUMINCRBY` | 3.9 us | 37.2 us | **9.4x** |
| `JSON.ARRLEN` | 1.4 us | 34.6 us | **25.2x** |
| `JSON.TYPE` | 1.4 us | 34.9 us | **24.2x** |
| `BF.ADD` | 31.6 us | 37.0 us | **1.2x** |
| `BF.EXISTS` | 0.65 us | 36.9 us | **57.0x** |
| `BF.INFO` | 0.36 us | 37.0 us | **102.9x** |
| `CF.ADD` | 2.5 us | 37.1 us | **14.9x** |
| `CF.EXISTS` | 0.68 us | 36.1 us | **53.1x** |
| `CF.DEL` | 9.9 us | 74.3 us | **7.5x** |
| `TDIGEST.ADD` | 2.6 us | 34.1 us | **12.9x** |
| `TDIGEST.QUANTILE` | 1.3 us | 21.9 us | **17.3x** |
| `TDIGEST.BYRANK` | 1.2 us | 33.9 us | **28.8x** |
| `TDIGEST.CDF` | 1.3 us | 33.6 us | **25.6x** |
| `TS.ADD` | 6.9 us | 19.3 us | **2.8x** |
| `TS.GET` | 1.5 us | 19.6 us | **13.2x** |
| `TS.RANGE` | 20.8 us | 19.6 us | **0.9x** |
| `TS.INCRBY` | 9.8 us | 19.2 us | **2.0x** |
| `FT.SEARCH` | 28.4 us | 34.3 us | **1.2x** |
| `FT.TAG` | 28.5 us | 33.9 us | **1.2x** |
| `VECTOR.KNN` | 5.5 us | 20.5 us | **3.7x** |

