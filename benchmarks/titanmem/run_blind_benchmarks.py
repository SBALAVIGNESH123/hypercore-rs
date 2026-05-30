import subprocess
import json
import statistics
import os
import time

MODEL = "C:\\Users\\balav\\Downloads\\qwen2.5-coder-3b-instruct-q5_k_m.gguf"
PROMPT = "Explain quantum tunneling."
RUNS = 1
BUDGETS = [1024, 2048]

TESTS = {
    "Test_A_Baseline": [],
    "Test_B_MmapOnly": ["--enable-mmap"],
    "Test_C_MmapBudget": ["--enable-mmap", "--enable-budget-manager"],
    "Test_D_MmapPrefetch": ["--enable-mmap", "--enable-budget-manager", "--enable-prefetch"],
    "Test_E_Everything": ["--enable-mmap", "--enable-budget-manager", "--enable-prefetch", "--enable-eviction"]
}

def ensure_model_exists():
    if not os.path.exists(MODEL):
        print(f"Warning: Model not found at {MODEL}. Please update MODEL path.")
        
def run_bench(budget, test_name, flags, i):
    cmd = [
        "target/release/benchmark.exe",
        "-m", MODEL,
        "-p", PROMPT,
        "--ram-budget", str(budget),
        "--mode", test_name
    ]
    if "Baseline" not in test_name:
        cmd.extend(flags)
    else:
        # Test A: Ensure budget manager is enabled so the OS restricts Baseline too!
        cmd.append("--enable-budget-manager")
        
    print(f"Running {test_name} (run {i+1}/{RUNS}) with budget {budget}MB...")
    res = subprocess.run(cmd, capture_output=True, text=True)
    if res.returncode != 0:
        print(f"Error running cmd: {' '.join(cmd)}")
        print(res.stderr)
        return None
        
    try:
        lines = res.stdout.strip().split('\n')
        json_str = ""
        for line in reversed(lines):
            if line == "}":
                json_str = line
            elif line == "{":
                json_str = line + "\n" + json_str
                break
            elif json_str:
                json_str = line + "\n" + json_str
                
        data = json.loads(json_str)
        return data
    except Exception as e:
        print(f"Parse error: {e}")
        print(res.stdout)
        return None

def main():
    ensure_model_exists()

    # Compile first so it doesn't skew timing
    subprocess.run(["cargo", "build", "--release"])

    results_data = {test: {} for test in TESTS}

    for budget in BUDGETS:
        for test in TESTS:
            results_data[test][budget] = []
        
        for test_name, flags in TESTS.items():
            for i in range(RUNS):
                res = run_bench(budget, test_name, flags, i)
                if res:
                    results_data[test_name][budget].append(res)
                time.sleep(1) # Cooldown

    print("\n\n=== COMPONENT ISOLATION RESULTS ===")
    print(f"Budget\tTest                 \tFTL(s)\tTok/s\tPeakRAM(MB)\tPageFaults\tDiskRead(GB)\tAvgDisk(MB/s)")
    
    for budget in BUDGETS:
        for test_name in TESTS.keys():
            runs = results_data[test_name][budget]
            if not runs:
                print(f"{budget}\t{test_name.ljust(20)}\tFAILED")
                continue
                
            avg_ftl = statistics.mean([r["first_token_latency_s"] for r in runs])
            avg_tok = statistics.mean([r["tokens_per_sec"] for r in runs])
            avg_ram = statistics.mean([r["peak_ram_mb"] for r in runs])
            avg_pf = statistics.mean([r["page_faults"] for r in runs])
            avg_disk_gb = statistics.mean([r["disk_read_gb"] for r in runs])
            avg_disk_mbs = statistics.mean([r["avg_disk_mb_s"] for r in runs])
            
            print(f"{budget}MB\t{test_name.ljust(20)}\t{avg_ftl:.2f}\t{avg_tok:.2f}\t{avg_ram:.0f}\t\t{avg_pf:.0f}\t\t{avg_disk_gb:.2f}\t\t{avg_disk_mbs:.0f}")

if __name__ == "__main__":
    main()
