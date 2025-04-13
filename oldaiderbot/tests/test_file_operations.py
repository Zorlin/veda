import pytest
import os
import time
from pathlib import Path
import tempfile
from unittest.mock import patch

from main import get_file_mtime, read_file, reload_file

@pytest.fixture
def temp_test_file():
    """Create a temporary test file for testing file operations."""
    with tempfile.NamedTemporaryFile(mode='w+', delete=False) as f:
        f.write("Initial content")
        temp_path = Path(f.name)
    
    yield temp_path
    
    # Clean up
    if temp_path.exists():
        os.unlink(temp_path)

def test_get_file_mtime(temp_test_file):
    """Test that get_file_mtime returns the correct modification time."""
    # Get initial mtime
    initial_mtime = get_file_mtime(temp_test_file)
    
    # Wait a moment to ensure mtime will be different
    time.sleep(0.1)
    
    # Update the file
    with open(temp_test_file, 'w') as f:
        f.write("Updated content")
    
    # Get updated mtime
    updated_mtime = get_file_mtime(temp_test_file)
    
    # Verify mtime changed
    assert updated_mtime > initial_mtime, "File modification time should have increased"

def test_read_file(temp_test_file):
    """Test that read_file correctly reads file content."""
    # Write known content
    test_content = "Test content for read_file"
    with open(temp_test_file, 'w') as f:
        f.write(test_content)
    
    # Read the file
    content = read_file(temp_test_file)
    
    # Verify content
    assert content == test_content, "read_file should return the exact file content"

def test_reload_file_gets_latest_content(temp_test_file):
    """Test that reload_file gets the latest content even after changes."""
    # Initial content
    initial_content = "Initial content"
    with open(temp_test_file, 'w') as f:
        f.write(initial_content)
    
    # Read initial content
    content1 = reload_file(temp_test_file)
    assert content1 == initial_content
    
    # Update content
    updated_content = "Updated content"
    with open(temp_test_file, 'w') as f:
        f.write(updated_content)
    
    # Read updated content
    content2 = reload_file(temp_test_file)
    assert content2 == updated_content, "reload_file should get the latest content after file update"

def test_reload_file_handles_nonexistent_file():
    """Test that reload_file handles nonexistent files gracefully."""
    nonexistent_path = Path("nonexistent_file_that_should_not_exist.txt")
    
    # Make sure the file doesn't exist
    if nonexistent_path.exists():
        os.unlink(nonexistent_path)
    
    # Try to reload nonexistent file
    content = reload_file(nonexistent_path)
    
    # Verify empty string is returned
    assert content == "", "reload_file should return empty string for nonexistent files"

def test_reload_file_handles_permission_error():
    """Test that reload_file handles permission errors gracefully."""
    with tempfile.NamedTemporaryFile(delete=False) as f:
        f.write(b"Test content")
        temp_path = Path(f.name)
    
    try:
        # Mock os.open to raise PermissionError
        with patch('builtins.open', side_effect=PermissionError("Permission denied")):
            content = reload_file(temp_path)
            assert content == "", "reload_file should return empty string on permission error"
    finally:
        # Clean up
        if temp_path.exists():
            os.unlink(temp_path)
