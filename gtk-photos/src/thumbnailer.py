"""Thumbnail generation for images and videos."""

import os
import subprocess
import tempfile
from pathlib import Path
from gi.repository import GdkPixbuf, GLib


THUMBNAIL_SIZE = 200  # Size in pixels for thumbnails


def _get_video_duration_seconds(filepath: str):
    """Return duration in seconds, or None if unknown."""
    try:
        cmd = [
            'ffprobe', '-v', 'error', '-show_entries', 'format=duration',
            '-of', 'default=noprint_wrappers=1:nokey=1', filepath,
        ]
        r = subprocess.run(
            cmd, capture_output=True, text=True, timeout=15
        )
        if r.returncode == 0 and r.stdout.strip():
            d = float(r.stdout.strip())
            if d > 0:
                return d
    except (ValueError, subprocess.TimeoutExpired, OSError):
        pass
    return None


def generate_image_thumbnail(filepath: str, size: int = THUMBNAIL_SIZE) -> GdkPixbuf.Pixbuf:
    """
    Generate a thumbnail for an image file.
    
    Args:
        filepath: Path to the image file
        size: Desired thumbnail size in pixels (default: 200)
        
    Returns:
        GdkPixbuf.Pixbuf object with the thumbnail, or None on error
    """
    try:
        pixbuf = GdkPixbuf.Pixbuf.new_from_file(filepath)
        
        # Calculate scaling to maintain aspect ratio
        width = pixbuf.get_width()
        height = pixbuf.get_height()
        
        if width > height:
            new_width = size
            new_height = int((height * size) / width)
        else:
            new_height = size
            new_width = int((width * size) / height)
        
        # Scale the pixbuf
        scaled = pixbuf.scale_simple(new_width, new_height, GdkPixbuf.InterpType.BILINEAR)
        return scaled
    except Exception as e:
        print(f"Error generating thumbnail for {filepath}: {e}")
        return None


def generate_video_thumbnail(filepath: str, size: int = THUMBNAIL_SIZE) -> GdkPixbuf.Pixbuf:
    """
    Generate a thumbnail for a video file by extracting one frame near the middle
    (duration from ffprobe; fallbacks: 1s, then the first frame).
    
    Args:
        filepath: Path to the video file
        size: Desired thumbnail size in pixels (default: 200)
        
    Returns:
        GdkPixbuf.Pixbuf object with the thumbnail, or None on error
    """
    try:
        # Use ffmpeg to extract a single frame, ideally near the middle of the clip
        with tempfile.NamedTemporaryFile(suffix='.jpg', delete=False) as tmp_file:
            tmp_path = tmp_file.name
        
        try:
            duration = _get_video_duration_seconds(filepath)
            if duration is not None and duration > 0:
                # Slightly before exact half avoids edge glitches; stay inside the clip
                seek_s = min(duration * 0.5, max(0.0, duration - 0.01))
            else:
                seek_s = 1.0

            def run_ffmpeg(ss_seconds=None):
                cmd = ['ffmpeg', '-hide_banner', '-loglevel', 'error']
                if ss_seconds is not None:
                    # Input seeking: fast, good enough for mid-video thumbnails
                    cmd.extend(['-ss', f'{ss_seconds:.3f}'])
                cmd.extend(
                    [
                        '-i', filepath,
                        '-vframes', '1',
                        '-vf', f'scale={size}:-1',
                        '-y',
                        tmp_path,
                    ]
                )
                return subprocess.run(
                    cmd,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    timeout=30,
                )

            # 1) Middle of video (or 1s when duration unknown)
            result = run_ffmpeg(seek_s)
            # 2) 1s in (e.g. middle seek landed on a bad/empty frame)
            if result.returncode != 0 and seek_s != 1.0:
                result = run_ffmpeg(1.0)
            # 3) First decodable frame
            if result.returncode != 0:
                result = run_ffmpeg(None)
            
            if (
                result.returncode == 0
                and os.path.exists(tmp_path)
                and os.path.getsize(tmp_path) > 0
            ):
                pixbuf = GdkPixbuf.Pixbuf.new_from_file(tmp_path)
                os.unlink(tmp_path)  # Clean up temp file
                return pixbuf
            else:
                if os.path.exists(tmp_path):
                    os.unlink(tmp_path)
                return None
                
        except subprocess.TimeoutExpired:
            if os.path.exists(tmp_path):
                os.unlink(tmp_path)
            return None
        except Exception as e:
            if os.path.exists(tmp_path):
                os.unlink(tmp_path)
            print(f"Error generating video thumbnail for {filepath}: {e}")
            return None
            
    except Exception as e:
        print(f"Error generating video thumbnail for {filepath}: {e}")
        return None


def _pixbuf_save_options(ext: str) -> tuple[str, list[str], list[str]]:
    ext = ext.lower()
    if ext in (".jpg", ".jpeg"):
        return "jpeg", ["quality"], ["95"]
    if ext == ".png":
        return "png", [], []
    if ext == ".webp":
        return "webp", [], []
    if ext == ".bmp":
        return "bmp", [], []
    if ext in (".tiff", ".tif"):
        return "tiff", [], []
    if ext == ".gif":
        return "gif", [], []
    return "", [], []


def _rotate_image_file_convert(filepath: str, clockwise: bool) -> bool:
    """Fallback rotation using ImageMagick when GdkPixbuf cannot save the format."""
    angle = "-90" if clockwise else "90"
    try:
        result = subprocess.run(
            ["convert", filepath, "-rotate", angle, filepath],
            capture_output=True,
            timeout=60,
        )
        return result.returncode == 0
    except (FileNotFoundError, subprocess.TimeoutExpired, OSError):
        return False


def rotate_image_file(filepath: str, clockwise: bool) -> bool:
    """
    Rotate an image file 90° on disk.

    Args:
        filepath: Path to the image file
        clockwise: True for right (clockwise), False for left (counter-clockwise)

    Returns:
        True if the file was rotated successfully
    """
    filepath = os.path.abspath(filepath)
    if not os.path.isfile(filepath):
        return False

    try:
        pixbuf = GdkPixbuf.Pixbuf.new_from_file(filepath)
    except GLib.Error as e:
        print(f"Error loading image for rotation {filepath}: {e}")
        return _rotate_image_file_convert(filepath, clockwise)

    rotation = (
        GdkPixbuf.PixbufRotation.CLOCKWISE
        if clockwise
        else GdkPixbuf.PixbufRotation.COUNTERCLOCKWISE
    )
    rotated = pixbuf.rotate_simple(rotation)

    ext = os.path.splitext(filepath)[1]
    save_type, keys, values = _pixbuf_save_options(ext)
    if not save_type:
        return _rotate_image_file_convert(filepath, clockwise)

    tmp_fd, tmp_path = tempfile.mkstemp(
        suffix=ext, dir=os.path.dirname(filepath) or "."
    )
    os.close(tmp_fd)
    try:
        rotated.savev(tmp_path, save_type, keys, values)
        os.replace(tmp_path, filepath)
        return True
    except GLib.Error:
        if os.path.exists(tmp_path):
            os.unlink(tmp_path)
        return _rotate_image_file_convert(filepath, clockwise)
    except OSError as e:
        if os.path.exists(tmp_path):
            os.unlink(tmp_path)
        print(f"Error saving rotated image {filepath}: {e}")
        return False


def create_placeholder_thumbnail(size: int = THUMBNAIL_SIZE) -> GdkPixbuf.Pixbuf:
    """
    Create a placeholder thumbnail when actual thumbnail generation fails.
    
    Args:
        size: Size of the placeholder in pixels
        
    Returns:
        GdkPixbuf.Pixbuf with a gray placeholder
    """
    pixbuf = GdkPixbuf.Pixbuf.new(
        GdkPixbuf.Colorspace.RGB,
        False,
        8,
        size,
        size
    )
    # Fill with light gray
    pixbuf.fill(0xCCCCCCCC)
    return pixbuf
