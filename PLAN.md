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

## Technical Dependencies
- Python 3.10+
- Git
- Ollama (for internal chat and coordination)
- Aider (as the primary coding engine)
- OpenRouter API (for accessing advanced models)

## Workflow Implementation
1. **User Goal Prompting**
   - Prompt user for natural-language goal
   - Parse mentioned files
   - Structure into goal.prompt

2. **File Awareness / Context Injection**
   - Load mentioned files automatically
   - Store file contents alongside goal

3. **Planning Phase**
   - Use DeepCoder:14B via Ollama
   - Generate technical plan as JSON

4. **Aider Worker Spawning**
   - Spawn up to 4 parallel Aider agents
   - Assign tasks based on plan
   - Track progress in workflow JSON files

5. **Aider Response UX**
   - Use Gemma3:12B for answering prompts
   - Cache prompts for efficiency
   - Continue silently when appropriate

6. **User Interaction**
   - Provide terminal-like tabs for each worker
   - Enable chat and command functionality
   - Display agent overview on home screen

7. **GPU Queue Management**
   - Ensure only one Ollama GPU job runs at a time
   - Maintain queue for GPU-bound jobs
   - Have workers wait for GPU availability

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
