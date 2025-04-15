#!/usr/bin/env python3
import subprocess
import sys

def run_pytest():
    try:
        # Run pytest without capturing output to avoid device errors
        result = subprocess.run(
            ["pytest", "--quiet", "--disable-warnings", "-v"],
            check=False
        )
        
        if result.returncode != 0:
            print("\n❌ Commit blocked: Some tests FAILED.")
            return 1

        print("✅ All tests passed or were skipped.")
        return 0
    except Exception as e:
        print(f"\n❌ Error running tests: {e}")
        return 1

if __name__ == "__main__":
    sys.exit(run_pytest())
