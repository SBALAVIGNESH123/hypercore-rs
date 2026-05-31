# Project Notes — 2025

## HyperCore (June - Present)

Building a local AI personal intelligence system. Started as an inference server, evolved into something more interesting when we added the memory layer.

Working on making the system understand patterns in user behavior, not just retrieve documents.

## CloudSync (January - May)

Tried building a cloud-based file sync tool. Abandoned it after 4 months. The market is saturated (Dropbox, Google Drive, iCloud) and we couldn't find a meaningful differentiator.

Lesson learned: don't enter a crowded market without a clear 10x advantage.

## DataVault (2024)

Developed a local data backup tool in Go. Shipped it, got about 200 users. Switched to Rust for the next project because Go's garbage collector caused unpredictable memory spikes during large file operations.

I consistently prefer tools that give me control over resource usage rather than abstracting it away.

## Meeting Notes

### With Sarah (Aug 12)
Discussed API design. She strongly preferred REST over gRPC for developer adoption. I agreed — most developers already have curl and Postman. gRPC adds a protobuf compilation step that scares people away.

### With Raj (Aug 20)  
Reviewed the TitanMem benchmarks together. He pointed out that our prefetch strategy was competing with the OS page cache. We decided to archive TitanMem v1 and publish the honest results instead of hiding the failure.

### With the whole team (Sep 1)
Agreed to focus entirely on the memory/intelligence layer. No more infrastructure features, no more TitanMem iterations, no more benchmark tooling. The product is the insight, not the plumbing.
