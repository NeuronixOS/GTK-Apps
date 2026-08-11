#!/usr/bin/env python3
"""Script to recursively search Photos drive and update file dates in folders with no files newer than 01/01/26"""

import os
import sys
import json
import argparse
from pathlib import Path
from datetime import datetime


def get_config_file():
    """Get the path to the config.json file (``~/.config/gtk-apps/gtk-photos/``)."""
    script_dir = os.path.dirname(os.path.abspath(__file__))
    src_dir = os.path.join(os.path.dirname(script_dir), 'src')
    if src_dir not in sys.path:
        sys.path.insert(0, src_dir)
    from config_paths import config_file
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


config = load_config()
PHOTO_DRIVE = config.get('photo_drive', '')
CUTOFF_DATE = datetime(2026, 1, 11)  # 01/01/26
TARGET_DATE = datetime(2020, 1, 1)  # 01/01/20


def get_newest_file_date(folder_path):
    """Get the newest modified or created date of any file in the folder."""
    newest_date = None
    try:
        for root, dirs, files in os.walk(folder_path):
            for file_name in files:
                file_path = os.path.join(root, file_name)
                try:
                    # Get both modification time and change time (creation-like on Linux)
                    mtime = os.path.getmtime(file_path)
                    ctime = os.path.getctime(file_path)
                    # Use the newer of the two
                    file_date = datetime.fromtimestamp(max(mtime, ctime))
                    if newest_date is None or file_date > newest_date:
                        newest_date = file_date
                except (OSError, ValueError) as e:
                    # Skip files we can't access or get time for
                    continue
    except (PermissionError, OSError) as e:
        print(f"Warning: Could not access {folder_path}: {e}")
        return None
    
    return newest_date


def has_files_newer_than_cutoff(folder_path, cutoff_date):
    """Check if folder contains any files newer than cutoff_date."""
    newest_date = get_newest_file_date(folder_path)
    if newest_date is None:
        return True  # Assume has newer files to be safe
    return newest_date > cutoff_date


def update_file_dates(folder_path, target_date, dry_run=False):
    """Update modification and change times of all files in folder to target_date."""
    updated_count = 0
    target_timestamp = target_date.timestamp()
    
    try:
        for root, dirs, files in os.walk(folder_path):
            for file_name in files:
                file_path = os.path.join(root, file_name)
                try:
                    if not dry_run:
                        # Update both mtime (modification) and atime (access)
                        # Note: ctime cannot be changed on Linux, but mtime is what matters
                        os.utime(file_path, (target_timestamp, target_timestamp))
                    updated_count += 1
                except (OSError, PermissionError) as e:
                    print(f"  Warning: Could not update {file_path}: {e}")
    except (PermissionError, OSError) as e:
        print(f"Warning: Could not process {folder_path}: {e}")
    
    return updated_count


def main():
    """Main function to process folders and update dates."""
    parser = argparse.ArgumentParser(
        description='Update file dates in folders with no files newer than 01/01/26'
    )
    parser.add_argument(
        '--dry-run',
        action='store_true',
        help='Show what would be changed without actually modifying files'
    )
    args = parser.parse_args()
    
    if not os.path.isdir(PHOTO_DRIVE):
        print(f"Error: {PHOTO_DRIVE} does not exist!")
        sys.exit(1)
    
    print(f"Scanning {PHOTO_DRIVE} recursively...")
    print(f"Cutoff date: {CUTOFF_DATE.strftime('%m/%d/%y')}")
    print(f"Target date for old folders: {TARGET_DATE.strftime('%m/%d/%y')}")
    if args.dry_run:
        print("DRY RUN MODE - No files will be modified")
    print()
    
    folders_to_update = {}  # {folder_path: newest_date}
    folders_kept = {}  # {folder_path: newest_date}
    
    # First pass: identify folders that need updating
    print("Scanning folders...")
    try:
        for root, dirs, files in os.walk(PHOTO_DRIVE):
            folder_path = root
            newest_date = get_newest_file_date(folder_path)
            
            if newest_date is None:
                # Skip folders with no accessible files
                continue
            
            if newest_date > CUTOFF_DATE:
                folders_kept[folder_path] = newest_date
            else:
                # Check if folder has any files at all
                has_files = False
                try:
                    for item in os.listdir(folder_path):
                        item_path = os.path.join(folder_path, item)
                        if os.path.isfile(item_path):
                            has_files = True
                            break
                except (OSError, PermissionError):
                    pass
                
                if has_files:
                    folders_to_update[folder_path] = newest_date
    except KeyboardInterrupt:
        print("\n\nInterrupted by user")
        sys.exit(1)
    except Exception as e:
        print(f"\nError during scan: {e}")
        sys.exit(1)
    
    print(f"\n{'='*70}")
    print(f"Folders that will STAY THE SAME ({len(folders_kept)} folders):")
    print(f"{'='*70}")
    if folders_kept:
        for folder_path, newest_date in sorted(folders_kept.items()):
            date_str = newest_date.strftime('%m/%d/%Y %H:%M:%S') if newest_date else 'N/A'
            print(f"  {folder_path}")
            print(f"    Newest file date: {date_str}")
    else:
        print("  (none)")
    
    print(f"\n{'='*70}")
    print(f"Folders that WON'T STAY THE SAME - will be updated ({len(folders_to_update)} folders):")
    print(f"{'='*70}")
    if folders_to_update:
        for folder_path, newest_date in sorted(folders_to_update.items()):
            date_str = newest_date.strftime('%m/%d/%Y %H:%M:%S') if newest_date else 'N/A'
            print(f"  {folder_path}")
            print(f"    Newest file date: {date_str}")
    else:
        print("  (none)")
    
    if not folders_to_update:
        print("\nNo folders need updating!")
        return
    
    # Second pass: update file dates
    print(f"\n{'[DRY RUN] ' if args.dry_run else ''}Updating file dates...")
    total_files_updated = 0
    
    for folder_path in folders_to_update.keys():
        print(f"Processing: {folder_path}")
        file_count = update_file_dates(folder_path, TARGET_DATE, dry_run=args.dry_run)
        total_files_updated += file_count
        print(f"  {'Would update' if args.dry_run else 'Updated'} {file_count} files")
    
    print(f"\n{'[DRY RUN] ' if args.dry_run else ''}Summary:")
    print(f"  Folders processed: {len(folders_to_update)}")
    print(f"  Total files {'would be updated' if args.dry_run else 'updated'}: {total_files_updated}")
    
    if args.dry_run:
        print("\nRun without --dry-run to apply changes")


if __name__ == '__main__':
    main()
