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
| **实际物理落盘大小** | **1053 MB** | **7965 MB** | **节省 87%** |
| **进程常驻内存 (RSS)** | **283 MB** | **4838 MB** | **节省 94%** |

#### wedb_embed vs Redis 核心指令性能对比

| 指令 | wedb_embed P95延迟 | Redis P95延迟 | 性能领先 |
| :--- | :--- | :--- | :--- |
| `SET` | 9.9 us | 21.5 us | **2.2x** |
| `GET` | 7.0 us | 23.8 us | **3.4x** |
| `MSET` | 67.1 us | 37.5 us | **0.6x** |
| `MGET` | 6.7 us | 27.6 us | **4.1x** |
| `INCRBY` | 0.87 us | 22.1 us | **25.4x** |
| `DECRBY` | 1.1 us | 23.1 us | **20.9x** |
| `APPEND` | 1.4 us | 22.6 us | **16.5x** |
| `STRLEN` | 0.39 us | 20.1 us | **51.9x** |
| `GETDEL` | 10.8 us | 48.4 us | **4.5x** |
| `GETRANGE` | 0.39 us | 22.6 us | **57.5x** |
| `SETRANGE` | 1.0 us | 20.7 us | **19.9x** |
| `HSET` | 2.4 us | 28.4 us | **11.8x** |
| `HGET` | 0.87 us | 28.6 us | **33.1x** |
| `HMGET` | 4.0 us | 25.8 us | **6.5x** |
| `HEXISTS` | 0.84 us | 26.5 us | **31.6x** |
| `HLEN` | 0.55 us | 28.0 us | **50.6x** |
| `HDEL` | 5.7 us | 28.3 us | **5.0x** |
| `HGETALL` | 3.7 us | 28.7 us | **7.7x** |
| `HKEYS` | 3.6 us | 26.2 us | **7.3x** |
| `HVALS` | 3.6 us | 27.4 us | **7.6x** |
| `HINCRBY` | 2.2 us | 27.5 us | **12.4x** |
| `LPUSH` | 2.4 us | 27.9 us | **11.6x** |
| `RPUSH` | 2.3 us | 22.1 us | **9.7x** |
| `LPOP` | 2.8 us | 27.3 us | **9.6x** |
| `RPOP` | 2.8 us | 27.2 us | **9.7x** |
| `LLEN` | 0.57 us | 30.0 us | **52.7x** |
| `LRANGE` | 3.7 us | 29.9 us | **8.0x** |
| `LINDEX` | 0.82 us | 25.3 us | **30.8x** |
| `LSET` | 1.4 us | 26.0 us | **18.4x** |
| `LREM` | 21.2 us | 56.3 us | **2.7x** |
| `LTRIM` | 1.4 us | 25.8 us | **19.1x** |
| `SADD` | 1.7 us | 22.0 us | **12.7x** |
| `SREM` | 4.6 us | 21.8 us | **4.7x** |
| `SISMEMBER` | 0.90 us | 22.8 us | **25.3x** |
| `SCARD` | 0.57 us | 25.9 us | **45.4x** |
| `SMEMBERS` | 4.2 us | 24.6 us | **5.9x** |
| `SPOP` | 7.8 us | 54.1 us | **7.0x** |
| `SRANDMEMBER` | 3.7 us | 22.6 us | **6.2x** |
| `ZADD` | 3.4 us | 21.4 us | **6.3x** |
| `ZSCORE` | 1.00 us | 21.3 us | **21.3x** |
| `ZRANGE` | 4.1 us | 20.9 us | **5.1x** |
| `ZCARD` | 0.60 us | 20.7 us | **34.4x** |
| `ZCOUNT` | 3.6 us | 21.5 us | **6.0x** |
| `ZINCRBY` | 3.6 us | 24.2 us | **6.6x** |
| `ZRANK` | 3.7 us | 20.9 us | **5.6x** |
| `ZREVRANGE` | 6.0 us | 20.6 us | **3.5x** |
| `ZPOPMIN` | 8.7 us | 49.9 us | **5.7x** |
| `ZREM` | 5.2 us | 21.1 us | **4.0x** |
| `SETBIT` | 17.2 us | 24.7 us | **1.4x** |
| `GETBIT` | 0.57 us | 28.1 us | **49.6x** |
| `BITCOUNT` | 0.52 us | 30.5 us | **58.3x** |
| `BITPOS` | 0.57 us | 27.9 us | **49.1x** |
| `PFADD` | 3.1 us | 28.2 us | **9.0x** |
| `PFCOUNT` | 41.6 us | 27.5 us | **0.7x** |
| `GEOADD` | 3.1 us | 29.8 us | **9.5x** |
| `GEODIST` | 1.6 us | 26.5 us | **16.9x** |
| `GEOPOS` | 0.88 us | 29.0 us | **33.1x** |
| `GEOHASH` | 0.90 us | 27.0 us | **30.0x** |
| `XADD` | 2.0 us | 28.3 us | **14.0x** |
| `XLEN` | 0.65 us | 20.5 us | **31.4x** |
| `XRANGE` | 4.3 us | 30.8 us | **7.1x** |
| `XREAD` | 4.3 us | 31.9 us | **7.5x** |
| `XDEL` | 4.5 us | 55.2 us | **12.2x** |
| `DEL` | 4.7 us | 27.0 us | **5.7x** |
| `EXISTS` | 0.29 us | 26.5 us | **91.6x** |
| `EXPIRE` | 0.97 us | 28.9 us | **29.8x** |
| `TTL` | 0.33 us | 29.1 us | **87.5x** |
| `JSON.SET` | 3.9 us | 27.3 us | **6.9x** |
| `JSON.GET` | 1.6 us | 24.9 us | **15.7x** |
| `JSON.DEL` | 10.3 us | 56.1 us | **5.4x** |
| `JSON.NUMINCRBY` | 4.5 us | 28.1 us | **6.3x** |
| `JSON.ARRLEN` | 1.5 us | 28.6 us | **19.5x** |
| `JSON.TYPE` | 1.5 us | 28.7 us | **19.0x** |
| `BF.ADD` | 14.0 us | 28.1 us | **2.0x** |
| `BF.EXISTS` | 0.80 us | 26.0 us | **32.4x** |
| `BF.INFO` | 0.50 us | 23.9 us | **47.4x** |
| `CF.ADD` | 3.5 us | 28.7 us | **8.2x** |
| `CF.EXISTS` | 0.84 us | 28.3 us | **33.8x** |
| `CF.DEL` | 8.2 us | 54.8 us | **6.6x** |
| `TDIGEST.ADD` | 3.1 us | 21.4 us | **6.9x** |
| `TDIGEST.QUANTILE` | 1.2 us | 20.8 us | **17.0x** |
| `TDIGEST.BYRANK` | 1.2 us | 21.0 us | **17.7x** |
| `TDIGEST.CDF` | 1.3 us | 21.7 us | **16.8x** |
| `TS.ADD` | 7.4 us | 20.7 us | **2.8x** |
| `TS.GET` | 1.6 us | 20.9 us | **13.4x** |
| `TS.RANGE` | 31.2 us | 20.5 us | **0.7x** |
| `TS.INCRBY` | 9.5 us | 21.1 us | **2.2x** |
| `FT.SEARCH` | 24.5 us | 22.5 us | **0.9x** |
| `FT.TAG` | 24.3 us | 23.0 us | **0.9x** |
| `VECTOR.KNN` | 3.8 us | 20.9 us | **5.6x** |

