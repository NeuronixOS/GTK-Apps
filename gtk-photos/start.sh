#!/bin/bash
# Launch the Photo Organizer (gtk-photos)

# Get the directory where this script is located
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"

# Change to the script directory
cd "$SCRIPT_DIR"

# Set Python path to include the project directory
export PYTHONPATH="$SCRIPT_DIR:$PYTHONPATH"
export PYTHONUNBUFFERED=1

# Run the application
python3 -m src.main
