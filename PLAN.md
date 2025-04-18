# Veda Implementation Plan

## Overview
This document outlines the implementation plan for Veda, a meta-agent system that orchestrates AI agents (primarily using Aider) to assist with software development through an intuitive terminal user interface (TUI).

## Project Goals
Based on the README.md, Veda aims to:
1. Provide a user-centric Terminal User Interface (TUI) as the primary interaction method
2. Integrate AI within the UI for intelligent assistance
3. Seamlessly manage Aider instances in the background
4. Orchestrate multiple AI agents to work on development tasks
5. Maintain user control throughout the development process

## Implementation Phases

### Phase 1: Core Infrastructure (Weeks 1-2)
- [ ] Set up project structure and dependencies
- [ ] Implement basic TUI framework using Textual
- [ ] Create agent manager for handling Aider instances
- [ ] Develop configuration management system
- [ ] Implement Ollama client for internal reasoning

### Phase 2: Agent Orchestration (Weeks 3-4)
- [ ] Implement goal parsing and file awareness
- [ ] Develop planning phase using DeepCoder:14B via Ollama
- [ ] Create Aider worker spawning mechanism
- [ ] Implement GPU queue management
- [ ] Develop worker status tracking and logging

### Phase 3: User Interface Enhancement (Weeks 5-6)
- [ ] Implement tabbed interface for agent monitoring
- [ ] Create home screen with agent overview
- [ ] Develop broadcast messaging to all workers
- [ ] Implement user input handling and command processing
- [ ] Add keyboard shortcuts and help documentation

### Phase 4: Integration and Workflow (Weeks 7-8)
- [ ] Implement file reading and context injection
- [ ] Develop JSON schema for goal.prompt, goal.plan.json, and worker logs
- [ ] Create workflow directory structure
- [ ] Implement detach functionality (Ctrl+D)
- [ ] Add session persistence

### Phase 5: Testing and Refinement (Weeks 9-10)
- [ ] Develop comprehensive test suite
- [ ] Perform user testing and gather feedback
- [ ] Refine UI/UX based on feedback
- [ ] Optimize performance
- [ ] Document code and create user guide

## Technical Architecture

### Core Components
1. **TUI Layer**
   - Terminal-based user interface using Textual
   - Multiple tabs for different agents
   - Status displays and command input

2. **Agent Manager**
   - Spawns and manages Aider instances
   - Handles inter-agent communication
   - Manages GPU resource allocation

3. **Planning System**
   - Parses user goals
   - Creates structured plans using LLM
   - Breaks down tasks for worker agents

4. **File Management**
   - Automatic file reading for context
   - Change tracking and version control integration
   - File dependency management

## Detailed Workflow Implementation

### 1. User Goal Prompting
- When Veda starts, it prompts the user for their project goal
- Any mentioned filenames are automatically read and analyzed
- The goal is parsed into a structured `goal.prompt` JSON document
- Implementation tasks:
  - Create prompt mechanism in TUI
  - Develop file mention detection
  - Implement JSON structure generation

### 2. File Awareness / Context Injection
- Files mentioned in the prompt are loaded automatically
- File contents are stored alongside the goal in a JSON document
- Implementation tasks:
  - Create file reading functionality
  - Develop content storage mechanism
  - Implement context injection for agents

### 3. Planning Phase (Ollama: DeepCoder:14B)
- A Planner process using DeepCoder:14B via Ollama is spawned
- The planner reads the goal and attached files
- It produces a technical plan as `goal.plan.json`
- Implementation tasks:
  - Integrate with Ollama API
  - Create planning prompt templates
  - Implement JSON plan parsing and validation

### 4. Aider Worker Spawning
- Up to 4 parallel Aider agents are spawned based on the plan
- Each worker handles a subset of tasks from the plan
- Workers write status to `workflows/<worker-name>.json`
- Implementation tasks:
  - Create worker management system
  - Implement task distribution logic
  - Develop worker status tracking

### 5. Aider Response UX (Gemma3:12B)
- Workers use Gemma3:12B to handle Aider prompts
- Prompts are cached and answered automatically
- Implementation tasks:
  - Implement prompt caching mechanism
  - Create automatic response handling
  - Develop silent continuation logic

### 6. User Interaction (TUI)
- Textual TUI provides tabs for each worker
- Home screen shows overview of all agents
- Broadcast messaging allows sending notes to all workers
- Implementation tasks:
  - Create tabbed interface with Terminal widget
  - Implement agent overview screen
  - Develop broadcast messaging functionality

### 7. GPU Queue Management
- Only one Ollama GPU job runs at a time
- A queue manages GPU access for all workers
- Implementation tasks:
  - Create GPU job queue
  - Implement worker waiting mechanism
  - Develop priority system for GPU access

## Technical Dependencies
- Python 3.10+
- Git
- Ollama (for internal chat and coordination)
- Aider (as the primary coding engine)
- OpenRouter API (for accessing advanced models)

## JSON Schema Layouts

### goal.prompt
```json
{
  "goal": "Description of the user's goal",
  "mentioned_files": ["file1.py", "file2.py"]
}
```

### goal.plan.json
```json
{
  "strategy": "Overall approach description",
  "tasks": [
    { "file": "file1.py", "action": "Description of changes" }
  ]
}
```

### workflows/worker-N.json
```json
{
  "worker": "worker-N",
  "status": "editing|waiting|complete",
  "file": "current_file.py",
  "summary": "Description of current work",
  "dependencies": ["dependent_file.py"]
}
```

## Implementation Timeline and Milestones

| Milestone | Timeline | Deliverables |
|-----------|----------|--------------|
| **Core Infrastructure** | Weeks 1-2 | Basic TUI, Agent Manager, Config System |
| **Agent Orchestration** | Weeks 3-4 | Goal Parsing, Planning, Worker Spawning |
| **UI Enhancement** | Weeks 5-6 | Tabbed Interface, Home Screen, Broadcast |
| **Integration** | Weeks 7-8 | File Reading, JSON Schema, Workflow Structure |
| **Testing & Refinement** | Weeks 9-10 | Test Suite, Documentation, Optimization |

## Risk Assessment

### Potential Challenges
1. **GPU Resource Management**: Ensuring efficient use of GPU resources when multiple agents need access
2. **Agent Coordination**: Preventing conflicts when multiple agents work on related files
3. **User Experience**: Maintaining intuitive UX while providing comprehensive control
4. **Model Performance**: Ensuring consistent performance across different LLM models

### Mitigation Strategies
1. Implement robust GPU queue with priority system
2. Develop dependency tracking between worker tasks
3. Conduct regular usability testing of the TUI
4. Include fallback options for different model configurations

## Success Criteria
- TUI provides clear visibility into agent activities
- Users can effectively control and guide the development process
- Agents successfully coordinate to complete complex development tasks
- System handles file dependencies and conflicts gracefully
- Performance remains responsive even with multiple agents active

## Future Enhancements (Post v1.0)
- Enhanced project analysis capabilities
- Integration with additional coding tools beyond Aider
- Support for collaborative development with multiple users
- Advanced visualization of agent activities and dependencies
- Customizable agent personalities and specializations
# Veda Implementation Plan

## Overview
This document outlines the implementation plan for Veda, a meta-agent system that orchestrates AI agents (primarily using Aider) to assist with software development through an intuitive terminal user interface (TUI).

## Project Goals
Based on the README.md, Veda aims to:
1. Provide a user-centric Terminal User Interface (TUI) as the primary interaction method
2. Enhance user experience through AI integration within the UI
3. Seamlessly integrate with Aider for code generation and management
4. Orchestrate multiple AI agents working in parallel
5. Maintain user control throughout the development process

## Implementation Timeline

### Phase 1: Core Infrastructure (Weeks 1-2)
- [ ] Set up project structure and dependencies
- [ ] Implement basic TUI framework using Textual
- [ ] Create agent manager for orchestrating Aider instances
- [ ] Develop configuration management system
- [ ] Implement basic logging and error handling

### Phase 2: Agent Orchestration (Weeks 3-4)
- [ ] Implement goal parsing and file awareness
- [ ] Develop planning phase using Ollama (DeepCoder:14B)
- [ ] Create worker spawning mechanism for Aider agents
- [ ] Implement GPU queue management
- [ ] Develop inter-agent communication protocol

### Phase 3: User Interface (Weeks 5-6)
- [ ] Implement terminal-like tabs for each worker
- [ ] Create home screen with agent overview
- [ ] Develop broadcast functionality to all workers
- [ ] Implement user input handling and command processing
- [ ] Add progress visualization and status reporting

### Phase 4: Integration & Testing (Weeks 7-8)
- [ ] Integrate all components
- [ ] Implement comprehensive error handling
- [ ] Develop automated testing suite
- [ ] Perform user acceptance testing
- [ ] Optimize performance and resource usage

## Technical Architecture

### Core Components
1. **TUI Layer**
   - Terminal-based user interface using Textual
   - Multiple tabs for different workers
   - Status display and command input

2. **Agent Manager**
   - Spawns and manages Aider instances
   - Handles inter-agent communication
   - Manages GPU resource allocation

3. **Planning System**
   - Parses user goals
   - Generates technical plans using Ollama
   - Breaks down tasks for worker assignment

4. **File Management**
   - Tracks file changes across workers
   - Prevents conflicts between agents
   - Manages file dependencies

5. **Communication Protocol**
   - JSON-based message format
   - Structured logging
   - Status reporting

## Data Schemas

### goal.prompt
```json
{
  "goal": "Add FastAPI login routes with session persistence",
  "mentioned_files": ["app/main.py", "auth/session.py"]
}
```

### goal.plan.json
```json
{
  "strategy": "Add authentication routes, create session store, update middleware.",
  "tasks": [
    { "file": "auth/session.py", "action": "add SQLite-backed session store" },
    { "file": "app/main.py", "action": "add login/logout routes" }
  ]
}
```

### workflows/worker-N.json
```json
{
  "worker": "worker-N",
  "status": "editing",
  "file": "x.py",
  "summary": "added API route for login",
  "dependencies": ["y.py"]
}
```

## Dependencies
- Python 3.10+
- Git
- Ollama (for internal chat and coordination)
- Aider (as the primary coding engine)
- Textual (for TUI implementation)
- OpenRouter API (for accessing advanced models)

## Risk Assessment

### Technical Risks
- GPU resource contention
- Model API rate limits or downtime
- File conflicts between agents

### Mitigation Strategies
- Implement queue management for GPU resources
- Add fallback mechanisms for API failures
- Develop conflict resolution protocols for file changes

## Success Criteria
1. Users can define project goals through the TUI
2. Veda successfully orchestrates multiple Aider agents
3. Agents can work in parallel without conflicts
4. Users maintain control throughout the development process
5. The system produces high-quality, working code

## Future Enhancements
- Enhanced AI assistance within the TUI
- Improved project visualization
- Integration with additional coding tools
- Support for more complex project structures
- Performance optimizations for larger codebases
