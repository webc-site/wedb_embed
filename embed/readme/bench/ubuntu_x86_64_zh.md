### Ubuntu CI (GitHub Actions Runner)

#### 硬件与测试环境

CPU: AMD EPYC 9V74 80-Core Processor (4核)<br>
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
| **实际物理落盘大小** | **1053 MB** | **7556 MB** | **节省 86%** |
| **进程常驻内存 (RSS)** | **284 MB** | **4818 MB** | **节省 94%** |

#### wedb_embed vs Redis 核心指令性能对比

| 指令 | wedb_embed P95延迟 | Redis P95延迟 | 性能领先 |
| :--- | :--- | :--- | :--- |
| `SET` | 7.8 us | 32.7 us | **4.2x** |
| `GET` | 5.3 us | 22.8 us | **4.3x** |
| `MSET` | 52.5 us | 38.9 us | **0.7x** |
| `MGET` | 5.7 us | 31.5 us | **5.5x** |
| `INCRBY` | 0.78 us | 31.9 us | **40.9x** |
| `DECRBY` | 0.66 us | 32.7 us | **49.7x** |
| `APPEND` | 0.85 us | 31.5 us | **37.1x** |
| `STRLEN` | 0.29 us | 22.5 us | **77.7x** |
| `GETDEL` | 8.7 us | 65.8 us | **7.5x** |
| `GETRANGE` | 0.38 us | 22.9 us | **60.4x** |
| `SETRANGE` | 0.69 us | 31.5 us | **45.8x** |
| `HSET` | 2.0 us | 33.8 us | **16.8x** |
| `HGET` | 0.74 us | 21.8 us | **29.3x** |
| `HMGET` | 3.6 us | 31.8 us | **8.8x** |
| `HEXISTS` | 0.65 us | 22.5 us | **34.6x** |
| `HLEN` | 0.47 us | 24.3 us | **52.3x** |
| `HDEL` | 5.9 us | 26.9 us | **4.6x** |
| `HGETALL` | 3.7 us | 31.9 us | **8.6x** |
| `HKEYS` | 3.6 us | 23.4 us | **6.6x** |
| `HVALS` | 3.6 us | 22.6 us | **6.3x** |
| `HINCRBY` | 1.6 us | 34.1 us | **21.1x** |
| `LPUSH` | 2.2 us | 31.8 us | **14.7x** |
| `RPUSH` | 2.1 us | 31.6 us | **15.4x** |
| `LPOP` | 2.6 us | 31.7 us | **12.1x** |
| `RPOP` | 2.6 us | 29.9 us | **11.6x** |
| `LLEN` | 0.48 us | 22.4 us | **46.6x** |
| `LRANGE` | 3.8 us | 18.1 us | **4.8x** |
| `LINDEX` | 0.72 us | 22.3 us | **31.0x** |
| `LSET` | 1.2 us | 25.4 us | **21.1x** |
| `LREM` | 17.6 us | 55.1 us | **3.1x** |
| `LTRIM` | 1.1 us | 22.4 us | **19.7x** |
| `SADD` | 1.3 us | 26.4 us | **20.0x** |
| `SREM` | 5.0 us | 26.2 us | **5.2x** |
| `SISMEMBER` | 0.72 us | 22.8 us | **31.6x** |
| `SCARD` | 0.48 us | 22.6 us | **46.7x** |
| `SMEMBERS` | 4.2 us | 22.3 us | **5.3x** |
| `SPOP` | 7.4 us | 61.7 us | **8.3x** |
| `SRANDMEMBER` | 3.4 us | 22.8 us | **6.8x** |
| `ZADD` | 3.0 us | 30.4 us | **10.2x** |
| `ZSCORE` | 0.82 us | 22.3 us | **27.2x** |
| `ZRANGE` | 4.3 us | 25.4 us | **5.9x** |
| `ZCARD` | 0.51 us | 22.4 us | **43.9x** |
| `ZCOUNT` | 4.0 us | 22.9 us | **5.7x** |
| `ZINCRBY` | 3.2 us | 34.8 us | **10.9x** |
| `ZRANK` | 3.7 us | 22.5 us | **6.1x** |
| `ZREVRANGE` | 6.5 us | 23.8 us | **3.7x** |
| `ZPOPMIN` | 8.2 us | 66.6 us | **8.1x** |
| `ZREM` | 4.6 us | 25.7 us | **5.6x** |
| `SETBIT` | 12.2 us | 34.2 us | **2.8x** |
| `GETBIT` | 0.50 us | 23.8 us | **47.4x** |
| `BITCOUNT` | 0.41 us | 22.7 us | **55.9x** |
| `BITPOS` | 0.48 us | 29.4 us | **61.3x** |
| `PFADD` | 2.7 us | 26.1 us | **9.6x** |
| `PFCOUNT` | 8.3 us | 23.8 us | **2.9x** |
| `GEOADD` | 2.6 us | 35.7 us | **14.0x** |
| `GEODIST` | 0.96 us | 23.2 us | **24.1x** |
| `GEOPOS` | 0.71 us | 22.9 us | **32.0x** |
| `GEOHASH` | 0.76 us | 24.1 us | **31.6x** |
| `XADD` | 1.6 us | 34.5 us | **22.2x** |
| `XLEN` | 0.59 us | 22.4 us | **38.0x** |
| `XRANGE` | 4.8 us | 37.5 us | **7.8x** |
| `XREAD` | 4.5 us | 37.8 us | **8.4x** |
| `XDEL` | 3.6 us | 68.9 us | **19.0x** |
| `DEL` | 3.2 us | 24.1 us | **7.6x** |
| `EXISTS` | 0.28 us | 22.7 us | **82.5x** |
| `EXPIRE` | 0.77 us | 33.7 us | **43.9x** |
| `TTL` | 0.32 us | 22.2 us | **70.5x** |
| `JSON.SET` | 3.4 us | 24.2 us | **7.1x** |
| `JSON.GET` | 1.5 us | 22.4 us | **14.5x** |
| `JSON.DEL` | 9.0 us | 52.8 us | **5.8x** |
| `JSON.NUMINCRBY` | 3.8 us | 25.1 us | **6.7x** |
| `JSON.ARRLEN` | 1.4 us | 21.9 us | **16.1x** |
| `JSON.TYPE` | 1.4 us | 22.7 us | **15.7x** |
| `BF.ADD` | 58.5 us | 22.6 us | **0.4x** |
| `BF.EXISTS` | 0.99 us | 22.3 us | **22.6x** |
| `BF.INFO` | 0.58 us | 22.0 us | **37.8x** |
| `CF.ADD` | 3.6 us | 21.8 us | **6.1x** |
| `CF.EXISTS` | 0.68 us | 22.3 us | **32.5x** |
| `CF.DEL` | 9.9 us | 45.4 us | **4.6x** |
| `TDIGEST.ADD` | 2.7 us | 23.7 us | **8.9x** |
| `TDIGEST.QUANTILE` | 1.2 us | 22.3 us | **19.1x** |
| `TDIGEST.BYRANK` | 1.3 us | 22.8 us | **18.0x** |
| `TDIGEST.CDF` | 1.3 us | 21.6 us | **16.5x** |
| `TS.ADD` | 7.4 us | 22.8 us | **3.1x** |
| `TS.GET` | 1.6 us | 22.3 us | **14.1x** |
| `TS.RANGE` | 23.3 us | 26.8 us | **1.2x** |
| `TS.INCRBY` | 9.7 us | 22.3 us | **2.3x** |
| `FT.SEARCH` | 32.3 us | 22.9 us | **0.7x** |
| `FT.TAG` | 30.5 us | 22.3 us | **0.7x** |
| `VECTOR.KNN` | 6.9 us | 29.2 us | **4.2x** |

