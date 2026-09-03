### Ubuntu CI (GitHub Actions Runner)

#### 硬件与测试环境

CPU: AMD EPYC 7763 64-Core Processor (4核)<br>
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
| **实际物理落盘大小** | **1053 MB** | **7656 MB** | **节省 86%** |
| **进程常驻内存 (RSS)** | **275 MB** | **4808 MB** | **节省 94%** |

#### wedb_embed vs Redis 核心指令性能对比

| 指令 | wedb_embed P95延迟 | Redis P95延迟 | 性能领先 |
| :--- | :--- | :--- | :--- |
| `SET` | 7.4 us | 40.1 us | **5.4x** |
| `GET` | 5.1 us | 30.6 us | **6.0x** |
| `MSET` | 49.5 us | 46.1 us | **0.9x** |
| `MGET` | 5.2 us | 39.0 us | **7.5x** |
| `INCRBY` | 0.82 us | 39.2 us | **47.7x** |
| `DECRBY` | 0.66 us | 39.3 us | **59.6x** |
| `APPEND` | 0.86 us | 39.2 us | **45.6x** |
| `STRLEN` | 0.36 us | 30.3 us | **84.9x** |
| `GETDEL` | 8.3 us | 79.6 us | **9.6x** |
| `GETRANGE` | 0.28 us | 33.3 us | **121.0x** |
| `SETRANGE` | 0.83 us | 40.1 us | **48.2x** |
| `HSET` | 1.9 us | 42.4 us | **22.8x** |
| `HGET` | 0.70 us | 35.0 us | **49.9x** |
| `HMGET` | 2.8 us | 39.8 us | **14.5x** |
| `HEXISTS` | 0.60 us | 34.3 us | **56.8x** |
| `HLEN` | 0.42 us | 33.8 us | **79.7x** |
| `HDEL` | 5.6 us | 40.8 us | **7.3x** |
| `HGETALL` | 3.5 us | 39.8 us | **11.4x** |
| `HKEYS` | 3.2 us | 35.6 us | **11.2x** |
| `HVALS` | 3.4 us | 36.8 us | **10.8x** |
| `HINCRBY` | 1.7 us | 43.8 us | **25.6x** |
| `LPUSH` | 2.5 us | 39.6 us | **16.1x** |
| `RPUSH` | 2.2 us | 39.8 us | **18.3x** |
| `LPOP` | 2.4 us | 58.8 us | **24.8x** |
| `RPOP` | 2.7 us | 54.8 us | **20.6x** |
| `LLEN` | 0.44 us | 33.9 us | **76.6x** |
| `LRANGE` | 3.4 us | 33.6 us | **10.0x** |
| `LINDEX` | 0.69 us | 34.9 us | **50.7x** |
| `LSET` | 1.2 us | 39.5 us | **34.3x** |
| `LREM` | 17.4 us | 81.5 us | **4.7x** |
| `LTRIM` | 1.1 us | 33.1 us | **31.2x** |
| `SADD` | 1.3 us | 37.3 us | **29.0x** |
| `SREM` | 4.9 us | 37.6 us | **7.7x** |
| `SISMEMBER` | 0.63 us | 33.9 us | **54.0x** |
| `SCARD` | 0.45 us | 33.5 us | **74.7x** |
| `SMEMBERS` | 4.0 us | 33.6 us | **8.3x** |
| `SPOP` | 9.7 us | 82.4 us | **8.5x** |
| `SRANDMEMBER` | 2.8 us | 34.2 us | **12.1x** |
| `ZADD` | 2.9 us | 41.1 us | **14.2x** |
| `ZSCORE` | 0.92 us | 33.8 us | **36.6x** |
| `ZRANGE` | 4.2 us | 36.2 us | **8.6x** |
| `ZCARD` | 0.56 us | 33.7 us | **59.8x** |
| `ZCOUNT` | 3.6 us | 36.0 us | **10.0x** |
| `ZINCRBY` | 3.1 us | 41.1 us | **13.1x** |
| `ZRANK` | 3.6 us | 33.5 us | **9.2x** |
| `ZREVRANGE` | 6.4 us | 36.0 us | **5.6x** |
| `ZPOPMIN` | 9.3 us | 82.7 us | **8.9x** |
| `ZREM` | 5.2 us | 34.7 us | **6.6x** |
| `SETBIT` | 16.7 us | 30.2 us | **1.8x** |
| `GETBIT` | 0.68 us | 30.1 us | **44.4x** |
| `BITCOUNT` | 0.36 us | 36.1 us | **99.3x** |
| `BITPOS` | 0.51 us | 19.6 us | **38.8x** |
| `PFADD` | 2.7 us | 40.6 us | **14.8x** |
| `PFCOUNT` | 8.2 us | 33.9 us | **4.1x** |
| `GEOADD` | 2.4 us | 43.8 us | **18.2x** |
| `GEODIST` | 0.94 us | 36.1 us | **38.6x** |
| `GEOPOS` | 0.68 us | 37.3 us | **55.1x** |
| `GEOHASH` | 0.78 us | 35.2 us | **45.1x** |
| `XADD` | 1.6 us | 42.1 us | **25.9x** |
| `XLEN` | 0.64 us | 33.0 us | **51.5x** |
| `XRANGE` | 4.4 us | 43.8 us | **9.9x** |
| `XREAD` | 4.3 us | 44.7 us | **10.5x** |
| `XDEL` | 3.9 us | 84.5 us | **21.8x** |
| `DEL` | 4.9 us | 24.9 us | **5.1x** |
| `EXISTS` | 0.24 us | 23.3 us | **97.4x** |
| `EXPIRE` | 0.75 us | 43.4 us | **58.2x** |
| `TTL` | 0.28 us | 32.7 us | **116.4x** |
| `JSON.SET` | 3.6 us | 36.8 us | **10.3x** |
| `JSON.GET` | 1.5 us | 35.3 us | **23.9x** |
| `JSON.DEL` | 8.3 us | 71.0 us | **8.6x** |
| `JSON.NUMINCRBY` | 3.9 us | 36.7 us | **9.4x** |
| `JSON.ARRLEN` | 1.4 us | 34.0 us | **24.7x** |
| `JSON.TYPE` | 1.4 us | 32.2 us | **22.8x** |
| `BF.ADD` | 55.5 us | 22.5 us | **0.4x** |
| `BF.EXISTS` | 1.0 us | 22.2 us | **21.5x** |
| `BF.INFO` | 0.57 us | 22.5 us | **39.8x** |
| `CF.ADD` | 3.5 us | 22.6 us | **6.5x** |
| `CF.EXISTS` | 1.1 us | 22.1 us | **20.7x** |
| `CF.DEL` | 13.5 us | 45.1 us | **3.3x** |
| `TDIGEST.ADD` | 3.0 us | 33.6 us | **11.3x** |
| `TDIGEST.QUANTILE` | 1.2 us | 32.9 us | **27.1x** |
| `TDIGEST.BYRANK` | 1.2 us | 33.2 us | **26.8x** |
| `TDIGEST.CDF` | 1.3 us | 34.1 us | **26.6x** |
| `TS.ADD` | 7.3 us | 36.0 us | **4.9x** |
| `TS.GET` | 1.6 us | 33.2 us | **21.4x** |
| `TS.RANGE` | 20.9 us | 27.3 us | **1.3x** |
| `TS.INCRBY` | 10.5 us | 33.5 us | **3.2x** |
| `FT.SEARCH` | 30.0 us | 35.0 us | **1.2x** |
| `FT.TAG` | 29.3 us | 33.3 us | **1.1x** |
| `VECTOR.KNN` | 4.3 us | 26.7 us | **6.2x** |

