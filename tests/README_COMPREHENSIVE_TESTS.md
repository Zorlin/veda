# Comprehensive End-to-End Tests for Veda Log Pipeline

This directory contains comprehensive tests that validate the entire log pipeline from Claude process spawning through UI rendering. These tests were created to catch and prevent the tab routing bug where all messages were being displayed on Tab 0.

## Test Files

### 1. `comprehensive_pipeline_test.rs`
Tests the complete pipeline with message routing validation:
- Tab creation with unique instance and session IDs
- Message routing based on session and instance IDs
- Detection of the "1 message per tab while main overflows" bug
- UI structure validation
- Message buffering and overflow scenarios
- Concurrent message handling
- Empty tab detection

### 2. `session_routing_validation_test.rs`
Focuses specifically on session ID routing issues:
- Session ID mismatch detection
- The "1 message per tab" bug reproduction
- Message structure validation
- Automode session ID problems
- Session resumption consistency
- Complex routing scenarios with priority rules

### 3. `main_routing_logic_test.rs`
Tests the actual routing logic from main.rs:
- Exact routing priority (session > instance > main tab)
- spawn_instances IPC handling
- Message accumulation patterns
- Veda message types validation
- Coordinated instance spawning issues
- Environment variable inheritance
- UI rendering with empty/overflow tabs

### 4. `async_pipeline_integration_test.rs`
Tests async operations and concurrent behavior:
- Async message pipeline processing
- Concurrent message load testing
- IPC socket communication simulation
- Session resumption flow
- UI overflow and empty tab detection
- Complete end-to-end validation with phases

## Key Test Scenarios

### The Tab Routing Bug
The original bug caused all Claude instances to display output on Tab 0 because:
1. IPC handler used `Uuid::new_v4()` instead of actual instance IDs
2. Session IDs were mismatched or missing (especially in automode)
3. Messages were routed to main tab by default when routing failed

### Test Coverage
These tests validate:
1. **Message Routing**: Each tab receives only its own messages
2. **Session Consistency**: Session IDs are properly maintained
3. **Instance Isolation**: Each Claude instance has unique VEDA_TARGET_INSTANCE_ID
4. **UI State**: Tabs display correct message counts and content
5. **Error Conditions**: Handling of missing/wrong session IDs
6. **Concurrency**: Multiple instances sending messages simultaneously
7. **Pipeline Flow**: IPC → Message Processing → UI Rendering

## Running the Tests

```bash
# Run all comprehensive tests
cargo test comprehensive_pipeline_test
cargo test session_routing_validation_test  
cargo test main_routing_logic_test
cargo test async_pipeline_integration_test

# Run with output for debugging
cargo test -- --nocapture

# Run specific test
cargo test test_one_message_per_tab_bug
```

## Expected Failures
Some tests are designed to fail if the bug is reintroduced:
- `test_one_message_per_tab_bug` - Fails if tabs get only 1 message
- `test_main_tab_overflow_detection` - Fails if main tab accumulates all messages
- `test_ui_overflow_empty_detection` - Fails on the overflow pattern

## Integration with CI
These tests should be run in CI to prevent regression of the tab routing fix.