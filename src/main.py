import argparse
import threading
import time
import sys
import logging
import webbrowser
from http.server import HTTPServer, BaseHTTPRequestHandler

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

    def start(self):
        with self.lock:
            if self.running:
                logging.info("AgentManager already running.")
                return
            self.running = True
            logging.info("Starting AgentManager...")
            t = threading.Thread(target=self._run_agents, daemon=True)
            t.start()
            self.threads.append(t)

    def _run_agents(self):
        while self.running:
            if self.instances == "auto":
                agent_count = 2  # Placeholder for auto-scaling logic
            else:
                agent_count = self.instances
            logging.info(f"Running {agent_count} agent(s)...")
            time.sleep(5)

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
    while True:
        msg = input("You: ")
        if msg.strip().lower() == "exit":
            print("Exiting chat.")
            break
        print(f"Veda: [simulated response to '{msg}']")

def main():
    parser = argparse.ArgumentParser(description="Veda - Software development that doesn't sleep.")
    subparsers = parser.add_subparsers(dest="command")

    subparsers.add_parser("start", help="Start Veda in the background.")
    set_parser = subparsers.add_parser("set", help="Set configuration options.")
    set_parser.add_argument("option", choices=["instances"])
    set_parser.add_argument("value")
    subparsers.add_parser("chat", help="Chat with Veda.")
    subparsers.add_parser("web", help="Open the Veda web interface.")

    args = parser.parse_args()
    manager = AgentManager()

    if args.command == "start":
        manager.start()
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
