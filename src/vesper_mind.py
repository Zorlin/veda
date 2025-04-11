import logging
from typing import Dict, Any, List, Optional, Tuple
import json
import os
from pathlib import Path
from datetime import datetime
import time

from .llm_interaction import get_llm_response
from .ledger import Ledger

# Configure logging
logger = logging.getLogger(__name__)

class VesperMind:
    """
    Implements the VESPER.MIND council architecture for code evaluation and improvement.
    
    The council consists of:
    - Open-Source Thinkers: Theorist, Architect, Skeptic, Historian, Coordinator
    - Final Authority: Arbiter, Canonizer, Redactor
    """
    
    def __init__(
        self, 
        config: Dict[str, Any],
        ledger: Ledger,
        work_dir: Path
    ):
        self.config = config
        self.ledger = ledger
        self.work_dir = work_dir
        
        # Define council members and their roles
        self.open_source_council = {
            "theorist": {
                "model": config.get("theorist_model", "qwen2.5:14b"),
                "description": "Synthesizes new rules, infers structure from patterns"
            },
            "architect": {
                "model": config.get("architect_model", "deepcoder:14b"),
                "description": "Optimizes actual code changes from proposed topology shifts"
            },
            "skeptic": {
                "model": config.get("skeptic_model", "gemma3:12b"),
                "description": "Challenges logic and points out risks, fragility, or overfit"
            },
            "historian": {
                "model": config.get("historian_model", "qwen2.5:14b"),
                "description": "Tracks long-term patterns, suggests rollback or reinforcement"
            },
            "coordinator": {
                "model": config.get("coordinator_model", "command-r7b"),
                "description": "Mediates dialogue and relays findings to upper quorum"
            }
        }
        
        self.closed_source_council = {
            "arbiter": {
                "model": config.get("arbiter_model", "claude-3.7-sonnet"),
                "description": "Final judge of logical and structural soundness"
            },
            "canonizer": {
                "model": config.get("canonizer_model", "gemini-2.5-pro"),
                "description": "Decides whether a proposal is worthy of merge + new tag"
            },
            "redactor": {
                "model": config.get("redactor_model", "gpt-4-turbo"),
                "description": "Refines changelog entry, formalizes language, ensures consistency"
            }
        }
        
        # Default model to use as fallback for closed-source models
        self.default_model = config.get("ollama_model", "gemma3:12b")
        
        # Check which models are available
        self.available_models = self._check_available_models()
        logger.info(f"VESPER.MIND initialized with {len(self.available_models)} available models")
        
        # Create directories for council outputs
        self.council_dir = work_dir / "council_outputs"
        self.council_dir.mkdir(exist_ok=True)

    def _check_available_models(self) -> List[str]:
        """Check which models are available in the Ollama installation."""
        available_models = []
        
        # For testing environments, skip actual model checks to speed up tests
        if os.environ.get("PYTEST_CURRENT_TEST"):
            logger.info("Running in test environment, skipping actual model availability checks")
            # In tests, pretend all models are available
            for role, details in self.open_source_council.items():
                available_models.append(details["model"])
            for role, details in self.closed_source_council.items():
                available_models.append(details["model"])
            return available_models
        
        # For open-source models, check if they're available in Ollama
        for role, details in self.open_source_council.items():
            model_name = details["model"]
            try:
                # Use a simple prompt to test if the model is available
                test_prompt = "Hello"
                response = get_llm_response(
                    test_prompt,
                    {"ollama_model": model_name},
                    history=None,
                    system_prompt="Respond with 'OK' if you can see this message."
                )
                available_models.append(model_name)
                logger.info(f"Model {model_name} for role {role} is available")
            except Exception as e:
                logger.warning(f"Model {model_name} for role {role} is not available: {e}")
                # Log which model will be used as fallback
                logger.info(f"Will use {self.default_model} as fallback for {role}")
        
        # For closed-source models, we'll check if API keys are configured
        # For now, we'll assume they're not available and use the default model
        for role, details in self.closed_source_council.items():
            model_name = details["model"]
            # Check if there's an API key configured for this model
            api_key_var = f"{role.upper()}_API_KEY"
            if os.environ.get(api_key_var):
                logger.info(f"API key found for {model_name}, will use for {role}")
                available_models.append(model_name)
            else:
                logger.warning(f"No API key found for {model_name}, will use {self.default_model} as fallback for {role}")
        
        return available_models

    def evaluate_iteration(
        self,
        run_id: int,
        iteration_id: int,
        initial_goal: str,
        aider_diff: str,
        pytest_output: str,
        pytest_passed: bool,
        history: List[Dict[str, str]]
    ) -> Tuple[str, str, Dict[str, Any]]:
        """
        Run the VESPER.MIND council evaluation on an iteration.
        
        Args:
            run_id: The run ID.
            iteration_id: The iteration ID.
            initial_goal: The initial goal prompt.
            aider_diff: The diff generated by Aider.
            pytest_output: The output from pytest.
            pytest_passed: Whether pytest passed.
            history: The conversation history.
            
        Returns:
            Tuple containing:
            - verdict: "SUCCESS", "RETRY", or "FAILURE"
            - suggestions: Suggestions for improvement if verdict is "RETRY"
            - council_results: Dictionary with detailed council evaluations
        """
        logger.info(f"Running VESPER.MIND council evaluation for iteration {iteration_id}")
        
        # Create iteration-specific directory for council outputs
        council_iter_dir = self.council_dir / f"run_{run_id}_iter_{iteration_id}"
        council_iter_dir.mkdir(exist_ok=True)
        
        # Initialize council results
        council_results = {
            "open_source": {},
            "closed_source": {},
            "final_verdict": None,
            "final_suggestions": None,
            "timestamp": datetime.now().isoformat()
        }
        
        # Run open-source council evaluations in parallel (if possible)
        # For now, we'll run them sequentially
        for role, details in self.open_source_council.items():
            model_name = details["model"]
            if model_name in self.available_models:
                logger.info(f"Running {role} evaluation with {model_name}")
                evaluation = self._run_open_source_evaluation(
                    role, model_name, initial_goal, aider_diff, pytest_output, pytest_passed, history
                )
                council_results["open_source"][role] = evaluation
                
                # Store in ledger - ensure evaluation content is stored as a string (JSON if dict)
                eval_content = evaluation.get("evaluation", "No evaluation content")
                if isinstance(eval_content, dict):
                    eval_content_str = json.dumps(eval_content)
                else:
                    eval_content_str = str(eval_content) # Ensure it's a string
                
                self.ledger.add_council_evaluation(
                    iteration_id, model_name, role, eval_content_str, evaluation.get("score")
                )
                
                # Save to file
                eval_file = council_iter_dir / f"{role}_evaluation.json"
                with open(eval_file, 'w') as f:
                    json.dump(evaluation, f, indent=2)
            else:
                # Use default model as fallback
                logger.info(f"Using {self.default_model} as fallback for {role}")
                evaluation = self._run_open_source_evaluation(
                    role, self.default_model, initial_goal, aider_diff, pytest_output, pytest_passed, history
                )
                council_results["open_source"][role] = evaluation
                
                # Store in ledger - ensure evaluation content is stored as a string (JSON if dict)
                eval_content = evaluation.get("evaluation", "No evaluation content")
                if isinstance(eval_content, dict):
                    eval_content_str = json.dumps(eval_content)
                else:
                    eval_content_str = str(eval_content) # Ensure it's a string
                    
                self.ledger.add_council_evaluation(
                    iteration_id, f"{self.default_model} (as fallback for {model_name})", 
                    role, eval_content_str, evaluation.get("score")
                )
                
                # Save to file
                eval_file = council_iter_dir / f"{role}_evaluation.json"
                with open(eval_file, 'w') as f:
                    json.dump(evaluation, f, indent=2)
        
        # Synthesize open-source evaluations for the closed-source council
        open_source_summary = self._synthesize_open_source_evaluations(council_results["open_source"])
        
        # Save the synthesis
        synthesis_file = council_iter_dir / "open_source_synthesis.md"
        with open(synthesis_file, 'w') as f:
            f.write(open_source_summary)
        
        # Run closed-source council evaluations
        for role, details in self.closed_source_council.items():
            model_name = details["model"]
            if model_name in self.available_models:
                # This would use the actual closed-source API if available
                logger.info(f"Running {role} evaluation with {model_name}")
                # For now, this is a placeholder
            else:
                # Use default model as fallback
                logger.info(f"Using {self.default_model} as fallback for {role} ({model_name})")
                evaluation = self._run_closed_source_evaluation(
                    role, self.default_model, initial_goal, aider_diff, pytest_output, 
                    pytest_passed, history, open_source_summary
                )
                council_results["closed_source"][role] = evaluation
                
                # Store in ledger - ensure evaluation content is stored as a string (JSON if dict)
                eval_content = evaluation.get("evaluation", "No evaluation content")
                if isinstance(eval_content, dict):
                    eval_content_str = json.dumps(eval_content)
                else:
                    eval_content_str = str(eval_content) # Ensure it's a string
                    
                self.ledger.add_council_evaluation(
                    iteration_id, f"{self.default_model} (as {model_name})", role, 
                    eval_content_str, evaluation.get("score")
                )
                
                # Save to file
                eval_file = council_iter_dir / f"{role}_evaluation.json"
                with open(eval_file, 'w') as f:
                    json.dump(evaluation, f, indent=2)
        
        # Determine final verdict and suggestions
        verdict, suggestions = self._determine_final_verdict(council_results)
        council_results["final_verdict"] = verdict
        council_results["final_suggestions"] = suggestions
        
        # Save final verdict
        verdict_file = council_iter_dir / "final_verdict.json"
        with open(verdict_file, 'w') as f:
            json.dump({
                "verdict": verdict,
                "suggestions": suggestions,
                "timestamp": council_results["timestamp"]
            }, f, indent=2)
        
        logger.info(f"VESPER.MIND council verdict: {verdict}")
        return verdict, suggestions, council_results

    def _run_open_source_evaluation(
        self,
        role: str,
        model_name: str,
        initial_goal: str,
        aider_diff: str,
        pytest_output: str,
        pytest_passed: bool,
        history: List[Dict[str, str]]
    ) -> Dict[str, Any]:
        """Run evaluation with an open-source council member."""
        role_description = self.open_source_council[role]["description"]
        
        # Create role-specific system prompt based on the role
        if role == "theorist":
            system_prompt = f"""You are the Theorist in the VESPER.MIND council.
Your role: {role_description}

As the Theorist, your job is to synthesize new rules and infer structure from patterns.
Analyze the code changes deeply and identify underlying patterns, principles, and structures.
Consider how the changes align with software design principles and patterns.

Respond in JSON format with the following structure:
{{
    "evaluation": "Your detailed evaluation focusing on patterns and principles...",
    "score": A score from 0.0 to 1.0 representing your assessment,
    "concerns": ["List any specific concerns about structural integrity"],
    "recommendations": ["List specific recommendations for improving structure"],
    "patterns_identified": ["List any patterns or anti-patterns you've identified"],
    "entropy_assessment": "Your assessment of whether these changes increase or decrease system entropy"
}}"""
        elif role == "architect":
            system_prompt = f"""You are the Architect in the VESPER.MIND council.
Your role: {role_description}

As the Architect, your job is to optimize actual code changes from proposed topology shifts.
Analyze the code changes from a technical implementation perspective.
Focus on code quality, efficiency, and how well the implementation achieves the goal.

Respond in JSON format with the following structure:
{{
    "evaluation": "Your detailed evaluation focusing on code quality and implementation...",
    "score": A score from 0.0 to 1.0 representing your assessment,
    "concerns": ["List any specific concerns about implementation"],
    "recommendations": ["List specific code-level recommendations"],
    "optimization_opportunities": ["List any opportunities for optimization"],
    "architectural_impact": "Your assessment of how these changes impact the overall architecture"
}}"""
        elif role == "skeptic":
            system_prompt = f"""You are the Skeptic in the VESPER.MIND council.
Your role: {role_description}

As the Skeptic, your job is to challenge logic and point out risks, fragility, or overfit.
Be critical but constructive. Look for edge cases, potential bugs, and logical flaws.
Consider security implications, performance bottlenecks, and maintainability issues.

Respond in JSON format with the following structure:
{{
    "evaluation": "Your detailed critical evaluation...",
    "score": A score from 0.0 to 1.0 representing your assessment,
    "concerns": ["List specific concerns, risks, and potential issues"],
    "recommendations": ["List specific recommendations to address concerns"],
    "edge_cases": ["List edge cases that might not be handled"],
    "risk_assessment": "Your assessment of the overall risk level of these changes"
}}"""
        elif role == "historian":
            system_prompt = f"""You are the Historian in the VESPER.MIND council.
Your role: {role_description}

As the Historian, your job is to track long-term patterns and suggest rollback or reinforcement.
Consider how the current changes relate to the conversation history and previous iterations.
Evaluate whether the changes represent progress or regression compared to previous attempts.

Respond in JSON format with the following structure:
{{
    "evaluation": "Your detailed evaluation from a historical perspective...",
    "score": A score from 0.0 to 1.0 representing your assessment,
    "concerns": ["List any concerns about regression or inconsistency"],
    "recommendations": ["List specific recommendations based on historical patterns"],
    "historical_patterns": ["List patterns observed across the conversation history"],
    "trajectory_assessment": "Your assessment of whether the project is on a positive trajectory"
}}"""
        elif role == "coordinator":
            system_prompt = f"""You are the Coordinator in the VESPER.MIND council.
Your role: {role_description}

As the Coordinator, your job is to mediate dialogue and relay findings to the upper quorum.
Synthesize the overall state of the project and provide a balanced assessment.
Consider all aspects: goal alignment, test results, code quality, and potential issues.

Respond in JSON format with the following structure:
{{
    "evaluation": "Your detailed balanced evaluation...",
    "score": A score from 0.0 to 1.0 representing your assessment,
    "concerns": ["List key concerns that should be addressed"],
    "recommendations": ["List prioritized recommendations"],
    "consensus_points": ["List points where there seems to be agreement"],
    "divergence_points": ["List points where there might be disagreement"],
    "overall_assessment": "Your overall assessment of the current state"
}}"""
        else:
            # Generic system prompt for any other roles
            system_prompt = f"""You are the {role.title()} in the VESPER.MIND council.
Your role: {role_description}

Analyze the code changes and test results based on your specific perspective.
Focus on your area of expertise and provide a detailed evaluation.

Respond in JSON format with the following structure:
{{
    "evaluation": "Your detailed evaluation...",
    "score": A score from 0.0 to 1.0 representing your assessment,
    "concerns": ["List any specific concerns"],
    "recommendations": ["List specific recommendations"]
}}"""

        # Create role-specific evaluation prompt
        prompt = f"""
As the {role.title()}, evaluate the following code changes:

Initial Goal:
{initial_goal}

Code Changes (diff):
```diff
{aider_diff if aider_diff else "[No changes made]"}
```

Test Results:
Passed: {pytest_passed}
```
{pytest_output}
```

Conversation History Summary:
{self._summarize_history(history, max_entries=3)}

Based on your role as {role.title()} ({role_description}), provide your evaluation.
Remember to respond in the JSON format specified in your instructions.
"""

        # Get response from the model
        try:
            # Set temperature based on role
            # Theorist and Skeptic benefit from higher temperature for creativity and critical thinking
            # Architect and Historian benefit from lower temperature for precision
            temperature = 0.8 if role in ["theorist", "skeptic"] else 0.4
            
            response = get_llm_response(
                prompt,
                {"ollama_model": model_name, "ollama_options": {"temperature": temperature}},
                history=None,
                system_prompt=system_prompt
            )
            
            # Parse JSON response
            try:
                # Extract JSON if it's wrapped in markdown code blocks
                if "```json" in response:
                    json_start = response.find("```json") + 7
                    json_end = response.find("```", json_start)
                    json_str = response[json_start:json_end].strip()
                    evaluation = json.loads(json_str)
                elif "```" in response:
                    json_start = response.find("```") + 3
                    json_end = response.find("```", json_start)
                    json_str = response[json_start:json_end].strip()
                    evaluation = json.loads(json_str)
                else:
                    evaluation = json.loads(response)
                
                # Ensure all required fields are present
                if "evaluation" not in evaluation:
                    evaluation["evaluation"] = "No evaluation provided"
                if "score" not in evaluation:
                    evaluation["score"] = 0.5
                if "concerns" not in evaluation:
                    evaluation["concerns"] = []
                if "recommendations" not in evaluation:
                    evaluation["recommendations"] = []
                
                # Add metadata
                evaluation["model"] = model_name
                evaluation["role"] = role
                evaluation["timestamp"] = datetime.now().isoformat()
                evaluation["test_passed"] = pytest_passed
                
                return evaluation
            except json.JSONDecodeError:
                logger.warning(f"Could not parse JSON response from {role} evaluation")
                return {
                    "evaluation": response,
                    "score": 0.5,
                    "concerns": ["Response format error"],
                    "recommendations": ["Review manually"],
                    "model": model_name,
                    "role": role,
                    "timestamp": datetime.now().isoformat(),
                    "test_passed": pytest_passed
                }
        except Exception as e:
            logger.error(f"Error running {role} evaluation: {e}")
            return {
                "evaluation": f"Error: {str(e)}",
                "score": 0.0,
                "concerns": ["Evaluation failed"],
                "recommendations": ["Try with a different model"],
                "model": model_name,
                "role": role,
                "timestamp": datetime.now().isoformat(),
                "test_passed": pytest_passed
            }

    def _run_closed_source_evaluation(
        self,
        role: str,
        model_name: str,
        initial_goal: str,
        aider_diff: str,
        pytest_output: str,
        pytest_passed: bool,
        history: List[Dict[str, str]],
        open_source_summary: str
    ) -> Dict[str, Any]:
        """Run evaluation with a closed-source council member (or stand-in)."""
        role_description = self.closed_source_council[role]["description"]
        
        # Create role-specific system prompt based on the role
        if role == "arbiter":
            system_prompt = f"""You are the Arbiter in the VESPER.MIND council.
Your role: {role_description}

As the Arbiter, you are the final judge of logical and structural soundness.
You must carefully review the evaluations from the Open-Source Thinkers and make a definitive judgment.
Your verdict carries significant weight in determining whether the changes are accepted.

Respond in JSON format with the following structure:
{{
    "evaluation": "Your detailed evaluation of logical and structural soundness...",
    "score": A score from 0.0 to 1.0 representing your assessment,
    "verdict": "SUCCESS", "RETRY", or "FAILURE",
    "rationale": "Detailed explanation for your verdict",
    "suggestions": "Specific suggestions for improvement if verdict is RETRY",
    "critical_issues": ["List any critical issues that must be addressed"],
    "strengths": ["List key strengths of the implementation"]
}}"""
        elif role == "canonizer":
            system_prompt = f"""You are the Canonizer in the VESPER.MIND council.
Your role: {role_description}

As the Canonizer, you decide whether a proposal is worthy of merge and a new version tag.
You must consider not just correctness, but also whether the changes meet quality standards
and align with the project's goals and architecture.

Respond in JSON format with the following structure:
{{
    "evaluation": "Your detailed evaluation of merge-worthiness...",
    "score": A score from 0.0 to 1.0 representing your assessment,
    "verdict": "SUCCESS", "RETRY", or "FAILURE",
    "rationale": "Detailed explanation for your verdict",
    "suggestions": "Specific suggestions for improvement if verdict is RETRY",
    "version_tag": "Suggested version tag if verdict is SUCCESS (e.g., v0.1.2-vesper)",
    "changelog_notes": "Key points that should be included in the changelog"
}}"""
        elif role == "redactor":
            system_prompt = f"""You are the Redactor in the VESPER.MIND council.
Your role: {role_description}

As the Redactor, you refine changelog entries, formalize language, and ensure consistency.
Your job is to take the raw evaluations and create polished, professional documentation.

Respond in JSON format with the following structure:
{{
    "evaluation": "Your detailed evaluation of documentation needs...",
    "score": A score from 0.0 to 1.0 representing your assessment,
    "verdict": "SUCCESS", "RETRY", or "FAILURE",
    "rationale": "Explanation for your verdict",
    "suggestions": "Specific suggestions for improvement if verdict is RETRY",
    "changelog_entry": "A polished changelog entry for these changes",
    "documentation_updates": "Suggestions for any documentation that should be updated"
}}"""
        else:
            # Generic system prompt for any other roles
            system_prompt = f"""You are the {role.title()} in the VESPER.MIND council.
Your role: {role_description}

You are part of the Final Authority layer that reviews the evaluations from the Open-Source Thinkers.
Your task is to make a final judgment based on your expertise and the input from other council members.

Respond in JSON format with the following structure:
{{
    "evaluation": "Your detailed evaluation...",
    "score": A score from 0.0 to 1.0 representing your assessment,
    "verdict": "SUCCESS", "RETRY", or "FAILURE",
    "rationale": "Explanation for your verdict",
    "suggestions": "Specific suggestions for improvement if verdict is RETRY"
}}"""

        # Create role-specific evaluation prompt
        prompt = f"""
As the {role.title()}, review the following code changes and open-source council evaluations:

Initial Goal:
{initial_goal}

Code Changes (diff):
```diff
{aider_diff if aider_diff else "[No changes made]"}
```

Test Results:
Passed: {pytest_passed}
```
{pytest_output}
```

Open-Source Council Evaluations:
{open_source_summary}

Conversation History Summary:
{self._summarize_history(history, max_entries=2)}

Based on your role as {role.title()} ({role_description}), provide your final evaluation.
Remember to respond in the JSON format specified in your instructions.
"""

        # Get response from the model
        try:
            # Set temperature based on role
            # Arbiter benefits from lower temperature for precision
            # Redactor benefits from higher temperature for creativity
            temperature = 0.7 if role == "redactor" else 0.3
            
            response = get_llm_response(
                prompt,
                {"ollama_model": model_name, "ollama_options": {"temperature": temperature}},
                history=None,
                system_prompt=system_prompt
            )
            
            # Parse JSON response
            try:
                # Extract JSON if it's wrapped in markdown code blocks
                if "```json" in response:
                    json_start = response.find("```json") + 7
                    json_end = response.find("```", json_start)
                    json_str = response[json_start:json_end].strip()
                    evaluation = json.loads(json_str)
                elif "```" in response:
                    json_start = response.find("```") + 3
                    json_end = response.find("```", json_start)
                    json_str = response[json_start:json_end].strip()
                    evaluation = json.loads(json_str)
                else:
                    evaluation = json.loads(response)
                
                # Ensure all required fields are present
                if "evaluation" not in evaluation:
                    evaluation["evaluation"] = "No evaluation provided"
                if "score" not in evaluation:
                    evaluation["score"] = 0.5
                if "verdict" not in evaluation:
                    evaluation["verdict"] = "RETRY"
                if "rationale" not in evaluation:
                    evaluation["rationale"] = "No rationale provided"
                if "suggestions" not in evaluation:
                    evaluation["suggestions"] = "No suggestions provided"
                
                # Add metadata
                evaluation["model"] = model_name
                evaluation["role"] = role
                evaluation["timestamp"] = datetime.now().isoformat()
                evaluation["test_passed"] = pytest_passed
                
                return evaluation
            except json.JSONDecodeError:
                logger.warning(f"Could not parse JSON response from {role} evaluation")
                return {
                    "evaluation": response,
                    "score": 0.5,
                    "verdict": "RETRY",
                    "rationale": "Response format error",
                    "suggestions": "Review manually",
                    "model": model_name,
                    "role": role,
                    "timestamp": datetime.now().isoformat(),
                    "test_passed": pytest_passed
                }
        except Exception as e:
            logger.error(f"Error running {role} evaluation: {e}")
            return {
                "evaluation": f"Error: {str(e)}",
                "score": 0.0,
                "verdict": "RETRY",
                "rationale": "Evaluation failed",
                "suggestions": "Try with a different model",
                "model": model_name,
                "role": role,
                "timestamp": datetime.now().isoformat(),
                "test_passed": pytest_passed
            }

    def _synthesize_open_source_evaluations(self, evaluations: Dict[str, Dict[str, Any]]) -> str:
        """Synthesize open-source evaluations into a summary for closed-source council."""
        if not evaluations:
            return "No open-source evaluations available."
        
        # Calculate average score
        scores = [eval.get('score', 0.5) for eval in evaluations.values() if 'score' in eval]
        avg_score = sum(scores) / len(scores) if scores else 0.5
        
        # Collect all concerns and recommendations
        all_concerns = []
        all_recommendations = []
        for role, eval_data in evaluations.items():
            concerns = eval_data.get('concerns', [])
            recommendations = eval_data.get('recommendations', [])
            for concern in concerns:
                all_concerns.append(f"{role.title()}: {concern}")
            for rec in recommendations:
                all_recommendations.append(f"{role.title()}: {rec}")
        
        # Start with a summary
        summary = [
            "# Open-Source Council Evaluation Summary",
            f"Average Score: {avg_score:.2f} (0.0-1.0 scale)",
            f"Number of Evaluations: {len(evaluations)}",
            f"Test Passed: {evaluations[next(iter(evaluations))].get('test_passed', 'Unknown') if evaluations else 'Unknown'}",
            "",
            "## Key Concerns Across Council",
        ]
        
        # Add concerns
        if all_concerns:
            for concern in all_concerns:
                summary.append(f"- {concern}")
        else:
            summary.append("- No concerns identified")
        
        summary.append("")
        summary.append("## Key Recommendations Across Council")
        
        # Add recommendations
        if all_recommendations:
            for rec in all_recommendations:
                summary.append(f"- {rec}")
        else:
            summary.append("- No recommendations provided")
        
        summary.append("")
        summary.append("## Detailed Evaluations")
        
        # Add detailed evaluations
        for role, evaluation in evaluations.items():
            summary.append(f"### {role.title()} Evaluation")
            summary.append(f"Score: {evaluation.get('score', 'N/A')}")
            summary.append(f"Model: {evaluation.get('model', 'Unknown')}")
            summary.append("")
            # Ensure evaluation content is treated as a string
            summary.append(str(evaluation.get('evaluation', 'No evaluation provided')))
            summary.append("")
            
            # Add role-specific fields, ensuring values are strings
            if role == "theorist" and "patterns_identified" in evaluation:
                summary.append("#### Patterns Identified")
                for pattern in evaluation["patterns_identified"]:
                    summary.append(f"- {pattern}")
                summary.append("")
                
                if "entropy_assessment" in evaluation:
                    summary.append(f"#### Entropy Assessment")
                    summary.append(str(evaluation["entropy_assessment"])) # Ensure string
                    summary.append("")
            
            elif role == "architect" and "optimization_opportunities" in evaluation:
                summary.append("#### Optimization Opportunities")
                for opt in evaluation["optimization_opportunities"]:
                    summary.append(f"- {opt}")
                summary.append("")
                
                if "architectural_impact" in evaluation:
                    summary.append(f"#### Architectural Impact")
                    summary.append(str(evaluation["architectural_impact"])) # Ensure string
                    summary.append("")
            
            elif role == "skeptic" and "edge_cases" in evaluation:
                summary.append("#### Edge Cases")
                for edge in evaluation["edge_cases"]:
                    summary.append(f"- {edge}")
                summary.append("")
                
                if "risk_assessment" in evaluation:
                    summary.append(f"#### Risk Assessment")
                    summary.append(str(evaluation["risk_assessment"])) # Ensure string
                    summary.append("")
            
            elif role == "historian" and "historical_patterns" in evaluation:
                summary.append("#### Historical Patterns")
                for pattern in evaluation["historical_patterns"]:
                    summary.append(f"- {pattern}")
                summary.append("")
                
                if "trajectory_assessment" in evaluation:
                    summary.append(f"#### Trajectory Assessment")
                    summary.append(str(evaluation["trajectory_assessment"])) # Ensure string
                    summary.append("")
            
            elif role == "coordinator" and "consensus_points" in evaluation:
                summary.append("#### Consensus Points")
                for point in evaluation["consensus_points"]:
                    summary.append(f"- {point}")
                summary.append("")
                
                if "divergence_points" in evaluation:
                    summary.append("#### Divergence Points")
                    for point in evaluation["divergence_points"]:
                        summary.append(f"- {point}")
                    summary.append("")
                
                if "overall_assessment" in evaluation:
                    summary.append(f"#### Overall Assessment")
                    summary.append(str(evaluation["overall_assessment"])) # Ensure string
                    summary.append("")
            
            # Add concerns and recommendations for all roles
            concerns = evaluation.get('concerns', [])
            if concerns:
                summary.append("#### Concerns")
                for concern in concerns:
                    summary.append(f"- {concern}")
                summary.append("")
            
            recommendations = evaluation.get('recommendations', [])
            if recommendations:
                summary.append("#### Recommendations")
                for recommendation in recommendations:
                    summary.append(f"- {recommendation}")
                summary.append("")
        
        return "\n".join(summary)

    def _determine_final_verdict(self, council_results: Dict[str, Any]) -> Tuple[str, str]:
        """Determine the final verdict and suggestions based on all council evaluations."""
        # Start with the Arbiter's verdict if available
        if "arbiter" in council_results["closed_source"]:
            arbiter_verdict = council_results["closed_source"]["arbiter"].get("verdict", "RETRY")
            arbiter_suggestions = council_results["closed_source"]["arbiter"].get("suggestions", "")
            arbiter_rationale = council_results["closed_source"]["arbiter"].get("rationale", "")
            
            # If Canonizer agrees with SUCCESS, it's a success
            if arbiter_verdict == "SUCCESS" and "canonizer" in council_results["closed_source"]:
                canonizer_verdict = council_results["closed_source"]["canonizer"].get("verdict", "")
                if canonizer_verdict == "SUCCESS":
                    # Get refined suggestions from Redactor if available
                    if "redactor" in council_results["closed_source"]:
                        redactor_eval = council_results["closed_source"]["redactor"].get("evaluation", "")
                        # If redactor has a changelog entry, use that
                        if "changelog_entry" in council_results["closed_source"]["redactor"]:
                            changelog_entry = council_results["closed_source"]["redactor"]["changelog_entry"]
                            return "SUCCESS", f"## Changelog\n\n{changelog_entry}\n\n## Evaluation\n\n{redactor_eval}"
                        return "SUCCESS", redactor_eval
                    
                    # If canonizer has a version tag, include it
                    version_tag = council_results["closed_source"]["canonizer"].get("version_tag", "")
                    if version_tag:
                        return "SUCCESS", f"## {version_tag}\n\nAll council members agree on success.\n\n{arbiter_rationale}"
                    
                    return "SUCCESS", f"All council members agree on success.\n\n{arbiter_rationale}"
            
            # If Arbiter says FAILURE, it's a failure
            if arbiter_verdict == "FAILURE":
                # Include critical issues if available
                critical_issues = council_results["closed_source"]["arbiter"].get("critical_issues", [])
                if critical_issues:
                    issues_str = "\n".join([f"- {issue}" for issue in critical_issues])
                    return "FAILURE", f"{arbiter_suggestions}\n\n## Critical Issues\n\n{issues_str}"
                return "FAILURE", f"{arbiter_suggestions}"
            
            # Otherwise, it's a RETRY with suggestions
            suggestions = arbiter_suggestions
            if "redactor" in council_results["closed_source"]:
                redactor_suggestions = council_results["closed_source"]["redactor"].get("suggestions", "")
                if redactor_suggestions:
                    suggestions = redactor_suggestions
            
            return "RETRY", suggestions
        
        # If no closed-source evaluations, use open-source consensus
        open_source_scores = [eval.get("score", 0.5) for eval in council_results["open_source"].values()]
        if open_source_scores:
            avg_score = sum(open_source_scores) / len(open_source_scores)
            
            # Check if tests passed
            test_passed = False
            for role, eval_data in council_results["open_source"].items():
                if "test_passed" in eval_data:
                    test_passed = eval_data["test_passed"]
                    break
            
            # Get coordinator's overall assessment if available
            coordinator_assessment = ""
            if "coordinator" in council_results["open_source"]:
                coordinator = council_results["open_source"]["coordinator"]
                if "overall_assessment" in coordinator:
                    coordinator_assessment = f"\n\n## Coordinator Assessment\n\n{coordinator['overall_assessment']}"
            
            # Determine verdict based on score and test status
            if avg_score >= 0.8 and test_passed:
                # Check if architect and skeptic both have high scores (>0.7)
                architect_score = council_results["open_source"].get("architect", {}).get("score", 0)
                skeptic_score = council_results["open_source"].get("skeptic", {}).get("score", 0)
                
                if architect_score >= 0.7 and skeptic_score >= 0.7:
                    # Strong consensus for success
                    return "SUCCESS", f"Open-source council consensus indicates success with high confidence (Avg: {avg_score:.2f}, Architect: {architect_score:.2f}, Skeptic: {skeptic_score:.2f}).{coordinator_assessment}"
                else:
                    # Good average but some key roles have concerns
                    return "SUCCESS", f"Open-source council indicates success with moderate confidence (Avg: {avg_score:.2f}).{coordinator_assessment}"
            
            elif avg_score < 0.3 or not test_passed:
                # Either very low score or tests failed
                failure_reason = "tests failed" if not test_passed else f"low evaluation score ({avg_score:.2f})"
                
                # Collect concerns from all open-source members
                all_concerns = []
                for role, eval_data in council_results["open_source"].items():
                    concerns = eval_data.get("concerns", [])
                    for concern in concerns:
                        all_concerns.append(f"{role.title()}: {concern}")
                
                concerns_str = "\n".join([f"- {concern}" for concern in all_concerns]) if all_concerns else "No specific concerns provided."
                
                return "FAILURE", f"Open-source council consensus indicates fundamental issues: {failure_reason}.\n\n## Key Concerns\n\n{concerns_str}{coordinator_assessment}"
            
            else:
                # Middle ground - collect recommendations for improvement
                all_recommendations = []
                for role, eval_data in council_results["open_source"].items():
                    recommendations = eval_data.get("recommendations", [])
                    for rec in recommendations:
                        all_recommendations.append(f"{role.title()}: {rec}")
                
                # Prioritize recommendations from architect and skeptic
                architect_recs = []
                skeptic_recs = []
                other_recs = []
                
                for rec in all_recommendations:
                    if rec.startswith("Architect:"):
                        architect_recs.append(rec)
                    elif rec.startswith("Skeptic:"):
                        skeptic_recs.append(rec)
                    else:
                        other_recs.append(rec)
                
                # Combine recommendations with architect and skeptic first
                prioritized_recs = architect_recs + skeptic_recs + other_recs
                suggestions = "\n".join([f"- {rec}" for rec in prioritized_recs]) if prioritized_recs else "Review and improve based on test results."
                
                return "RETRY", f"Open-source council suggests improvements (Score: {avg_score:.2f}).\n\n## Recommendations\n\n{suggestions}{coordinator_assessment}"
        
        # Default fallback
        return "RETRY", "Insufficient council evaluations. Review and improve based on test results."

    def generate_changelog(self, run_id: int, iteration_id: int, verdict: str) -> str:
        """
        Generate a changelog entry for a successful iteration.
        
        Args:
            run_id: The run ID.
            iteration_id: The iteration ID.
            verdict: The final verdict.
            
        Returns:
            Changelog entry as a string.
        """
        if verdict != "SUCCESS":
            return ""
        
        # Get run summary from ledger
        run_summary = self.ledger.get_run_summary(run_id)
        
        # Find the iteration
        iteration = None
        for iter_data in run_summary.get("iterations", []):
            if iter_data["iteration_id"] == iteration_id:
                iteration = iter_data
                break
        
        if not iteration:
            return "Could not generate changelog: iteration not found."
        
        # Check if we already have a changelog from the Redactor
        council_evaluations = self.ledger.get_council_evaluations(iteration_id)
        for eval_data in council_evaluations:
            if eval_data.get("role") == "redactor":
                try:
                    # Try to parse the evaluation as JSON
                    evaluation = json.loads(eval_data.get("evaluation", "{}"))
                    if "changelog_entry" in evaluation:
                        logger.info("Using existing changelog entry from Redactor")
                        
                        # Get version tag from Canonizer if available
                        version_tag = None
                        for canon_eval in council_evaluations:
                            if canon_eval.get("role") == "canonizer":
                                try:
                                    canon_data = json.loads(canon_eval.get("evaluation", "{}"))
                                    if "version_tag" in canon_data:
                                        version_tag = canon_data["version_tag"]
                                        break
                                except:
                                    pass
                        
                        changelog = evaluation["changelog_entry"]
                        
                        # Add version tag if not present and we have one from Canonizer
                        if version_tag and "# v" not in changelog and "## v" not in changelog:
                            changelog = f"## {version_tag}\n\n{changelog}"
                        elif "# v" not in changelog and "## v" not in changelog:
                            # Generate a default version tag
                            iteration_num = iteration.get("iteration_number", 0)
                            timestamp = time.strftime("%Y%m%d")
                            changelog = f"## v0.1.{iteration_num}-vesper-{timestamp}\n\n{changelog}"
                        
                        return changelog
                except:
                    # If parsing fails, continue to generate a new changelog
                    pass
        
        # Use the default model to generate a changelog
        model_name = self.default_model
        
        # Create changelog prompt
        prompt = f"""
Generate a formal changelog entry for the following successful code changes:

Initial Goal:
{run_summary.get("initial_goal", "Unknown goal")}

Code Changes (diff):
```diff
{iteration.get("aider_diff", "[No changes recorded]")}
```

Test Results:
```
{iteration.get("pytest_output", "[No test output recorded]")}
```

The changes were successful and all tests passed.

Generate a concise, professional changelog entry that:
1. Summarizes what was changed
2. Explains why it was changed
3. Notes any potential impacts
4. Lists any new features or improvements
5. Mentions any bug fixes

Format the changelog as a Markdown entry with a version tag in the format v0.1.X-vesper-YYYYMMDD.
"""

        system_prompt = """You are the Redactor in the VESPER.MIND council.
Your role is to refine changelog entries, formalize language, and ensure consistency.
Generate a professional, concise changelog entry in Markdown format.
Focus on clarity, completeness, and technical accuracy.
Use bullet points for individual changes and group related changes together.
"""

        try:
            changelog = get_llm_response(
                prompt,
                {"ollama_model": model_name, "ollama_options": {"temperature": 0.4}},
                history=None,
                system_prompt=system_prompt
            )
            
            # Add version tag if not present
            if "# v" not in changelog and "## v" not in changelog:
                iteration_num = iteration.get("iteration_number", 0)
                timestamp = time.strftime("%Y%m%d")
                changelog = f"## v0.1.{iteration_num}-vesper-{timestamp}\n\n{changelog}"
            
            # Save the changelog to a file
            changelog_dir = self.work_dir / "changelogs"
            changelog_dir.mkdir(exist_ok=True)
            changelog_file = changelog_dir / f"changelog_run{run_id}_iter{iteration_id}.md"
            with open(changelog_file, 'w') as f:
                f.write(changelog)
            
            return changelog
        except Exception as e:
            logger.error(f"Error generating changelog: {e}")
            return f"Error generating changelog: {e}"
    def _summarize_history(self, history: List[Dict[str, str]], max_entries: int = 3) -> str:
        """Create a concise summary of the conversation history."""
        if not history:
            return "No conversation history available."
        
        # Take the last few entries
        recent_history = history[-max_entries:] if len(history) > max_entries else history
        
        summary = []
        for i, msg in enumerate(recent_history):
            role = msg.get("role", "unknown")
            content = msg.get("content", "")
            # Truncate content if too long
            if len(content) > 200:
                content = content[:197] + "..."
            summary.append(f"{i+1}. {role.capitalize()}: {content}")
        
        return "\n".join(summary)
