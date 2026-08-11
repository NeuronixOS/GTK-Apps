#!/usr/bin/env python3
"""One-time script to add ALL folders recursively from Photos drive to favorites.json"""

import os
import json
import sys
from pathlib import Path

# Add src to path to import favorites / config_paths
sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(__file__)), 'src'))

from favorites import (
    get_favorites_file,
    load_favorites,
    save_favorites,
    generate_favorite_title,
    load_config,
)


config = load_config()
PHOTO_DRIVE = config.get('photo_drive', '')


def get_all_folders_recursive(base_path):
    """Recursively get all folders under a base path."""
    folders = []
    if not os.path.isdir(base_path):
        return folders
    
    try:
        # Use os.walk to recursively traverse the directory tree
        for root, dirs, files in os.walk(base_path):
            # Add all directories found at this level
            for dir_name in dirs:
                dir_path = os.path.join(root, dir_name)
                folders.append(dir_path)
    except (PermissionError, OSError) as e:
        print(f"Warning: Could not access {base_path}: {e}")
    
    return folders


def main():
    """Main function to update favorites.json with all folders."""
    favorites_file = get_favorites_file()
    backup_file = os.path.join(os.path.dirname(favorites_file), 'favorites copy.json')
    
    # Create backup if it doesn't exist
    if os.path.exists(favorites_file) and not os.path.exists(backup_file):
        import shutil
        shutil.copy2(favorites_file, backup_file)
        print(f"Created backup: {backup_file}")
    
    print(f"Scanning {PHOTO_DRIVE} recursively...")
    if not os.path.isdir(PHOTO_DRIVE):
        print(f"Error: {PHOTO_DRIVE} does not exist!")
        sys.exit(1)
    
    # Get all folders recursively
    all_folders = get_all_folders_recursive(PHOTO_DRIVE)
    print(f"Found {len(all_folders)} folders")
    
    # Load existing favorites
    print("Loading existing favorites...")
    existing_favorites = load_favorites()
    existing_paths = {fav['path'] for fav in existing_favorites}
    print(f"Found {len(existing_favorites)} existing favorites")
    
    # Create new favorites list
    new_favorites = []
    added_count = 0
    skipped_count = 0
    
    print("\nProcessing folders...")
    for folder_path in all_folders:
        if folder_path in existing_paths:
            # Keep existing favorite
            existing_fav = next((f for f in existing_favorites if f['path'] == folder_path), None)
            if existing_fav:
                new_favorites.append(existing_fav)
            skipped_count += 1
        else:
            # Add new favorite
            title = generate_favorite_title(folder_path)
            new_favorites.append({
                'path': folder_path,
                'title': title
            })
            added_count += 1
            if added_count % 50 == 0:
                print(f"  Processed {added_count} new folders...")
    
    print(f"\nSummary:")
    print(f"  Existing favorites kept: {skipped_count}")
    print(f"  New folders added: {added_count}")
    print(f"  Total favorites: {len(new_favorites)}")
    
    # Save favorites
    print(f"\nSaving to {favorites_file}...")
    if save_favorites(new_favorites):
        print("Success! favorites.json has been updated.")
        print(f"Backup saved at: {backup_file}")
    else:
        print("Error: Failed to save favorites.json")
        sys.exit(1)


if __name__ == '__main__':
    main()
