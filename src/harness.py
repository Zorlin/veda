import logging
import os
import subprocess
import time
from pathlib import Path
import json
from typing import Dict, Any, Optional, List, Tuple

import yaml

from .aider_interaction import run_aider
from .llm_interaction import get_llm_response
from .pytest_interaction import run_pytest
from .ledger import Ledger
from .vesper_mind import VesperMind
import re


class Harness:
    """
    Orchestrates the Aider-Ollama-Pytest loop with enhanced features:
    - SQLite/JSON ledger for persistent state
    - VESPER.MIND council for evaluation
    - Code review capabilities
    """

    def __init__(
        self,
        config_file: str = "config.yaml",
        max_retries: int = 5,
        work_dir: Path = Path("harness_work_dir"),
        reset_state: bool = False,
        ollama_model: Optional[str] = None,
        storage_type: str = "sqlite",  # "sqlite" or "json"
        enable_council: bool = True,
        enable_code_review: bool = False
    ):
        self.config_file = config_file
        self.max_retries = max_retries
        self.work_dir = work_dir
        self.config: Dict[str, Any] = self._load_config()
        
        # Override config model if CLI argument is provided
        if ollama_model:
            logging.info(f"Overriding configured Ollama model with command-line argument: {ollama_model}")
            self.config["ollama_model"] = ollama_model
        
        # Initialize ledger for persistent state
        self.ledger = Ledger(
            work_dir=self.work_dir,
            storage_type=storage_type
        )
        
        # Initialize VESPER.MIND council if enabled
        self.enable_council = enable_council
        if enable_council:
            self.council = VesperMind(
                config=self.config,
                ledger=self.ledger,
                work_dir=self.work_dir
            )
        else:
            self.council = None
        
        # Initialize state from ledger or create new state
        self.state = self._initialize_state(reset_state)
        
        # Code review settings
        self.enable_code_review = enable_code_review
        self.current_run_id = None
        
        logging.info(f"Harness initialized. Max retries: {self.max_retries}")
        logging.info(f"Working directory: {self.work_dir.resolve()}")
        logging.info(f"Storage type: {storage_type}")
        logging.info(f"VESPER.MIND council enabled: {enable_council}")
        logging.info(f"Code review enabled: {enable_code_review}")

    def _load_config(self) -> Dict[str, Any]:
        """Loads configuration from the YAML file."""
        default_config = {
            "ollama_model": "gemma3:12b", # Set default to gemma3:12b
            "ollama_api_url": "http://localhost:11434/api/generate", # TODO: Use this
            "aider_command": "aider", # Adjust if aider is not in PATH
            "aider_test_command": "pytest -v", # Default test command for Aider
            "project_dir": ".", # Directory Aider should operate on
            "ollama_request_timeout": 300, # Default timeout for Ollama requests (seconds)
            # TODO: Add other necessary config like pytest command, etc.
        }
        config = default_config.copy()
        config_path = None # Initialize config_path

        # Handle config_file=None case explicitly
        if self.config_file is None:
            logging.info("No config file specified. Using default configuration.")
            # project_dir defaults to "." from default_config
            project_dir_path = Path(config.get("project_dir", "."))
            if not project_dir_path.is_absolute():
                project_dir_path = Path.cwd() / project_dir_path
            config["project_dir"] = str(project_dir_path.resolve())
            # IMPORTANT: When config_file is None, use work_dir directly as passed in __init__
            # Do not resolve it relative to project_dir here. Ensure it's absolute.
            self.work_dir = self.work_dir.resolve()
            logging.info(f"Using provided working directory directly: {self.work_dir}")
            # State initialization happens after this method returns in __init__
            return config
        else:
             # Proceed with loading from the specified config file
             logging.info(f"Loading configuration from {self.config_file}...")
             config_path = Path(self.config_file)
             try:
                 if config_path.is_file():
                     with open(config_path, 'r') as f:
                         user_config = yaml.safe_load(f)
                     # Check user_config *after* the 'with open' block closes the file
                     if user_config: # Ensure file is not empty and is a dict
                         if isinstance(user_config, dict):
                             config.update(user_config)
                             logging.info(f"Loaded and merged configuration from {self.config_file}")
                         else:
                             logging.warning(f"Config file {self.config_file} does not contain a valid dictionary. Using defaults.")
                     else:
                         logging.info(f"Config file {self.config_file} is empty. Using defaults.")
                 else: # This corresponds to 'if config_path.is_file():'
                     logging.warning(f"Config file {self.config_file} not found. Using default configuration.")
                     # Optionally create a default config file here
                 # try:
                 #     with open(config_path, 'w') as f:
                 #         yaml.dump(default_config, f, default_flow_style=False)
                 #     logging.info(f"Created default config file at {self.config_file}")
                 # except IOError as e_write:
                     #     logging.error(f"Could not write default config file {config_path}: {e_write}")

             except yaml.YAMLError as e:
                 logging.error(f"Error parsing config file {config_path}: {e}. Using default configuration.")
             except IOError as e:
                 logging.error(f"Error reading config file {config_path}: {e}. Using default configuration.")
             except Exception as e:
                 logging.error(f"Unexpected error loading config file {config_path}: {e}. Using default configuration.")

        # This part runs only if config_file was not None
        # Resolve project_dir (relative to CWD if needed)
        project_dir_path = Path(config.get("project_dir", "."))
        if not project_dir_path.is_absolute():
             # Assuming the script runs from the project root
             project_dir_path = Path.cwd() / project_dir_path
        config["project_dir"] = str(project_dir_path.resolve()) # Store absolute path

        # Resolve work_dir passed from __init__ relative to CWD and ensure it's absolute
        # This should happen *independently* of the project_dir
        self.work_dir = self.work_dir.resolve()
        self.work_dir.mkdir(parents=True, exist_ok=True)
        logging.info(f"Using working directory (resolved relative to CWD): {self.work_dir}")

        # State initialization happens after this method returns in __init__
        return config

    def _initialize_state(self, reset_state: bool) -> Dict[str, Any]:
        """
        Initializes the harness state.
        If reset_state is False, tries to load the latest run from the ledger.
        Otherwise, returns a fresh state.
        """
        if reset_state:
            logging.info("Resetting state as requested.")
            return {
                "current_iteration": 0,
                "prompt_history": [],
                "converged": False,
                "last_error": None,
                "run_id": None
            }
        
        # Try to get the latest run ID from the ledger
        latest_run_id = self.ledger.get_latest_run_id()
        
        if latest_run_id is not None:
            # Get run summary
            run_summary = self.ledger.get_run_summary(latest_run_id)
            
            # Check if the run is still in progress (no end_time)
            if run_summary and not run_summary.get("end_time"):
                logging.info(f"Resuming run {latest_run_id}")
                
                # Get conversation history
                history = self.ledger.get_conversation_history(latest_run_id)
                
                # Determine current iteration
                current_iteration = run_summary.get("iteration_count", 0)
                
                return {
                    "current_iteration": current_iteration,
                    "prompt_history": history,
                    "converged": run_summary.get("converged", False),
                    "last_error": run_summary.get("final_status"),
                    "run_id": latest_run_id
                }
        
        # No valid run to resume or reset requested
        logging.info("Initializing fresh state.")
        return {
            "current_iteration": 0,
            "prompt_history": [],
            "converged": False,
            "last_error": None,
            "run_id": None
        }

    def run(self, initial_goal_prompt: str):
        """Runs the main Aider-Pytest-Ollama loop with enhanced features."""
        logging.info("Starting harness run...")
        
        # Start a new run in the ledger if we don't have an active one
        if self.state["run_id"] is None:
            self.current_run_id = self.ledger.start_run(
                initial_goal_prompt,
                self.max_retries,
                self.config
            )
            self.state["run_id"] = self.current_run_id
            logging.info(f"Started new run with ID {self.current_run_id}")
        else:
            self.current_run_id = self.state["run_id"]
            logging.info(f"Continuing run with ID {self.current_run_id}")
        
        # Initialize prompt history only if starting fresh
        if self.state["current_iteration"] == 0 and not self.state["prompt_history"]:
            logging.info("Initializing prompt history with the initial goal.")
            current_prompt = initial_goal_prompt
            # Ensure history is clean before adding the first prompt
            self.state["prompt_history"] = [{"role": "user", "content": current_prompt}]
            # Add to ledger
            self.ledger.add_message(self.current_run_id, None, "user", current_prompt)
        elif self.state["prompt_history"]:
            # If resuming, the last message should be the user prompt for the current iteration
            last_message = self.state["prompt_history"][-1]
            if last_message.get("role") == "user":
                current_prompt = last_message["content"]
                logging.info(f"Resuming run from iteration {self.state['current_iteration'] + 1}. Last user prompt retrieved from history.")
            else:  # Last message is from assistant
                # This means the previous iteration's Aider run completed, but didn't generate a new user prompt.
                logging.info("Previous run concluded (last message was from assistant). Starting a fresh run with the initial goal.")
                current_prompt = initial_goal_prompt
                # Reset state for a fresh run
                self.state["current_iteration"] = 0
                self.state["prompt_history"] = [{"role": "user", "content": current_prompt}]
                self.state["converged"] = False
                self.state["last_error"] = None
                # Start a new run in the ledger
                self.current_run_id = self.ledger.start_run(
                    initial_goal_prompt,
                    self.max_retries,
                    self.config
                )
                self.state["run_id"] = self.current_run_id
                # Add to ledger
                self.ledger.add_message(self.current_run_id, None, "user", current_prompt)
        else:
            # Should not happen if initialization is correct, but handle defensively
            logging.warning("State indicates resumption but history is empty. Starting with initial goal.")
            current_prompt = initial_goal_prompt
            self.state["prompt_history"] = [{"role": "user", "content": current_prompt}]
            # Add to ledger
            self.ledger.add_message(self.current_run_id, None, "user", current_prompt)


        while (
            self.state["current_iteration"] < self.max_retries
            and not self.state["converged"]
        ):
            iteration = self.state["current_iteration"]
            logging.info(f"--- Starting Iteration {iteration + 1} ---")
            
            # Start iteration in ledger
            iteration_id = self.ledger.start_iteration(
                self.current_run_id,
                iteration + 1,
                current_prompt
            )

            try:
                # 1. Run Aider
                logging.info("Running Aider...")
                aider_diff, aider_error = run_aider(
                    prompt=current_prompt,
                    config=self.config,
                    history=self.state["prompt_history"][:-1],
                    work_dir=self.config["project_dir"]
                )

                if aider_error:
                    logging.error(f"Aider failed: {aider_error}")
                    self.state["last_error"] = f"Aider failed: {aider_error}"
                    # Update ledger with error
                    self.ledger.complete_iteration(
                        self.current_run_id,
                        iteration_id,
                        None,
                        f"Error: {aider_error}",
                        False,
                        "FAILURE",
                        f"Aider failed: {aider_error}"
                    )
                    break

                if aider_diff is None:
                    logging.error("Aider returned None for diff without error. Stopping.")
                    self.state["last_error"] = "Aider returned None diff unexpectedly."
                    # Update ledger with error
                    self.ledger.complete_iteration(
                        self.current_run_id,
                        iteration_id,
                        None,
                        "Aider returned None diff unexpectedly.",
                        False,
                        "FAILURE",
                        "Aider returned None diff unexpectedly."
                    )
                    break

                logging.info(f"Aider finished. Diff:\n{aider_diff if aider_diff else '[No changes detected]'}")

                # Add Aider's response to history and ledger
                assistant_message = aider_diff if aider_diff is not None else "[Aider encountered an error or produced no output]"
                if aider_diff == "":
                    assistant_message = "[Aider made no changes]"
                self.state["prompt_history"].append({"role": "assistant", "content": assistant_message})
                self.ledger.add_message(self.current_run_id, iteration_id, "assistant", assistant_message)

                # 2. Run Pytest
                logging.info("Running pytest...")
                pytest_passed, pytest_output = run_pytest(self.config["project_dir"])
                summary_output = (pytest_output[:500] + '...' if len(pytest_output) > 500 else pytest_output)
                logging.info(f"Pytest finished. Passed: {pytest_passed}\nOutput (truncated):\n{summary_output}")

                # 3. Evaluate with VESPER.MIND council or standard LLM
                if self.enable_council and self.council:
                    logging.info("Evaluating with VESPER.MIND council...")
                    verdict, suggestions, council_results = self.council.evaluate_iteration(
                        self.current_run_id,
                        iteration_id,
                        initial_goal_prompt,
                        aider_diff if aider_diff is not None else "",
                        pytest_output,
                        pytest_passed,
                        self.state["prompt_history"]
                    )
                    logging.info(f"VESPER.MIND council verdict: {verdict}")
                    
                    # Generate changelog if successful
                    if verdict == "SUCCESS":
                        changelog = self.council.generate_changelog(
                            self.current_run_id,
                            iteration_id,
                            verdict
                        )
                        logging.info(f"Generated changelog:\n{changelog}")
                        
                        # Save changelog to file
                        changelog_dir = self.work_dir / "changelogs"
                        changelog_dir.mkdir(exist_ok=True)
                        changelog_file = changelog_dir / f"changelog_run{self.current_run_id}_iter{iteration_id}.md"
                        with open(changelog_file, 'w') as f:
                            f.write(changelog)
                else:
                    # Standard LLM evaluation
                    logging.info("Evaluating outcome with standard LLM...")
                    verdict, suggestions = self._evaluate_outcome(
                        initial_goal_prompt,
                        aider_diff if aider_diff is not None else "",
                        pytest_output,
                        pytest_passed
                    )
                    logging.info(f"LLM evaluation result: Verdict={verdict}, Suggestions='{suggestions}'")

                # Update ledger with iteration results
                self.ledger.complete_iteration(
                    self.current_run_id,
                    iteration_id,
                    aider_diff,
                    pytest_output,
                    pytest_passed,
                    verdict,
                    suggestions
                )

                # 4. Run code review if enabled and successful
                if self.enable_code_review and verdict == "SUCCESS":
                    logging.info("Running code review...")
                    review_result = self._run_code_review(
                        initial_goal_prompt,
                        aider_diff if aider_diff is not None else "",
                        pytest_output
                    )
                    logging.info(f"Code review result:\n{review_result}")
                    
                    # Save review to file
                    review_dir = self.work_dir / "reviews"
                    review_dir.mkdir(exist_ok=True)
                    review_file = review_dir / f"review_run{self.current_run_id}_iter{iteration_id}.md"
                    with open(review_file, 'w') as f:
                        f.write(review_result)

                # 5. Decide next step based on verdict
                if verdict == "SUCCESS":
                    logging.info("Evaluation confirms SUCCESS. Stopping loop.")
                    self.state["converged"] = True
                elif verdict == "RETRY":
                    logging.info("Evaluation suggests RETRY.")
                    if self.state["current_iteration"] + 1 >= self.max_retries:
                        logging.warning(f"Retry suggested, but max retries ({self.max_retries}) reached. Stopping.")
                        self.state["last_error"] = "Max retries reached after RETRY verdict."
                        break

                    logging.info("Creating retry prompt...")
                    current_prompt = self._create_retry_prompt(
                        initial_goal_prompt,
                        aider_diff if aider_diff is not None else "",
                        pytest_output,
                        suggestions
                    )
                    self.state["prompt_history"].append({"role": "user", "content": current_prompt})
                    self.ledger.add_message(self.current_run_id, None, "user", current_prompt)
                    logging.debug(f"Next prompt for Aider:\n{current_prompt}")
                else:  # verdict == "FAILURE"
                    logging.error(f"Structural failure detected. Stopping loop. Reason: {suggestions}")
                    self.state["last_error"] = f"Evaluation reported FAILURE: {suggestions}"
                    self.state["converged"] = False
                    break

            except Exception as e:
                logging.exception(f"Critical error during iteration {iteration + 1}: {e}")
                self.state["last_error"] = str(e)
                
                # Update ledger with error
                try:
                    self.ledger.complete_iteration(
                        self.current_run_id,
                        iteration_id,
                        None,
                        f"Exception: {str(e)}",
                        False,
                        "FAILURE",
                        f"Internal error: {str(e)}"
                    )
                except Exception as ledger_error:
                    logging.error(f"Failed to update ledger with error: {ledger_error}")
                
                break

            finally:
                self.state["current_iteration"] += 1
                # State is saved implicitly via ledger updates and end_run below
                time.sleep(1) # Keep delay if needed for external processes

        # End of loop
        if self.state["converged"]:
            logging.info(f"Harness finished successfully after {self.state['current_iteration']} iterations.")
            final_status = "SUCCESS"
        elif self.state["current_iteration"] >= self.max_retries:
            logging.warning(f"Harness stopped after reaching max retries ({self.max_retries}).")
            final_status = f"MAX_RETRIES_REACHED: {self.state.get('last_error', 'Unknown error')}"
        else:
            logging.error(f"Harness stopped prematurely due to error: {self.state.get('last_error', 'Unknown error')}")
            final_status = f"ERROR: {self.state.get('last_error', 'Unknown error')}"

        # Update run status in ledger
        self.ledger.end_run(
            self.current_run_id,
            self.state["converged"],
            final_status
        )

        logging.info("Harness run complete.")
        
        # Return summary
        return {
            "run_id": self.current_run_id,
            "iterations": self.state["current_iteration"],
            "converged": self.state["converged"],
            "final_status": final_status
        }


    def _evaluate_outcome(
        self,
        initial_goal: str,
        aider_diff: str,
        pytest_output: str,
        pytest_passed: bool
    ) -> Tuple[str, str]:
        """
        Evaluates the outcome of an iteration using the standard LLM.
        This is used when the VESPER.MIND council is disabled.

        Args:
            initial_goal: The original goal prompt.
            aider_diff: The diff generated by Aider in the last iteration.
            pytest_output: The output from pytest.
            pytest_passed: Boolean indicating if pytest passed.

        Returns:
            A tuple containing:
            - str: The verdict ("SUCCESS", "RETRY", "FAILURE").
            - str: Suggestions from the LLM (empty if not RETRY).
        """
        evaluation_prompt = self._create_evaluation_prompt(
            initial_goal,
            self.state["prompt_history"],
            aider_diff,
            pytest_output,
            pytest_passed
        )
        try:
            # Enhanced system prompt for better evaluation
            evaluation_system_prompt = """You are an expert software development assistant and test harness evaluator.
Analyze the provided goal, history, code changes (diff), and test results.
Determine if the changes meet the goal and tests pass.

Consider:
1. Do the changes address the specific requirements in the goal?
2. Do all tests pass? If not, are the failures related to the changes?
3. Is the code well-structured, maintainable, and following best practices?
4. Are there any potential issues or edge cases not covered?

Respond ONLY in the specified format:
Verdict: [SUCCESS|RETRY|FAILURE]
Suggestions: [Provide concise, actionable suggestions ONLY if verdict is RETRY, otherwise leave blank]

SUCCESS = Goal achieved and tests pass
RETRY = Changes need improvement but are on the right track
FAILURE = Fundamental issues that require a different approach
"""

            llm_evaluation_response = get_llm_response(
                evaluation_prompt,
                self.config,
                history=None,
                system_prompt=evaluation_system_prompt
            )
            logging.debug(f"LLM Evaluation Response:\n{llm_evaluation_response}")

            # Parse the LLM response
            verdict_match = re.search(r"Verdict:\s*(SUCCESS|RETRY|FAILURE)", llm_evaluation_response, re.IGNORECASE)
            suggestions_match = re.search(r"Suggestions:\s*(.*)", llm_evaluation_response, re.IGNORECASE | re.DOTALL)

            if verdict_match:
                verdict = verdict_match.group(1).upper()
                suggestions = suggestions_match.group(1).strip() if suggestions_match else ""
                # Ensure suggestions are only returned if verdict is RETRY
                if verdict != "RETRY":
                    suggestions = ""
                logging.info(f"LLM evaluation parsed: Verdict={verdict}, Suggestions='{suggestions}'")
                return verdict, suggestions
            else:
                logging.warning(f"Could not parse verdict from LLM evaluation response. Defaulting to RETRY.")
                verdict = "RETRY"
                suggestions = "LLM response format was invalid. Please review the previous attempt and try again."
                return verdict, suggestions

        except Exception as e:
            logging.error(f"Error during LLM evaluation: {e}. Defaulting to RETRY.")
            verdict = "RETRY"
            suggestions = f"An error occurred during the evaluation step ({e}). Please review the previous code changes and test results, then try to improve the code to meet the original goal."
            return verdict, suggestions


    def _create_evaluation_prompt(
        self,
        initial_goal: str,
        history: List[Dict[str, str]],
        aider_diff: str,
        pytest_output: str,
        pytest_passed: bool
    ) -> str:
        """Creates an enhanced prompt for the LLM evaluation step."""
        # Create a concise history string for the prompt, showing last few turns
        history_limit = 5
        limited_history = history[-(history_limit * 2):]
        history_str = "\n".join([f"{msg['role']}: {msg['content'][:300]}{'...' if len(msg['content']) > 300 else ''}"
                                 for msg in limited_history])

        prompt = f"""
Analyze the results of an automated code generation step in a test harness.

Initial Goal:
{initial_goal}

Conversation History (summary of last {history_limit} exchanges):
{history_str}

Last Code Changes (diff):
```diff
{aider_diff if aider_diff else "[No changes made]"}
```

Test Results:
Status: {'PASSED' if pytest_passed else 'FAILED'}
```
{pytest_output if pytest_output else "[No output captured]"}
```

Based on the initial goal, the conversation history, the latest code changes (diff), and the test results, evaluate the outcome.

Detailed Evaluation Criteria:
1. Goal Alignment: Do the changes directly address the requirements in the initial goal?
2. Test Results: Do all tests pass? If not, what specific issues are causing failures?
3. Code Quality: Is the code well-structured, maintainable, and following best practices?
4. Completeness: Does the implementation fully satisfy the goal, or are there missing elements?
5. Edge Cases: Are there potential issues or edge cases not addressed?

Respond using the EXACT format below:

Verdict: [SUCCESS|RETRY|FAILURE]
Suggestions: [Provide specific, actionable suggestions ONLY if the verdict is RETRY. Explain exactly what needs to be fixed and how. If SUCCESS or FAILURE, leave this blank.]
"""
        return prompt.strip()

    def _create_retry_prompt(
        self,
        initial_goal: str,
        aider_diff: str,
        pytest_output: str,
        suggestions: str
    ) -> str:
        """
        Creates an enhanced user prompt for the next Aider attempt based on evaluation suggestions.
        """
        retry_prompt = f"""
The previous attempt to achieve the goal needs improvement:

Original Goal:
"{initial_goal}"

Last Code Changes:
```diff
{aider_diff if aider_diff else "[No changes made]"}
```

Test Results:
```
{pytest_output if pytest_output else "[No output captured]"}
```

Evaluation and Specific Suggestions for Improvement:
{suggestions if suggestions else "No specific suggestions were provided by the evaluation. Please analyze the previous changes and test results to identify issues and determine next steps."}

Your Task:
1. Carefully review the suggestions and test results
2. Address each specific issue mentioned in the evaluation
3. Ensure all tests pass
4. Make sure your changes fully satisfy the original goal

Focus on implementing the suggested improvements while maintaining code quality and best practices.
"""
        # Add a specific note if the evaluation itself failed
        if "An error occurred during the evaluation step" in suggestions:
            retry_prompt += "\n\nNote: The automated evaluation step encountered an error, so the suggestions are generic. Please carefully review the goal, the last code changes, and the test results yourself to decide how to proceed."

        return retry_prompt.strip()


# Example usage (for testing purposes, normally called from main.py)
if __name__ == "__main__":
    logging.basicConfig(level=logging.INFO, format="%(asctime)s - %(levelname)s - %(message)s")
    # Create dummy work dir for standalone testing
    dummy_work_dir = Path("temp_harness_work_dir")
    dummy_work_dir.mkdir(exist_ok=True)
    # Create dummy config and goal files if they don't exist
    if not Path("config.yaml").exists(): Path("config.yaml").touch()
    if not Path("goal.prompt").exists(): Path("goal.prompt").write_text("Create a hello world function.")

    harness = Harness(work_dir=dummy_work_dir)
    harness.run("Create a simple Python function that prints 'Hello, World!' and a test for it using pytest.")
    # Clean up dummy files/dirs after test run
    # import shutil
    # shutil.rmtree(dummy_work_dir)
    # Path("config.yaml").unlink(missing_ok=True)
    # Path("goal.prompt").unlink(missing_ok=True)
    def _run_code_review(
        self,
        initial_goal: str,
        aider_diff: str,
        pytest_output: str
    ) -> str:
        """
        Runs a code review on successful changes using Aider.
        
        Args:
            initial_goal: The original goal prompt.
            aider_diff: The diff generated by Aider.
            pytest_output: The output from pytest.
            
        Returns:
            The code review result as a string.
        """
        logging.info("Running code review with Aider...")
        
        # Create code review prompt
        review_prompt = f"""
Act as a senior code reviewer. Review the following code changes that were made to achieve this goal:

Goal: {initial_goal}

Code Changes:
```diff
{aider_diff if aider_diff else "[No changes made]"}
```

Test Results:
```
{pytest_output}
```

Provide a thorough code review that includes:
1. Overall assessment of code quality
2. Specific strengths of the implementation
3. Areas for potential improvement
4. Any potential bugs or edge cases
5. Suggestions for better practices or optimizations

Format your review as a professional code review document with markdown headings and bullet points.
"""
        
        try:
            # Use the configured LLM directly instead of running another Aider instance
            # This simplifies the process while still providing valuable feedback
            review_system_prompt = """You are an expert code reviewer with years of experience.
Provide thorough, constructive code reviews that highlight both strengths and areas for improvement.
Focus on code quality, maintainability, performance, and adherence to best practices.
Format your review as a professional markdown document with clear sections and specific examples."""
            
            review_result = get_llm_response(
                review_prompt,
                self.config,
                history=None,
                system_prompt=review_system_prompt
            )
            
            # Add header to the review
            timestamp = time.strftime("%Y-%m-%d %H:%M:%S")
            header = f"# Code Review\n\n**Date:** {timestamp}\n\n**Reviewer:** AI Code Reviewer\n\n---\n\n"
            
            return header + review_result
            
        except Exception as e:
            logging.error(f"Error during code review: {e}")
            return f"# Code Review\n\nError during code review: {e}\n\nPlease review the code manually."
