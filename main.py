import sys
from pathlib import Path

# Ensure the src directory is in the Python path
project_root = Path(__file__).parent
src_path = project_root / "src"
sys.path.insert(0, str(src_path))

from config import load_config
from tui import VedaApp


def main():
    """Loads configuration and runs the Veda TUI application."""
    config_path = project_root / "config.yaml"
    config = load_config(config_path)
    app = VedaApp(config=config)
    app.run()


if __name__ == "__main__":
    main()
