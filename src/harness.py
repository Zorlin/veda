import logging
import os
import subprocess
import time
from pathlib import Path
import json
from typing import Dict, Any, Optional, List, Tuple # Add Tuple

import yaml

from .aider_interaction import run_aider
from .llm_interaction import get_llm_response # Import the LLM function
# TODO: Implement persistence layer (e.g., JSON or SQLite logger)
# from .persistence import Logger
from .pytest_interaction import run_pytest
import re # Import re for parsing LLM response


class Harness:
    """
    Orchestrates the Aider-Ollama-Pytest loop.
    """

    def __init__(
        self,
        config_file: str = "config.yaml",
        max_retries: int = 5,
        work_dir: Path = Path("harness_work_dir"),
        reset_state: bool = False,
        ollama_model: Optional[str] = None, # Add ollama_model parameter
    ):
        self.config_file = config_file
        self.max_retries = max_retries
        self.work_dir = work_dir # Initial work_dir path
        self.config: Dict[str, Any] = self._load_config() # Load config, potentially updating self.work_dir
        # Override config model if CLI argument is provided
        if ollama_model:
            logging.info(f"Overriding configured Ollama model with command-line argument: {ollama_model}")
            self.config["ollama_model"] = ollama_model
        # Initialize state *after* config is loaded and work_dir is finalized
        self.state: Dict[str, Any] = self._initialize_state(reset_state) # Pass flag from __init__
        # self.logger = Logger(log_dir=self.work_dir / "logs") # Placeholder for future logging/ledger
        logging.info(f"Harness initialized. Max retries: {self.max_retries}")
        logging.info(f"Working directory used for state: {self.work_dir.resolve()}")

    def _load_config(self) -> Dict[str, Any]:
        """Loads configuration from the YAML file."""
        logging.info(f"Loading configuration from {self.config_file}...")
        default_config = {
            "ollama_model": "gemma3:12b", # Set default to gemma3:12b
            "ollama_api_url": "http://localhost:11434/api/generate", # TODO: Use this
            "aider_command": "aider", # Adjust if aider is not in PATH
            "project_dir": ".", # Directory Aider should operate on
            # TODO: Add other necessary config like pytest command, etc.
        }
        config = default_config.copy()
        try:
            # Ensure config file exists before trying to open it
            config_path = Path(self.config_file)
            if config_path.is_file():
                with open(config_path, 'r') as f:
                    user_config = yaml.safe_load(f)
                    if user_config: # Ensure file is not empty and is a dict
                        if isinstance(user_config, dict):
                            config.update(user_config)
                            logging.info(f"Loaded and merged configuration from {self.config_file}")
                        else:
                            logging.warning(f"Config file {self.config_file} does not contain a valid dictionary. Using defaults.")
                    else:
                        logging.info(f"Config file {self.config_file} is empty. Using defaults.")
            else:
                 logging.warning(f"Config file {self.config_file} not found. Using default configuration.")
                 # Optionally create a default config file here
                 # try:
                 #     with open(config_path, 'w') as f:
                 #         yaml.dump(default_config, f, default_flow_style=False)
                 #     logging.info(f"Created default config file at {self.config_file}")
                 # except IOError as e_write:
                 #     logging.error(f"Could not write default config file {self.config_file}: {e_write}")

        except yaml.YAMLError as e:
            logging.error(f"Error parsing config file {self.config_file}: {e}. Using default configuration.")
        except IOError as e:
            logging.error(f"Error reading config file {self.config_file}: {e}. Using default configuration.")
        except Exception as e:
            logging.error(f"Unexpected error loading config file {self.config_file}: {e}. Using default configuration.")

        # Ensure work_dir exists *after* config is loaded (in case it's specified)
        # If project_dir is relative, resolve it relative to the project root (where main.py likely runs)
        project_dir_path = Path(config.get("project_dir", "."))
        if not project_dir_path.is_absolute():
             # Assuming the script runs from the project root
             project_dir_path = Path.cwd() / project_dir_path
        config["project_dir"] = str(project_dir_path.resolve()) # Store absolute path

        # Resolve work_dir relative to project_dir *before* initializing state
        # Use the name attribute of the Path object passed in __init__
        # Ensure work_dir is resolved relative to the project dir from config
        self.work_dir = project_dir_path / self.work_dir.name
        self.work_dir.mkdir(parents=True, exist_ok=True)
        logging.info(f"Resolved working directory: {self.work_dir.resolve()}")

        # State initialization moved to __init__ after this method returns

        return config

    def _initialize_state(self, reset_state: bool) -> Dict[str, Any]:
        """
        Initializes the harness state.
        Loads from file if it exists and reset_state is False.
        Otherwise, returns a fresh state.
        """
        state_file = self.work_dir / "harness_state.json"

        if not reset_state and state_file.is_file(): # Check reset_state flag
            logging.info(f"Attempting to load state from {state_file}...")
            try:
                with open(state_file, 'r') as f:
                    state = json.load(f)
                # Basic validation: check if it's a dictionary and has expected keys
                if isinstance(state, dict) and "current_iteration" in state and "prompt_history" in state:
                    logging.info(f"Successfully loaded and validated previous state from {state_file}")
                    # Ensure prompt_history is a list
                    if not isinstance(state.get("prompt_history"), list):
                        logging.warning("Loaded state has invalid 'prompt_history'. Resetting history.")
                        state["prompt_history"] = []
                    return state
                else:
                    logging.warning(f"State file {state_file} has invalid format. Initializing fresh state.")
            except (json.JSONDecodeError, IOError) as e:
                logging.warning(f"Could not load or parse state file {state_file}: {e}. Initializing fresh state.")
            except Exception as e:
                 logging.error(f"Unexpected error loading state file {state_file}: {e}. Initializing fresh state.")
        elif reset_state:
            logging.info(f"Resetting state as requested. Ignoring existing state file {state_file} if present.")
        else: # File not found and not resetting
            logging.info(f"State file {state_file} not found. Initializing fresh state.")

        # Default state if no valid state file is found, loading fails, or reset requested
        logging.info("Initializing fresh state.")
        return {
            "current_iteration": 0,
            "prompt_history": [], # Stores conversation: [{"role": "user", "content": "..."}, {"role": "assistant", "content": "..."}]
            "converged": False,
            "last_error": None,
            # TODO: Add fields for logging diffs, outcomes per iteration if needed beyond history
        }

    def _save_state(self):
        """Saves the current harness state to harness_state.json."""
        state_file = self.work_dir / "harness_state.json"
        logging.info(f"Saving harness state to {state_file}...")
        try:
            # Ensure the directory exists before writing
            state_file.parent.mkdir(parents=True, exist_ok=True)
            with open(state_file, 'w') as f:
                json.dump(self.state, f, indent=4)
            logging.info(f"Successfully saved state to {state_file}")
        except IOError as e:
            logging.error(f"Could not write state file {state_file}: {e}")
        except TypeError as e:
            # This can happen if non-serializable objects are in the state
            logging.error(f"Could not serialize state to JSON: {e}. State: {self.state}")
        except Exception as e:
            logging.error(f"Unexpected error saving state: {e}")


    def run(self, initial_goal_prompt: str):
        """Runs the main Aider-Pytest-Ollama loop."""
        logging.info("Starting harness run...")

        # Initialize prompt history only if starting fresh
        if self.state["current_iteration"] == 0 and not self.state["prompt_history"]:
            logging.info("Initializing prompt history with the initial goal.")
            current_prompt = initial_goal_prompt
            # Ensure history is clean before adding the first prompt
            self.state["prompt_history"] = [{"role": "user", "content": current_prompt}]
        elif self.state["prompt_history"]:
            # If resuming, the last message should be the user prompt for the current iteration
            last_message = self.state["prompt_history"][-1]
            if last_message.get("role") == "user":
                current_prompt = last_message["content"]
                logging.info(f"Resuming run from iteration {self.state['current_iteration'] + 1}. Last user prompt retrieved from history.")
            else:
                logging.error("Cannot resume: Last message in history is not from 'user'. Starting with initial goal.")
                # Fallback to initial goal if history state is unexpected
                current_prompt = initial_goal_prompt
                self.state["prompt_history"].append({"role": "user", "content": current_prompt})
        else:
            # Should not happen if initialization is correct, but handle defensively
            logging.warning("State indicates resumption but history is empty. Starting with initial goal.")
            current_prompt = initial_goal_prompt
            self.state["prompt_history"] = [{"role": "user", "content": current_prompt}]


        while (
            self.state["current_iteration"] < self.max_retries
            and not self.state["converged"]
        ):
            iteration = self.state["current_iteration"]
            logging.info(f"--- Starting Iteration {iteration + 1} ---")

            try:
                # 1. Run Aider
                logging.info("Running Aider...")
                aider_diff, aider_error = run_aider(
                    prompt=current_prompt, # Pass the current prompt for this iteration
                    config=self.config,
                    # Pass history *excluding* the current user prompt (Aider adds it)
                    history=self.state["prompt_history"][:-1],
                    work_dir=self.config["project_dir"] # Run aider in the target project dir
                )

                if aider_error:
                    logging.error(f"Aider failed: {aider_error}")
                    self.state["last_error"] = f"Aider failed: {aider_error}"
                    # Decide if we should stop or try to recover
                    break # Stop loop on Aider error for now

                if aider_diff is None: # Should not happen if error handling is correct, but check anyway
                    logging.error("Aider returned None for diff without error. Stopping.")
                    self.state["last_error"] = "Aider returned None diff unexpectedly."
                    break

                logging.info(f"Aider finished. Diff:\n{aider_diff if aider_diff else '[No changes detected]'}")
                # self.logger.log_iteration(iteration, "aider_diff", aider_diff) # Placeholder

                # Add Aider's response (diff or status message) to history for context
                # Use the diff as the assistant's message. If no diff, use a status message.
                assistant_message = aider_diff if aider_diff is not None else "[Aider encountered an error or produced no output]"
                # Ensure assistant message is added even if empty (indicates no changes)
                if aider_diff == "":
                    assistant_message = "[Aider made no changes]"
                self.state["prompt_history"].append({"role": "assistant", "content": assistant_message})
                # Save state immediately after Aider finishes and history is updated
                self._save_state()

                # 2. Run Pytest
                logging.info("Running pytest...")
                pytest_passed, pytest_output = run_pytest(self.config["project_dir"])
                # Log the outcome and truncated output for clarity
                summary_output = (pytest_output[:500] + '...' if len(pytest_output) > 500 else pytest_output) # Truncate long output for info log
                logging.info(f"Pytest finished. Passed: {pytest_passed}\nOutput (truncated):\n{summary_output}")
                # self.logger.log_iteration(iteration, "pytest_output", pytest_output) # Placeholder
                # self.logger.log_iteration(iteration, "pytest_passed", pytest_passed) # Placeholder

                # 3. Check for Convergence (Simple check: tests passed and Aider made changes)
                # More sophisticated check using LLM evaluation comes next
                if pytest_passed and aider_diff: # Simple convergence: tests pass AND aider made changes
                    logging.info("Initial check: Pytest passed and Aider made changes.")
                    # Proceed to LLM evaluation to confirm convergence against the goal
                elif pytest_passed and not aider_diff:
                    logging.info("Pytest passed, but Aider made no changes. Assuming convergence if goal seems met (LLM will verify).")
                    # Let LLM decide if the goal is met even without changes
                else: # pytest failed
                    logging.warning("Pytest failed. Proceeding to LLM evaluation for retry suggestions.")
                    # LLM evaluation is needed to generate retry prompt

                # 4. Evaluate Outcome with LLM
                logging.info("Evaluating outcome with LLM...")
                verdict, suggestions = self._evaluate_outcome(
                    initial_goal_prompt,
                    aider_diff if aider_diff is not None else "", # Pass empty string if None
                    pytest_output,
                    pytest_passed
                )
                logging.info(f"LLM evaluation result: Verdict={verdict}, Suggestions='{suggestions}'")
                # self.logger.log_iteration(iteration, "llm_verdict", verdict) # Placeholder
                # self.logger.log_iteration(iteration, "llm_suggestions", suggestions) # Placeholder


                # 5. Decide next step based on LLM Verdict
                if verdict == "SUCCESS":
                    logging.info("LLM evaluation confirms SUCCESS. Stopping loop.")
                    self.state["converged"] = True
                    # No need to add another user prompt if converged
                elif verdict == "RETRY":
                    logging.info("LLM evaluation suggests RETRY.")
                    if self.state["current_iteration"] + 1 >= self.max_retries:
                         logging.warning(f"Retry suggested, but max retries ({self.max_retries}) reached. Stopping.")
                         self.state["last_error"] = "Max retries reached after RETRY verdict."
                         break # Stop loop if max retries reached

                    logging.info("Creating retry prompt...")
                    current_prompt = self._create_retry_prompt(
                        initial_goal_prompt,
                        # History is already updated with last assistant message
                        aider_diff if aider_diff is not None else "",
                        pytest_output,
                        suggestions # Pass suggestions from LLM
                    )
                    # Add the *new* user prompt (the retry instructions) to history for the *next* iteration
                    self.state["prompt_history"].append({"role": "user", "content": current_prompt})
                    logging.debug(f"Next prompt for Aider:\n{current_prompt}")

                else: # verdict == "FAILURE"
                    logging.error(f"Structural failure detected by LLM. Stopping loop. Reason: {suggestions}")
                    self.state["last_error"] = f"LLM reported FAILURE: {suggestions}"
                    self.state["converged"] = False # Explicitly set converged to False on failure
                    break # Exit loop on failure

            except Exception as e:
                logging.exception(f"Critical error during iteration {iteration + 1}: {e}")
                self.state["last_error"] = str(e)
                # Decide if we should retry on internal errors or just stop
                break # Exit loop on internal error for now

            finally:
                self.state["current_iteration"] += 1
                self._save_state() # Save state after each iteration
                time.sleep(1) # Small delay between iterations

        # End of loop
        if self.state["converged"]:
            logging.info(f"Harness finished successfully after {self.state['current_iteration']} iterations.")
        elif self.state["current_iteration"] >= self.max_retries:
            logging.warning(f"Harness stopped after reaching max retries ({self.max_retries}).")
        else:
            logging.error(f"Harness stopped prematurely due to error: {self.state.get('last_error', 'Unknown error')}")

        logging.info("Harness run complete.")


    def _evaluate_outcome(
        self,
        initial_goal: str,
        aider_diff: str,
        pytest_output: str,
        pytest_passed: bool
    ) -> Tuple[str, str]:
        """
        Evaluates the outcome of an iteration using the LLM.

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
            self.state["prompt_history"], # Pass full history
            aider_diff,
            pytest_output,
            pytest_passed
        )
        try:
            # Use a separate system prompt for evaluation
            evaluation_system_prompt = """You are an expert software development assistant.
Analyze the provided goal, history, code changes (diff), and test results.
Determine if the changes meet the goal and tests pass.
Respond ONLY in the specified format:
Verdict: [SUCCESS|RETRY|FAILURE]
Suggestions: [Provide concise suggestions ONLY if verdict is RETRY, otherwise leave blank]"""

            llm_evaluation_response = get_llm_response(
                evaluation_prompt,
                self.config,
                history=None, # Evaluation is self-contained, history is in the prompt
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
                logging.warning(f"Could not parse verdict from LLM evaluation response. Defaulting to RETRY.\nResponse:\n{llm_evaluation_response}")
                verdict = "RETRY"
                suggestions = "LLM response format was invalid. Please review the previous attempt and try again."
                return verdict, suggestions

        except Exception as e:
            logging.error(f"Error during LLM evaluation: {e}. Defaulting to RETRY.")
            verdict = "RETRY"
            # Provide a more generic suggestion if the evaluation itself failed
            suggestions = f"An error occurred during the evaluation step ({e}). Please review the previous code changes and test results, then try to improve the code to meet the original goal."
            return verdict, suggestions


    def _create_evaluation_prompt(
        self,
        initial_goal: str,
        history: List[Dict[str, str]], # Use full history
        aider_diff: str,
        pytest_output: str,
        pytest_passed: bool
    ) -> str:
        """Creates the prompt for the LLM evaluation step."""
        # Create a concise history string for the prompt, showing last few turns
        history_limit = 5 # Show last N pairs of user/assistant messages
        limited_history = history[-(history_limit * 2):] # Get last N*2 messages
        history_str = "\n".join([f"{msg['role']}: {msg['content'][:300]}{'...' if len(msg['content']) > 300 else ''}"
                                 for msg in limited_history])

        prompt = f"""
Analyze the results of an automated code generation step.

Initial Goal:
{initial_goal}

Conversation History (summary):
{history_str}

Last Aider Diff:
```diff
{aider_diff if aider_diff else "[No changes made by Aider]"}
```

Pytest Result: {'Success' if pytest_passed else 'Failure'}
Pytest Output:
```
{pytest_output}
```

Pytest Result: {'Pass' if pytest_passed else 'Fail'}
Pytest Output:
```
{pytest_output if pytest_output else "[No output captured]"}
```

Based on the initial goal, the conversation history, the latest code changes (diff), and the test results, evaluate the outcome.

Evaluation Criteria:
1. Did the changes address the last request/goal?
2. Do the tests pass? If not, why?
3. Is the overall goal being achieved?

Respond using the EXACT format below:

Verdict: [SUCCESS|RETRY|FAILURE]
Suggestions: [Provide concise, actionable suggestions ONLY if the verdict is RETRY. Explain *why* it needs retry (e.g., failed tests, didn't address goal, introduced bug). If SUCCESS or FAILURE, leave this blank.]
"""
        return prompt.strip()

    def _create_retry_prompt(
        self,
        initial_goal: str,
        # History is not needed here, it's passed separately to run_aider
        aider_diff: str,
        pytest_output: str,
        suggestions: str
    ) -> str:
        """
        Creates the user prompt for the *next* Aider attempt based on LLM's suggestions.
        This prompt will be added to the history with role 'user'.
        """
        # The prompt should focus on the *task* for the next iteration, using the suggestions.
        retry_prompt = f"""
The previous attempt to achieve the goal "{initial_goal}" had issues.

Last Aider Diff (changes made):
```diff
{aider_diff if aider_diff else "[No changes made by Aider]"}
```

Pytest Output (results of running tests on the changes):
```
{pytest_output if pytest_output else "[No output captured]"}
```

Evaluation and Suggestions for Improvement from the previous step:
{suggestions if suggestions else "No specific suggestions were provided by the evaluation. Please analyze the previous diff and test output yourself to identify the issue and determine the next steps to achieve the goal."}

Based *only* on the suggestions above (if provided) and your analysis of the previous attempt's diff and test results, please modify the code to address the identified issues and progress towards the initial goal: "{initial_goal}". Focus on applying the suggested changes or fixing the errors indicated by the pytest output.
"""
        # Add a specific note if the evaluation itself failed (indicated by specific suggestion text)
        if "An error occurred during the evaluation step" in suggestions:
            retry_prompt += "\n\nNote: The automated evaluation step encountered an error, so the suggestions are generic. Please carefully review the goal, the last code changes (diff), and the test results yourself to decide how to proceed."

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
