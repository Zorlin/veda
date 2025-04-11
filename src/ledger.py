import json
import logging
import os
import sqlite3
from datetime import datetime
from pathlib import Path
from typing import Dict, Any, List, Optional, Tuple, Union

# Configure logging
logger = logging.getLogger(__name__)

class Ledger:
    """
    Handles persistence of harness state, diffs, outcomes, and retry metadata.
    Supports both SQLite and JSON storage formats.
    """

    def __init__(
        self,
        work_dir: Path,
        storage_type: str = "sqlite",  # "sqlite" or "json"
        db_name: str = "harness_ledger.db",
        json_file: str = "harness_state.json"
    ):
        self.work_dir = work_dir
        self.storage_type = storage_type.lower()
        self.db_path = work_dir / db_name if storage_type.lower() == "sqlite" else None
        self.json_path = work_dir / json_file if storage_type.lower() == "json" else None
        
        # Ensure work directory exists
        self.work_dir.mkdir(parents=True, exist_ok=True)
        
        if self.storage_type == "sqlite":
            self._initialize_sqlite()
        elif self.storage_type == "json":
            self._initialize_json()
        else:
            raise ValueError(f"Unsupported storage type: {storage_type}. Use 'sqlite' or 'json'.")
            
        logger.info(f"Ledger initialized with {storage_type} storage at {self.work_dir}")

    def _initialize_sqlite(self):
        """Initialize SQLite database with necessary tables."""
        try:
            conn = sqlite3.connect(self.db_path)
            cursor = conn.cursor()
            
            # Create runs table (overall harness runs)
            cursor.execute('''
            CREATE TABLE IF NOT EXISTS runs (
                run_id INTEGER PRIMARY KEY AUTOINCREMENT,
                start_time TEXT NOT NULL,
                end_time TEXT,
                initial_goal TEXT NOT NULL,
                max_retries INTEGER NOT NULL,
                converged BOOLEAN,
                final_status TEXT,
                config TEXT
            )
            ''')
            
            # Create iterations table (individual Aider-Pytest-LLM cycles)
            cursor.execute('''
            CREATE TABLE IF NOT EXISTS iterations (
                iteration_id INTEGER PRIMARY KEY AUTOINCREMENT,
                run_id INTEGER NOT NULL,
                iteration_number INTEGER NOT NULL,
                start_time TEXT NOT NULL,
                end_time TEXT,
                prompt TEXT NOT NULL,
                aider_diff TEXT,
                pytest_output TEXT,
                pytest_passed BOOLEAN,
                llm_verdict TEXT,
                llm_suggestions TEXT,
                FOREIGN KEY (run_id) REFERENCES runs (run_id)
            )
            ''')
            
            # Create messages table (conversation history)
            cursor.execute('''
            CREATE TABLE IF NOT EXISTS messages (
                message_id INTEGER PRIMARY KEY AUTOINCREMENT,
                run_id INTEGER NOT NULL,
                iteration_id INTEGER,
                timestamp TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                FOREIGN KEY (run_id) REFERENCES runs (run_id),
                FOREIGN KEY (iteration_id) REFERENCES iterations (iteration_id)
            )
            ''')
            
            # Create council_evaluations table (for VESPER.MIND council)
            cursor.execute('''
            CREATE TABLE IF NOT EXISTS council_evaluations (
                evaluation_id INTEGER PRIMARY KEY AUTOINCREMENT,
                iteration_id INTEGER NOT NULL,
                model_name TEXT NOT NULL,
                role TEXT NOT NULL,
                evaluation TEXT NOT NULL,
                score REAL,
                timestamp TEXT NOT NULL,
                FOREIGN KEY (iteration_id) REFERENCES iterations (iteration_id)
            )
            ''')
            
            conn.commit()
            conn.close()
            logger.info(f"SQLite database initialized at {self.db_path}")
            
        except sqlite3.Error as e:
            logger.error(f"SQLite initialization error: {e}")
            raise

    def _initialize_json(self):
        """Initialize JSON storage, ensuring valid structure."""
        initial_state = {
            "runs": [],
            "current_run": None, # Ensure this key exists
            "metadata": {
                "created_at": datetime.now().isoformat(),
                "version": "1.0"
            }
        }
        write_initial = False
        if self.json_path.exists():
            try:
                with open(self.json_path, 'r') as f:
                    # Handle empty file case
                    content = f.read()
                    if not content:
                        logger.warning(f"JSON state file {self.json_path} is empty. Re-initializing.")
                        write_initial = True
                    else:
                        state = json.loads(content)
                        # Basic structure check
                        if "runs" not in state or "current_run" not in state or "metadata" not in state:
                            logger.warning(f"JSON state file {self.json_path} has invalid structure. Re-initializing.")
                            write_initial = True
            except (json.JSONDecodeError, IOError) as e:
                logger.warning(f"Error reading or parsing JSON state file {self.json_path}: {e}. Re-initializing.")
                write_initial = True
        else:
            write_initial = True # File doesn't exist, need to create it

        if write_initial:
            try:
                with open(self.json_path, 'w') as f:
                    json.dump(initial_state, f, indent=4)
                logger.info(f"JSON state file initialized/re-initialized at {self.json_path}")
            except IOError as e:
                logger.error(f"Could not write initial JSON state file {self.json_path}: {e}")
                raise # Re-raise if we can't even write the initial file

    def start_run(self, initial_goal: str, max_retries: int, config: Dict[str, Any]) -> int:
        """
        Start a new harness run and return its ID.
        
        Args:
            initial_goal: The initial goal prompt.
            max_retries: Maximum number of retry attempts.
            config: Configuration dictionary.
            
        Returns:
            int: The run ID.
        """
        start_time = datetime.now().isoformat()
        
        if self.storage_type == "sqlite":
            try:
                conn = sqlite3.connect(self.db_path)
                cursor = conn.cursor()
                cursor.execute(
                    "INSERT INTO runs (start_time, initial_goal, max_retries, config) VALUES (?, ?, ?, ?)",
                    (start_time, initial_goal, max_retries, json.dumps(config))
                )
                run_id = cursor.lastrowid
                conn.commit()
                conn.close()
                logger.info(f"Started new run with ID {run_id}")
                return run_id
            except sqlite3.Error as e:
                logger.error(f"SQLite error starting run: {e}")
                raise
        else:  # JSON storage
            try:
                with open(self.json_path, 'r') as f:
                    state = json.load(f)
                
                run_id = len(state["runs"]) + 1
                new_run = {
                    "run_id": run_id,
                    "start_time": start_time,
                    "initial_goal": initial_goal,
                    "max_retries": max_retries,
                    "config": config,
                    "iterations": [],
                    "messages": [],
                    "converged": False,
                    "final_status": None
                }
                
                state["runs"].append(new_run)
                state["current_run"] = run_id
                
                with open(self.json_path, 'w') as f:
                    json.dump(state, f, indent=4)
                
                logger.info(f"Started new run with ID {run_id} in JSON storage")
                return run_id
            except (json.JSONDecodeError, IOError) as e:
                logger.error(f"JSON error starting run: {e}")
                raise

    def end_run(self, run_id: int, converged: bool, final_status: str):
        """
        Mark a run as completed.
        
        Args:
            run_id: The run ID.
            converged: Whether the run converged successfully.
            final_status: Final status message.
        """
        end_time = datetime.now().isoformat()
        
        if self.storage_type == "sqlite":
            try:
                conn = sqlite3.connect(self.db_path)
                cursor = conn.cursor()
                cursor.execute(
                    "UPDATE runs SET end_time = ?, converged = ?, final_status = ? WHERE run_id = ?",
                    (end_time, converged, final_status, run_id)
                )
                conn.commit()
                conn.close()
                logger.info(f"Ended run {run_id} with status: {final_status}")
            except sqlite3.Error as e:
                logger.error(f"SQLite error ending run: {e}")
                raise
        else:  # JSON storage
            try:
                with open(self.json_path, 'r') as f:
                    state = json.load(f)
                
                for run in state["runs"]:
                    if run["run_id"] == run_id:
                        run["end_time"] = end_time
                        run["converged"] = converged
                        run["final_status"] = final_status
                        break
                
                state["current_run"] = None
                
                with open(self.json_path, 'w') as f:
                    json.dump(state, f, indent=4)
                
                logger.info(f"Ended run {run_id} with status: {final_status} in JSON storage")
            except (json.JSONDecodeError, IOError) as e:
                logger.error(f"JSON error ending run: {e}")
                raise

    def start_iteration(self, run_id: int, iteration_number: int, prompt: str) -> int:
        """
        Start a new iteration and return its ID.
        
        Args:
            run_id: The run ID.
            iteration_number: The iteration number.
            prompt: The prompt for this iteration.
            
        Returns:
            int: The iteration ID.
        """
        start_time = datetime.now().isoformat()
        
        if self.storage_type == "sqlite":
            try:
                conn = sqlite3.connect(self.db_path)
                cursor = conn.cursor()
                cursor.execute(
                    "INSERT INTO iterations (run_id, iteration_number, start_time, prompt) VALUES (?, ?, ?, ?)",
                    (run_id, iteration_number, start_time, prompt)
                )
                iteration_id = cursor.lastrowid
                conn.commit()
                conn.close()
                logger.info(f"Started iteration {iteration_number} for run {run_id}")
                return iteration_id
            except sqlite3.Error as e:
                logger.error(f"SQLite error starting iteration: {e}")
                raise
        else:  # JSON storage
            try:
                with open(self.json_path, 'r') as f:
                    state = json.load(f)
                
                for run in state["runs"]:
                    if run["run_id"] == run_id:
                        iteration_id = len(run["iterations"]) + 1
                        new_iteration = {
                            "iteration_id": iteration_id,
                            "iteration_number": iteration_number,
                            "start_time": start_time,
                            "prompt": prompt,
                            "aider_diff": None,
                            "pytest_output": None,
                            "pytest_passed": None,
                            "llm_verdict": None,
                            "llm_suggestions": None
                        }
                        run["iterations"].append(new_iteration)
                        break
                
                with open(self.json_path, 'w') as f:
                    json.dump(state, f, indent=4)
                
                logger.info(f"Started iteration {iteration_number} for run {run_id} in JSON storage")
                return iteration_id
            except (json.JSONDecodeError, IOError) as e:
                logger.error(f"JSON error starting iteration: {e}")
                raise

    def complete_iteration(
        self, 
        run_id: int, 
        iteration_id: int, 
        aider_diff: Optional[str], 
        pytest_output: str, 
        pytest_passed: bool, 
        llm_verdict: str, 
        llm_suggestions: Optional[str]
    ):
        """
        Complete an iteration with results.
        
        Args:
            run_id: The run ID.
            iteration_id: The iteration ID.
            aider_diff: The diff generated by Aider.
            pytest_output: The output from pytest.
            pytest_passed: Whether pytest passed.
            llm_verdict: The LLM's verdict (SUCCESS, RETRY, FAILURE).
            llm_suggestions: The LLM's suggestions for retry.
        """
        end_time = datetime.now().isoformat()
        
        if self.storage_type == "sqlite":
            try:
                conn = sqlite3.connect(self.db_path)
                cursor = conn.cursor()
                cursor.execute(
                    """
                    UPDATE iterations 
                    SET end_time = ?, aider_diff = ?, pytest_output = ?, 
                        pytest_passed = ?, llm_verdict = ?, llm_suggestions = ?
                    WHERE iteration_id = ?
                    """,
                    (end_time, aider_diff, pytest_output, pytest_passed, 
                     llm_verdict, llm_suggestions, iteration_id)
                )
                conn.commit()
                conn.close()
                logger.info(f"Completed iteration {iteration_id} with verdict: {llm_verdict}")
            except sqlite3.Error as e:
                logger.error(f"SQLite error completing iteration: {e}")
                raise
        else:  # JSON storage
            try:
                with open(self.json_path, 'r') as f:
                    state = json.load(f)
                
                for run in state["runs"]:
                    if run["run_id"] == run_id:
                        for iteration in run["iterations"]:
                            if iteration["iteration_id"] == iteration_id:
                                iteration["end_time"] = end_time
                                iteration["aider_diff"] = aider_diff
                                iteration["pytest_output"] = pytest_output
                                iteration["pytest_passed"] = pytest_passed
                                iteration["llm_verdict"] = llm_verdict
                                iteration["llm_suggestions"] = llm_suggestions
                                break
                        break
                
                with open(self.json_path, 'w') as f:
                    json.dump(state, f, indent=4)
                
                logger.info(f"Completed iteration {iteration_id} with verdict: {llm_verdict} in JSON storage")
            except (json.JSONDecodeError, IOError) as e:
                logger.error(f"JSON error completing iteration: {e}")
                raise

    def add_message(self, run_id: int, iteration_id: Optional[int], role: str, content: str):
        """
        Add a message to the conversation history.
        
        Args:
            run_id: The run ID.
            iteration_id: The iteration ID (optional).
            role: The message role (user, assistant, system).
            content: The message content.
        """
        timestamp = datetime.now().isoformat()
        
        if self.storage_type == "sqlite":
            try:
                conn = sqlite3.connect(self.db_path)
                cursor = conn.cursor()
                cursor.execute(
                    "INSERT INTO messages (run_id, iteration_id, timestamp, role, content) VALUES (?, ?, ?, ?, ?)",
                    (run_id, iteration_id, timestamp, role, content)
                )
                conn.commit()
                conn.close()
                logger.debug(f"Added {role} message to run {run_id}, iteration {iteration_id}")
            except sqlite3.Error as e:
                logger.error(f"SQLite error adding message: {e}")
                raise
        else:  # JSON storage
            try:
                with open(self.json_path, 'r') as f:
                    state = json.load(f)
                
                for run in state["runs"]:
                    if run["run_id"] == run_id:
                        new_message = {
                            "message_id": len(run["messages"]) + 1,
                            "iteration_id": iteration_id,
                            "timestamp": timestamp,
                            "role": role,
                            "content": content
                        }
                        run["messages"].append(new_message)
                        break
                
                with open(self.json_path, 'w') as f:
                    json.dump(state, f, indent=4)
                
                logger.debug(f"Added {role} message to run {run_id}, iteration {iteration_id} in JSON storage")
            except (json.JSONDecodeError, IOError) as e:
                logger.error(f"JSON error adding message: {e}")
                raise

    def get_conversation_history(self, run_id: int) -> List[Dict[str, str]]:
        """
        Get the conversation history for a run.
        
        Args:
            run_id: The run ID.
            
        Returns:
            List of message dictionaries with role and content.
        """
        if self.storage_type == "sqlite":
            try:
                conn = sqlite3.connect(self.db_path)
                cursor = conn.cursor()
                cursor.execute(
                    "SELECT role, content FROM messages WHERE run_id = ? ORDER BY message_id",
                    (run_id,)
                )
                messages = [{"role": role, "content": content} for role, content in cursor.fetchall()]
                conn.close()
                return messages
            except sqlite3.Error as e:
                logger.error(f"SQLite error getting conversation history: {e}")
                raise
        else:  # JSON storage
            try:
                with open(self.json_path, 'r') as f:
                    state = json.load(f)
                
                for run in state["runs"]:
                    if run["run_id"] == run_id:
                        return [{"role": msg["role"], "content": msg["content"]} 
                                for msg in sorted(run["messages"], key=lambda x: x["message_id"])]
                
                return []  # Run not found
            except (json.JSONDecodeError, IOError) as e:
                logger.error(f"JSON error getting conversation history: {e}")
                raise

    def add_council_evaluation(
        self, 
        iteration_id: int, 
        model_name: str, 
        role: str, 
        evaluation: str, 
        score: Optional[float] = None
    ):
        """
        Add a VESPER.MIND council evaluation.
        
        Args:
            iteration_id: The iteration ID.
            model_name: The model name.
            role: The council role (Theorist, Architect, etc.).
            evaluation: The evaluation text.
            score: Optional numerical score.
        """
        timestamp = datetime.now().isoformat()
        
        if self.storage_type == "sqlite":
            try:
                conn = sqlite3.connect(self.db_path)
                cursor = conn.cursor()
                cursor.execute(
                    """
                    INSERT INTO council_evaluations 
                    (iteration_id, model_name, role, evaluation, score, timestamp) 
                    VALUES (?, ?, ?, ?, ?, ?)
                    """,
                    (iteration_id, model_name, role, evaluation, score, timestamp)
                )
                conn.commit()
                conn.close()
                logger.info(f"Added {role} evaluation from {model_name} for iteration {iteration_id}")
            except sqlite3.Error as e:
                logger.error(f"SQLite error adding council evaluation: {e}")
                raise
        else:  # JSON storage
            try:
                with open(self.json_path, 'r') as f:
                    state = json.load(f)
                
                # Find the run containing this iteration
                for run in state["runs"]:
                    for iteration in run.get("iterations", []):
                        if iteration["iteration_id"] == iteration_id:
                            # Initialize council_evaluations if it doesn't exist
                            if "council_evaluations" not in iteration:
                                iteration["council_evaluations"] = []
                            
                            new_evaluation = {
                                "evaluation_id": len(iteration["council_evaluations"]) + 1,
                                "model_name": model_name,
                                "role": role,
                                "evaluation": evaluation,
                                "score": score,
                                "timestamp": timestamp
                            }
                            iteration["council_evaluations"].append(new_evaluation)
                            
                            with open(self.json_path, 'w') as f:
                                json.dump(state, f, indent=4)
                            
                            logger.info(f"Added {role} evaluation from {model_name} for iteration {iteration_id} in JSON storage")
                            return
                
                logger.warning(f"Iteration {iteration_id} not found for adding council evaluation")
            except (json.JSONDecodeError, IOError) as e:
                logger.error(f"JSON error adding council evaluation: {e}")
                raise

    def get_run_summary(self, run_id: int) -> Dict[str, Any]:
        """
        Get a summary of a run.
        
        Args:
            run_id: The run ID.
            
        Returns:
            Dictionary with run summary.
        """
        if self.storage_type == "sqlite":
            try:
                conn = sqlite3.connect(self.db_path)
                conn.row_factory = sqlite3.Row
                cursor = conn.cursor()
                
                # Get run details
                cursor.execute("SELECT * FROM runs WHERE run_id = ?", (run_id,))
                run_row = cursor.fetchone()
                if not run_row:
                    logger.warning(f"Run {run_id} not found in SQLite database.")
                    return {}
                run = dict(run_row)
                
                # Convert converged integer back to boolean
                if run.get("converged") is not None:
                    run["converged"] = bool(run["converged"])
                    
                # Get iterations
                cursor.execute("SELECT * FROM iterations WHERE run_id = ? ORDER BY iteration_number", (run_id,))
                iterations = [dict(row) for row in cursor.fetchall()]
                
                # Get message count
                cursor.execute("SELECT COUNT(*) FROM messages WHERE run_id = ?", (run_id,))
                message_count = cursor.fetchone()[0]
                
                conn.close()
                
                return {
                    "run_id": run_id,
                    "start_time": run["start_time"],
                    "end_time": run["end_time"],
                    "initial_goal": run["initial_goal"],
                    "converged": run["converged"],
                    "final_status": run["final_status"],
                    "iteration_count": len(iterations),
                    "message_count": message_count,
                    "iterations": iterations
                }
            except sqlite3.Error as e:
                logger.error(f"SQLite error getting run summary: {e}")
                raise
        else:  # JSON storage
            try:
                with open(self.json_path, 'r') as f:
                    state = json.load(f)
                
                for run in state["runs"]:
                    if run["run_id"] == run_id:
                        return {
                            "run_id": run_id,
                            "start_time": run["start_time"],
                            "end_time": run.get("end_time"),
                            "initial_goal": run["initial_goal"],
                            "converged": run.get("converged", False),
                            "final_status": run.get("final_status"),
                            "iteration_count": len(run.get("iterations", [])),
                            "message_count": len(run.get("messages", [])),
                            "iterations": run.get("iterations", [])
                        }
                
                logger.warning(f"Run {run_id} not found")
                return {}
            except (json.JSONDecodeError, IOError) as e:
                logger.error(f"JSON error getting run summary: {e}")
                raise

    def get_latest_run_id(self) -> Optional[int]:
        """Get the ID of the latest run."""
        if self.storage_type == "sqlite":
            try:
                conn = sqlite3.connect(self.db_path)
                cursor = conn.cursor()
                cursor.execute("SELECT MAX(run_id) FROM runs")
                run_id = cursor.fetchone()[0]
                conn.close()
                return run_id
            except sqlite3.Error as e:
                logger.error(f"SQLite error getting latest run ID: {e}")
                return None
        else:  # JSON storage
            try:
                with open(self.json_path, 'r') as f:
                    state = json.load(f)

                # Use .get() to safely access keys
                current_run = state.get("current_run")
                if current_run is not None:
                    return current_run

                runs = state.get("runs", []) # Default to empty list if 'runs' key is missing
                if runs:
                    # Ensure runs have 'run_id' before trying to get max
                    valid_run_ids = [run.get("run_id") for run in runs if isinstance(run, dict) and "run_id" in run]
                    if valid_run_ids:
                        return max(valid_run_ids)

                return None # No runs or no valid run_ids found
            except (json.JSONDecodeError, IOError) as e: # Removed KeyError as .get handles it
                logger.error(f"JSON error getting latest run ID from {self.json_path}: {e}")
                return None
    def get_council_evaluations(self, iteration_id: int) -> List[Dict[str, Any]]:
        """
        Get all council evaluations for an iteration.
        
        Args:
            iteration_id: The iteration ID.
            
        Returns:
            List of council evaluation dictionaries.
        """
        if self.storage_type == "sqlite":
            try:
                conn = sqlite3.connect(self.db_path)
                conn.row_factory = sqlite3.Row
                cursor = conn.cursor()
                cursor.execute(
                    "SELECT * FROM council_evaluations WHERE iteration_id = ? ORDER BY timestamp",
                    (iteration_id,)
                )
                evaluations = [dict(row) for row in cursor.fetchall()]
                conn.close()
                return evaluations
            except sqlite3.Error as e:
                logger.error(f"SQLite error getting council evaluations: {e}")
                return []
        else:  # JSON storage
            try:
                with open(self.json_path, 'r') as f:
                    state = json.load(f)
                
                # Find the iteration
                for run in state.get("runs", []):
                    for iteration in run.get("iterations", []):
                        if iteration.get("iteration_id") == iteration_id:
                            return iteration.get("council_evaluations", [])
                
                return []
            except (json.JSONDecodeError, IOError) as e:
                logger.error(f"JSON error getting council evaluations: {e}")
                return []
