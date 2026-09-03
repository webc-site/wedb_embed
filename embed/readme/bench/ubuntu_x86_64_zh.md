### Ubuntu CI (GitHub Actions Runner)

#### 硬件与测试环境

CPU: AMD EPYC 9V74 80-Core Processor (4核)<br>
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
| **实际物理落盘大小** | **1036 MB** | **7295 MB** | **节省 86%** |
| **进程常驻内存 (RSS)** | **273 MB** | **4805 MB** | **节省 94%** |

#### wedb_embed vs Redis 核心指令性能对比

| 指令 | wedb_embed P95延迟 | Redis P95延迟 | 性能领先 |
| :--- | :--- | :--- | :--- |
| `SET` | 7.7 us | 28.8 us | **3.7x** |
| `GET` | 5.3 us | 20.9 us | **4.0x** |
| `MSET` | 52.2 us | 32.1 us | **0.6x** |
| `MGET` | 5.8 us | 20.0 us | **3.4x** |
| `INCRBY` | 0.78 us | 26.2 us | **33.4x** |
| `DECRBY` | 0.69 us | 28.6 us | **41.5x** |
| `APPEND` | 0.86 us | 25.7 us | **30.0x** |
| `STRLEN` | 0.31 us | 20.7 us | **66.5x** |
| `GETDEL` | 8.6 us | 58.2 us | **6.7x** |
| `GETRANGE` | 0.30 us | 20.9 us | **69.2x** |
| `SETRANGE` | 0.98 us | 28.8 us | **29.3x** |
| `HSET` | 2.5 us | 29.0 us | **11.5x** |
| `HGET` | 0.69 us | 20.4 us | **29.7x** |
| `HMGET` | 3.3 us | 19.8 us | **6.1x** |
| `HEXISTS` | 0.66 us | 20.5 us | **31.1x** |
| `HLEN` | 0.44 us | 20.2 us | **46.3x** |
| `HDEL` | 4.1 us | 20.8 us | **5.1x** |
| `HGETALL` | 3.7 us | 19.7 us | **5.3x** |
| `HKEYS` | 3.5 us | 20.3 us | **5.7x** |
| `HVALS` | 3.5 us | 19.9 us | **5.6x** |
| `HINCRBY` | 2.0 us | 29.1 us | **14.3x** |
| `LPUSH` | 2.3 us | 28.4 us | **12.1x** |
| `RPUSH` | 2.7 us | 28.4 us | **10.5x** |
| `LPOP` | 2.6 us | 28.1 us | **10.7x** |
| `RPOP` | 2.8 us | 28.0 us | **10.1x** |
| `LLEN` | 0.44 us | 20.6 us | **46.9x** |
| `LRANGE` | 3.9 us | 21.0 us | **5.4x** |
| `LINDEX` | 0.67 us | 20.2 us | **29.9x** |
| `LSET` | 1.1 us | 29.2 us | **26.0x** |
| `LREM` | 18.2 us | 58.2 us | **3.2x** |
| `LTRIM` | 1.1 us | 21.0 us | **18.8x** |
| `SADD` | 1.4 us | 24.4 us | **17.6x** |
| `SREM` | 4.6 us | 21.0 us | **4.6x** |
| `SISMEMBER` | 0.67 us | 21.0 us | **31.3x** |
| `SCARD` | 0.46 us | 20.6 us | **45.0x** |
| `SMEMBERS` | 3.6 us | 21.1 us | **5.8x** |
| `SPOP` | 6.6 us | 53.3 us | **8.1x** |
| `SRANDMEMBER` | 2.3 us | 20.7 us | **8.9x** |
| `ZADD` | 3.1 us | 24.6 us | **7.9x** |
| `ZSCORE` | 0.85 us | 21.8 us | **25.7x** |
| `ZRANGE` | 4.3 us | 20.4 us | **4.8x** |
| `ZCARD` | 0.51 us | 20.9 us | **40.6x** |
| `ZCOUNT` | 3.8 us | 21.1 us | **5.6x** |
| `ZINCRBY` | 4.3 us | 28.1 us | **6.5x** |
| `ZRANK` | 3.8 us | 21.2 us | **5.6x** |
| `ZREVRANGE` | 6.0 us | 20.6 us | **3.4x** |
| `ZPOPMIN` | 15.1 us | 53.7 us | **3.6x** |
| `ZREM` | 5.4 us | 23.4 us | **4.3x** |
| `SETBIT` | 12.4 us | 39.1 us | **3.2x** |
| `GETBIT` | 0.69 us | 28.1 us | **40.7x** |
| `BITCOUNT` | 0.40 us | 38.2 us | **96.0x** |
| `BITPOS` | 0.46 us | 21.9 us | **47.5x** |
| `PFADD` | 2.7 us | 20.6 us | **7.7x** |
| `PFCOUNT` | 8.3 us | 20.7 us | **2.5x** |
| `GEOADD` | 2.5 us | 23.7 us | **9.3x** |
| `GEODIST` | 1.0 us | 20.9 us | **20.3x** |
| `GEOPOS` | 0.80 us | 20.0 us | **24.9x** |
| `GEOHASH` | 0.86 us | 20.3 us | **23.7x** |
| `XADD` | 1.6 us | 29.4 us | **18.3x** |
| `XLEN` | 0.59 us | 20.6 us | **34.7x** |
| `XRANGE` | 4.4 us | 28.6 us | **6.5x** |
| `XREAD` | 4.2 us | 40.3 us | **9.5x** |
| `XDEL` | 3.7 us | 56.9 us | **15.3x** |
| `DEL` | 3.3 us | 20.4 us | **6.3x** |
| `EXISTS` | 0.28 us | 20.8 us | **74.5x** |
| `EXPIRE` | 1.1 us | 28.9 us | **27.3x** |
| `TTL` | 0.32 us | 20.4 us | **63.1x** |
| `JSON.SET` | 3.6 us | 20.7 us | **5.8x** |
| `JSON.GET` | 1.5 us | 20.0 us | **13.1x** |
| `JSON.DEL` | 7.2 us | 38.6 us | **5.4x** |
| `JSON.NUMINCRBY` | 3.8 us | 19.6 us | **5.1x** |
| `JSON.ARRLEN` | 1.4 us | 19.6 us | **14.2x** |
| `JSON.TYPE` | 1.5 us | 20.0 us | **13.2x** |
| `BF.ADD` | 34.9 us | 20.4 us | **0.6x** |
| `BF.EXISTS` | 0.67 us | 20.2 us | **30.3x** |
| `BF.INFO` | 0.50 us | 20.7 us | **41.7x** |
| `CF.ADD` | 2.8 us | 23.4 us | **8.3x** |
| `CF.EXISTS` | 0.72 us | 20.0 us | **27.9x** |
| `CF.DEL` | 10.4 us | 39.5 us | **3.8x** |
| `TDIGEST.ADD` | 3.0 us | 21.0 us | **6.9x** |
| `TDIGEST.QUANTILE` | 1.3 us | 19.1 us | **14.6x** |
| `TDIGEST.BYRANK` | 1.2 us | 20.8 us | **17.5x** |
| `TDIGEST.CDF` | 1.4 us | 20.5 us | **15.0x** |
| `TS.ADD` | 7.6 us | 20.6 us | **2.7x** |
| `TS.GET` | 1.6 us | 20.4 us | **13.1x** |
| `TS.RANGE` | 23.0 us | 21.0 us | **0.9x** |
| `TS.INCRBY` | 9.8 us | 21.2 us | **2.2x** |
| `FT.SEARCH` | 29.9 us | 20.1 us | **0.7x** |
| `FT.TAG` | 29.6 us | 21.3 us | **0.7x** |
| `VECTOR.KNN` | 4.4 us | 19.3 us | **4.4x** |

