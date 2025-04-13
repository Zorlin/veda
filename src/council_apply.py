import re
from pathlib import Path
import logging

logger = logging.getLogger(__name__)

def council_apply_plan(plan_path="PLAN.md"):
    """
    Parse PLAN.md and extract actionable steps from the most recent council round.
    Returns a list of actions (strings).
    """
    plan_file = Path(plan_path)
    if not plan_file.exists():
        logger.warning(f"PLAN.md not found at {plan_path}")
        return []

    text = plan_file.read_text(encoding="utf-8")

    # Find the most recent council round section
    rounds = list(re.finditer(r"### Council Round \d+ \([^)]+\)", text))
    if not rounds:
        logger.warning("No council round found in PLAN.md")
        return []

    # Use the last round
    last_round_start = rounds[-1].start()
    next_round = rounds[-1].end()
    # Find the next council round or end of file
    next_round_match = re.search(r"### Council Round \d+ \([^)]+\)", text[next_round:])
    last_round_end = next_round + next_round_match.start() if next_round_match else len(text)
    last_round_text = text[last_round_start:last_round_end]

    # Extract actionable steps from "Next Steps" or checkboxes
    actions = []
    # Look for checkboxes or numbered steps
    checkbox_pattern = re.compile(r"^\s*[-*]\s+\[ \]\s*(.+)$", re.MULTILINE)
    numbered_pattern = re.compile(r"^\s*\d+\.\s+(.+)$", re.MULTILINE)
    for match in checkbox_pattern.finditer(last_round_text):
        actions.append(match.group(1).strip())
    if not actions:
        for match in numbered_pattern.finditer(last_round_text):
            actions.append(match.group(1).strip())

    logger.info(f"Council actions queued: {actions}")
    return actions
