#!/usr/bin/env python3
"""
Veda TUI - Interactive terminal interface for managing Claude instances
"""
import asyncio
from pathlib import Path
from typing import List, Dict, Optional, Tuple
from datetime import datetime
import textwrap
try:
    import pyperclip
    HAS_CLIPBOARD = True
except ImportError:
    HAS_CLIPBOARD = False
from textual.app import App, ComposeResult
from textual.widgets import Header, Footer, TabbedContent, TabPane, Label, RichLog, Input, Static, TextArea
from textual.containers import Horizontal, Vertical, Container, ScrollableContainer
from textual.binding import Binding
from textual import events, work
from textual.reactive import reactive
from rich.text import Text
from rich.panel import Panel
from rich.markdown import Markdown

from .claude_manager import ClaudeOrchestrator


class SelectableRichLog(RichLog):
    """RichLog with mouse-based text selection support"""
    
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.mouse_down = False
        self.selection_start: Optional[Tuple[int, int]] = None
        self.selection_end: Optional[Tuple[int, int]] = None
        
    def on_mount(self):
        """Called when widget is mounted"""
        super().on_mount()
        self.can_focus = True
        
    def on_mouse_down(self, event: events.MouseDown) -> None:
        """Start text selection"""
        self.focus()
        # Store the starting position
        self.mouse_down = True
        self.selection_start = (event.x, self.scroll_offset.y + event.y)
        self.selection_end = self.selection_start
        # Don't refresh yet to avoid interference with native selection
        
    def on_mouse_move(self, event: events.MouseMove) -> None:
        """Update selection while dragging"""
        if self.mouse_down:
            # Update the end position
            self.selection_end = (event.x, self.scroll_offset.y + event.y)
            # Don't refresh to avoid interference
            
    def on_mouse_up(self, event: events.MouseUp) -> None:
        """Finish selection"""
        self.mouse_down = False
        # Selection is complete, but don't interfere with native text selection
        
    def on_key(self, event: events.Key) -> None:
        """Handle key events"""
        if event.key == "escape":
            # Clear any internal state on escape
            self.selection_start = None
            self.selection_end = None
            self.mouse_down = False
            

class ClaudeInstanceTab(TabPane):
    """Tab for a single Claude instance"""
    
    def __init__(self, instance_id: int, orchestrator: ClaudeOrchestrator):
        super().__init__(f"Claude {instance_id}", id=f"instance-{instance_id}")
        self.instance_id = instance_id
        self.orchestrator = orchestrator
        # Use TextArea for native text selection support
        self.message_log = TextArea(read_only=True, show_line_numbers=False, theme="monokai")
        self.input_field = Input(placeholder="Type a message and press Enter...")
        self.is_processing = False
        self.current_line = ""  # Buffer for streaming text
        self.wrap_width = 80  # Default wrap width, will be updated
        
    def compose(self) -> ComposeResult:
        with Vertical():
            yield Label(f"Claude Instance {self.instance_id}")
            with ScrollableContainer(id="message-container"):
                yield self.message_log
            with Container(id="input-container"):
                yield self.input_field
    
    def get_wrap_width(self):
        """Get the appropriate wrap width based on container size"""
        try:
            # Get the width of the message log container
            width = self.message_log.size.width
            # Leave some margin for timestamps and formatting
            return max(40, width - 20)
        except:
            return 80  # Default fallback
    
    def append_to_log(self, text: str, use_ansi: bool = False):
        """Append text to the message log"""
        current_text = self.message_log.text
        if use_ansi:
            # Add text with ANSI codes preserved
            self.message_log.text = current_text + text
        else:
            # Add plain text
            self.message_log.text = current_text + text
        # Scroll to bottom
        self.message_log.cursor_location = (self.message_log.text.count('\n'), 0)
    
    async def handle_message(self, msg_type: str, content: any):
        """Handle incoming messages from Claude"""
        timestamp = datetime.now().strftime("%H:%M:%S")
        
        if msg_type == "stream":
            # Streaming text - buffer and write complete lines
            self.current_line += content
            # Check for newlines and write complete lines
            while '\n' in self.current_line:
                line, self.current_line = self.current_line.split('\n', 1)
                # Wrap long lines
                if line:
                    wrapped_lines = textwrap.wrap(line, width=self.get_wrap_width())
                    for wrapped_line in wrapped_lines:
                        self.append_to_log(wrapped_line + "\n")
                else:
                    self.append_to_log("\n")  # Empty line
            # For text without newlines, check if we should wrap
            if self.current_line and len(self.current_line) > self.get_wrap_width():
                # Wrap and write the current buffer
                wrapped = textwrap.wrap(self.current_line, width=self.get_wrap_width())
                if len(wrapped) > 1:
                    # Write all complete wrapped lines
                    for line in wrapped[:-1]:
                        self.append_to_log(line + "\n")
                    self.current_line = wrapped[-1]
                else:
                    self.current_line = wrapped[0] if wrapped else ""
        elif msg_type == "complete":
            # Message complete - flush any remaining text and add newline
            if self.current_line:
                # Wrap any remaining text
                wrapped_lines = textwrap.wrap(self.current_line, width=self.get_wrap_width())
                for line in wrapped_lines:
                    self.append_to_log(line + "\n")
                self.current_line = ""
            self.append_to_log("\n")
            self.is_processing = False
            self.input_field.disabled = False
        elif msg_type == "error":
            self.append_to_log(f"\nError: {content}\n")
            self.is_processing = False
            self.input_field.disabled = False
        elif msg_type == "text":
            # Plain text output
            self.append_to_log(f"{content}\n")
        elif msg_type == "auto_response":
            # Auto-generated response
            self.append_to_log(f"\n[AUTO] {content}\n")
        elif msg_type == "system":
            # System messages
            self.append_to_log(f"{content}\n")
        elif msg_type == "tool_use":
            # Tool usage
            if isinstance(content, dict):
                tool_name = content.get("name", "unknown")
                self.append_to_log(f"\n🔧 Using tool: {tool_name}\n")
                # Optionally show tool input
                tool_input = content.get("input", {})
                if tool_input and isinstance(tool_input, dict):
                    for key, value in tool_input.items():
                        if isinstance(value, str) and len(value) > 100:
                            value = value[:100] + "..."
                        self.append_to_log(f"  {key}: {value}\n")
            else:
                self.append_to_log(f"{content}\n")
            # No extra empty line to save space
        elif msg_type == "tool_result":
            # Tool result
            if isinstance(content, dict):
                result = content.get("content", "")
                if len(result) > 200:
                    result = result[:200] + "..."
                self.append_to_log(f"✓ Tool result: {result}\n")
            else:
                self.append_to_log(f"✓ Tool result\n")
        elif msg_type == "new_response":
            # New response from Claude (after tool use)
            self.append_to_log(f"\n═══ {content} ═══\n")
            timestamp = datetime.now().strftime("%H:%M:%S")
            self.append_to_log(f"{timestamp} Claude: ")
        elif msg_type == "data":
            # Other data (debug info)
            pass  # Don't display raw data to avoid clutter
    
    @work
    async def send_message(self, message: str):
        """Send a message to Claude"""
        if not message.strip() or self.is_processing:
            return
            
        self.is_processing = True
        self.input_field.disabled = True
        
        # Display user message with wrapping
        timestamp = datetime.now().strftime("%H:%M:%S")
        # Use ANSI codes: \033[2m for dim, \033[1;36m for bold cyan, \033[0m to reset
        prefix = f"\033[2m{timestamp}\033[0m \033[1;36mYou:\033[0m "
        wrapped_msg = textwrap.wrap(message, width=self.get_wrap_width() - len(timestamp) - 6)
        if wrapped_msg:
            self.append_to_log(f"{prefix}{wrapped_msg[0]}\n", use_ansi=True)
            for line in wrapped_msg[1:]:
                self.append_to_log(f"{' ' * (len(timestamp) + 6)}{line}\n")
        else:
            self.append_to_log(prefix + "\n", use_ansi=True)
        self.append_to_log("\n")  # Empty line after user message
        
        # Display Claude is typing indicator
        # \033[1;32m for bold green
        self.append_to_log(f"\033[2m{timestamp}\033[0m \033[1;32mClaude:\033[0m \n", use_ansi=True)
        
        # Send to Claude
        await self.orchestrator.send_to_instance(self.instance_id, message)
        
        # Clear input
        self.input_field.value = ""
    
    async def on_input_submitted(self, event: Input.Submitted) -> None:
        """Handle input submission"""
        if event.input == self.input_field:
            self.send_message(event.value)


class VedaTUI(App):
    """Main TUI application for Veda"""
    
    CSS = """
    #message-container {
        height: 85%;
        border: solid $primary;
        padding: 1;
    }
    
    #input-container {
        height: 3;
        padding: 0 1;
    }
    
    Input {
        width: 100%;
    }
    
    RichLog {
        padding: 0 1;
    }
    """
    
    BINDINGS = [
        Binding("left", "prev_tab", "Previous Instance", key_display="←"),
        Binding("right", "next_tab", "Next Instance", key_display="→"),
        Binding("ctrl+n", "new_instance", "New Instance"),
        Binding("ctrl+d", "toggle_auto", "Toggle Auto-Mode"),
        Binding("ctrl+c", "quit", "Quit"),
        Binding("escape", "quit", "Quit", key_display="ESC"),
    ]
    
    def __init__(self, num_instances: int = 1):
        super().__init__()
        self.num_instances = num_instances
        self.orchestrator = ClaudeOrchestrator(self._on_instance_message)
        self.tabs: Dict[int, ClaudeInstanceTab] = {}
        self.next_instance_id = 0
        
    def compose(self) -> ComposeResult:
        yield Header(show_clock=True)
        yield Footer()
        
        with TabbedContent() as self.tabbed_content:
            # Create initial instances
            for i in range(self.num_instances):
                tab = self._create_instance_tab()
                yield tab
    
    def _create_instance_tab(self) -> ClaudeInstanceTab:
        """Create a new instance tab"""
        instance_id = self.next_instance_id
        self.next_instance_id += 1
        
        # Create Claude instance
        self.orchestrator.create_instance(instance_id)
        
        # Create tab
        tab = ClaudeInstanceTab(instance_id, self.orchestrator)
        self.tabs[instance_id] = tab
        
        return tab
    
    async def _on_instance_message(self, instance_id: int, msg_type: str, content: any):
        """Handle messages from Claude instances"""
        tab = self.tabs.get(instance_id)
        if tab:
            await tab.handle_message(msg_type, content)
    
    def action_prev_tab(self):
        """Switch to previous tab"""
        tabs = list(self.tabbed_content.query(TabPane))
        if tabs:
            current_active = self.tabbed_content.active
            current_index = next((i for i, tab in enumerate(tabs) if tab.id == current_active), 0)
            if current_index > 0:
                self.tabbed_content.active = tabs[current_index - 1].id
    
    def action_next_tab(self):
        """Switch to next tab"""
        tabs = list(self.tabbed_content.query(TabPane))
        if tabs:
            current_active = self.tabbed_content.active
            current_index = next((i for i, tab in enumerate(tabs) if tab.id == current_active), 0)
            if current_index < len(tabs) - 1:
                self.tabbed_content.active = tabs[current_index + 1].id
    
    def action_new_instance(self):
        """Create a new Claude instance"""
        tab = self._create_instance_tab()
        self.tabbed_content.add_pane(tab)
        self.tabbed_content.active = tab.id
    
    def action_toggle_auto(self):
        """Toggle auto-response mode"""
        self.orchestrator.set_auto_mode(not self.orchestrator.auto_mode)
        
        # Show notification
        mode = "enabled" if self.orchestrator.auto_mode else "disabled"
        self.notify(f"Auto-response mode {mode}", severity="information")
        
        # Log in current tab
        current_tab_id = self.tabbed_content.active
        if current_tab_id:
            instance_id = int(current_tab_id.split("-")[1])
            tab = self.tabs.get(instance_id)
            if tab:
                tab.message_log.write(f"\n[yellow]Auto-response mode {mode}[/yellow]\n")
    
    def action_quit(self):
        """Quit the application"""
        self.exit()
    
    async def on_unmount(self):
        """Clean up when app closes"""
        await self.orchestrator.shutdown()


def main():
    """Main entry point for Veda TUI"""
    import argparse
    parser = argparse.ArgumentParser(description="Veda - Interactive Claude Code Orchestrator")
    parser.add_argument(
        "-n", "--instances", 
        type=int, 
        default=1, 
        help="Number of Claude instances to start (default: 1)"
    )
    
    args = parser.parse_args()
    
    app = VedaTUI(max(1, args.instances))
    app.run()


if __name__ == "__main__":
    main()