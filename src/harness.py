import logging
import os
import subprocess
import time
from pathlib import Path
import json
from typing import Dict, Any, Optional

import yaml

from .aider_interaction import run_aider
import re # Import re for parsing LLM response
from typing import Dict, Any, Optional, List # Add List

import yaml

from .aider_interaction import run_aider
from .llm_interaction import get_llm_response # Import the LLM function
# from .persistence import Logger
from .pytest_interaction import run_pytest


class Harness:
    """
    Orchestrates the Aider-Ollama-Pytest loop.
    """

    def __init__(
        self,
        config_file: str = "config.yaml",
        max_retries: int = 5,
        work_dir: Path = Path("harness_work_dir"),
    ):
        self.config_file = config_file
        self.max_retries = max_retries
        self.work_dir = work_dir
        self.config: Dict[str, Any] = self._load_config()
        self.state: Dict[str, Any] = self._initialize_state()
        # self.logger = Logger(log_dir=self.work_dir / "logs") # Placeholder
        logging.info(f"Harness initialized. Max retries: {self.max_retries}")
        logging.info(f"Working directory: {self.work_dir.resolve()}")

    def _load_config(self) -> Dict[str, Any]:
        """Loads configuration from the YAML file."""
        logging.info(f"Loading configuration from {self.config_file}...")
        default_config = {
            "ollama_model": "deepcoder:14b", # Updated default model
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

        # Resolve work_dir relative to project_dir
        self.work_dir = project_dir_path / self.work_dir.name # Use name to avoid nesting if work_dir was relative
        self.work_dir.mkdir(parents=True, exist_ok=True)
        logging.info(f"Using working directory: {self.work_dir.resolve()}")

        return config

    def _initialize_state(self) -> Dict[str, Any]:
        """Initializes the harness state, loading from file if exists."""
        state_file = self.work_dir / "harness_state.json"
        logging.info(f"Attempting to load state from {state_file}...")
        if state_file.is_file(): # Check if it's a file specifically
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
        else:
            logging.info(f"State file {state_file} not found or is not a file. Initializing fresh state.")

        # Default state if no valid state file is found or loading fails
        logging.info("Initializing fresh state.")
        return {
            "current_iteration": 0,
            "prompt_history": [], # Stores conversation: [{"role": "user", "content": "..."}, {"role": "assistant", "content": "..."}]
            "converged": False,
            "last_error": None,
        }
        # Placeholder return

    def _save_state(self):
        """Saves the current harness state."""
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
        current_prompt = initial_goal_prompt
        self.state["prompt_history"].append({"role": "user", "content": current_prompt})

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
                    prompt=current_prompt,
                    config=self.config,
                    history=self.state["prompt_history"],
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

                # Add Aider's response (diff) to history for context
                # Use the diff as the assistant's message. If no diff, maybe add a note?
                assistant_message = aider_diff if aider_diff else "[Aider made no changes]"
                self.state["prompt_history"].append({"role": "assistant", "content": assistant_message})

                # 2. Run Pytest
                logging.info("Running pytest...")
                pytest_passed, pytest_output = run_pytest(self.config["project_dir"])
                # Log the outcome and truncated output for clarity
                summary_output = (pytest_output[:500] + '...' if len(pytest_output) > 500 else pytest_output) # Truncate long output for info log
                logging.info(f"Pytest finished. Passed: {pytest_passed}\nOutput (truncated):\n{summary_output}")
                # self.logger.log_iteration(iteration, "pytest_output", pytest_output) # Placeholder
                # self.logger.log_iteration(iteration, "pytest_passed", pytest_passed) # Placeholder

                # 3. Run Pytest
                logging.info("Running pytest...")
                pytest_passed, pytest_output = run_pytest(self.config["project_dir"])
                summary_output = (pytest_output[:500] + '...' if len(pytest_output) > 500 else pytest_output)
                logging.info(f"Pytest finished. Passed: {pytest_passed}\nOutput (truncated):\n{summary_output}")

                # 4. Evaluate Outcome with LLM
                logging.info("Evaluating outcome with LLM...")
                evaluation_prompt = self._create_evaluation_prompt(
                    initial_goal_prompt, # Pass the original goal
                    self.state["prompt_history"], # Pass history for context
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
                        logging.info(f"LLM evaluation: Verdict={verdict}, Suggestions='{suggestions}'")
                    else:
                        logging.warning(f"Could not parse verdict from LLM evaluation response. Defaulting to RETRY.\nResponse:\n{llm_evaluation_response}")
                        verdict = "RETRY"
                        suggestions = "LLM response format was invalid. Please review the previous attempt and try again."

                except Exception as e:
                    logging.error(f"Error during LLM evaluation: {e}. Defaulting to RETRY.")
                    verdict = "RETRY"
                    suggestions = f"An error occurred during evaluation: {e}. Please review the previous attempt and try again."

                # 5. Decide next step
                if verdict == "SUCCESS":
                    logging.info("Convergence criteria met. Stopping loop.")
                    self.state["converged"] = True
                elif verdict == "RETRY":
                    logging.info("Retrying with suggestions...")
                    # Pass necessary context to create the retry prompt
                    current_prompt = self._create_retry_prompt(
                        initial_goal_prompt,
                        self.state["prompt_history"], # Pass full history
                        aider_diff,
                        pytest_output,
                        suggestions
                    )
                    # Add the *new* user prompt (the retry instructions) to history
                    self.state["prompt_history"].append({"role": "user", "content": current_prompt})
                else: # verdict == "FAILURE"
                    logging.error(f"Structural failure detected by LLM. Stopping loop. Reason: {suggestions}")
                    self.state["last_error"] = f"LLM reported failure: {suggestions}"
                    break # Exit loop on failure

            except Exception as e:
                logging.exception(f"Error during iteration {iteration + 1}: {e}")
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

    def _create_evaluation_prompt(
        self,
        initial_goal: str,
        history: List[Dict[str, str]],
        aider_diff: str,
        pytest_output: str,
        pytest_passed: bool
    ) -> str:
        """Creates the prompt for LLM evaluation."""
        # Create a concise history string for the prompt
        history_str = "\n".join([f"{msg['role']}: {msg['content'][:200]}..." # Truncate long messages
                                 for msg in history])

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
        initial_goal: str, # Include initial goal for context
        history: List[Dict[str, str]], # Full history needed
        aider_diff: str,
        pytest_output: str,
        suggestions: str
    ) -> str:
        """Creates the prompt for the next Aider attempt based on LLM's suggestions."""
        # We don't need to include the full history *in* the prompt string itself,
        # as it will be passed separately to run_aider -> get_llm_response.
        # The prompt should focus on the *task* for the next iteration.

        retry_prompt = f"""
The previous attempt to achieve the goal "{initial_goal}" resulted in the following:

Last Aider Diff:
```diff
{aider_diff if aider_diff else "[No changes made by Aider]"}
```

Pytest Output:
```
{pytest_output}
```

Evaluation and Suggestions for Improvement:
{suggestions}

Based *only* on the suggestions above, please modify the code to address the identified issues and progress towards the initial goal. Focus on applying the suggested changes.
"""
        return retry_prompt.strip()

# Example usage (for testing purposes, normally called from main.py)

Aider Diff:
```diff
{aider_diff}
```

Pytest Output:
```
{pytest_output}
```

Evaluation and Suggestions for Improvement:
{suggestions}

Based on these suggestions, please refine the code to address the issues and better achieve the original goal.
Original Goal Reminder: [Insert original goal here]
Previous Prompt: {previous_prompt}

Apply the necessary changes.
"""
        logging.warning("Retry prompt creation not fully implemented.")
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
