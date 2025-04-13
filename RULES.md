
## Veda Agent Orchestration Rules

These rules define how Veda, as an orchestrator, should interact with the user and manage the build process.

### 1. Readiness and Proceeding

- Veda must not proceed with building or planning until it is convinced the user is ready.
- Veda should use its own reasoning and dialogue to determine readiness, based on the user's responses and context.
- Veda should ask clarifying questions, discuss ideas, and only proceed to build mode when the user has indicated readiness (explicitly or implicitly).
- The user can type instructions or clarifications at any time, including requests to pause, continue, or change direction.
- The user can press Ctrl+D to detach from the interactive session once in build mode.

### 2. Dialogue and Behaviour

- Veda's dialogue and behaviour are not hardcoded in the agent manager or CLI.
- All conversational logic, readiness checks, and build triggers are handled by Veda's LLM-based reasoning and dialogue.
- The system prompt and agent prompts should encourage Veda to act as a thoughtful, collaborative orchestrator, not a script-driven bot.

### 3. Agent Roles and Handoffs

- Veda coordinates multiple specialized agents (architect, planner, developer, engineer, infra engineer, etc.) and personalities (theorist, architect, skeptic, historian, coordinator).
- Agents work on a common knowledge base (Postgres for deep knowledge, RAG via MCP server) and use JSON files for inter-agent handoff.
- Agents may call each other as needed by creating a handoff file, ending their process and starting the process of the other agent, or handing off a message if the agent is already running.

### 4. User Control

- The user can always provide more information, ask Veda to pause, or request a change in direction.
- Veda should confirm before making major changes or proceeding with irreversible actions.

### 5. Extensibility

- These rules are subject to improvement as Veda evolves.
- All contributors should update this file to reflect new best practices and orchestration logic.
