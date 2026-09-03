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
| **实际物理落盘大小** | **1053 MB** | **7662 MB** | **节省 86%** |
| **进程常驻内存 (RSS)** | **280 MB** | **4814 MB** | **节省 94%** |

#### wedb_embed vs Redis 核心指令性能对比

| 指令 | wedb_embed P95延迟 | Redis P95延迟 | 性能领先 |
| :--- | :--- | :--- | :--- |
| `SET` | 7.3 us | 44.6 us | **6.1x** |
| `GET` | 5.1 us | 19.6 us | **3.8x** |
| `MSET` | 49.8 us | 49.4 us | **1.0x** |
| `MGET` | 5.3 us | 40.9 us | **7.7x** |
| `INCRBY` | 0.79 us | 44.0 us | **55.6x** |
| `DECRBY` | 0.75 us | 26.9 us | **35.7x** |
| `APPEND` | 1.3 us | 27.4 us | **21.7x** |
| `STRLEN` | 0.27 us | 32.8 us | **119.4x** |
| `GETDEL` | 8.4 us | 57.9 us | **6.9x** |
| `GETRANGE` | 0.35 us | 19.1 us | **54.0x** |
| `SETRANGE` | 0.96 us | 42.5 us | **44.2x** |
| `HSET` | 3.2 us | 28.8 us | **9.0x** |
| `HGET` | 1.2 us | 20.7 us | **17.5x** |
| `HMGET` | 3.3 us | 27.5 us | **8.3x** |
| `HEXISTS` | 0.65 us | 20.5 us | **31.6x** |
| `HLEN` | 0.45 us | 20.7 us | **46.0x** |
| `HDEL` | 4.9 us | 22.9 us | **4.7x** |
| `HGETALL` | 3.2 us | 19.5 us | **6.1x** |
| `HKEYS` | 3.3 us | 18.6 us | **5.7x** |
| `HVALS` | 3.2 us | 18.5 us | **5.8x** |
| `HINCRBY` | 2.2 us | 31.6 us | **14.5x** |
| `LPUSH` | 3.0 us | 28.2 us | **9.5x** |
| `RPUSH` | 2.5 us | 28.0 us | **11.3x** |
| `LPOP` | 2.6 us | 25.2 us | **9.7x** |
| `RPOP` | 2.4 us | 28.8 us | **11.8x** |
| `LLEN` | 0.47 us | 20.4 us | **43.5x** |
| `LRANGE` | 3.5 us | 19.3 us | **5.6x** |
| `LINDEX` | 0.68 us | 18.9 us | **27.6x** |
| `LSET` | 1.2 us | 29.1 us | **24.5x** |
| `LREM` | 18.4 us | 60.1 us | **3.3x** |
| `LTRIM` | 1.1 us | 21.1 us | **18.6x** |
| `SADD` | 1.3 us | 28.4 us | **21.1x** |
| `SREM` | 4.5 us | 22.5 us | **5.0x** |
| `SISMEMBER` | 0.71 us | 19.1 us | **26.9x** |
| `SCARD` | 0.47 us | 18.6 us | **40.1x** |
| `SMEMBERS` | 3.4 us | 19.0 us | **5.5x** |
| `SPOP` | 6.3 us | 56.5 us | **9.0x** |
| `SRANDMEMBER` | 2.5 us | 19.1 us | **7.5x** |
| `ZADD` | 3.3 us | 29.0 us | **8.9x** |
| `ZSCORE` | 0.78 us | 34.5 us | **44.1x** |
| `ZRANGE` | 3.9 us | 19.6 us | **5.0x** |
| `ZCARD` | 0.49 us | 19.1 us | **38.6x** |
| `ZCOUNT` | 3.3 us | 18.8 us | **5.6x** |
| `ZINCRBY` | 3.5 us | 30.8 us | **8.7x** |
| `ZRANK` | 3.5 us | 28.2 us | **8.0x** |
| `ZREVRANGE` | 6.0 us | 37.6 us | **6.3x** |
| `ZPOPMIN` | 14.7 us | 62.8 us | **4.3x** |
| `ZREM` | 4.5 us | 40.5 us | **9.1x** |
| `SETBIT` | 11.5 us | 34.6 us | **3.0x** |
| `GETBIT` | 0.45 us | 25.9 us | **57.1x** |
| `BITCOUNT` | 0.39 us | 31.7 us | **82.0x** |
| `BITPOS` | 0.50 us | 25.2 us | **50.4x** |
| `PFADD` | 2.8 us | 22.8 us | **8.2x** |
| `PFCOUNT` | 8.1 us | 20.6 us | **2.6x** |
| `GEOADD` | 2.6 us | 31.6 us | **12.0x** |
| `GEODIST` | 1.5 us | 20.2 us | **13.3x** |
| `GEOPOS` | 1.2 us | 19.2 us | **16.0x** |
| `GEOHASH` | 0.75 us | 21.2 us | **28.1x** |
| `XADD` | 1.6 us | 44.4 us | **27.5x** |
| `XLEN` | 0.58 us | 32.3 us | **56.2x** |
| `XRANGE` | 3.9 us | 37.8 us | **9.7x** |
| `XREAD` | 3.8 us | 32.3 us | **8.4x** |
| `XDEL` | 3.8 us | 90.2 us | **23.7x** |
| `DEL` | 3.1 us | 20.7 us | **6.8x** |
| `EXISTS` | 0.25 us | 20.7 us | **84.4x** |
| `EXPIRE` | 0.75 us | 30.6 us | **40.9x** |
| `TTL` | 0.29 us | 20.6 us | **70.3x** |
| `JSON.SET` | 3.6 us | 28.2 us | **7.8x** |
| `JSON.GET` | 1.5 us | 19.2 us | **12.5x** |
| `JSON.DEL` | 8.4 us | 38.1 us | **4.5x** |
| `JSON.NUMINCRBY` | 3.8 us | 18.7 us | **4.9x** |
| `JSON.ARRLEN` | 1.3 us | 19.1 us | **14.3x** |
| `JSON.TYPE` | 1.5 us | 36.1 us | **24.6x** |
| `BF.ADD` | 31.7 us | 28.8 us | **0.9x** |
| `BF.EXISTS` | 0.66 us | 30.4 us | **46.1x** |
| `BF.INFO` | 0.36 us | 38.3 us | **106.0x** |
| `CF.ADD` | 2.5 us | 28.7 us | **11.4x** |
| `CF.EXISTS` | 0.64 us | 19.3 us | **30.0x** |
| `CF.DEL` | 10.3 us | 37.9 us | **3.7x** |
| `TDIGEST.ADD` | 2.8 us | 19.6 us | **7.0x** |
| `TDIGEST.QUANTILE` | 1.2 us | 18.8 us | **16.3x** |
| `TDIGEST.BYRANK` | 1.2 us | 19.1 us | **16.0x** |
| `TDIGEST.CDF` | 1.3 us | 18.6 us | **14.2x** |
| `TS.ADD` | 7.4 us | 18.5 us | **2.5x** |
| `TS.GET` | 1.5 us | 19.2 us | **12.5x** |
| `TS.RANGE` | 20.8 us | 19.9 us | **1.0x** |
| `TS.INCRBY` | 10.4 us | 18.8 us | **1.8x** |
| `FT.SEARCH` | 28.7 us | 19.0 us | **0.7x** |
| `FT.TAG` | 28.3 us | 19.0 us | **0.7x** |
| `VECTOR.KNN` | 5.9 us | 28.2 us | **4.8x** |

