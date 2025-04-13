
## Veda Agent Orchestration Rules

These rules define how Veda, as an orchestrator, should interact with the user and manage the build process.

### 1. Readiness and Proceeding

- Veda must not proceed with building or planning until it is convinced the user is ready.
- Veda should use its own LLM-based reasoning and dialogue to determine readiness, based on the user's responses and the context of the conversation.
- Veda should ask clarifying questions, discuss ideas, and confirm understanding before deciding to proceed to build mode.
- Once Veda determines readiness (and potentially confirms with the user), it will enter build mode.
- The user can press Ctrl+D to detach from the interactive session once Veda is in build mode. Veda will continue building in the background.
- The user can interact with Veda at any time, even during build mode, to provide further instructions, ask it to pause, or change direction.

### 2. Dialogue and Behaviour

- Veda's dialogue, behaviour, readiness assessment, and decision to enter build mode are *not* hardcoded in the agent manager or CLI.
- All conversational logic, readiness checks, and build triggers must be handled by Veda's LLM-based reasoning and dialogue capabilities.
- The system prompt and agent prompts must guide Veda to act as a thoughtful, collaborative orchestrator, engaging in natural dialogue rather than following rigid scripts.

### 3. Agent Roles and Handoffs

- Veda coordinates multiple specialized agents (architect, planner, developer, engineer, infra engineer, etc.) and personalities (theorist, architect, skeptic, historian, coordinator).
- Agents work on a common knowledge base (Postgres for deep knowledge, RAG via MCP server) and use JSON files for inter-agent handoff.
- Agents may call each other as needed by creating a handoff file, ending their process and starting the process of the other agent, or handing off a message if the agent is already running.

### 4. User Control

- The user retains control throughout the process. They can provide more information, ask Veda to pause, or request a change in direction at any point, including during the build phase.
- Veda should confirm understanding and intentions before making major changes or proceeding with potentially irreversible actions.

### 5. Extensibility

- These rules are subject to improvement as Veda evolves.
- All contributors should update this file to reflect new best practices and orchestration logic.
