### Ubuntu CI (GitHub Actions Runner)

#### 硬件与测试环境

CPU: AMD EPYC 7763 64-Core Processor (4核)<br>
内存: 15.6 GB<br>
硬盘: Azure Managed Virtual Disk (Cloud Standard SSD)<br>
系统: Ubuntu 24.04.4 LTS (Linux 6.17.0-1022-azure)<br>
Rust: 1.98.0 (88d9e12ae 2026-08-18)<br>
Redis: v8.10.1

#### 真实物理落盘与内存占用实测 (5GB 数据规模)

| 资源维度 | wedb_embed (嵌入式 LSM+LZ4) | Redis (v8.10.1 AOF持久化) | 资源节省比例 |
| :--- | :--- | :--- | :--- |
| **测试数据规模** | 5,000,000 条全格式结构化数据 | 5,000,000 条全格式结构化数据 | 14 种数据格式等比实测 |
| **原始数据载荷** | 4377 MB | 4377 MB | 真实结构化载荷 |
| **实际物理落盘大小** | **1053 MB** | **7652 MB** | **节省 86%** |
| **进程常驻内存 (RSS)** | **287 MB** | **4797 MB** | **节省 94%** |

#### wedb_embed vs Redis 核心指令性能对比

| 指令 | wedb_embed P95延迟 | Redis P95延迟 | 性能领先 |
| :--- | :--- | :--- | :--- |
| `SET` | 7.4 us | 28.7 us | **3.9x** |
| `GET` | 5.1 us | 19.4 us | **3.8x** |
| `MSET` | 50.0 us | 34.9 us | **0.7x** |
| `MGET` | 5.0 us | 26.1 us | **5.3x** |
| `INCRBY` | 1.2 us | 28.9 us | **23.2x** |
| `DECRBY` | 0.72 us | 27.9 us | **39.0x** |
| `APPEND` | 1.3 us | 27.8 us | **22.0x** |
| `STRLEN` | 0.32 us | 20.0 us | **63.0x** |
| `GETDEL` | 8.3 us | 57.1 us | **6.9x** |
| `GETRANGE` | 0.33 us | 19.4 us | **59.5x** |
| `SETRANGE` | 1.3 us | 28.4 us | **22.3x** |
| `HSET` | 3.1 us | 28.5 us | **9.2x** |
| `HGET` | 1.3 us | 18.8 us | **15.0x** |
| `HMGET` | 3.3 us | 26.8 us | **8.1x** |
| `HEXISTS` | 0.66 us | 20.6 us | **31.4x** |
| `HLEN` | 0.45 us | 20.1 us | **44.6x** |
| `HDEL` | 4.9 us | 28.9 us | **5.9x** |
| `HGETALL` | 3.3 us | 19.4 us | **5.9x** |
| `HKEYS` | 3.2 us | 20.3 us | **6.4x** |
| `HVALS` | 3.5 us | 18.8 us | **5.4x** |
| `HINCRBY` | 2.0 us | 30.8 us | **15.2x** |
| `LPUSH` | 3.1 us | 28.6 us | **9.3x** |
| `RPUSH` | 3.0 us | 28.6 us | **9.7x** |
| `LPOP` | 2.5 us | 29.7 us | **12.1x** |
| `RPOP` | 2.8 us | 28.4 us | **10.1x** |
| `LLEN` | 0.71 us | 19.3 us | **27.2x** |
| `LRANGE` | 3.4 us | 19.2 us | **5.7x** |
| `LINDEX` | 0.68 us | 20.1 us | **29.4x** |
| `LSET` | 1.3 us | 28.9 us | **22.7x** |
| `LREM` | 17.7 us | 59.6 us | **3.4x** |
| `LTRIM` | 1.1 us | 20.0 us | **17.5x** |
| `SADD` | 1.5 us | 23.4 us | **15.5x** |
| `SREM` | 4.9 us | 24.1 us | **4.9x** |
| `SISMEMBER` | 0.72 us | 19.3 us | **26.8x** |
| `SCARD` | 0.47 us | 19.7 us | **42.3x** |
| `SMEMBERS` | 3.8 us | 19.4 us | **5.1x** |
| `SPOP` | 8.4 us | 57.1 us | **6.8x** |
| `SRANDMEMBER` | 4.0 us | 18.8 us | **4.7x** |
| `ZADD` | 3.1 us | 43.2 us | **14.1x** |
| `ZSCORE` | 0.87 us | 34.6 us | **39.8x** |
| `ZRANGE` | 4.0 us | 39.1 us | **9.8x** |
| `ZCARD` | 0.53 us | 34.7 us | **65.1x** |
| `ZCOUNT` | 3.6 us | 37.0 us | **10.3x** |
| `ZINCRBY` | 2.9 us | 45.0 us | **15.4x** |
| `ZRANK` | 3.5 us | 36.8 us | **10.7x** |
| `ZREVRANGE` | 6.1 us | 38.1 us | **6.3x** |
| `ZPOPMIN` | 7.8 us | 88.6 us | **11.4x** |
| `ZREM` | 4.7 us | 41.2 us | **8.8x** |
| `SETBIT` | 11.3 us | 36.1 us | **3.2x** |
| `GETBIT` | 0.64 us | 23.6 us | **36.7x** |
| `BITCOUNT` | 0.36 us | 29.9 us | **83.2x** |
| `BITPOS` | 0.71 us | 36.2 us | **50.9x** |
| `PFADD` | 2.9 us | 24.0 us | **8.4x** |
| `PFCOUNT` | 8.2 us | 20.5 us | **2.5x** |
| `GEOADD` | 3.2 us | 45.2 us | **14.0x** |
| `GEODIST` | 1.6 us | 19.3 us | **12.4x** |
| `GEOPOS` | 1.2 us | 19.4 us | **15.9x** |
| `GEOHASH` | 1.2 us | 19.0 us | **16.4x** |
| `XADD` | 2.7 us | 31.5 us | **11.8x** |
| `XLEN` | 0.61 us | 18.6 us | **30.3x** |
| `XRANGE` | 4.2 us | 30.9 us | **7.3x** |
| `XREAD` | 4.3 us | 32.3 us | **7.5x** |
| `XDEL` | 3.9 us | 63.4 us | **16.3x** |
| `DEL` | 3.0 us | 19.7 us | **6.5x** |
| `EXISTS` | 0.24 us | 20.3 us | **83.5x** |
| `EXPIRE` | 0.77 us | 29.6 us | **38.3x** |
| `TTL` | 0.29 us | 20.1 us | **68.7x** |
| `JSON.SET` | 3.8 us | 21.2 us | **5.6x** |
| `JSON.GET` | 1.6 us | 19.2 us | **12.4x** |
| `JSON.DEL` | 9.5 us | 38.7 us | **4.1x** |
| `JSON.NUMINCRBY` | 4.0 us | 18.0 us | **4.5x** |
| `JSON.ARRLEN` | 1.4 us | 18.6 us | **13.5x** |
| `JSON.TYPE` | 1.5 us | 19.2 us | **12.9x** |
| `BF.ADD` | 32.4 us | 34.8 us | **1.1x** |
| `BF.EXISTS` | 0.64 us | 38.4 us | **59.9x** |
| `BF.INFO` | 0.37 us | 19.7 us | **53.8x** |
| `CF.ADD` | 3.3 us | 19.7 us | **6.0x** |
| `CF.EXISTS` | 0.83 us | 19.8 us | **23.9x** |
| `CF.DEL` | 9.8 us | 40.4 us | **4.1x** |
| `TDIGEST.ADD` | 2.8 us | 18.6 us | **6.6x** |
| `TDIGEST.QUANTILE` | 1.2 us | 37.3 us | **30.3x** |
| `TDIGEST.BYRANK` | 1.2 us | 19.6 us | **16.4x** |
| `TDIGEST.CDF` | 1.3 us | 18.6 us | **14.6x** |
| `TS.ADD` | 11.8 us | 36.0 us | **3.0x** |
| `TS.GET` | 1.5 us | 36.6 us | **24.3x** |
| `TS.RANGE` | 20.8 us | 37.8 us | **1.8x** |
| `TS.INCRBY` | 9.0 us | 37.1 us | **4.1x** |
| `FT.SEARCH` | 27.2 us | 19.5 us | **0.7x** |
| `FT.TAG` | 27.3 us | 19.9 us | **0.7x** |
| `VECTOR.KNN` | 5.4 us | 40.2 us | **7.4x** |

