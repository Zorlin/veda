#!/usr/bin/env python3
import subprocess
import sys

def run_pytest():
    result = subprocess.run(
        ["pytest", "--quiet", "--disable-warnings"],
        capture_output=True, text=True
    )
    output = result.stdout + result.stderr

    print(output)

    if result.returncode != 0:
        print("\n❌ Commit blocked: Some tests FAILED.")
        return 1

    print("✅ All tests passed or were skipped.")
    return 0

if __name__ == "__main__":
    sys.exit(run_pytest())
