"""Favorites management for folders."""

import json
import os
from pathlib import Path
from typing import List, Set, Dict


def get_config_file():
    """Get the path to the config.json file (``~/.config/gtk-apps/gtk-photos/``)."""
    from .config_paths import config_file
    return str(config_file())


def load_config():
    """Load configuration from config.json."""
    config_file = get_config_file()
    default_photo_drive = ''
    
    if not os.path.exists(config_file):
        return {'photo_drive': default_photo_drive}
    
    try:
        with open(config_file, 'r') as f:
            config = json.load(f)
            if 'photo_drive' not in config:
                config['photo_drive'] = default_photo_drive
            return config
    except (json.JSONDecodeError, IOError):
        return {'photo_drive': default_photo_drive}


def get_favorites_file() -> str:
    """Get the path to the favorites JSON file."""
    from .config_paths import favorites_file
    return str(favorites_file())


def get_recent_file() -> str:
    """Get the path to the recent folders JSON file."""
    from .config_paths import recent_file
    return str(recent_file())


def load_recent_folders(max_items: int = 10) -> List[Dict[str, str]]:
    """
    Load the list of recently used folders.
    
    Args:
        max_items: Maximum number of recent items to return
        
    Returns:
        List of dicts with 'path' and 'title' keys, most recent first
    """
    recent_file = get_recent_file()
    if not os.path.exists(recent_file):
        return []
    
    try:
        with open(recent_file, 'r') as f:
            data = json.load(f)
            recent = data.get('recent', [])
            # Limit to max_items
            return recent[:max_items]
    except (json.JSONDecodeError, IOError):
        return []


def add_recent_folder(folder_path: str):
    """
    Add a folder to the recent list (moves it to the top if it already exists).
    
    Args:
        folder_path: Path to the folder to add
    """
    folder_path = os.path.normpath(folder_path)
    recent_file = get_recent_file()
    
    # Load existing recent folders
    recent = load_recent_folders(max_items=100)  # Load more to check for duplicates
    
    # Remove if already exists
    recent = [r for r in recent if os.path.normpath(r['path']) != folder_path]
    
    # Add to the beginning
    title = generate_favorite_title(folder_path)
    new_item = {
        'path': folder_path,
        'title': title
    }
    recent.insert(0, new_item)
    
    # Keep only the most recent 20
    recent = recent[:20]
    
    # Save
    try:
        data = {'recent': recent}
        with open(recent_file, 'w') as f:
            json.dump(data, f, indent=2)
    except IOError:
        pass


def generate_favorite_title(folder_path: str, base_path: str = None) -> str:
    """
    Generate a title from folder path by reversing path components.
    
    Args:
        folder_path: Full path to the folder
        base_path: Base path to strip from the folder path (if None, loads from config)
        
    Returns:
        Title string like "VIDEO-LILITH-FEATURES"
    """
    # Load base_path from config if not provided
    if base_path is None:
        config = load_config()
        base_path = config.get('photo_drive', '')
    
    # Get relative path from base
    if folder_path.startswith(base_path):
        relative_path = folder_path[len(base_path):].lstrip('/')
    else:
        # If path doesn't start with base, use the last few components
        parts = folder_path.split(os.sep)
        # Get last 3-4 meaningful parts
        relative_path = os.sep.join(parts[-3:]) if len(parts) > 3 else folder_path
    
    # Split into components and filter out empty strings
    components = [c for c in relative_path.split(os.sep) if c]
    
    # Reverse the components
    components.reverse()
    
    # Uppercase and join with dashes
    title = '-'.join(c.upper() for c in components)
    
    return title


def load_favorites() -> List[Dict[str, str]]:
    """
    Load the list of favorites (with path and title).
    
    Returns:
        List of dicts with 'path' and 'title' keys
    """
    favorites_file = get_favorites_file()
    if not os.path.exists(favorites_file):
        return []
    
    try:
        with open(favorites_file, 'r') as f:
            data = json.load(f)
            # Handle both old format (list of strings) and new format (list of dicts)
            favorites = data.get('favorites', [])
            result = []
            for fav in favorites:
                if isinstance(fav, str):
                    # Old format - convert to new format
                    result.append({
                        'path': fav,
                        'title': generate_favorite_title(fav)
                    })
                else:
                    # New format
                    result.append(fav)
            return result
    except (json.JSONDecodeError, IOError):
        return []


def save_favorites(favorites: List[Dict[str, str]]) -> bool:
    """
    Save the list of favorites (with path and title).
    
    Args:
        favorites: List of dicts with 'path' and 'title' keys
        
    Returns:
        True if successful, False otherwise
    """
    favorites_file = get_favorites_file()
    try:
        # Remove duplicates based on path and sort by title
        seen_paths = set()
        unique_favorites = []
        for fav in favorites:
            if fav['path'] not in seen_paths:
                seen_paths.add(fav['path'])
                unique_favorites.append(fav)
        
        unique_favorites.sort(key=lambda x: x.get('title', ''))
        
        data = {'favorites': unique_favorites}
        with open(favorites_file, 'w') as f:
            json.dump(data, f, indent=2)
        return True
    except IOError:
        return False


def add_favorite(folder_path: str) -> bool:
    """
    Add a folder to favorites.
    
    Args:
        folder_path: Path to the folder to add
        
    Returns:
        True if successful, False otherwise
    """
    favorites = load_favorites()
    # Check if path already exists
    existing_paths = {fav['path'] for fav in favorites}
    if folder_path not in existing_paths:
        title = generate_favorite_title(folder_path)
        favorites.append({
            'path': folder_path,
            'title': title
        })
        return save_favorites(favorites)
    return True


def remove_favorite(folder_path: str) -> bool:
    """
    Remove a folder from favorites.
    
    Args:
        folder_path: Path to the folder to remove
        
    Returns:
        True if successful, False otherwise
    """
    favorites = load_favorites()
    favorites = [fav for fav in favorites if fav['path'] != folder_path]
    return save_favorites(favorites)


def is_favorite(folder_path: str) -> bool:
    """
    Check if a folder is in favorites.
    
    Args:
        folder_path: Path to check
        
    Returns:
        True if the folder is a favorite, False otherwise
    """
    favorites = load_favorites()
    existing_paths = {fav['path'] for fav in favorites}
    return folder_path in existing_paths


def get_folders_in_directory(directory: str) -> List[str]:
    """
    Get all subdirectories in a directory recursively.
    Ignores hidden folders (those starting with a dot).
    
    Args:
        directory: Path to the directory to scan
        
    Returns:
        List of subdirectory paths (all levels, recursively)
    """
    folders = []
    if not os.path.isdir(directory):
        return folders
    
    try:
        # Use os.walk to recursively traverse the directory tree
        for root, dirs, files in os.walk(directory):
            # Filter out hidden directories (starting with .) from dirs list
            # This prevents os.walk from descending into hidden directories
            dirs[:] = [d for d in dirs if not d.startswith('.')]
            
            # Add all directories found at this level (excluding hidden ones)
            for dir_name in dirs:
                dir_path = os.path.join(root, dir_name)
                folders.append(dir_path)
    except (PermissionError, OSError):
        # Handle permission errors and other filesystem errors gracefully
        pass
    
    folders.sort()
    return folders
