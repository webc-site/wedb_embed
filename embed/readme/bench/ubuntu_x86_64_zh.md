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
| **实际物理落盘大小** | **1053 MB** | **7656 MB** | **节省 86%** |
| **进程常驻内存 (RSS)** | **286 MB** | **4822 MB** | **节省 94%** |

#### wedb_embed vs Redis 核心指令性能对比

| 指令 | wedb_embed P95延迟 | Redis P95延迟 | 性能领先 |
| :--- | :--- | :--- | :--- |
| `SET` | 7.4 us | 30.1 us | **4.0x** |
| `GET` | 5.0 us | 19.3 us | **3.9x** |
| `MSET` | 49.3 us | 34.6 us | **0.7x** |
| `MGET` | 4.9 us | 26.2 us | **5.4x** |
| `INCRBY` | 1.0 us | 27.8 us | **26.7x** |
| `DECRBY` | 1.0 us | 27.8 us | **26.7x** |
| `APPEND` | 0.83 us | 27.2 us | **32.9x** |
| `STRLEN` | 0.32 us | 19.5 us | **60.3x** |
| `GETDEL` | 8.1 us | 58.7 us | **7.3x** |
| `GETRANGE` | 0.27 us | 19.0 us | **70.0x** |
| `SETRANGE` | 1.2 us | 29.4 us | **24.0x** |
| `HSET` | 2.9 us | 30.7 us | **10.7x** |
| `HGET` | 0.65 us | 20.4 us | **31.1x** |
| `HMGET` | 2.9 us | 26.0 us | **9.0x** |
| `HEXISTS` | 0.66 us | 20.9 us | **31.8x** |
| `HLEN` | 0.40 us | 20.8 us | **51.9x** |
| `HDEL` | 3.9 us | 27.8 us | **7.0x** |
| `HGETALL` | 3.1 us | 19.0 us | **6.1x** |
| `HKEYS` | 3.0 us | 18.9 us | **6.4x** |
| `HVALS` | 3.1 us | 19.9 us | **6.5x** |
| `HINCRBY` | 1.7 us | 31.4 us | **18.1x** |
| `LPUSH` | 2.3 us | 28.3 us | **12.2x** |
| `RPUSH` | 3.2 us | 30.8 us | **9.6x** |
| `LPOP` | 2.5 us | 23.8 us | **9.5x** |
| `RPOP` | 2.5 us | 30.3 us | **12.0x** |
| `LLEN` | 0.42 us | 20.8 us | **49.5x** |
| `LRANGE` | 3.7 us | 18.8 us | **5.1x** |
| `LINDEX` | 0.63 us | 19.1 us | **30.1x** |
| `LSET` | 1.1 us | 31.1 us | **29.6x** |
| `LREM` | 16.3 us | 63.2 us | **3.9x** |
| `LTRIM` | 1.1 us | 19.9 us | **18.4x** |
| `SADD` | 1.3 us | 28.8 us | **22.7x** |
| `SREM` | 3.6 us | 24.4 us | **6.8x** |
| `SISMEMBER` | 0.71 us | 19.1 us | **26.9x** |
| `SCARD` | 0.42 us | 19.3 us | **45.4x** |
| `SMEMBERS` | 3.6 us | 18.8 us | **5.2x** |
| `SPOP` | 6.5 us | 58.6 us | **9.0x** |
| `SRANDMEMBER` | 3.2 us | 19.2 us | **6.0x** |
| `ZADD` | 3.0 us | 29.9 us | **10.0x** |
| `ZSCORE` | 0.85 us | 19.2 us | **22.7x** |
| `ZRANGE` | 3.9 us | 19.2 us | **4.9x** |
| `ZCARD` | 0.50 us | 19.3 us | **38.7x** |
| `ZCOUNT` | 3.3 us | 19.2 us | **5.9x** |
| `ZINCRBY` | 2.9 us | 31.1 us | **10.7x** |
| `ZRANK` | 3.4 us | 19.7 us | **5.9x** |
| `ZREVRANGE` | 6.7 us | 19.8 us | **3.0x** |
| `ZPOPMIN` | 7.9 us | 61.6 us | **7.8x** |
| `ZREM` | 4.5 us | 23.4 us | **5.2x** |
| `SETBIT` | 11.4 us | 43.7 us | **3.8x** |
| `GETBIT` | 0.46 us | 36.6 us | **79.8x** |
| `BITCOUNT` | 0.36 us | 26.6 us | **74.3x** |
| `BITPOS` | 0.69 us | 37.0 us | **54.0x** |
| `PFADD` | 2.7 us | 23.1 us | **8.5x** |
| `PFCOUNT` | 8.1 us | 20.8 us | **2.6x** |
| `GEOADD` | 2.5 us | 32.4 us | **13.1x** |
| `GEODIST` | 1.1 us | 19.0 us | **17.7x** |
| `GEOPOS` | 0.74 us | 20.9 us | **28.4x** |
| `GEOHASH` | 0.77 us | 20.8 us | **27.1x** |
| `XADD` | 1.6 us | 30.4 us | **18.5x** |
| `XLEN` | 0.57 us | 19.1 us | **33.6x** |
| `XRANGE` | 4.1 us | 31.9 us | **7.8x** |
| `XREAD` | 4.2 us | 32.3 us | **7.8x** |
| `XDEL` | 3.7 us | 64.1 us | **17.5x** |
| `DEL` | 3.1 us | 20.8 us | **6.8x** |
| `EXISTS` | 0.26 us | 20.7 us | **81.3x** |
| `EXPIRE` | 0.88 us | 30.6 us | **34.9x** |
| `TTL` | 0.27 us | 20.8 us | **77.8x** |
| `JSON.SET` | 3.6 us | 20.7 us | **5.7x** |
| `JSON.GET` | 1.6 us | 19.3 us | **12.2x** |
| `JSON.DEL` | 8.1 us | 39.6 us | **4.9x** |
| `JSON.NUMINCRBY` | 3.9 us | 21.0 us | **5.4x** |
| `JSON.ARRLEN` | 1.4 us | 20.9 us | **15.4x** |
| `JSON.TYPE` | 1.4 us | 20.7 us | **14.8x** |
| `BF.ADD` | 36.9 us | 29.8 us | **0.8x** |
| `BF.EXISTS` | 0.62 us | 21.1 us | **34.2x** |
| `BF.INFO` | 0.36 us | 20.8 us | **57.8x** |
| `CF.ADD` | 2.6 us | 20.4 us | **7.8x** |
| `CF.EXISTS` | 0.82 us | 21.5 us | **26.1x** |
| `CF.DEL` | 9.7 us | 39.3 us | **4.0x** |
| `TDIGEST.ADD` | 2.8 us | 18.8 us | **6.7x** |
| `TDIGEST.QUANTILE` | 1.1 us | 19.9 us | **17.7x** |
| `TDIGEST.BYRANK` | 1.2 us | 19.5 us | **16.4x** |
| `TDIGEST.CDF` | 1.2 us | 18.7 us | **15.0x** |
| `TS.ADD` | 11.8 us | 19.0 us | **1.6x** |
| `TS.GET` | 1.5 us | 18.6 us | **12.6x** |
| `TS.RANGE` | 20.7 us | 19.0 us | **0.9x** |
| `TS.INCRBY` | 9.1 us | 18.5 us | **2.0x** |
| `FT.SEARCH` | 28.3 us | 19.5 us | **0.7x** |
| `FT.TAG` | 28.1 us | 19.0 us | **0.7x** |
| `VECTOR.KNN` | 4.6 us | 24.0 us | **5.2x** |

