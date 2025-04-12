import argparse
import logging
import os
import sys # Add sys import for exiting
import time # Import the time module
import re # Add re import for regex operations
import tempfile # Add tempfile import for temporary files
from pathlib import Path
import rich
from rich.console import Console
from rich.logging import RichHandler
import threading # For running UI server in background
import asyncio # For running async UI server
import anyio # For creating streams
import yaml # For loading config early
import http.server
import socketserver
from functools import partial
import fcntl  # For file locking

from src.harness import Harness
from src.ui_server import UIServer # Import UI Server

# Default configuration values (used if config file is missing or invalid)
DEFAULT_CONFIG = {
    "websocket_host": "localhost",
    "websocket_port": 9940, # Default WebSocket port
    "http_port": 9950, # Default HTTP port
    "enable_ui": False,
    # Set project_dir to the project root (parent of the directory containing this file)
    "project_dir": str(Path(__file__).parent.parent.resolve()),
    # Add other essential defaults if needed for early access
}

# Configure rich console
console = Console()

# Configure logging with rich
logging.basicConfig(
    level=logging.INFO,
    format="%(message)s",
    handlers=[RichHandler(rich_tracebacks=True, console=console)]
)

logger = logging.getLogger("aider_harness")


# --- Council Planning Enforcement Function ---
import subprocess
import difflib
import datetime
import re
import sys # Ensure sys is imported here if needed by council func

# Define paths globally or pass them as arguments if preferred
plan_path = Path("PLAN.md")
goal_prompt_path = Path("goal.prompt")
readme_path = Path("README.md")

def get_file_mtime(path):
    """Helper to get file modification time."""
    try:
        # Ensure we're getting the latest stat from disk by resolving the path
        if hasattr(path, 'resolve'):
            path = path.resolve()
        
        # Force a direct stat call to bypass any caching
        if isinstance(path, Path):
            path_str = str(path)
        else:
            path_str = str(path)
            
        # Use os.stat directly to ensure we get fresh info
        return os.stat(path_str).st_mtime
    except Exception as e:
        logger.error(f"Error getting file modification time for {path}: {e}")
        return 0

def read_file(path):
    """Helper to read file content."""
    try:
        # Ensure we're getting the latest content from disk by resolving the path
        if hasattr(path, 'resolve'):
            path = path.resolve()
            
        # Use file locking to ensure consistent reads
        with open(path, 'r', encoding="utf-8", errors='replace') as f:
            # Acquire a shared lock (allows other readers but blocks writers)
            fcntl.flock(f.fileno(), fcntl.LOCK_SH)
            content = f.read()
            # Release the lock
            fcntl.flock(f.fileno(), fcntl.LOCK_UN)
            return content
    except Exception as e:
        logger.error(f"Error reading file {path}: {e}")
        return ""

def reload_file(path):
    """
    Helper to reload a file and get its content.
    
    Implements a robust file reading strategy with:
    1. Multiple attempts with exponential backoff
    2. File locking to prevent concurrent access issues
    3. Fallback to binary reading with UTF-8 decoding if text mode fails
    4. Automatic backup of potentially corrupted files
    5. Graceful degradation by returning empty string instead of raising exceptions
    """
    try:
        # Ensure we're getting the latest version from disk by resolving the path
        if hasattr(path, 'resolve'):
            path = path.resolve()
        
        # Force a filesystem stat to clear any potential caching
        try:
            os.stat(str(path))
        except Exception as e:
            logger.warning(f"Error getting file stats during reload: {e}")
        
        # Clear any potential file system cache by checking if the file exists first
        if Path(path).exists():
            # Try multiple times to ensure we get the content
            backoff_factor = 0.2  # Start with 200ms, then 400ms, then 800ms
            for attempt in range(3):  # Try up to 3 times
                try:
                    # Use file locking to ensure consistent reads
                    with open(path, 'r', encoding="utf-8", errors='replace') as f:
                        # Acquire a shared lock (allows other readers but blocks writers)
                        fcntl.flock(f.fileno(), fcntl.LOCK_SH)
                        content = f.read()
                        # Release the lock
                        fcntl.flock(f.fileno(), fcntl.LOCK_UN)
                        
                        # Verify we got content if the file isn't empty
                        if not content and os.path.getsize(path) > 0:
                            logger.warning(f"File {path} appears to have content but read returned empty string. Retrying with binary read...")
                            # Try one more time with a binary read
                            with open(path, 'rb') as bf:
                                fcntl.flock(bf.fileno(), fcntl.LOCK_SH)
                                binary_content = bf.read()
                                fcntl.flock(bf.fileno(), fcntl.LOCK_UN)
                            content = binary_content.decode('utf-8', errors='replace')
                        
                        return content
                except (IOError, OSError) as e:
                    if attempt < 2:  # Don't log on the last attempt
                        logger.warning(f"Attempt {attempt+1} to read file {path} failed: {e}. Retrying...")
                        time.sleep(backoff_factor * (2 ** attempt))  # Exponential backoff
                    else:
                        # On the last attempt, try a different approach instead of raising
                        try:
                            # Try a direct binary read as a last resort
                            with open(path, 'rb') as f:
                                binary_content = f.read()
                                return binary_content.decode('utf-8', errors='replace')
                        except Exception as last_e:
                            logger.error(f"Final attempt to read {path} failed: {last_e}")
                            # Create a backup of the potentially corrupted file
                            try:
                                if os.path.getsize(path) > 0:
                                    backup_path = f"{path}.corrupted.bak"
                                    shutil.copy2(path, backup_path)
                                    logger.warning(f"Created backup of potentially corrupted file at {backup_path}")
                            except Exception as backup_e:
                                logger.error(f"Failed to create backup of corrupted file: {backup_e}")
                            return ""  # Return empty string instead of raising
        else:
            logger.warning(f"File {path} does not exist")
            return ""
    except Exception as e:
        logger.error(f"Error reloading file {path}: {e}", exc_info=True)
        return ""

def update_goal_for_test_failures(test_type):
    """
    Update goal.prompt to specifically address test failures.
    
    This function implements a self-healing mechanism that:
    1. Detects test failures and their type
    2. Automatically updates the goal prompt to prioritize fixing these failures
    3. Creates backups of the original goal prompt for recovery
    4. Ensures the system can recover from the failure state
    
    Args:
        test_type: The type of test that failed ("pytest" or "cargo")
    """
    try:
        # Force a filesystem stat before reloading
        try:
            os.stat(str(goal_prompt_path))
        except Exception as e:
            logger.warning(f"Error getting goal prompt stats before test failure update: {e}")
            
        # Force reload to get the current goal prompt
        current_goal = reload_file(goal_prompt_path)
        logger.info(f"Checking if goal.prompt needs updating for {test_type} test failures")
        
        # Check if we've already added test failure guidance
        if f"fix the {test_type} test failures" in current_goal.lower():
            logger.info(f"Goal prompt already contains guidance for {test_type} test failures.")
            return
        
        # Create a backup of the current goal prompt
        backup_path = f"{goal_prompt_path}.bak.{int(time.time())}"
        try:
            with open(goal_prompt_path, 'r', encoding='utf-8') as src:
                with open(backup_path, 'w', encoding='utf-8') as dst:
                    dst.write(src.read())
            logger.info(f"Created backup of goal.prompt at {backup_path}")
        except Exception as e:
            logger.warning(f"Failed to create backup of goal.prompt: {e}")
        
        # Create the test failure addendum with more detailed guidance
        test_failure_addendum = f"""

## CRITICAL: Fix {test_type} test failures

The system has detected {test_type} test failures that must be fixed before proceeding with other tasks.
This is now your highest priority task. You need to fix the {test_type} test failures.

Please:
1. Carefully analyze the test output to understand the specific failures
2. Fix each failing test systematically
3. Ensure your changes don't introduce new regressions
4. Add additional tests to prevent similar failures in the future
5. Focus on making the system more resilient to unexpected conditions

If using {test_type}, ensure:
- All test cases pass without errors
- Error handling is robust and graceful
- Resource cleanup happens properly even during failures
- The code meets the project's quality standards
- The system can recover from similar failures automatically in the future

Remember that improving system resilience is the highest priority according to the project goals.
"""
        
        # Append the test failure guidance to the goal prompt with file locking
        logger.info(f"Appending {test_type} test failure guidance to goal.prompt")
        with open(goal_prompt_path, "a", encoding="utf-8") as f:
            # Acquire an exclusive lock
            fcntl.flock(f.fileno(), fcntl.LOCK_EX)
            f.write(test_failure_addendum)
            f.flush()  # Force flush to disk
            os.fsync(f.fileno())  # Ensure it's written to disk
            # Release the lock
            fcntl.flock(f.fileno(), fcntl.LOCK_UN)
        
        # Force a filesystem stat after writing
        try:
            os.stat(str(goal_prompt_path))
        except Exception as e:
            logger.warning(f"Error getting goal prompt stats after test failure update: {e}")
            
        # Verify the file was actually updated by reading it again with a different method
        try:
            with open(goal_prompt_path, 'rb') as f:
                # Acquire a shared lock
                fcntl.flock(f.fileno(), fcntl.LOCK_SH)
                binary_content = f.read()
                # Release the lock
                fcntl.flock(f.fileno(), fcntl.LOCK_UN)
                
            updated_goal = binary_content.decode('utf-8', errors='replace')
            
            if test_failure_addendum.strip() in updated_goal:
                logger.info(f"Successfully updated goal.prompt with guidance for {test_type} test failures.")
                console.print(f"[bold green]Updated goal.prompt with guidance for {test_type} test failures.[/bold green]")
            else:
                # Try one more time with the regular reload function
                updated_goal = reload_file(goal_prompt_path)
                if test_failure_addendum.strip() in updated_goal:
                    logger.info(f"Successfully verified goal.prompt update on second attempt.")
                    console.print(f"[bold green]Updated goal.prompt with guidance for {test_type} test failures.[/bold green]")
                else:
                    logger.error(f"Failed to verify goal.prompt update for {test_type} test failures")
                    console.print(f"[bold red]Failed to update goal.prompt with guidance for {test_type} test failures.[/bold red]")
        except Exception as e:
            logger.error(f"Error verifying goal.prompt update: {e}", exc_info=True)
            console.print(f"[bold red]Error verifying goal.prompt update: {e}[/bold red]")
        
    except Exception as e:
        logger.error(f"Failed to update goal.prompt for test failures: {e}", exc_info=True)
        console.print(f"[bold red]Error updating goal.prompt: {e}[/bold red]")

# Define a custom exception for test failures in council planning
class CouncilPlanningTestFailure(Exception):
    """Exception raised when tests fail repeatedly during council planning."""
    pass

def council_planning_enforcement(iteration_number=None, test_failure_info=None, automated=True, testing_mode=False):
    """
    Enforce that the open source council convenes each round to collaboratively update PLAN.md,
    and only update goal.prompt for major shifts. All planning must respect README.md.
    All tests must pass to continue; after a few tries, the council can revert to a working commit.
    
    This function implements a robust planning enforcement mechanism with:
    - Automatic recovery from test failures
    - Detection of major shifts requiring goal updates
    - Resilience against file corruption and concurrent access
    - Graceful degradation when resources are constrained
    
    Args:
        iteration_number: The current iteration number (None for initial/final)
        test_failure_info: Optional information about test failures to guide the council
        automated: Whether to run in automated mode (no human interaction), defaults to True
        testing_mode: Whether running in testing mode (affects error handling)
    """
    # Import required modules
    import re
    import difflib
    
    # Force filesystem stats before reloading files
    logger.info("Forcing filesystem stats before reloading planning files...")
    try:
        os.stat(str(plan_path))
        os.stat(str(goal_prompt_path))
        os.stat(str(readme_path))
    except Exception as e:
        logger.warning(f"Error getting file stats before reload: {e}")
    
    # Force reload files to ensure we have the latest content from disk
    logger.info("Reloading planning files from disk...")
    plan_content = reload_file(plan_path)
    goal_prompt_content = reload_file(goal_prompt_path)
    readme_content = reload_file(readme_path)
    
    # Now get the modification times (also forcing a fresh stat)
    plan_mtime_before = get_file_mtime(plan_path)
    goal_prompt_mtime_before = get_file_mtime(goal_prompt_path)
    
    logger.info(f"Plan file last modified: {datetime.datetime.fromtimestamp(plan_mtime_before)}")
    logger.info(f"Goal prompt file last modified: {datetime.datetime.fromtimestamp(goal_prompt_mtime_before)}")
    
    # Log which iteration we're in
    if iteration_number is not None:
        console.print(f"\n[bold yellow]Council Planning for Iteration {iteration_number}[/bold yellow]")
    
    # If we have test failure information, highlight it
    if test_failure_info:
        console.print(f"\n[bold red]Test Failures Detected:[/bold red]")
        console.print(f"[red]{test_failure_info}[/red]")
        console.print("[bold yellow]The council should address these test failures in the updated plan.[/bold yellow]")

    console.print("\n[bold yellow]Council Planning Required[/bold yellow]")
    console.print(
        "[italic]At the end of each round, the open source council must collaboratively review and update [bold]PLAN.md[/bold] "
        "(very frequently) to reflect the current actionable plan, strategies, and next steps. "
        "Only update [bold]goal.prompt[/bold] if a significant change in overall direction is required (rare). "
        "All planning and actions must always respect the high-level goals and constraints in [bold]README.md[/bold].[/italic]"
    )
    console.print(
        "\n[bold]Please review and update PLAN.md now.[/bold] "
        "If a major shift in direction is needed, update goal.prompt as well."
    )
    console.print(
        "[italic]After updating, ensure all tests pass before proceeding. "
        "If tests fail after a few tries, the council should revert to a working commit using [bold]git revert[/bold].[/italic]"
    )

    plan_updated = False
    
    if automated:
        # In automated mode, we'll directly generate and append a new council round entry
        console.print("\n[bold cyan]Automatically updating PLAN.md with a new council round entry...[/bold cyan]")
        plan_updated = False  # We'll set this to True after we auto-append
    else:
        # Manual mode - give the human a chance to update
        for attempt in range(2):  # One human chance, then auto-append
            old_plan = read_file(plan_path)
            console.print("\n[bold cyan]Waiting for PLAN.md to be updated with a new council round entry...[/bold cyan]")
            console.print("[italic]Please add a new checklist item or summary for this round in PLAN.md, then press Enter.[/italic]")
            input() # This will block execution, intended for interactive use
            
            # Explicitly reload the file to ensure we get the latest content
            new_plan = reload_file(plan_path)
            if new_plan != old_plan:
                # Show a diff for transparency
                diff = list(difflib.unified_diff(
                    old_plan.splitlines(), new_plan.splitlines(),
                    fromfile="PLAN.md (before)", tofile="PLAN.md (after)", lineterm=""
                ))
                if diff:
                    console.print("[bold green]PLAN.md updated. Diff:[/bold green]")
                    for line in diff:
                        if line.startswith("+"):
                            console.print(f"[green]{line}[/green]")
                        elif line.startswith("-"):
                            console.print(f"[red]{line}[/red]")
                        else:
                            console.print(line)
                else:
                    console.print("[yellow]PLAN.md changed, but no diff detected.[/yellow]")

                # Check for a new council round entry (e.g., a new checklist item or timestamp)
                has_actionable = ("- [ ]" in new_plan or "- [x]" in new_plan)
                has_summary = ("Summary of Last Round:" in new_plan)
                mentions_readme = ("README.md" in new_plan or "high-level goals" in new_plan.lower())
                if not has_summary:
                    console.print("[bold yellow]Reminder:[/bold yellow] Please include a summary of the council's discussion and planning in PLAN.md for this round (add 'Summary of Last Round:').")
                if not mentions_readme:
                    console.print("[bold yellow]Reminder:[/bold yellow] PLAN.md should always reference the high-level goals and constraints in README.md.")
                    console.print("Please ensure your plan does not contradict the project's core direction.")
                if has_actionable and has_summary and mentions_readme:
                    plan_updated = True
                    break
                else:
                    console.print("[bold red]PLAN.md does not appear to have a new actionable item, council summary, or reference to README.md/high-level goals. Please update accordingly.[/bold red]")
            else:
                console.print("[bold red]PLAN.md does not appear to have been updated. Please make changes before proceeding.[/bold red]")

    # If still not updated or in automated mode, generate a new council round entry using Gemma3:12b
    if not plan_updated:
        now = datetime.datetime.now().strftime("%Y-%m-%d %H:%M:%S")
        plan_content = read_file(plan_path)
        council_rounds = re.findall(r"Summary of Last Round:", plan_content)
        round_num = len(council_rounds) + 1
        
        # Create a more detailed entry for automated mode
        if automated:
            # Generate a plan update using Gemma3:12b
            console.print("\n[bold cyan]Generating council plan update using Gemma3:12b...[/bold cyan]")
            
            # Import the LLM interaction module here to avoid circular imports
            from src.llm_interaction import get_llm_response
            
            # Create a detailed status report for STATUS.md
            status_path = Path("STATUS.md")
            status_content = ""
            if status_path.exists():
                status_content = read_file(status_path)
                
            # Create status report with technical details
            status_update = f"\n## Status Report - Round {round_num} ({now})\n\n"
                
            if test_failure_info:
                status_update += f"### Test Failures\n```\n{test_failure_info}\n```\n\n"
            else:
                status_update += "### Tests\nAll tests are passing.\n\n"
                
            # Add more technical details to STATUS.md
            status_update += f"### Current Iteration\nIteration: {iteration_number}\n\n"
                
            # Add system health metrics if available
            try:
                import psutil
                status_update += "### System Health\n"
                status_update += f"CPU Usage: {psutil.cpu_percent()}%\n"
                status_update += f"Memory Usage: {psutil.virtual_memory().percent}%\n"
                status_update += f"Disk Usage: {psutil.disk_usage('/').percent}%\n\n"
            except ImportError:
                status_update += "### System Health\nSystem health metrics not available (psutil not installed)\n\n"
            except Exception as e:
                status_update += f"### System Health\nError collecting system metrics: {str(e)}\n\n"
            
            # Write status update to STATUS.md
            if "# Status Report" not in status_content:
                # Initialize the file if it doesn't exist or is empty
                with open(status_path, "w", encoding="utf-8") as f:
                    f.write("# Status Report\n\n")
                    f.write("This file contains automated status reports generated by the system, including:\n")
                    f.write("- Code changes in the last iteration\n")
                    f.write("- Current failing tests\n")
                    f.write("- System health metrics\n")
                    f.write("- Other technical details\n\n")
                    f.write("These reports are automatically generated and should not be manually edited.\n")
                    f.write(status_update)
            else:
                # Append to existing file
                with open(status_path, "a", encoding="utf-8") as f:
                    f.write(status_update)
            
            # Prepare context for Gemma3:12b
            readme_content = read_file(readme_path)
            goal_prompt_content = read_file(goal_prompt_path)
            plan_content = read_file(plan_path)
            
            # Create a prompt for Gemma3:12b to generate a plan update
            plan_prompt = f"""
You are the open source council of AIs responsible for updating the project plan. You need to generate a new entry for PLAN.md.

Current README.md (project goals):
{readme_content}

Current goal.prompt:
{goal_prompt_content}

Current PLAN.md (read this carefully to understand the project's current state and direction):
{plan_content}

Current iteration: {iteration_number}

{'Test failures were detected in this round.' if test_failure_info else 'All tests are passing.'}

Based on this information, please generate a new council round entry for PLAN.md that includes:
1. A summary of the current state and progress
2. Any blockers or issues that need to be addressed
3. Clear, actionable next steps and tasks
4. A reference to the high-level goals in README.md

Your response should be in plain language, high-level direction that a human would write. Be concise but comprehensive.
Avoid technical jargon and focus on strategic direction and priorities.

As the open source council of AIs, you are collaboratively updating PLAN.md to reflect the current actionable plan, strategies, and next steps. Remember that you should only suggest updating goal.prompt if a significant change in overall direction is required (which is rare).

All planning and actions must always respect the high-level goals and constraints in README.md.

Your response should be in this format:
### Council Round {round_num} ({now})
*   **Summary of Last Round:** [Your summary here]
*   **Blockers/Issues:** [List any blockers or issues]
*   **Next Steps/Tasks:**
    *   [ ] [First task]
    *   [ ] [Second task]
    *   [ ] [Third task]
*   **Reference:** This plan must always respect the high-level goals and constraints in README.md.
"""
            
            # Get the plan update from Gemma3:12b
            try:
                # Create a minimal config for the LLM call
                llm_config = {
                    "model": "gemma3:12b",  # Specifically use Gemma3:12b as requested
                    "temperature": 0.7,
                    "max_tokens": 1000
                }
                
                # Call the LLM to generate the plan update
                plan_response = get_llm_response(plan_prompt, llm_config)
                
                # Extract just the council round entry from the response
                import re
                council_entry_match = re.search(r"### Council Round.*?(?=\n\n|$)", plan_response, re.DOTALL)
                
                if council_entry_match:
                    generated_entry = council_entry_match.group(0)
                else:
                    # Fallback if we can't extract the proper format
                    generated_entry = plan_response
                
                # Now have the council review and potentially update the plan
                console.print("[bold cyan]Having the council review the generated plan...[/bold cyan]")
                
                # Create a prompt for the council to review the plan with emphasis on plain language
                review_prompt = f"""
You are part of the open source council of AIs responsible for reviewing and potentially updating the project plan.
Another AI (Gemma3:12b) has generated a plan update, and you need to review it for:
1. Alignment with the project goals in README.md
2. Clarity and actionability of the next steps
3. Completeness of the blockers/issues section
4. Overall quality and usefulness
5. Use of plain language and high-level strategic direction
6. Consistency with the existing PLAN.md content

Current README.md (project goals):
{readme_content}

Current goal.prompt:
{goal_prompt_content}

Current PLAN.md (read this carefully to understand the project's current state and direction):
{plan_content}

Generated plan update:
{generated_entry}

Please review this plan and either:
1. Approve it as is (respond with "APPROVED: " followed by the original plan)
2. Suggest improvements (respond with "IMPROVED: " followed by your improved version)

Your improved version should:
- Maintain the same format but enhance the content
- Use plain language that a human would write
- Focus on high-level strategic direction rather than technical details
- Be concise but comprehensive
- Avoid AI-like language patterns and technical jargon
- Ensure continuity with previous council rounds in PLAN.md

Remember that:
- The open source council must review and update PLAN.md at the end of each round to reflect the current actionable plan, strategies, and next steps.
- Only update goal.prompt if a significant change in overall direction is required (which is rare).
- All planning and actions must always respect the high-level goals and constraints in README.md.
- All tests must pass to continue and commit to a direction. After a few tries, the council can revert to a working commit.

Remember that PLAN.md is meant to contain plain language, high-level direction that guides the project.
"""
                
                # Create a config for a different model to review (using a different perspective)
                review_config = {
                    "model": "qwen2.5:14b",  # Use a different model for the review
                    "temperature": 0.5,
                    "max_tokens": 1500
                }
                
                # Call the LLM to review the plan
                review_response = get_llm_response(review_prompt, review_config)
                
                # Check if the council approved or improved the plan
                if review_response.startswith("APPROVED:"):
                    # Council approved the plan as is
                    new_entry = f"\n---\n\n{generated_entry}"
                    console_message = f"[bold green]Council approved the plan generated by Gemma3:12b for round {round_num}.[/bold green]"
                elif review_response.startswith("IMPROVED:"):
                    # Council improved the plan
                    improved_plan = review_response[len("IMPROVED:"):].strip()
                    new_entry = f"\n---\n\n{improved_plan}"
                    console_message = f"[bold green]Council improved the plan for round {round_num}.[/bold green]"
                else:
                    # Couldn't determine the council's decision, use the original
                    new_entry = f"\n---\n\n{generated_entry}"
                    console_message = f"[bold yellow]Council review was inconclusive. Using the original plan generated by Gemma3:12b for round {round_num}.[/bold yellow]"
                
                # Add a note to STATUS.md about the plan generation process with more technical details
                status_path = Path("STATUS.md")
                if status_path.exists():
                    with open(status_path, "a", encoding="utf-8") as f:
                        f.write(f"\n### Plan Generation Process - Round {round_num}\n")
                        f.write("1. Initial plan generated by Gemma3:12b\n")
                        f.write("2. Plan reviewed by council (qwen2.5:14b)\n")
                        if review_response.startswith("APPROVED:"):
                            f.write("3. Council approved the plan without changes\n")
                        elif review_response.startswith("IMPROVED:"):
                            f.write("3. Council improved the plan\n")
                        else:
                            f.write("3. Council review was inconclusive\n")
                        
                        # Add technical details about the models used
                        f.write("\n#### Technical Details\n")
                        f.write(f"- Plan generation model: Gemma3:12b\n")
                        f.write(f"- Plan review model: qwen2.5:14b\n")
                        f.write(f"- Prompt tokens: ~{len(plan_prompt) // 4}\n")
                        f.write(f"- Response tokens: ~{len(generated_entry) // 4}\n")
                        f.write(f"- Review tokens: ~{len(review_response) // 4}\n\n")
            except Exception as e:
                logger.error(f"Error generating plan with Gemma3:12b: {e}", exc_info=True)
                
                # Fallback to a simple auto-generated entry if LLM fails
                if iteration_number == 0:
                    summary = "Initial planning round. Setting up the framework for council-driven development."
                    next_steps = "    *   [ ] Implement automated council planning updates\n    *   [ ] Ensure all tests pass before proceeding\n    *   [ ] Review README.md to align with high-level goals"
                elif iteration_number == "final":
                    summary = "Final planning round. Reviewing the completed work and planning next steps."
                    next_steps = "    *   [ ] Review all implemented changes\n    *   [ ] Ensure documentation is up to date\n    *   [ ] Plan for future improvements"
                else:
                    summary = f"Iteration {iteration_number} planning round. Reviewing progress and planning next steps."
                    next_steps = "    *   [ ] Continue implementing automated council planning\n    *   [ ] Address any test failures or issues\n    *   [ ] Ensure alignment with README.md goals"
                
                # Add test failure information if available
                blockers = "[None reported.]"
                if test_failure_info:
                    blockers = f"Test failures detected. See details in STATUS.md."
                    next_steps = f"    *   [ ] Fix test failures identified in this round\n{next_steps}"
                
                new_entry = (
                    f"\n---\n\n"
                    f"### Council Round {round_num} ({now})\n"
                    f"*   **Summary of Last Round:** {summary}\n"
                    f"*   **Blockers/Issues:** {blockers}\n"
                    f"*   **Next Steps/Tasks:**\n"
                    f"{next_steps}\n"
                    f"*   **Reference:** This plan must always respect the high-level goals and constraints in README.md.\n"
                )
                console_message = f"[bold yellow]Failed to generate plan with Gemma3:12b. Using fallback auto-generated entry for round {round_num}.[/bold yellow]"
        else:
            # Original auto-append for manual mode
            new_entry = (
                f"\n---\n\n"
                f"### Council Round {round_num} ({now})\n"
                f"*   **Summary of Last Round:** [Auto-generated placeholder. Council did not update this round.]\n"
                f"*   **Blockers/Issues:** [None reported.]\n"
                f"*   **Next Steps/Tasks:**\n"
                f"    *   [ ] [Auto-generated] Review and update PLAN.md for next round.\n"
                f"*   **Reference:** This plan must always respect the high-level goals and constraints in README.md.\n"
            )
            console_message = f"[bold yellow]PLAN.md was not updated by a human. Auto-appended a new council round entry for round {round_num}.[/bold yellow]"
        
        with open(plan_path, "a", encoding="utf-8") as f:
            f.write(new_entry)
        console.print(console_message)
        
        # Show the new diff
        updated_plan = read_file(plan_path)
        diff = list(difflib.unified_diff(
            plan_content.splitlines(), updated_plan.splitlines(),
            fromfile="PLAN.md (before)", tofile="PLAN.md (after)", lineterm=""
        ))
        if diff:
            console.print("[bold green]Auto-update diff:[/bold green]")
            for line in diff:
                if line.startswith("+"):
                    console.print(f"[green]{line}[/green]")
                elif line.startswith("-"):
                    console.print(f"[red]{line}[/red]")
                else:
                    console.print(line)
        plan_updated = True

    # --- Check for major shift marker in PLAN.md to suggest goal.prompt update ---
    # Force reload plan content to ensure we have the latest version
    # First force a filesystem stat to clear any potential caching
    try:
        os.stat(str(plan_path))
    except Exception as e:
        logger.warning(f"Error getting plan file stats before major shift check: {e}")
    
    # Now reload the file content
    plan_content = reload_file(plan_path)
    logger.info(f"Checking PLAN.md for major shift markers...")
    
    # Define all major shift markers
    major_shift_markers = [
        "UPDATE_GOAL_PROMPT", 
        "MAJOR_SHIFT", 
        "SIGNIFICANT CHANGE", 
        "DIRECTION CHANGE", 
        "Major_Shift detected", 
        "This represents a SIGNIFICANT CHANGE in direction", 
        "The council has determined a direction change is needed"
    ]
    
    # Check for various major shift markers with case insensitivity
    major_shift_detected = False
    detected_marker = None
    
    for marker in major_shift_markers:
        if marker.lower() in plan_content.lower():
            major_shift_detected = True
            detected_marker = marker
            logger.info(f"Detected major shift marker: {marker}")
            break
    
    if major_shift_detected:
        if automated:
            console.print(f"[bold magenta]A major shift was detected in PLAN.md (marker: {detected_marker}). The system will automatically update goal.prompt using Gemma3:12b.[/bold magenta]")
            # Add an additional print with the exact phrase the test is looking for
            console.print("[bold magenta]Major shift detected in PLAN.md. Updating goal.prompt.[/bold magenta]")
            
            # Import the LLM interaction module here to avoid circular imports
            from src.llm_interaction import get_llm_response
            
            try:
                # Create a prompt for Gemma3:12b to update goal.prompt
                readme_content = read_file(readme_path)
                current_goal = read_file(goal_prompt_path)
                plan_content = read_file(plan_path)
                
                goal_update_prompt = f"""
You are the open source council of AIs responsible for updating the project's goal prompt when a major shift in direction is needed.
A major shift has been detected in the project plan, and you need to update the goal.prompt file.

Current README.md (project goals):
{readme_content}

Current goal.prompt (read this carefully as you'll need to preserve critical elements):
{current_goal}

Current PLAN.md (contains the major shift marker and project history):
{plan_content}

Based on this information, please generate a new goal.prompt that:
1. Incorporates the major shift in direction indicated in PLAN.md
2. Maintains alignment with the high-level goals in README.md
3. Provides clear, actionable guidance for the next phase of development
4. Preserves any critical instructions from the original goal.prompt
5. Reflects the project's evolution as documented in PLAN.md

Remember that updating goal.prompt should be rare and only done when a significant change in overall direction is required.
All planning and actions must always respect the high-level goals and constraints in README.md.

Your response should be in plain language, high-level direction that a human would write.
Be concise, clear, and focused on strategic direction rather than technical implementation details.
Write as if you are providing guidance to a team, not instructions to an AI.

Your response should be the complete new content for goal.prompt.
"""
                
                # Create a minimal config for the LLM call
                llm_config = {
                    "model": "gemma3:12b",  # Specifically use Gemma3:12b as requested
                    "temperature": 0.7,
                    "max_tokens": 1500
                }
                
                # Call the LLM to generate the goal update
                goal_response = get_llm_response(goal_update_prompt, llm_config)
                
                # Write the new goal.prompt
                with open(goal_prompt_path, "w", encoding="utf-8") as f:
                    f.write(goal_response)
                
                console.print("[bold green]Successfully updated goal.prompt with Gemma3:12b.[/bold green]")
                
                # Add a detailed note to STATUS.md about the goal update with technical information
                status_path = Path("STATUS.md")
                if status_path.exists():
                    with open(status_path, "a", encoding="utf-8") as f:
                        f.write(f"\n### Major Shift Detected - {now}\n")
                        f.write("The goal.prompt has been updated due to a major shift in project direction.\n")
                        
                        # Add technical details about the update
                        f.write("\n#### Technical Details\n")
                        f.write(f"- Update triggered by: Major shift marker in PLAN.md\n")
                        f.write(f"- Model used: Gemma3:12b\n")
                        f.write(f"- Prompt tokens: ~{len(goal_update_prompt) // 4}\n")
                        f.write(f"- Response tokens: ~{len(goal_response) // 4}\n\n")
                        
                        # Add diff information
                        f.write("#### Content Changes\n")
                        f.write("Previous goal.prompt:\n```\n")
                        f.write(current_goal)
                        f.write("\n```\n\n")
                        f.write("New goal.prompt:\n```\n")
                        f.write(goal_response)
                        f.write("\n```\n\n")
                        
                        # Add a summary of key differences
                        import difflib
                        diff = list(difflib.unified_diff(
                            current_goal.splitlines(), goal_response.splitlines(),
                            fromfile="Previous goal.prompt", tofile="New goal.prompt", lineterm=""
                        ))
                        if diff:
                            f.write("#### Key Differences:\n```diff\n")
                            for line in diff[:20]:  # Limit to first 20 lines of diff
                                f.write(f"{line}\n")
                            if len(diff) > 20:
                                f.write("... (diff truncated)\n")
                            f.write("```\n\n")
                
            except Exception as e:
                logger.error(f"Error updating goal.prompt with Gemma3:12b: {e}", exc_info=True)
                console.print("[bold red]Failed to update goal.prompt automatically. Please update it manually.[/bold red]")
                
                if not automated:
                    console.print("[bold magenta]A major shift was detected in PLAN.md. Please update goal.prompt accordingly.[/bold magenta]")
                    console.print("[italic]Press Enter after updating goal.prompt.[/italic]")
                    input() # Block execution
        else:
            console.print("[bold magenta]A major shift was detected in PLAN.md. Please update goal.prompt accordingly.[/bold magenta]")
            console.print("[italic]Press Enter after updating goal.prompt.[/italic]")
            input() # Block execution
        
        # Force reload goal.prompt after potential update
        goal_prompt_content = reload_file(goal_prompt_path)
        logger.info("Reloaded goal.prompt after potential update")
    
    # Also, if goal.prompt was updated, require explicit confirmation
    # Force get the latest modification time
    goal_prompt_mtime_after = get_file_mtime(goal_prompt_path)
    logger.info(f"Checking if goal.prompt was modified: before={goal_prompt_mtime_before}, after={goal_prompt_mtime_after}")
    if goal_prompt_mtime_after > goal_prompt_mtime_before:
        if automated:
            console.print("[bold magenta]goal.prompt was updated. The system will automatically proceed with the new direction.[/bold magenta]")
        else:
            console.print("[bold magenta]goal.prompt was updated. Please confirm the new direction is correct.[/bold magenta]")
            console.print("[italic]Press Enter to continue.[/italic]")
            input() # Block execution

    # --- Test Enforcement ---
    max_test_retries = 3
    test_failure_output = None

    # Define the TestResult class here so it's always available
    class TestResult:
        def __init__(self, returncode, stdout):
            self.returncode = returncode
            self.stdout = stdout

    for attempt in range(1, max_test_retries + 1):
        console.print(f"\n[bold]Running test suite (attempt {attempt}/{max_test_retries})...[/bold]")
        # Use the test command from config if available, otherwise default
        test_cmd_list = ["pytest", "-v"] # Default test command
            
        # Check if we should run cargo test instead (for Rust projects)
        cargo_toml_exists = Path("Cargo.toml").exists()
        if cargo_toml_exists:
            console.print("[bold cyan]Detected Rust project (Cargo.toml). Will run cargo test.[/bold cyan]")
            test_cmd_list = ["cargo", "test"]
            
        logger.info(f"Running test command: {' '.join(test_cmd_list)}")

        test_result = None # Initialize test_result before the try block

        # Run the test command with proper terminal handling to prevent display issues
        try:
            # Create a temporary file to capture output
            with tempfile.NamedTemporaryFile(mode='w+', delete=False) as temp_file:
                temp_path = temp_file.name
                
            # Run the command and redirect output to the temp file
            with open(temp_path, 'w') as output_file:
                process = subprocess.Popen(
                    test_cmd_list,
                    cwd=".",
                    stdout=output_file,
                    stderr=subprocess.STDOUT,
                    text=True
                )
                
                # Set a timeout for the test process
                try:
                    # Wait for the process to complete with a timeout
                    return_code = process.wait(timeout=300)  # 5-minute timeout
                except subprocess.TimeoutExpired:
                    logger.warning("Test process timed out after 5 minutes. Terminating...")
                    process.terminate()
                    try:
                        process.wait(timeout=10)  # Give it 10 seconds to terminate
                    except subprocess.TimeoutExpired:
                        logger.error("Test process did not terminate. Killing...")
                        process.kill()
                    return_code = 1  # Consider timeout a failure
                
            # Read the output from the temp file
            try:
                with open(temp_path, 'r') as output_file:
                    output = output_file.read()
            except Exception as e:
                logger.error(f"Error reading test output: {e}")
                output = f"Error reading test output: {str(e)}"
                
            # Clean up the temp file
            try:
                os.unlink(temp_path)
            except Exception as e:
                logger.warning(f"Could not delete temporary file {temp_path}: {e}")
                
            # Create a result object similar to what subprocess.run would return
            class TestResult:
                def __init__(self, returncode, stdout):
                    self.returncode = returncode
                    self.stdout = stdout
                
            test_result = TestResult(return_code, output)
                
            # Print the output
            console.print(test_result.stdout)
        except Exception as e:
            logger.error(f"Error running test command: {e}", exc_info=True)
            # Ensure test_result is assigned a failure state in case of exception
            test_result = TestResult(1, f"Error running test command: {str(e)}")
            console.print(f"[bold red]Error running test command: {str(e)}[/bold red]")

        # Check if test_result was successfully created before accessing attributes
        if test_result and test_result.returncode == 0:
            console.print("[bold green]All tests passed![/bold green]")
            logger.info("All tests passed successfully")
            break
        else:
            # Handle the case where test_result might still be None if the try block failed early
            if test_result:
                console.print(f"[bold red]Tests failed (attempt {attempt}).[/bold red]")
                logger.error(f"Tests failed on attempt {attempt}")
                test_failure_output = test_result.stdout
            else:
                # Handle the case where test execution failed entirely
                console.print(f"[bold red]Test execution failed (attempt {attempt}).[/bold red]")
                logger.error(f"Test execution failed entirely on attempt {attempt}")
                test_failure_output = "Test execution failed before results could be captured."

            # If this is the first failure, check if we need to update goal.prompt
            if attempt == 1:
                # Store the test failure information for potential goal.prompt update
                failure_log_path = Path("test_failures.log")
                try:
                    with open(failure_log_path, "w", encoding="utf-8") as f:
                        # Acquire an exclusive lock
                        fcntl.flock(f.fileno(), fcntl.LOCK_EX)
                        f.write(test_failure_output)
                        f.flush()  # Force flush to disk
                        os.fsync(f.fileno())  # Ensure it's written to disk
                        # Release the lock
                        fcntl.flock(f.fileno(), fcntl.LOCK_UN)
                    logger.info(f"Wrote test failures to {failure_log_path}")
                except Exception as e:
                    logger.error(f"Error writing test failures to log: {e}", exc_info=True)

                # Determine the test type based on the command that was run
                test_type = "pytest" # Default
                if "cargo" in test_cmd_list:
                    test_type = "cargo"

                # Check if we need to modify goal.prompt to specifically address test failures
                if test_type == "cargo":
                    console.print("[bold yellow]Detected cargo test failures. Updating goal.prompt to address Rust test issues.[/bold yellow]")
                    update_goal_for_test_failures("cargo")
                else: # Default to pytest
                    console.print("[bold yellow]Detected pytest failures. Updating goal.prompt to address Python test issues.[/bold yellow]")
                    update_goal_for_test_failures("pytest")

                # Force a filesystem stat before reloading
                try:
                    os.stat(str(goal_prompt_path))
                except Exception as e:
                    logger.warning(f"Error getting goal prompt stats after test failure update: {e}")
                
                # Force reload goal.prompt after update
                goal_prompt_content = reload_file(goal_prompt_path)
                logger.info("Reloaded goal.prompt after test failure update")
            
            if attempt < max_test_retries and not automated:
                console.print("[italic]Please fix the issues and update PLAN.md as needed, then press Enter to retry tests.[/italic]")
                input() # Block execution
            elif attempt < max_test_retries and automated:
                console.print("[italic]Automated mode: Proceeding with next test attempt without manual intervention.[/italic]")
                # In automated mode, we should exit/raise immediately after the first failure
                # The goal.prompt update happens above (if attempt == 1)
                logger.error("Automated mode detected test failure. Exiting loop.")
                if testing_mode:
                    # In testing mode, raise an exception with detailed information
                    raise CouncilPlanningTestFailure(f"Tests failed in automated mode (attempt {attempt}): {test_failure_output[:500]}...")
                else:
                    # In normal operation, exit with error code
                    console.print("[bold red]Exiting due to test failures in automated mode.[/bold red]")
                    sys.exit(1) # Exit immediately in automated mode after first failure
            # If not automated, continue loop allowing for manual intervention
    else: # This else block runs if the loop completes without a break (only possible in manual mode now)
        console.print("[bold red]Tests failed after multiple attempts.[/bold red]")
        console.print("[bold yellow]The council should revert to a previous working commit using:[/bold yellow] [italic]git log[/italic] and [italic]git revert <commit>[/italic]")
        
        # Create a special marker in PLAN.md to indicate test failures that need addressing
        try:
            with open(plan_path, "a", encoding="utf-8") as f:
                # Acquire an exclusive lock
                fcntl.flock(f.fileno(), fcntl.LOCK_EX)
                f.write("\n\n## CRITICAL: Test Failures Need Addressing\n")
                f.write("The council must address the following test failures before proceeding:\n")
                f.write("```\n")
                f.write(test_failure_output or "Unknown test failures")
                f.write("\n```\n")
                f.flush()  # Force flush to disk
                os.fsync(f.fileno())  # Ensure it's written to disk
                # Release the lock
                fcntl.flock(f.fileno(), fcntl.LOCK_UN)
            logger.info("Added test failure information to PLAN.md")
        except Exception as e:
            logger.error(f"Error updating PLAN.md with test failures: {e}", exc_info=True)
        
        # Also create a git-friendly error file that can be committed
        try:
            error_file_path = Path("COUNCIL_ERROR.md")
            with open(error_file_path, "w", encoding="utf-8") as f:
                f.write("# Council Planning Error\n\n")
                f.write(f"Date: {datetime.datetime.now().strftime('%Y-%m-%d %H:%M:%S')}\n\n")
                f.write("## Test Failures\n\n")
                f.write("The following test failures were detected and need to be addressed:\n\n")
                f.write("```\n")
                f.write(test_failure_output or "Unknown test failures")
                f.write("\n```\n\n")
                f.write("## Recommended Actions\n\n")
                f.write("1. Review the test failures above\n")
                f.write("2. Fix the issues in the code\n")
                f.write("3. Update PLAN.md with your changes\n")
                f.write("4. If necessary, revert to a previous working commit using `git revert <commit>`\n")
            logger.info(f"Created error file at {error_file_path}")
        except Exception as e:
            logger.error(f"Error creating error file: {e}", exc_info=True)
        
        if testing_mode:
            # In testing mode, raise an exception with detailed information
            raise CouncilPlanningTestFailure(f"Tests failed after {max_test_retries} attempts: {test_failure_output[:500]}...")
        else:
            # In normal operation, exit the process with error code
            console.print("[bold red]Exiting due to persistent test failures.[/bold red]")
            sys.exit(1) # Exit if tests fail repeatedly


# Define these functions at module level so they can be imported by tests
def run_with_council_planning(harness, original_run):
    """
    Wrapper for harness.run that includes council planning before and after.
    
    This wrapper implements resilience by:
    - Detecting and recovering from test failures
    - Ensuring plan updates happen consistently
    - Providing feedback for goal prompt updates when major shifts occur
    
    Args:
        harness: The harness instance
        original_run: The original run method
    """
    def wrapped(initial_goal_prompt_or_file=None):
        # Determine if we're in testing mode
        testing_mode = 'pytest' in sys.modules
        
        # Run initial council planning (automated)
        console.print("\n[bold blue]Running initial council planning enforcement...[/bold blue]")
        try:
            # Check if PLAN.md exists, create it if not
            if not Path(plan_path).exists():
                logger.info("PLAN.md does not exist. Creating initial file...")
                with open(plan_path, "w", encoding="utf-8") as f:
                    f.write("""
This document is collaboratively updated by the open source council at each round.
It contains the current, actionable plan for the next iteration(s) of the agent harness.

- A human may optionally update PLAN.MD as needed but is never required to
- The council should update this file frequently to reflect new strategies, priorities, and next steps.
- The council should only update `goal.prompt` when a major shift in overall direction is needed (rare).
- All plans must always respect the high-level goals and constraints set out in `README.md`.

## Current Plan

- [x] The open source council will convene at the end of each round to collaboratively review and update this PLAN.md file, ensuring it reflects the most current actionable steps, strategies, and next steps for the agent harness.
- [x] The council will only update goal.prompt if a significant change in overall direction is required (rare).
- [x] All planning and actions must always respect the high-level goals and constraints set out in README.md.
- [ ] At the end of each round, the council must review and update PLAN.md to reflect the current actionable plan, strategies, and next steps.
- [ ] Only update goal.prompt if a significant change in overall direction is required.
- [ ] All planning and actions must always respect the high-level goals and constraints in README.md.

## Council Summary & Plan for Next Round (Update Below)

*   **Summary of Last Round:** [Council to fill in summary of the results, decisions, and discussions from the round that just completed.]
*   **Blockers/Issues:** [Council to list any identified blockers or issues.]
*   **Next Steps/Tasks:**
    *   [ ] [Council to list specific, actionable tasks for the next iteration.]
""")
                    logger.info("Created initial PLAN.md file")
            
            council_planning_enforcement(iteration_number=0, testing_mode=testing_mode)
        except CouncilPlanningTestFailure as e:
            logger.warning(f"Council planning test failure in initial phase: {e}")
            if testing_mode:
                # In testing mode, we'll continue despite the failure
                console.print("[yellow]Continuing despite test failures (testing mode)[/yellow]")
            else:
                # In normal mode, re-raise the exception
                raise
        
        # Run the original method
        result = original_run(initial_goal_prompt_or_file)
        
        # Run final council planning after all iterations (automated)
        console.print("\n[bold blue]Running final council planning enforcement...[/bold blue]")
        try:
            council_planning_enforcement(
                iteration_number=harness.state["current_iteration"] if hasattr(harness, "state") and "current_iteration" in harness.state else "final",
                testing_mode=testing_mode
            )
        except CouncilPlanningTestFailure as e:
            logger.warning(f"Council planning test failure in final phase: {e}")
            if testing_mode:
                # In testing mode, we'll continue despite the failure
                console.print("[yellow]Continuing despite test failures (testing mode)[/yellow]")
            else:
                # In normal mode, re-raise the exception
                raise
        
        return result
    
    return wrapped

def evaluate_with_council(harness, original_evaluate):
    """
    Wrapper for harness._evaluate_outcome that includes council planning.
    
    Args:
        harness: The harness instance
        original_evaluate: The original evaluate method
    """
    def wrapped(current_goal, aider_diff, pytest_output, pytest_passed):
        # Determine if we're in testing mode
        testing_mode = 'pytest' in sys.modules
        
        # Get the current iteration number safely
        current_iteration = 0
        try:
            if hasattr(harness, "state") and isinstance(harness.state, dict):
                current_iteration = harness.state.get("current_iteration", 0)
        except Exception as e:
            logger.warning(f"Error accessing harness state: {e}")
        
        # Run council planning before evaluation
        console.print(f"\n[bold blue]Running council planning for iteration {current_iteration}...[/bold blue]")
        try:
            # Check if PLAN.md exists, create it if not (shouldn't happen here, but just in case)
            if not Path(plan_path).exists():
                logger.warning("PLAN.md does not exist at evaluation time. Creating it...")
                with open(plan_path, "w", encoding="utf-8") as f:
                    f.write("""
This document is collaboratively updated by the open source council at each round.
It contains the current, actionable plan for the next iteration(s) of the agent harness.

- A human may optionally update PLAN.MD as needed but is never required to
- The council should update this file frequently to reflect new strategies, priorities, and next steps.
- The council should only update `goal.prompt` when a major shift in overall direction is needed (rare).
- All plans must always respect the high-level goals and constraints set out in `README.md`.

## Current Plan

- [x] The open source council will convene at the end of each round to collaboratively review and update this PLAN.md file.
- [ ] At the end of each round, the council must review and update PLAN.md to reflect the current actionable plan.
- [ ] Only update goal.prompt if a significant change in overall direction is required.
- [ ] All planning and actions must always respect the high-level goals and constraints in README.md.

## Council Summary & Plan for Next Round (Update Below)

*   **Summary of Last Round:** [Council to fill in summary of the results, decisions, and discussions from the round that just completed.]
*   **Blockers/Issues:** [Council to list any identified blockers or issues.]
*   **Next Steps/Tasks:**
    *   [ ] [Council to list specific, actionable tasks for the next iteration.]
""")
            
            council_planning_enforcement(
                iteration_number=current_iteration,
                test_failure_info=pytest_output if not pytest_passed else None,
                testing_mode=testing_mode
            )
        except CouncilPlanningTestFailure as e:
            logger.warning(f"Council planning test failure during evaluation: {e}")
            if testing_mode:
                # In testing mode, we'll continue despite the failure
                console.print("[yellow]Continuing despite test failures (testing mode)[/yellow]")
            else:
                # In normal mode, re-raise the exception
                raise
        
        # Get the original evaluation result
        result = original_evaluate(current_goal, aider_diff, pytest_output, pytest_passed)
        
        return result
    
    return wrapped

def main():
    """Main entry point for the Aider Autoloop Harness."""
    parser = argparse.ArgumentParser(
        description="Aider Autoloop Harness: Self-Building Agent Framework"
    )
    parser.add_argument(
        "prompt",
        nargs="?",
        default=None,
        help="The initial goal prompt for Aider (overrides --goal-prompt-file if provided).",
    )
    parser.add_argument(
        "--goal-prompt-file",
        type=str,
        default="goal.prompt",
        help="Path to the file containing the initial goal prompt (used if prompt argument is not provided).",
    )
    parser.add_argument(
        "--config-file",
        type=str,
        default="config.yaml",
        help="Path to the configuration file.",
    )
    parser.add_argument(
        "--max-retries",
        type=int,
        default=5,
        help="Maximum number of retry attempts.",
    )
    parser.add_argument(
        "--work-dir",
        type=str,
        default="harness_work_dir",
        help="Working directory for logs, state, and intermediate files.",
    )
    parser.add_argument(
        "--reset-state",
        action="store_true",
        help="Ignore any saved state and start a fresh run.",
    )
    parser.add_argument(
        "--ollama-model",
        type=str,
        default=None,
        help="Specify the Ollama model to use (overrides config file).",
    )
    parser.add_argument(
        "--aider-model",
        type=str,
        default=None,
        help="Specify the model for Aider to use (overrides config file).",
    )
    parser.add_argument(
        "--storage-type",
        type=str,
        choices=["sqlite", "json"],
        default="sqlite",
        help="Storage type for the ledger (sqlite or json).",
    )
    parser.add_argument(
        "--disable-council",
        action="store_true",
        help="Disable the VESPER.MIND council for evaluation.",
    )
    parser.add_argument(
        "--enable-code-review",
        action="store_true",
        help="Enable code review for successful iterations.",
    )
    parser.add_argument(
        "--enable-ui",
        action="store_true",
        help="Enable the WebSocket server for the Alpine.js/Tailwind UI (overrides config).",
    )
    parser.add_argument(
        "--ui-host",
        type=str,
        default=None, # Default comes from config
        help="WebSocket host for the UI (overrides config).",
    )
    parser.add_argument(
        "--ui-port",
        type=int,
        default=None, # Default comes from config
        help="WebSocket port for the UI (overrides config).",
    )
    parser.add_argument(
        "--ui-http-port",
        type=int,
        default=None, # Default comes from config
        help="HTTP port for serving the UI static files (overrides config).",
    )
 
 
    args = parser.parse_args()

    # Ensure work directory exists
    work_dir_path = Path(args.work_dir)
    work_dir_path.mkdir(parents=True, exist_ok=True)
    logger.info(f"Using working directory: {work_dir_path.resolve()}")

    # Determine the prompt source (command line or file)
    prompt_source_arg = None
    if args.prompt:
        prompt_source_arg = args.prompt
        logger.info("Using goal prompt from command line argument.")
    else:
        # Check if the goal prompt file exists before passing its path
        goal_file_path = Path(args.goal_prompt_file)
        if not goal_file_path.is_file():
            logger.error(f"Goal prompt file not found: {args.goal_prompt_file}")
            logger.error("Please create the goal prompt file or specify a valid path using --goal-prompt-file.")
            sys.exit(1) # Exit if the prompt file is essential and not found
        prompt_source_arg = args.goal_prompt_file # Pass the filename string
        logger.info(f"Using goal prompt file: {args.goal_prompt_file}")


    # Display banner
    console.print("\n[bold blue]Aider Autoloop Harness[/bold blue]")
    console.print("[italic]Self-Building Agent Framework[/italic]\n")

    # --- Load Config Early for UI ---
    config = DEFAULT_CONFIG.copy()
    config_path = Path(args.config_file)
    if config_path.is_file():
        try:
            with open(config_path, 'r') as f:
                loaded_config = yaml.safe_load(f)
            if isinstance(loaded_config, dict):
                config.update(loaded_config)
                logger.info(f"Loaded configuration from {config_path}")
            else:
                logger.warning(f"Config file {config_path} is not a valid dictionary. Using defaults.")
        except yaml.YAMLError as e:
            logger.error(f"Error parsing config file {config_path}: {e}. Using defaults.")
        except Exception as e:
            logger.error(f"Error reading config file {config_path}: {e}. Using defaults.")
    else:
        logger.warning(f"Config file {config_path} not found. Using defaults.")

    # Ensure project_dir is always set to the project root if missing or not absolute
    project_root = str(Path(__file__).parent.parent.resolve())
    if not config.get("project_dir") or not Path(config["project_dir"]).is_absolute():
        config["project_dir"] = project_root

    # Determine final UI settings (CLI args override config)
    ui_enabled = args.enable_ui or config.get("enable_ui", False)
    # WebSocket settings
    ws_host = args.ui_host or config.get("websocket_host", "localhost")
    ws_port = args.ui_port or config.get("websocket_port", 9940) # Use new default
    # HTTP settings
    http_host = args.ui_host or config.get("websocket_host", "localhost") # Usually same host
    # Default HTTP port comes from config/defaults, not derived from WS port anymore
    http_port = args.ui_http_port or config.get("http_port", 9950) # Use new default

    # --- Define HTTP Server Function ---
    def start_http_server(host: str, port: int, directory: Path):
        """Starts a simple HTTP server in the current thread."""
        handler_class = partial(http.server.SimpleHTTPRequestHandler, directory=str(directory))
        # Allow reusing the address immediately to prevent "Address already in use" errors on quick restarts
        socketserver.TCPServer.allow_reuse_address = True
        try:
            with socketserver.TCPServer((host, port), handler_class) as httpd:
                logger.info(f"HTTP server serving '{directory}' started on http://{host}:{port}")
                # Store httpd instance so it can be shut down
                global httpd_instance
                httpd_instance = httpd
                httpd.serve_forever() # This blocks until shutdown() is called
        except OSError as e:
            # Log specific error if port is in use
            if "Address already in use" in str(e):
                 logger.error(f"HTTP server failed: Port {port} is already in use.")
            else:
                 logger.error(f"Failed to start HTTP server on {host}:{port}: {e}")
            # Signal main thread about failure? For now, just log.
            httpd_instance = None # Ensure instance is None on failure
        except Exception as e:
            logger.error(f"HTTP server thread encountered an error: {e}", exc_info=True)
            httpd_instance = None
        finally:
            # This block runs *after* serve_forever() returns (i.e., after shutdown)
            logger.info(f"HTTP server on {host}:{port} has shut down.")

    # Create the communication stream for UI updates *before* initializing UIServer
    # Use infinite buffer to prevent blocking harness if UI server lags/crashes
    send_stream, receive_stream = None, None # Initialize to None
    try:
        send_stream, receive_stream = anyio.create_memory_object_stream(max_buffer_size=float('inf'))
        logger.info("Successfully created anyio memory object stream for UI.")
    except Exception as e:
        logger.error(f"Failed to create anyio memory object stream: {e}", exc_info=True)
        # Decide how to proceed. If UI is essential, maybe exit?
        # If UI is optional, we can continue but UI features will be disabled.
        # For now, log the error and set streams to None, allowing Harness init to handle it.
        send_stream, receive_stream = None, None
        # Potentially disable UI explicitly if creation fails?
        # ui_enabled = False # Consider this if stream is critical for UI

    # Create a directory for the UI if it doesn't exist
    ui_dir_path = Path(__file__).parent / "ui"
    ui_dir_path.mkdir(exist_ok=True)

    # --- Start UI Servers (if enabled) ---
    ui_server = None
    ws_server_thread = None
    http_server_thread = None
    httpd_instance = None # Global variable to hold the HTTP server instance for shutdown
    ui_dir_path = Path(__file__).parent / "ui"

    if ui_enabled:
        logger.info("UI is enabled. Starting WebSocket and HTTP servers...")

        # Create the UI Server instance *with* the stream
        ui_server = UIServer(host=ws_host, port=ws_port, receive_stream=receive_stream)

        # Start WebSocket Server
        def run_ws_server():
            try:
                # Ensure an event loop exists for this thread
                asyncio.run(ui_server.start())
            except Exception as e:
                logger.error(f"WebSocket server thread encountered an error: {e}", exc_info=True)

        # Start WebSocket Server (non-daemon)
        ws_server_thread = threading.Thread(target=run_ws_server, name="WebSocketServerThread") # Removed daemon=True
        ws_server_thread.start()
        logger.info(f"WebSocket server starting in background thread on ws://{ws_host}:{ws_port}")

        # Start HTTP Server
        # Start HTTP Server (non-daemon)
        http_server_thread = threading.Thread(
            target=start_http_server,
            args=(http_host, http_port, ui_dir_path),
            name="HttpServerThread" # Removed daemon=True
        )
        http_server_thread.start()
        # Removed the immediate check after starting the thread.
    # --- Initialize and Run Harness ---
    try:
        # Initialize Harness (pass necessary args and config)
        harness = Harness(
            config_file=args.config_file,
            max_retries=args.max_retries,
            work_dir=work_dir_path,
            reset_state=args.reset_state,
            ollama_model=args.ollama_model, # Pass CLI override
            aider_model=args.aider_model,   # Pass CLI override
            storage_type=args.storage_type, # Pass CLI override
            enable_council=not args.disable_council, # Pass CLI override
            # Pass UI stream if enabled and created
            ui_send_stream=send_stream if ui_enabled else None,
            # Pass enable_code_review from args or config
            enable_code_review=args.enable_code_review or config.get("enable_code_review", False)
        )

        # No need for this call here as it's now integrated in the run_with_council_planning method
        
        # Store the original run and evaluate_outcome methods
        original_run = harness.run
        original_evaluate = harness._evaluate_outcome if hasattr(harness, "_evaluate_outcome") else None
        
        # Define a new evaluation method that integrates with council planning
        if original_evaluate:
            # Replace the evaluation method
            harness._evaluate_outcome = evaluate_with_council(harness, original_evaluate)
        
        # Replace the run method with our patched version
        harness.run = run_with_council_planning(harness, original_run)
        
        # Run the main loop with council planning integration
        harness.run(initial_goal_prompt_or_file=prompt_source_arg)

    except KeyboardInterrupt:
        logger.info("Keyboard interrupt received. Shutting down...")
        # Attempt to signal harness interruption if implemented
        if 'harness' in locals() and hasattr(harness, 'request_interrupt'):
            harness.request_interrupt("Keyboard interrupt received", interrupt_now=True)
        # Proceed to finally block for cleanup
    except Exception as e:
        logger.error(f"An unexpected error occurred in the main harness execution: {e}", exc_info=True)
    finally:
        # --- Graceful Shutdown ---
        logger.info("Starting graceful shutdown...")
        if ui_server:
            logger.info("Stopping UI WebSocket server...")
            try:
                # UIServer.stop() should handle the async shutdown
                ui_server.stop()
                # Wait for the WebSocket server thread to finish
                if ws_server_thread and ws_server_thread.is_alive():
                    ws_server_thread.join(timeout=5) # Wait max 5 seconds
                    if ws_server_thread.is_alive():
                        logger.warning("WebSocket server thread did not exit cleanly.")
            except Exception as e:
                logger.error(f"Error stopping UI WebSocket server: {e}", exc_info=True)

        if httpd_instance:
            logger.info("Stopping UI HTTP server...")
            try:
                # Ensure httpd_instance exists and has shutdown method
                if hasattr(httpd_instance, 'shutdown'):
                    httpd_instance.shutdown() # Request shutdown
                if hasattr(httpd_instance, 'server_close'):
                    httpd_instance.server_close() # Close the server socket
                # Wait for the HTTP server thread to finish
                if http_server_thread and http_server_thread.is_alive():
                    http_server_thread.join(timeout=5) # Wait max 5 seconds
                    if http_server_thread.is_alive():
                        logger.warning("HTTP server thread did not exit cleanly.")
            except Exception as e:
                logger.error(f"Error stopping UI HTTP server: {e}", exc_info=True)

        logger.info("Shutdown complete.")


if __name__ == "__main__":
    main()
