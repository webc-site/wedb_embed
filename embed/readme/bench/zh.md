### Ubuntu CI (GitHub Actions Runner)

#### 硬件与测试环境

CPU: INTEL(R) XEON(R) PLATINUM 8573C (4核)<br>
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
| **实际物理落盘大小** | **1053 MB** | **7951 MB** | **节省 87%** |
| **进程常驻内存 (RSS)** | **484 MB** | **4854 MB** | **节省 90%** |

#### wedb_embed vs Redis 核心指令性能对比

| 指令 | wedb_embed P95延迟 | Redis P95延迟 | 性能领先 |
| :--- | :--- | :--- | :--- |
| `SET` | 9.9 us | 26.4 us | **2.7x** |
| `GET` | 6.3 us | 18.9 us | **3.0x** |
| `MSET` | 66.7 us | 31.1 us | **0.5x** |
| `MGET` | 6.4 us | 25.2 us | **3.9x** |
| `INCRBY` | 0.84 us | 25.1 us | **30.0x** |
| `DECRBY` | 0.74 us | 24.9 us | **33.6x** |
| `APPEND` | 0.89 us | 24.6 us | **27.7x** |
| `STRLEN` | 0.36 us | 19.2 us | **53.0x** |
| `GETDEL` | 10.6 us | 49.1 us | **4.7x** |
| `GETRANGE` | 0.32 us | 20.6 us | **63.5x** |
| `SETRANGE` | 0.89 us | 26.1 us | **29.4x** |
| `HSET` | 2.4 us | 29.2 us | **12.1x** |
| `HGET` | 0.85 us | 29.9 us | **35.1x** |
| `HMGET` | 3.4 us | 24.1 us | **7.1x** |
| `HEXISTS` | 0.80 us | 32.1 us | **40.3x** |
| `HLEN` | 0.54 us | 27.0 us | **50.3x** |
| `HDEL` | 8.1 us | 36.3 us | **4.5x** |
| `HGETALL` | 3.7 us | 27.8 us | **7.5x** |
| `HKEYS` | 3.5 us | 28.4 us | **8.1x** |
| `HVALS` | 3.5 us | 25.6 us | **7.3x** |
| `HINCRBY` | 2.2 us | 30.1 us | **13.9x** |
| `LPUSH` | 2.4 us | 31.4 us | **13.2x** |
| `RPUSH` | 2.1 us | 37.7 us | **17.7x** |
| `LPOP` | 2.7 us | 18.9 us | **6.9x** |
| `RPOP` | 2.8 us | 37.0 us | **13.1x** |
| `LLEN` | 0.54 us | 36.6 us | **67.2x** |
| `LRANGE` | 3.6 us | 30.4 us | **8.5x** |
| `LINDEX` | 0.83 us | 34.3 us | **41.3x** |
| `LSET` | 1.3 us | 35.1 us | **27.1x** |
| `LREM` | 20.1 us | 63.2 us | **3.2x** |
| `LTRIM` | 1.3 us | 28.3 us | **22.3x** |
| `SADD` | 1.7 us | 25.1 us | **15.1x** |
| `SREM` | 4.7 us | 21.4 us | **4.6x** |
| `SISMEMBER` | 0.81 us | 20.7 us | **25.5x** |
| `SCARD` | 0.55 us | 21.3 us | **38.4x** |
| `SMEMBERS` | 3.7 us | 22.8 us | **6.1x** |
| `SPOP` | 7.0 us | 50.8 us | **7.3x** |
| `SRANDMEMBER` | 3.1 us | 20.2 us | **6.6x** |
| `ZADD` | 3.2 us | 25.2 us | **7.9x** |
| `ZSCORE` | 0.94 us | 20.8 us | **22.1x** |
| `ZRANGE` | 4.2 us | 24.4 us | **5.8x** |
| `ZCARD` | 0.60 us | 21.1 us | **35.0x** |
| `ZCOUNT` | 3.6 us | 22.8 us | **6.4x** |
| `ZINCRBY` | 3.2 us | 27.0 us | **8.4x** |
| `ZRANK` | 3.8 us | 21.8 us | **5.8x** |
| `ZREVRANGE` | 5.6 us | 24.8 us | **4.4x** |
| `ZPOPMIN` | 13.9 us | 52.5 us | **3.8x** |
| `ZREM` | 6.7 us | 22.8 us | **3.4x** |
| `SETBIT` | 21.0 us | 30.6 us | **1.5x** |
| `GETBIT` | 0.77 us | 26.7 us | **34.8x** |
| `BITCOUNT` | 0.69 us | 29.0 us | **42.3x** |
| `BITPOS` | 0.74 us | 31.1 us | **42.3x** |
| `PFADD` | 3.2 us | 28.5 us | **9.0x** |
| `PFCOUNT` | 41.6 us | 25.8 us | **0.6x** |
| `GEOADD` | 3.0 us | 52.7 us | **17.8x** |
| `GEODIST` | 1.2 us | 35.0 us | **30.2x** |
| `GEOPOS` | 0.83 us | 30.1 us | **36.4x** |
| `GEOHASH` | 0.87 us | 33.2 us | **38.3x** |
| `XADD` | 2.0 us | 27.6 us | **13.7x** |
| `XLEN` | 0.66 us | 19.3 us | **29.3x** |
| `XRANGE` | 4.2 us | 30.0 us | **7.2x** |
| `XREAD` | 4.2 us | 31.0 us | **7.4x** |
| `XDEL` | 4.5 us | 54.1 us | **12.1x** |
| `DEL` | 4.5 us | 27.1 us | **6.0x** |
| `EXISTS` | 0.29 us | 25.4 us | **87.8x** |
| `EXPIRE` | 0.95 us | 41.1 us | **43.3x** |
| `TTL` | 0.32 us | 26.5 us | **81.8x** |
| `JSON.SET` | 4.0 us | 28.8 us | **7.3x** |
| `JSON.GET` | 1.6 us | 27.3 us | **17.1x** |
| `JSON.DEL` | 9.0 us | 54.8 us | **6.1x** |
| `JSON.NUMINCRBY` | 4.4 us | 25.4 us | **5.7x** |
| `JSON.ARRLEN` | 1.5 us | 26.1 us | **17.9x** |
| `JSON.TYPE` | 1.5 us | 38.1 us | **25.3x** |
| `BF.ADD` | 21.4 us | 30.8 us | **1.4x** |
| `BF.EXISTS` | 0.85 us | 30.5 us | **35.9x** |
| `BF.INFO` | 0.51 us | 28.1 us | **54.9x** |
| `CF.ADD` | 3.3 us | 38.5 us | **11.7x** |
| `CF.EXISTS` | 0.83 us | 37.2 us | **44.6x** |
| `CF.DEL` | 8.0 us | 81.2 us | **10.1x** |
| `TDIGEST.ADD` | 3.0 us | 24.3 us | **8.2x** |
| `TDIGEST.QUANTILE` | 1.2 us | 24.2 us | **19.5x** |
| `TDIGEST.BYRANK` | 1.2 us | 24.3 us | **19.9x** |
| `TDIGEST.CDF` | 1.4 us | 24.3 us | **17.9x** |
| `TS.ADD` | 7.2 us | 25.2 us | **3.5x** |
| `TS.GET` | 1.6 us | 23.1 us | **14.5x** |
| `TS.RANGE` | 31.5 us | 24.6 us | **0.8x** |
| `TS.INCRBY` | 10.0 us | 24.0 us | **2.4x** |
| `FT.SEARCH` | 24.7 us | 32.5 us | **1.3x** |
| `FT.TAG` | 24.4 us | 33.5 us | **1.4x** |
| `VECTOR.KNN` | 6.1 us | 25.6 us | **4.2x** |

