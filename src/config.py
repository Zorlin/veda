from pathlib import Path
import yaml


def load_config(config_path: Path) -> dict:
    """Loads the YAML configuration file.

    Args:
        config_path: The path to the config.yaml file.

    Returns:
        A dictionary containing the configuration.

    Raises:
        FileNotFoundError: If the config file doesn't exist.
        yaml.YAMLError: If there's an error parsing the YAML.
    """
    if not config_path.is_file():
        raise FileNotFoundError(f"Configuration file not found: {config_path}")

    try:
        with open(config_path, "r") as f:
            config = yaml.safe_load(f)
        return config if config else {}
    except yaml.YAMLError as e:
        raise yaml.YAMLError(f"Error parsing configuration file {config_path}: {e}")
    except Exception as e:
        # Catch other potential file reading errors
        raise IOError(f"Error reading configuration file {config_path}: {e}")

# Example of accessing config (optional, for demonstration)
# if __name__ == "__main__":
#     try:
#         # Assuming config.yaml is in the parent directory relative to src/
#         project_root = Path(__file__).parent.parent
#         cfg_path = project_root / "config.yaml"
#         cfg = load_config(cfg_path)
#         print("Config loaded successfully:")
#         import json
#         print(json.dumps(cfg, indent=2))
#         print(f"\nOllama Model: {cfg.get('ollama_model', 'Not Set')}")
#     except (FileNotFoundError, yaml.YAMLError, IOError) as e:
#         print(f"Error: {e}")
