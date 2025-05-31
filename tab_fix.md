# Tab Management Fix for Veda

## Current Issues:
1. Tabs are created without being tied to Claude sessions
2. Session routing is backwards - sessions should belong to tabs, not vice versa
3. When clicking on Tab 0, it doesn't have a proper session to interact with
4. Multiple instances are spawned but not properly assigned to tabs

## Root Cause:
The fundamental issue is that `ClaudeInstance` (tabs) are created as visual containers without actually spawning Claude processes. When a message is sent, it spawns a Claude process which then gets a session ID, but this session isn't properly tied back to the tab.

## Solution:
1. Each tab should own its own Claude process and session from creation
2. When creating a new tab, immediately spawn a Claude process for it
3. Track the process handle and session ID within the tab instance
4. Route messages based on tab ownership, not session lookup

## Implementation Changes Needed:

### 1. Modify ClaudeInstance to properly track its Claude process:
- Add a method to spawn Claude process on tab creation
- Ensure each tab has its own independent session
- Track process lifecycle within the tab

### 2. Fix message routing:
- Route messages to tabs based on tab instance ID, not session ID
- Session ID should be used for resuming, not routing

### 3. Fix tab switching:
- When switching tabs, ensure we're working with the correct instance
- Make sure messages go to the current tab's Claude process

### 4. Fix spawn instances:
- When spawning new instances, create proper tabs with their own sessions
- Don't rely on session routing to find tabs