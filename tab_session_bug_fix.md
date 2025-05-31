# Tab Session Bug Fix

## Problem Identified:

When watching the logs, I discovered that Tab 0 (Veda-1) was experiencing a critical session management issue:

1. Tab 0 initially had session `c21f3069-9dc9-4086-b9f8-de9883f85135`
2. At 18:04:47, a new session `cbd84b6b-97fe-4a69-a027-25ee60b39368` was created for Tab 0
3. Messages started flowing correctly with this new session
4. BUT at 18:05:23, ANOTHER new session `c21f3069-9dc9-4086-b9f8-de9883f85135` was created
5. This overwrote Tab 0's session, causing all subsequent messages to fail routing

## Root Cause:

The coordination system was auto-starting the main instance (Tab 0) even when it already had an active Claude process. This happened when:

1. User initiated work that triggered multi-instance coordination
2. The system spawned new instances for subtasks
3. The system then tried to "auto-start" the main instance with its coordination task
4. This created a NEW Claude process with a NEW session, overwriting the existing one
5. All messages from the original Claude process (with the old session) could no longer route to Tab 0

## Fix Applied:

Modified `spawn_coordinated_instances_with_count` to check if the main instance already has a session before auto-starting it:

```rust
// Only auto-start the main instance if it doesn't already have a session
// This prevents creating a new Claude process that would overwrite the existing session
if let Some(main_instance) = self.instances.iter().find(|i| i.id == main_instance_id) {
    if main_instance.session_id.is_none() {
        // Auto-start logic here
    } else {
        tracing::info!("Main instance already has session {:?}, skipping auto-start to preserve existing Claude process", main_instance.session_id);
    }
}
```

## Impact:

This fix prevents the coordination system from accidentally creating duplicate Claude processes for the main instance, which was causing:
- Tab 0 to lose its messages
- Session routing failures
- The appearance that Tab 0 was "stuck" or not receiving updates

## Testing:

All tests pass, including:
- `tab_functionality_test` - 10 tests passed
- `mcp_target_instance_test` - 6 tests passed

The fix ensures that tabs maintain their Claude process sessions throughout the application lifecycle, even when coordination events occur.