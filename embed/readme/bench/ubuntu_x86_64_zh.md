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
| **实际物理落盘大小** | **1028 MB** | **7876 MB** | **节省 87%** |
| **进程常驻内存 (RSS)** | **248 MB** | **4875 MB** | **节省 95%** |

#### wedb_embed vs Redis 核心指令性能对比

| 指令 | wedb_embed P95延迟 | Redis P95延迟 | 性能领先 |
| :--- | :--- | :--- | :--- |
| `SET` | 6.6 us | 20.0 us | **3.0x** |
| `GET` | 4.4 us | 38.5 us | **8.8x** |
| `MSET` | 44.7 us | 30.0 us | **0.7x** |
| `MGET` | 4.3 us | 20.3 us | **4.7x** |
| `INCRBY` | 0.55 us | 38.5 us | **70.4x** |
| `DECRBY` | 0.56 us | 39.9 us | **70.9x** |
| `APPEND` | 0.65 us | 42.4 us | **65.3x** |
| `STRLEN` | 0.25 us | 20.5 us | **82.3x** |
| `GETDEL` | 7.3 us | 83.2 us | **11.4x** |
| `GETRANGE` | 0.22 us | 39.1 us | **181.3x** |
| `SETRANGE` | 0.62 us | 20.4 us | **32.7x** |
| `HSET` | 2.0 us | 48.6 us | **24.3x** |
| `HGET` | 0.56 us | 48.1 us | **86.2x** |
| `HMGET` | 2.3 us | 47.8 us | **20.6x** |
| `HEXISTS` | 0.52 us | 46.9 us | **90.2x** |
| `HLEN` | 0.35 us | 48.2 us | **138.1x** |
| `HDEL` | 3.8 us | 48.8 us | **12.8x** |
| `HGETALL` | 2.5 us | 48.5 us | **19.0x** |
| `HKEYS` | 2.4 us | 48.6 us | **20.4x** |
| `HVALS` | 2.4 us | 48.5 us | **20.3x** |
| `HINCRBY` | 1.4 us | 48.9 us | **34.2x** |
| `LPUSH` | 1.6 us | 47.6 us | **29.4x** |
| `RPUSH` | 2.0 us | 38.9 us | **19.8x** |
| `LPOP` | 1.8 us | 51.3 us | **27.7x** |
| `RPOP` | 1.9 us | 41.1 us | **21.2x** |
| `LLEN` | 0.37 us | 48.0 us | **131.2x** |
| `LRANGE` | 2.5 us | 37.3 us | **14.7x** |
| `LINDEX` | 0.54 us | 48.3 us | **89.9x** |
| `LSET` | 0.85 us | 37.3 us | **43.6x** |
| `LREM` | 13.6 us | 73.6 us | **5.4x** |
| `LTRIM` | 0.87 us | 37.6 us | **43.2x** |
| `SADD` | 1.2 us | 41.0 us | **34.3x** |
| `SREM` | 3.2 us | 41.5 us | **12.9x** |
| `SISMEMBER` | 0.56 us | 38.5 us | **68.3x** |
| `SCARD` | 0.36 us | 39.1 us | **107.5x** |
| `SMEMBERS` | 2.4 us | 39.0 us | **16.1x** |
| `SPOP` | 4.5 us | 82.9 us | **18.5x** |
| `SRANDMEMBER` | 1.9 us | 38.9 us | **20.4x** |
| `ZADD` | 2.3 us | 20.8 us | **9.1x** |
| `ZSCORE` | 0.67 us | 20.7 us | **30.9x** |
| `ZRANGE` | 2.9 us | 19.6 us | **6.7x** |
| `ZCARD` | 0.42 us | 20.9 us | **49.5x** |
| `ZCOUNT` | 2.4 us | 20.1 us | **8.3x** |
| `ZINCRBY` | 2.2 us | 20.4 us | **9.1x** |
| `ZRANK` | 2.6 us | 20.7 us | **8.1x** |
| `ZREVRANGE` | 3.8 us | 20.6 us | **5.4x** |
| `ZPOPMIN` | 10.4 us | 41.5 us | **4.0x** |
| `ZREM` | 3.6 us | 20.6 us | **5.8x** |
| `SETBIT` | 11.6 us | 49.0 us | **4.2x** |
| `GETBIT` | 0.37 us | 48.0 us | **128.5x** |
| `BITCOUNT` | 0.36 us | 44.0 us | **122.3x** |
| `BITPOS` | 0.37 us | 46.0 us | **123.9x** |
| `PFADD` | 2.4 us | 48.5 us | **20.4x** |
| `PFCOUNT` | 29.6 us | 48.0 us | **1.6x** |
| `GEOADD` | 2.0 us | 48.6 us | **23.8x** |
| `GEODIST` | 0.81 us | 48.5 us | **59.7x** |
| `GEOPOS` | 0.57 us | 48.5 us | **84.5x** |
| `GEOHASH` | 0.61 us | 48.2 us | **78.9x** |
| `XADD` | 1.3 us | 19.9 us | **15.1x** |
| `XLEN` | 0.46 us | 19.9 us | **43.5x** |
| `XRANGE` | 2.8 us | 26.8 us | **9.5x** |
| `XREAD` | 2.8 us | 28.5 us | **10.3x** |
| `XDEL` | 3.1 us | 44.4 us | **14.5x** |
| `DEL` | 3.1 us | 48.1 us | **15.6x** |
| `EXISTS` | 0.20 us | 48.3 us | **243.2x** |
| `EXPIRE` | 0.82 us | 49.0 us | **59.5x** |
| `TTL` | 0.22 us | 48.1 us | **222.7x** |
| `JSON.SET` | 2.7 us | 48.8 us | **18.0x** |
| `JSON.GET` | 1.1 us | 47.3 us | **42.4x** |
| `JSON.DEL` | 5.7 us | 95.8 us | **16.9x** |
| `JSON.NUMINCRBY` | 2.9 us | 48.1 us | **16.7x** |
| `JSON.ARRLEN` | 1.00 us | 47.6 us | **47.7x** |
| `JSON.TYPE` | 1.0 us | 48.8 us | **46.9x** |
| `BF.ADD` | 11.1 us | 47.4 us | **4.3x** |
| `BF.EXISTS` | 0.54 us | 48.3 us | **90.1x** |
| `BF.INFO` | 0.33 us | 47.9 us | **143.9x** |
| `CF.ADD` | 2.6 us | 47.0 us | **18.1x** |
| `CF.EXISTS` | 0.56 us | 47.1 us | **84.0x** |
| `CF.DEL` | 5.8 us | 96.9 us | **16.9x** |
| `TDIGEST.ADD` | 1.9 us | 20.3 us | **10.4x** |
| `TDIGEST.QUANTILE` | 0.78 us | 20.0 us | **25.5x** |
| `TDIGEST.BYRANK` | 0.88 us | 20.4 us | **23.2x** |
| `TDIGEST.CDF` | 0.95 us | 19.9 us | **20.9x** |
| `TS.ADD` | 6.7 us | 20.3 us | **3.0x** |
| `TS.GET` | 1.1 us | 20.7 us | **19.0x** |
| `TS.RANGE` | 21.3 us | 19.8 us | **0.9x** |
| `TS.INCRBY` | 6.2 us | 20.1 us | **3.3x** |
| `FT.SEARCH` | 17.0 us | 41.2 us | **2.4x** |
| `FT.TAG` | 16.9 us | 38.9 us | **2.3x** |
| `VECTOR.KNN` | 2.7 us | 20.2 us | **7.4x** |

