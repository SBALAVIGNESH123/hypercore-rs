# TitanMem Benchmark Results

**Status: Experimental — Did not outperform native OS paging under controlled testing.**

## Hypothesis

> A user-space prefetch + eviction engine can reduce page fault stalls and improve token generation speed when the model exceeds physical RAM.

## Result: Disproven

The Windows kernel's native demand paging for mmap'd files already handles this access pattern near-optimally. TitanMem's prefetch strategy actively increased page faults and reduced throughput.

## Machine
- **CPU**: AMD Ryzen 5 5600H (12 logical cores)
- **RAM**: 8 GB
- **SSD**: NVMe
- **OS**: Windows 11

## Model
- `qwen2.5-coder-3b-instruct-q5_k_m.gguf` (2.3 GB)

## Method
- Working set hard-limited via `SetProcessWorkingSetSizeEx` with `QUOTA_LIMITS_HARDWS_MAX_ENABLE`
- Peak RAM verified to match budget exactly
- 5 component isolation tests per budget level

## Results — 1024 MB Budget (model is 2.3x larger than RAM)

| Test | Description | Tok/s | Page Faults |
|---|---|---|---|
| A | Baseline (llama.cpp + budget) | **0.81** | 5,695,894 |
| C | + TitanMem mmap layer | 0.78 | 5,732,561 |
| D | + Prefetch | 0.76 | 5,739,157 |
| E | + Prefetch + Eviction | 0.87 | 5,738,633 |

## Results — 2048 MB Budget

| Test | Description | Tok/s | Page Faults |
|---|---|---|---|
| A | Baseline (llama.cpp + budget) | **1.92** | 1,594,375 |
| C | + TitanMem mmap layer | 1.65 | 1,670,253 |
| D | + Prefetch | 1.62 | **1,863,874** |
| E | + Prefetch + Eviction | 1.54 | 1,765,599 |

## Analysis

1. **Prefetch hurts.** Adding `PrefetchVirtualMemory` increased page faults by 17% at 2GB budget.
2. **Eviction hurts.** `VirtualUnlock` partially recovers from prefetch damage but still underperforms baseline.
3. **The OS wins.** Windows demand paging for sequential mmap access is already well-optimized.

## Lessons

- Measure before you claim.
- The OS page cache is not as naive as it looks.
- User-space prefetch can increase page churn if it competes with kernel prefetch.
- Honest benchmarks are more valuable than optimistic narratives.

## Future Directions

- Layer-aware tensor scheduling
- Custom block I/O bypassing mmap
- Access-order optimized model formats
- Blockwise execution with explicit memory management
