### Ubuntu CI (GitHub Actions Runner)

#### 硬件与测试环境

CPU: Intel(R) Xeon(R) 6973P-C (4核)<br>
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
| **实际物理落盘大小** | **1053 MB** | **7976 MB** | **节省 87%** |
| **进程常驻内存 (RSS)** | **264 MB** | **4846 MB** | **节省 95%** |

#### wedb_embed vs Redis 核心指令性能对比

| 指令 | wedb_embed P95延迟 | Redis P95延迟 | 性能领先 |
| :--- | :--- | :--- | :--- |
| `SET` | 6.9 us | 20.8 us | **3.0x** |
| `GET` | 4.4 us | 20.9 us | **4.7x** |
| `MSET` | 45.8 us | 29.6 us | **0.6x** |
| `MGET` | 4.3 us | 21.0 us | **4.9x** |
| `INCRBY` | 0.55 us | 20.4 us | **37.0x** |
| `DECRBY` | 0.56 us | 20.2 us | **36.0x** |
| `APPEND` | 0.65 us | 20.4 us | **31.1x** |
| `STRLEN` | 0.24 us | 20.9 us | **87.4x** |
| `GETDEL` | 7.5 us | 41.2 us | **5.5x** |
| `GETRANGE` | 0.22 us | 21.1 us | **94.9x** |
| `SETRANGE` | 0.63 us | 21.4 us | **34.1x** |
| `HSET` | 2.0 us | 48.3 us | **24.0x** |
| `HGET` | 0.57 us | 47.2 us | **82.6x** |
| `HMGET` | 2.4 us | 47.8 us | **19.6x** |
| `HEXISTS` | 0.54 us | 46.1 us | **85.9x** |
| `HLEN` | 0.37 us | 46.1 us | **124.0x** |
| `HDEL` | 3.9 us | 48.2 us | **12.5x** |
| `HGETALL` | 2.6 us | 47.1 us | **18.3x** |
| `HKEYS` | 2.5 us | 47.8 us | **19.4x** |
| `HVALS` | 2.5 us | 47.4 us | **19.0x** |
| `HINCRBY` | 1.5 us | 47.5 us | **32.7x** |
| `LPUSH` | 1.6 us | 48.3 us | **29.2x** |
| `RPUSH` | 1.6 us | 37.1 us | **23.5x** |
| `LPOP` | 1.9 us | 52.5 us | **27.7x** |
| `RPOP` | 2.0 us | 41.4 us | **21.2x** |
| `LLEN` | 0.36 us | 47.3 us | **131.2x** |
| `LRANGE` | 2.5 us | 47.8 us | **19.3x** |
| `LINDEX` | 0.54 us | 47.7 us | **88.2x** |
| `LSET` | 0.86 us | 37.1 us | **43.3x** |
| `LREM` | 13.4 us | 95.1 us | **7.1x** |
| `LTRIM` | 0.88 us | 36.6 us | **41.8x** |
| `SADD` | 1.0 us | 36.9 us | **35.2x** |
| `SREM` | 3.2 us | 20.4 us | **6.4x** |
| `SISMEMBER` | 0.57 us | 36.7 us | **64.4x** |
| `SCARD` | 0.38 us | 36.4 us | **94.5x** |
| `SMEMBERS` | 2.5 us | 36.1 us | **14.4x** |
| `SPOP` | 4.7 us | 51.4 us | **10.9x** |
| `SRANDMEMBER` | 2.0 us | 20.1 us | **10.0x** |
| `ZADD` | 2.2 us | 21.5 us | **9.6x** |
| `ZSCORE` | 0.64 us | 21.0 us | **32.7x** |
| `ZRANGE` | 3.0 us | 21.3 us | **7.1x** |
| `ZCARD` | 0.41 us | 19.6 us | **47.6x** |
| `ZCOUNT` | 2.4 us | 20.8 us | **8.5x** |
| `ZINCRBY` | 2.4 us | 21.3 us | **8.8x** |
| `ZRANK` | 2.5 us | 20.6 us | **8.2x** |
| `ZREVRANGE` | 3.8 us | 20.3 us | **5.3x** |
| `ZPOPMIN` | 6.2 us | 41.7 us | **6.7x** |
| `ZREM` | 3.6 us | 20.7 us | **5.7x** |
| `SETBIT` | 11.5 us | 48.9 us | **4.2x** |
| `GETBIT` | 0.39 us | 47.0 us | **119.6x** |
| `BITCOUNT` | 0.36 us | 42.7 us | **118.4x** |
| `BITPOS` | 0.38 us | 45.5 us | **119.1x** |
| `PFADD` | 2.3 us | 47.3 us | **20.6x** |
| `PFCOUNT` | 29.9 us | 47.8 us | **1.6x** |
| `GEOADD` | 2.1 us | 48.5 us | **23.5x** |
| `GEODIST` | 0.83 us | 47.3 us | **57.2x** |
| `GEOPOS` | 0.78 us | 47.0 us | **60.2x** |
| `GEOHASH` | 0.63 us | 47.7 us | **76.3x** |
| `XADD` | 1.4 us | 19.9 us | **14.7x** |
| `XLEN` | 0.45 us | 19.8 us | **44.4x** |
| `XRANGE` | 2.9 us | 27.7 us | **9.7x** |
| `XREAD` | 2.8 us | 29.3 us | **10.3x** |
| `XDEL` | 3.1 us | 43.6 us | **14.3x** |
| `DEL` | 3.1 us | 46.9 us | **15.3x** |
| `EXISTS` | 0.21 us | 47.1 us | **227.4x** |
| `EXPIRE` | 0.76 us | 48.4 us | **63.8x** |
| `TTL` | 0.22 us | 47.3 us | **212.3x** |
| `JSON.SET` | 2.6 us | 47.1 us | **17.9x** |
| `JSON.GET` | 1.1 us | 46.9 us | **42.6x** |
| `JSON.DEL` | 6.3 us | 95.5 us | **15.2x** |
| `JSON.NUMINCRBY` | 2.8 us | 46.9 us | **17.0x** |
| `JSON.ARRLEN` | 0.99 us | 46.6 us | **46.9x** |
| `JSON.TYPE` | 0.99 us | 46.8 us | **47.2x** |
| `BF.ADD` | 10.7 us | 48.1 us | **4.5x** |
| `BF.EXISTS` | 0.55 us | 47.4 us | **85.9x** |
| `BF.INFO` | 0.34 us | 46.4 us | **136.1x** |
| `CF.ADD` | 2.1 us | 48.0 us | **22.4x** |
| `CF.EXISTS` | 0.63 us | 47.6 us | **75.6x** |
| `CF.DEL` | 5.7 us | 96.5 us | **16.8x** |
| `TDIGEST.ADD` | 2.0 us | 19.5 us | **9.5x** |
| `TDIGEST.QUANTILE` | 0.81 us | 20.5 us | **25.4x** |
| `TDIGEST.BYRANK` | 0.87 us | 20.3 us | **23.3x** |
| `TDIGEST.CDF` | 0.93 us | 20.0 us | **21.5x** |
| `TS.ADD` | 5.0 us | 20.5 us | **4.1x** |
| `TS.GET` | 1.0 us | 21.0 us | **20.2x** |
| `TS.RANGE` | 21.9 us | 20.5 us | **0.9x** |
| `TS.INCRBY` | 6.8 us | 20.4 us | **3.0x** |
| `FT.SEARCH` | 18.1 us | 36.6 us | **2.0x** |
| `FT.TAG` | 17.8 us | 36.1 us | **2.0x** |
| `VECTOR.KNN` | 4.7 us | 21.0 us | **4.5x** |

