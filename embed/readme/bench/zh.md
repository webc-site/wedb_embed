### Ubuntu CI (GitHub Actions Runner)

#### 硬件与测试环境

CPU: Intel(R) Xeon(R) 6973P-C (4核)<br>
内存: 15.6 GB<br>
硬盘: Azure Managed Virtual Disk (Cloud Standard SSD)<br>
系统: Ubuntu 24.04.4 LTS (Linux 6.17.0-1022-azure)<br>
Rust: 1.98.0 (88d9e12ae 2026-08-18)<br>
Redis: v8.10.1

#### 真实物理落盘与内存占用实测 (4.3 GB 数据规模)

| 资源维度 | wedb_embed (嵌入式 LSM+LZ4) | Redis (v8.10.1 AOF持久化) | 资源节省比例 |
| :--- | :--- | :--- | :--- |
| **测试数据规模** | 5,000,000 条全格式结构化数据 | 5,000,000 条全格式结构化数据 | 14 种数据格式等比实测 |
| **原始数据载荷** | 4377 MB | 4377 MB | 真实结构化载荷 |
| **实际物理落盘大小** | **1053 MB** | **7976 MB** | **节省 87%** |
| **进程常驻内存 (RSS)** | **359 MB** | **4932 MB** | **节省 93%** |

#### wedb_embed vs Redis 核心指令性能对比

| 指令 | wedb_embed P95延迟 | Redis P95延迟 | 性能领先 |
| :--- | :--- | :--- | :--- |
| `SET` | 6.6 us | 24.0 us | **3.6x** |
| `GET` | 4.4 us | 18.5 us | **4.2x** |
| `MSET` | 45.2 us | 28.6 us | **0.6x** |
| `MGET` | 4.3 us | 24.0 us | **5.6x** |
| `INCRBY` | 0.52 us | 23.5 us | **44.9x** |
| `DECRBY` | 0.54 us | 23.6 us | **43.7x** |
| `APPEND` | 0.62 us | 23.8 us | **38.7x** |
| `STRLEN` | 0.24 us | 21.1 us | **87.2x** |
| `GETDEL` | 7.3 us | 46.3 us | **6.4x** |
| `GETRANGE` | 0.22 us | 19.7 us | **91.0x** |
| `SETRANGE` | 0.61 us | 24.8 us | **40.9x** |
| `HSET` | 1.7 us | 48.8 us | **29.2x** |
| `HGET` | 0.56 us | 48.3 us | **85.5x** |
| `HMGET` | 2.4 us | 49.0 us | **20.7x** |
| `HEXISTS` | 0.53 us | 48.4 us | **91.7x** |
| `HLEN` | 0.35 us | 48.4 us | **137.6x** |
| `HDEL` | 5.2 us | 49.1 us | **9.4x** |
| `HGETALL` | 2.7 us | 48.6 us | **18.2x** |
| `HKEYS` | 2.4 us | 48.9 us | **20.3x** |
| `HVALS` | 2.4 us | 48.7 us | **19.9x** |
| `HINCRBY` | 1.7 us | 49.1 us | **29.0x** |
| `LPUSH` | 1.6 us | 46.2 us | **28.3x** |
| `RPUSH` | 1.5 us | 35.3 us | **23.4x** |
| `LPOP` | 1.9 us | 18.1 us | **9.5x** |
| `RPOP` | 2.0 us | 41.2 us | **21.1x** |
| `LLEN` | 0.50 us | 48.3 us | **96.9x** |
| `LRANGE` | 2.6 us | 45.9 us | **17.9x** |
| `LINDEX` | 0.56 us | 48.7 us | **87.7x** |
| `LSET` | 0.87 us | 48.1 us | **55.5x** |
| `LREM` | 13.2 us | 98.3 us | **7.4x** |
| `LTRIM` | 0.88 us | 48.4 us | **55.2x** |
| `SADD` | 1.1 us | 35.5 us | **31.4x** |
| `SREM` | 3.1 us | 35.2 us | **11.4x** |
| `SISMEMBER` | 0.54 us | 34.8 us | **64.3x** |
| `SCARD` | 0.37 us | 36.0 us | **97.6x** |
| `SMEMBERS` | 2.8 us | 35.7 us | **12.6x** |
| `SPOP` | 4.8 us | 71.2 us | **14.9x** |
| `SRANDMEMBER` | 2.1 us | 34.4 us | **16.0x** |
| `ZADD` | 2.4 us | 25.0 us | **10.2x** |
| `ZSCORE` | 0.67 us | 19.1 us | **28.6x** |
| `ZRANGE` | 2.9 us | 22.5 us | **7.9x** |
| `ZCARD` | 0.42 us | 19.3 us | **46.1x** |
| `ZCOUNT` | 2.4 us | 22.3 us | **9.3x** |
| `ZINCRBY` | 2.2 us | 26.8 us | **11.9x** |
| `ZRANK` | 2.4 us | 22.1 us | **9.1x** |
| `ZREVRANGE` | 3.9 us | 23.2 us | **6.0x** |
| `ZPOPMIN` | 9.1 us | 50.1 us | **5.5x** |
| `ZREM` | 3.6 us | 22.6 us | **6.3x** |
| `SETBIT` | 11.8 us | 49.1 us | **4.1x** |
| `GETBIT` | 0.38 us | 48.7 us | **127.5x** |
| `BITCOUNT` | 0.37 us | 19.8 us | **53.5x** |
| `BITPOS` | 0.51 us | 24.1 us | **47.6x** |
| `PFADD` | 2.4 us | 48.6 us | **20.1x** |
| `PFCOUNT` | 29.6 us | 48.1 us | **1.6x** |
| `GEOADD` | 2.1 us | 48.8 us | **23.3x** |
| `GEODIST` | 0.79 us | 47.8 us | **60.5x** |
| `GEOPOS` | 0.56 us | 48.5 us | **86.0x** |
| `GEOHASH` | 0.61 us | 47.9 us | **79.1x** |
| `XADD` | 1.5 us | 26.8 us | **17.6x** |
| `XLEN` | 0.46 us | 18.6 us | **40.8x** |
| `XRANGE` | 3.0 us | 27.8 us | **9.2x** |
| `XREAD` | 3.0 us | 48.1 us | **16.1x** |
| `XDEL` | 3.1 us | 51.8 us | **16.8x** |
| `DEL` | 3.0 us | 47.5 us | **15.8x** |
| `EXISTS` | 0.20 us | 47.8 us | **240.3x** |
| `EXPIRE` | 0.80 us | 48.7 us | **61.2x** |
| `TTL` | 0.21 us | 47.6 us | **222.0x** |
| `JSON.SET` | 2.7 us | 48.5 us | **18.2x** |
| `JSON.GET` | 1.0 us | 46.7 us | **45.1x** |
| `JSON.DEL` | 7.4 us | 96.9 us | **13.0x** |
| `JSON.NUMINCRBY` | 2.8 us | 48.7 us | **17.5x** |
| `JSON.ARRLEN` | 0.95 us | 49.0 us | **51.6x** |
| `JSON.TYPE` | 0.99 us | 48.5 us | **48.9x** |
| `BF.ADD` | 12.9 us | 46.9 us | **3.6x** |
| `BF.EXISTS` | 0.54 us | 49.4 us | **92.1x** |
| `BF.INFO` | 0.33 us | 46.7 us | **140.4x** |
| `CF.ADD` | 2.3 us | 47.9 us | **20.5x** |
| `CF.EXISTS` | 0.60 us | 47.6 us | **79.1x** |
| `CF.DEL` | 5.8 us | 95.9 us | **16.6x** |
| `TDIGEST.ADD` | 2.2 us | 19.2 us | **8.8x** |
| `TDIGEST.QUANTILE` | 0.78 us | 23.3 us | **30.0x** |
| `TDIGEST.BYRANK` | 0.87 us | 22.5 us | **25.7x** |
| `TDIGEST.CDF` | 0.92 us | 22.7 us | **24.8x** |
| `TS.ADD` | 5.4 us | 23.7 us | **4.4x** |
| `TS.GET` | 1.1 us | 23.0 us | **21.0x** |
| `TS.RANGE` | 21.1 us | 22.8 us | **1.1x** |
| `TS.INCRBY` | 6.3 us | 22.7 us | **3.6x** |
| `FT.SEARCH` | 17.3 us | 36.6 us | **2.1x** |
| `FT.TAG` | 17.2 us | 35.5 us | **2.1x** |
| `VECTOR.KNN` | 5.5 us | 24.3 us | **4.4x** |

