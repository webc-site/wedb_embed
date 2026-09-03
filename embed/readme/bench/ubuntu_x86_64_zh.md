### Ubuntu CI (GitHub Actions Runner)

#### 硬件与测试环境

CPU: INTEL(R) XEON(R) PLATINUM 8573C (4核)<br>
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
| **实际物理落盘大小** | **1053 MB** | **7954 MB** | **节省 87%** |
| **进程常驻内存 (RSS)** | **269 MB** | **4823 MB** | **节省 94%** |

#### wedb_embed vs Redis 核心指令性能对比

| 指令 | wedb_embed P95延迟 | Redis P95延迟 | 性能领先 |
| :--- | :--- | :--- | :--- |
| `SET` | 9.3 us | 26.8 us | **2.9x** |
| `GET` | 6.5 us | 28.0 us | **4.3x** |
| `MSET` | 57.5 us | 31.7 us | **0.6x** |
| `MGET` | 5.8 us | 28.7 us | **5.0x** |
| `INCRBY` | 0.79 us | 26.4 us | **33.3x** |
| `DECRBY` | 0.73 us | 40.3 us | **55.5x** |
| `APPEND` | 0.87 us | 39.7 us | **45.7x** |
| `STRLEN` | 0.35 us | 27.1 us | **77.7x** |
| `GETDEL` | 10.1 us | 98.8 us | **9.7x** |
| `GETRANGE` | 0.34 us | 29.6 us | **88.4x** |
| `SETRANGE` | 0.95 us | 27.8 us | **29.2x** |
| `HSET` | 2.1 us | 28.5 us | **13.4x** |
| `HGET` | 0.70 us | 24.8 us | **35.3x** |
| `HMGET` | 3.0 us | 26.7 us | **8.8x** |
| `HEXISTS` | 0.67 us | 26.8 us | **39.8x** |
| `HLEN` | 0.50 us | 25.0 us | **50.2x** |
| `HDEL` | 4.8 us | 26.4 us | **5.5x** |
| `HGETALL` | 3.1 us | 28.4 us | **9.1x** |
| `HKEYS` | 3.3 us | 27.4 us | **8.4x** |
| `HVALS` | 3.4 us | 28.5 us | **8.5x** |
| `HINCRBY` | 2.1 us | 26.5 us | **12.7x** |
| `LPUSH` | 2.2 us | 27.3 us | **12.4x** |
| `RPUSH` | 1.9 us | 27.7 us | **14.2x** |
| `LPOP` | 2.5 us | 30.9 us | **12.2x** |
| `RPOP` | 2.2 us | 31.0 us | **13.9x** |
| `LLEN` | 0.46 us | 28.1 us | **61.1x** |
| `LRANGE` | 3.6 us | 28.9 us | **8.1x** |
| `LINDEX` | 0.68 us | 26.9 us | **39.3x** |
| `LSET` | 1.2 us | 27.5 us | **22.5x** |
| `LREM` | 16.9 us | 57.7 us | **3.4x** |
| `LTRIM` | 1.2 us | 26.6 us | **23.0x** |
| `SADD` | 1.5 us | 26.4 us | **17.2x** |
| `SREM` | 4.5 us | 33.0 us | **7.3x** |
| `SISMEMBER` | 0.77 us | 26.0 us | **33.7x** |
| `SCARD` | 0.52 us | 26.6 us | **51.2x** |
| `SMEMBERS` | 3.9 us | 27.1 us | **7.0x** |
| `SPOP` | 7.1 us | 56.1 us | **7.9x** |
| `SRANDMEMBER` | 4.0 us | 29.8 us | **7.4x** |
| `ZADD` | 2.8 us | 26.3 us | **9.3x** |
| `ZSCORE` | 0.91 us | 27.7 us | **30.5x** |
| `ZRANGE` | 3.6 us | 26.5 us | **7.4x** |
| `ZCARD` | 0.51 us | 26.8 us | **52.2x** |
| `ZCOUNT` | 3.0 us | 26.6 us | **8.9x** |
| `ZINCRBY` | 2.9 us | 24.4 us | **8.5x** |
| `ZRANK` | 3.4 us | 26.7 us | **7.9x** |
| `ZREVRANGE` | 5.8 us | 25.8 us | **4.4x** |
| `ZPOPMIN` | 7.0 us | 53.9 us | **7.6x** |
| `ZREM` | 4.9 us | 28.5 us | **5.8x** |
| `SETBIT` | 14.7 us | 46.1 us | **3.1x** |
| `GETBIT` | 0.56 us | 32.5 us | **57.9x** |
| `BITCOUNT` | 0.45 us | 29.2 us | **64.9x** |
| `BITPOS` | 0.48 us | 30.1 us | **63.0x** |
| `PFADD` | 3.2 us | 28.1 us | **8.8x** |
| `PFCOUNT` | 35.6 us | 26.6 us | **0.7x** |
| `GEOADD` | 2.5 us | 28.1 us | **11.2x** |
| `GEODIST` | 0.98 us | 24.4 us | **25.0x** |
| `GEOPOS` | 0.76 us | 26.6 us | **35.0x** |
| `GEOHASH` | 0.74 us | 27.7 us | **37.2x** |
| `XADD` | 2.0 us | 30.5 us | **15.2x** |
| `XLEN` | 0.56 us | 25.6 us | **45.5x** |
| `XRANGE` | 3.8 us | 27.8 us | **7.3x** |
| `XREAD` | 3.7 us | 28.0 us | **7.6x** |
| `XDEL` | 3.8 us | 53.9 us | **14.0x** |
| `DEL` | 3.9 us | 26.9 us | **6.9x** |
| `EXISTS` | 0.24 us | 26.6 us | **110.7x** |
| `EXPIRE` | 0.87 us | 26.9 us | **30.8x** |
| `TTL` | 0.26 us | 25.7 us | **97.3x** |
| `JSON.SET` | 3.8 us | 28.1 us | **7.4x** |
| `JSON.GET` | 1.3 us | 26.7 us | **20.2x** |
| `JSON.DEL` | 8.0 us | 53.0 us | **6.7x** |
| `JSON.NUMINCRBY` | 3.6 us | 27.9 us | **7.8x** |
| `JSON.ARRLEN` | 1.3 us | 26.2 us | **20.8x** |
| `JSON.TYPE` | 1.2 us | 28.0 us | **22.9x** |
| `BF.ADD` | 11.9 us | 34.8 us | **2.9x** |
| `BF.EXISTS` | 0.69 us | 45.2 us | **65.5x** |
| `BF.INFO` | 0.43 us | 43.2 us | **100.4x** |
| `CF.ADD` | 2.8 us | 42.9 us | **15.6x** |
| `CF.EXISTS` | 0.72 us | 29.1 us | **40.6x** |
| `CF.DEL` | 7.3 us | 63.8 us | **8.7x** |
| `TDIGEST.ADD` | 2.6 us | 27.3 us | **10.6x** |
| `TDIGEST.QUANTILE` | 1.1 us | 28.4 us | **26.9x** |
| `TDIGEST.BYRANK` | 0.99 us | 26.3 us | **26.5x** |
| `TDIGEST.CDF` | 1.1 us | 26.8 us | **24.9x** |
| `TS.ADD` | 8.0 us | 27.1 us | **3.4x** |
| `TS.GET` | 1.5 us | 26.7 us | **17.8x** |
| `TS.RANGE` | 26.7 us | 29.4 us | **1.1x** |
| `TS.INCRBY` | 9.0 us | 27.5 us | **3.0x** |
| `FT.SEARCH` | 24.5 us | 26.5 us | **1.1x** |
| `FT.TAG` | 23.5 us | 27.0 us | **1.2x** |
| `VECTOR.KNN` | 3.6 us | 28.5 us | **7.9x** |

