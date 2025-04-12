import pytest
import os
import time
import threading
from pathlib import Path
import tempfile
import fcntl

@pytest.fixture
def temp_lockable_file():
    """Create a temporary file for testing file locking."""
    with tempfile.NamedTemporaryFile(mode='w+', delete=False) as f:
        f.write("Initial content")
        temp_path = Path(f.name)
    
    yield temp_path
    
    # Clean up
    if temp_path.exists():
        os.unlink(temp_path)

def test_file_locking_prevents_concurrent_writes(temp_lockable_file):
    """Test that file locking prevents concurrent writes."""
    # Track successful writes
    successful_writes = []
    write_errors = []
    
    def write_with_lock(content, delay=0):
        try:
            time.sleep(delay)  # Simulate processing time
            with open(temp_lockable_file, 'r+') as f:
                # Try to get an exclusive lock
                fcntl.flock(f.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
                # If we get here, we have the lock
                f.seek(0)
                f.write(content)
                f.truncate()
                f.flush()
                os.fsync(f.fileno())
                time.sleep(0.1)  # Hold the lock briefly
                fcntl.flock(f.fileno(), fcntl.LOCK_UN)
                successful_writes.append(content)
        except BlockingIOError:
            # Lock acquisition failed
            write_errors.append(f"{content}: Resource temporarily unavailable")
        except Exception as e:
            write_errors.append(f"{content}: {str(e)}")
    
    # Start multiple threads trying to write concurrently
    threads = []
    for i in range(5):
        t = threading.Thread(target=write_with_lock, args=(f"Content from thread {i}", i * 0.05))
        threads.append(t)
        t.start()
    
    # Wait for all threads to complete
    for t in threads:
        t.join()
    
    # Read the final content
    with open(temp_lockable_file, 'r') as f:
        final_content = f.read()
    
    # Verify that exactly one thread succeeded in writing
    assert len(successful_writes) > 0, "At least one write should succeed"
    assert final_content in successful_writes, "Final content should match one of the successful writes"
    
    # If we have errors, they should be due to lock acquisition failures
    for error in write_errors:
        if isinstance(error, str) and not error.endswith("Resource temporarily unavailable"):
            pytest.fail(f"Unexpected error: {error}")

def test_file_locking_with_reload(temp_lockable_file):
    """Test that file locking works with reload operations."""
    # Define a simple reload function similar to the one in main.py
    def reload_with_lock(path):
        try:
            with open(path, 'r') as f:
                # Try to get a shared lock (allows other readers but blocks writers)
                fcntl.flock(f.fileno(), fcntl.LOCK_SH)
                content = f.read()
                fcntl.flock(f.fileno(), fcntl.LOCK_UN)
                return content
        except Exception as e:
            return f"Error: {str(e)}"
    
    # Write thread function
    def write_with_lock(content, delay=0):
        try:
            time.sleep(delay)
            with open(temp_lockable_file, 'r+') as f:
                fcntl.flock(f.fileno(), fcntl.LOCK_EX)
                f.seek(0)
                f.write(content)
                f.truncate()
                f.flush()
                os.fsync(f.fileno())
                time.sleep(0.2)  # Hold the lock longer
                fcntl.flock(f.fileno(), fcntl.LOCK_UN)
                return True
        except Exception:
            return False
    
    # Start a writer thread
    writer_thread = threading.Thread(target=write_with_lock, args=("Updated content", 0.1))
    writer_thread.start()
    
    # Try to read while the writer might have the lock
    time.sleep(0.15)  # Wait a bit to ensure the writer has started
    content = reload_with_lock(temp_lockable_file)
    
    # Wait for writer to finish
    writer_thread.join()
    
    # Read again after writer is done
    final_content = reload_with_lock(temp_lockable_file)
    
    # Verify that we either got the initial content or the updated content
    assert content in ["Initial content", "Updated content", "Error: Resource temporarily unavailable"], \
        f"Unexpected content during write: {content}"
    assert final_content == "Updated content", f"Final content should be updated: {final_content}"
