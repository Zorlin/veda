import argparse
import threading
import time
import sys
import logging
import webbrowser
import os
import json
from http.server import HTTPServer, BaseHTTPRequestHandler

import sys
import os

# Allow running as "python src/main.py" from project root and finding src/constants.py
sys.path.insert(0, os.path.abspath(os.path.dirname(__file__)))

from constants import OLLAMA_URL, VEDA_CHAT_MODEL, ROLE_MODELS, MCP_URL, POSTGRES_DSN, HANDOFF_DIR

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s [%(levelname)s] %(message)s"
)

class AgentManager:
    def __init__(self):
        self.instances = "auto"
        self.running = False
        self.lock = threading.Lock()
        self.threads = []
        self.handoff_dir = HANDOFF_DIR
        os.makedirs(self.handoff_dir, exist_ok=True)
        self.active_roles = {}

    def set_instances(self, value):
        with self.lock:
            if value == "auto":
                self.instances = "auto"
                logging.info("Agent instance management set to auto.")
            else:
                try:
                    count = int(value)
                    if count < 1:
                        raise ValueError
                    self.instances = count
                    logging.info(f"Agent instances set to {count}.")
                except ValueError:
                    logging.error("Invalid instance count. Must be a positive integer or 'auto'.")

    def start(self, initial_prompt=None):
        with self.lock:
            if self.running:
                logging.info("AgentManager already running.")
                return
            self.running = True
            logging.info("Starting AgentManager...")
            t = threading.Thread(target=self._run_agents, args=(initial_prompt,), daemon=True)
            t.start()
            self.threads.append(t)

    def _run_agents(self, initial_prompt=None):
        # Start the main coordinator agent with the initial prompt
        if initial_prompt:
            self._start_role_agent("coordinator", initial_prompt)
        else:
            self._start_role_agent("coordinator", "No prompt provided.")

        while self.running:
            if self.instances == "auto":
                agent_count = 2  # Placeholder for auto-scaling logic
            else:
                agent_count = self.instances
            logging.info(f"Running {agent_count} agent(s)...")
            # Monitor handoff files and spawn/route as needed
            self._process_handoffs()
            time.sleep(5)

    def _start_role_agent(self, role, prompt, handoff=None):
        # Simulate starting an agent for a role with a prompt
        if role in self.active_roles:
            logging.info(f"{role.capitalize()} agent already running.")
            return
        t = threading.Thread(target=self._role_agent_thread, args=(role, prompt, handoff), daemon=True)
        t.start()
        self.active_roles[role] = t
        logging.info(f"Started {role} agent with model {ROLE_MODELS.get(role, VEDA_CHAT_MODEL)}.")

    def _role_agent_thread(self, role, prompt, handoff):
        # Simulate agent work and handoff
        logging.info(f"[{role.upper()}] Model: {ROLE_MODELS.get(role, VEDA_CHAT_MODEL)} | Prompt: {prompt}")

        # Coordinator should ask clarifying questions if the prompt is too vague
        if role == "coordinator":
            # If the prompt is too short, ask for more details
            if len(prompt.strip()) < 20 or prompt.strip().lower() in ["hey veda.", "hi", "hello"]:
                print("\nVeda: Could you please provide more details about what you want to build or change? "
                      "For example, describe the type of project, its purpose, or any specific features you want.")
                # Try to get more input from the user if running interactively
                if sys.stdin.isatty():
                    try:
                        user_input = input("You: ")
                        if user_input.strip():
                            prompt = user_input.strip()
                    except EOFError:
                        pass
            # After clarification, hand off to architect
            self._create_handoff("architect", f"Design the system for: {prompt}")
        elif role == "architect":
            # Architect should ask for missing requirements if prompt is still vague
            if len(prompt.strip()) < 30:
                print("\nArchitect: Can you specify any technical requirements, preferred stack, or constraints?")
                if sys.stdin.isatty():
                    try:
                        user_input = input("You: ")
                        if user_input.strip():
                            prompt = prompt + " " + user_input.strip()
                    except EOFError:
                        pass
            self._create_handoff("developer", f"Implement the plan for: {prompt}")
        # You can add more role logic here for planner, engineer, infra engineer, etc.
        logging.info(f"[{role.upper()}] Finished and handed off.")

    def _create_handoff(self, next_role, message):
        handoff_file = os.path.join(self.handoff_dir, f"{next_role}_handoff.json")
        with open(handoff_file, "w") as f:
            json.dump({"role": next_role, "message": message}, f)
        logging.info(f"Created handoff for {next_role}.")

    def _process_handoffs(self):
        for fname in os.listdir(self.handoff_dir):
            if fname.endswith("_handoff.json"):
                path = os.path.join(self.handoff_dir, fname)
                try:
                    with open(path, "r") as f:
                        data = json.load(f)
                    role = data.get("role")
                    message = data.get("message")
                    if role and message:
                        self._start_role_agent(role, message)
                    os.remove(path)
                except Exception as e:
                    logging.error(f"Error processing handoff {fname}: {e}")

    def stop(self):
        with self.lock:
            self.running = False
            logging.info("Stopping AgentManager...")
            for t in self.threads:
                t.join(timeout=1)

class SimpleWebHandler(BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.send_header("Content-type", "text/html")
        self.end_headers()
        self.wfile.write(b"<html><head><title>Veda Web UI</title></head><body><h1>Veda Web Interface</h1><p>Status: Running</p></body></html>")

def start_web_server():
    server_address = ('', 9900)
    httpd = HTTPServer(server_address, SimpleWebHandler)
    logging.info("Web server started at http://localhost:9900")
    threading.Thread(target=httpd.serve_forever, daemon=True).start()

def chat_interface():
    print("Welcome to Veda chat. Type 'exit' to quit.")
    print("Connecting to Ollama at", OLLAMA_URL)
    system_prompt = (
        "You are Veda, an advanced AI orchestrator for software development. "
        "You coordinate multiple specialized AI agents (architect, planner, developer, engineer, infra engineer, etc) "
        "and personalities (theorist, architect, skeptic, historian, coordinator) to collaboratively build, improve, "
        "and maintain software projects. You use a common knowledge base (Postgres for deep knowledge, RAG via MCP server) "
        "and JSON files for inter-agent handoff. Your job is to understand the user's goals and break them down for your agents. "
        "Ask the user what they want to build or change, then coordinate the agents accordingly."
    )
    try:
        import requests
    except ImportError:
        print("Please install 'requests' to use the chat interface.")
        return

    def ollama_chat(messages):
        # Use Ollama's /api/chat endpoint
        url = f"{OLLAMA_URL}/api/chat"
        payload = {
            "model": VEDA_CHAT_MODEL,
            "messages": messages,
            "stream": False
        }
        try:
            resp = requests.post(url, json=payload, timeout=60)
            resp.raise_for_status()
            data = resp.json()
            return data.get("message", {}).get("content", "[No response]")
        except Exception as e:
            return f"[Error communicating with Ollama: {e}]"

    messages = [
        {"role": "system", "content": system_prompt},
    ]
    while True:
        msg = input("You: ")
        if msg.strip().lower() == "exit":
            print("Exiting chat.")
            break
        messages.append({"role": "user", "content": msg})
        print("Veda (thinking)...")
        response = ollama_chat(messages)
        print(f"Veda: {response}")
        messages.append({"role": "assistant", "content": response})

def main():
    parser = argparse.ArgumentParser(description="Veda - Software development that doesn't sleep.")
    subparsers = parser.add_subparsers(dest="command")

    start_parser = subparsers.add_parser("start", help="Start Veda in the background.")
    start_parser.add_argument("--prompt", help="Initial project prompt (if not provided, Veda will ask you).")
    set_parser = subparsers.add_parser("set", help="Set configuration options.")
    set_parser.add_argument("option", choices=["instances"])
    set_parser.add_argument("value")
    subparsers.add_parser("chat", help="Chat with Veda.")
    subparsers.add_parser("web", help="Open the Veda web interface.")

    args = parser.parse_args()
    manager = AgentManager()

    if args.command == "start":
        initial_prompt = args.prompt
        # If running in a non-interactive environment (like pytest), skip prompt
        if not initial_prompt:
            if not sys.stdin.isatty():
                initial_prompt = "Automated test run: default project prompt."
            else:
                print("No prompt provided. Let's chat to define your project goal.")
                # Use the chat interface to get a prompt from the user
                system_prompt = (
                    "You are Veda, an advanced AI orchestrator for software development. "
                    "You coordinate multiple specialized AI agents (architect, planner, developer, engineer, infra engineer, etc) "
                    "and personalities (theorist, architect, skeptic, historian, coordinator) to collaboratively build, improve, "
                    "and maintain software projects. You use a common knowledge base (Postgres for deep knowledge, RAG via MCP server) "
                    "and JSON files for inter-agent handoff. Your job is to understand the user's goals and break them down for your agents. "
                    "Ask the user what they want to build or change, then coordinate the agents accordingly."
                )
                try:
                    import requests
                except ImportError:
                    print("Please install 'requests' to use the chat interface.")
                    sys.exit(1)
                messages = [
                    {"role": "system", "content": system_prompt},
                ]
                while True:
                    msg = input("You: ")
                    if msg.strip().lower() == "exit":
                        print("Exiting.")
                        sys.exit(0)
                    messages.append({"role": "user", "content": msg})
                    print("Veda (thinking)...")
                    url = f"{OLLAMA_URL}/api/chat"
                    payload = {
                        "model": VEDA_CHAT_MODEL,
                        "messages": messages,
                        "stream": False
                    }
                    try:
                        resp = requests.post(url, json=payload, timeout=60)
                        resp.raise_for_status()
                        data = resp.json()
                        response = data.get("message", {}).get("content", "[No response]")
                    except Exception as e:
                        response = f"[Error communicating with Ollama: {e}]"
                    print(f"Veda: {response}")
                    messages.append({"role": "assistant", "content": response})
                    # Accept the first user message as the project prompt
                    if len(messages) > 2:
                        initial_prompt = msg
                        break
        manager.start(initial_prompt=initial_prompt)
        start_web_server()
        print("Veda is running in the background.")
        print("Open http://localhost:9900 in your browser for the web interface.")
        try:
            while True:
                time.sleep(1)
        except KeyboardInterrupt:
            print("\nShutting down Veda...")
            manager.stop()
    elif args.command == "set":
        if args.option == "instances":
            manager.set_instances(args.value)
    elif args.command == "chat":
        chat_interface()
    elif args.command == "web":
        start_web_server()
        webbrowser.open("http://localhost:9900")
    else:
        parser.print_help()
        print("\nExamples:")
        print("  veda start")
        print("  veda set instances 10")
        print("  veda set instances auto")
        print("  veda chat")
        print("  veda web")

if __name__ == "__main__":
    main()
