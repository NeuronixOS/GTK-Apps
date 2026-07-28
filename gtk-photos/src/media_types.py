"""File type detection and categorization utilities."""

import os
from pathlib import Path
from typing import List, Tuple

# Image file extensions
IMAGE_EXTENSIONS = {
    '.jpg', '.jpeg', '.png', '.gif', '.webp', '.bmp', '.svg', '.ico',
    '.tiff', '.tif', '.heic', '.heif'
}

# Video file extensions (.mkv excluded — not shown in the app)
VIDEO_EXTENSIONS = {
    '.mp4', '.avi', '.webm', '.mov', '.m4v', '.flv', '.wmv',
    '.mpg', '.mpeg', '.3gp', '.ogv', '.mts', '.m2ts'
}


def is_image_file(filepath: str) -> bool:
    """Check if a file is an image based on its extension."""
    ext = Path(filepath).suffix.lower()
    return ext in IMAGE_EXTENSIONS


def is_video_file(filepath: str) -> bool:
    """Check if a file is a video based on its extension."""
    ext = Path(filepath).suffix.lower()
    return ext in VIDEO_EXTENSIONS


def get_media_files(directory: str) -> Tuple[List[str], List[str]]:
    """
    Scan a directory and return separate lists of image and video files.
    
    Args:
        directory: Path to the directory to scan
        
    Returns:
        Tuple of (image_files, video_files)
    """
    image_files = []
    video_files = []
    
    if not os.path.isdir(directory):
        return image_files, video_files
    
    try:
        for root, dirs, files in os.walk(directory):
            for file in files:
                filepath = os.path.join(root, file)
                if is_image_file(filepath):
                    image_files.append(filepath)
                elif is_video_file(filepath):
                    video_files.append(filepath)
    except PermissionError:
        # Handle permission errors gracefully
        pass
    
    # Sort files alphabetically
    image_files.sort()
    video_files.sort()
    
    return image_files, video_files


def get_all_media_files_sorted_by_date(directory: str) -> List[Tuple[str, bool]]:
    """
    Get all media files (images and videos) sorted by modification time (most recent first).
    
    Args:
        directory: Path to the directory to scan
        
    Returns:
        List of tuples (filepath, is_video) sorted by modification time (newest first)
    """
    all_files = []
    
    if not os.path.isdir(directory):
        return all_files
    
    try:
        for root, dirs, files in os.walk(directory):
            for file in files:
                filepath = os.path.join(root, file)
                try:
                    if is_image_file(filepath):
                        mtime = os.path.getmtime(filepath)
                        all_files.append((filepath, False, mtime))
                    elif is_video_file(filepath):
                        mtime = os.path.getmtime(filepath)
                        all_files.append((filepath, True, mtime))
                except OSError:
                    # Skip files we can't access
                    continue
    except PermissionError:
        pass
    
    # Sort by modification time (most recent first)
    all_files.sort(key=lambda x: x[2], reverse=True)
    
    # Return list of (filepath, is_video) tuples
    return [(fp, is_vid) for fp, is_vid, _ in all_files]


def newest_media_in_folder(folder_path: str) -> Tuple[str, bool, float] | None:
    """
    Find the newest image or video under folder_path by modification time.

    Returns:
        (filepath, is_video, mtime) or None if the folder has no media.
    """
    folder_path = os.path.normpath(folder_path)
    newest_path: str | None = None
    newest_is_video = False
    newest_mtime = -1.0

    try:
        for root, dirs, files in os.walk(folder_path):
            dirs[:] = [d for d in dirs if not d.startswith('.')]
            for file in files:
                filepath = os.path.join(root, file)
                if is_image_file(filepath):
                    is_video = False
                elif is_video_file(filepath):
                    is_video = True
                else:
                    continue
                try:
                    mtime = os.path.getmtime(filepath)
                except OSError:
                    continue
                if mtime > newest_mtime:
                    newest_mtime = mtime
                    newest_path = filepath
                    newest_is_video = is_video
    except (OSError, PermissionError):
        pass

    if newest_path is None:
        return None
    return newest_path, newest_is_video, newest_mtime


def get_folder_previews(photo_drive: str) -> List[Tuple[str, str, bool]]:
    """
    For each subdirectory under photo_drive, return one sample media file if any exists.

    Returns:
        List of (folder_path, sample_filepath, is_video), sorted by newest file
        mtime in each folder (most recently updated folders first).
    """
    from .favorites import get_folders_in_directory

    results: List[Tuple[str, str, bool, float]] = []
    if not photo_drive or not os.path.isdir(photo_drive):
        return results

    for folder in get_folders_in_directory(photo_drive):
        sample = newest_media_in_folder(folder)
        if sample:
            filepath, is_video, mtime = sample
            results.append((folder, filepath, is_video, mtime))

    results.sort(key=lambda x: x[3], reverse=True)
    return [(folder, filepath, is_video) for folder, filepath, is_video, _ in results]
