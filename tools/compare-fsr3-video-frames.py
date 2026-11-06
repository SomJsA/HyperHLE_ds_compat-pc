#!/usr/bin/env python3
"""Build reproducible quality comparisons from the Android FSR video matrix.

This script deliberately compares captured display frames. It does not measure
input-to-photon latency and it does not infer temporal quality from one frame.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import statistics
import subprocess
from dataclasses import dataclass
from pathlib import Path

import cv2
import numpy as np
from PIL import Image, ImageDraw, ImageFont
from skimage.color import rgb2lab
from skimage.metrics import peak_signal_noise_ratio, structural_similarity


@dataclass(frozen=True)
class Profile:
    key: str
    label: str
    short: str
    input_size: str
    surface: str
    mode: str
    scale: str
    fps: str
    one_percent_low: str
    minimum_fps: str
    frame_ms: str
    pss_gb: str
    video: Path


@dataclass(frozen=True)
class Roi:
    key: str
    label: str
    box: tuple[int, int, int, int]
    kind: str


PROFILE_META = {
    "01-native-scale1": ("原生 1×", "N1"),
    "02-native-scale2": ("原生 2×", "N2"),
    "03-native-scale3": ("原生 3×", "N3"),
    "04-sync-fsr-scale2": ("同步 FSR 2×", "SF2"),
    "05-async-fsr-scale1": ("异步 FSR 1×", "AF1"),
    "06-async-fsr-scale2": ("异步 FSR 2×", "AF2"),
    "07-async-fsr-scale3": ("异步 FSR 3×", "AF3"),
    "08-async-direct-scale2": ("异步直写 2×", "AD2"),
    "09-async-direct-scale3": ("异步直写 3×", "AD3"),
}

# Coordinates are in the captured 2376x1080 frame. The 3:2 game viewport is
# x=378..1998; the ROIs exclude the CPU/GPU overlay in the black pillar box.
ROIS = (
    Roi("armor", "装甲 / 肩部", (480, 370, 660, 660), "game"),
    Roi("weapon", "手臂 / 武器", (790, 500, 500, 560), "game"),
    Roi("poster", "海报 / 标牌", (1280, 40, 350, 280), "text"),
    Roi("metal", "栏杆 / 金属地面", (1080, 420, 810, 620), "texture"),
    Roi("hud", "HUD 文字", (1635, 25, 345, 165), "ui"),
)

# These smaller windows are kept at their captured pixel dimensions in the
# 1:1 contact sheets. They are intentionally separate from the larger context
# crops above, which are scaled down to fit the overview layout.
PIXEL_ROIS = (
    Roi("armor-pixel", "装甲发光细节", (560, 350, 340, 340), "game"),
    Roi("weapon-pixel", "武器硬边与地面", (850, 600, 340, 340), "game"),
    Roi("poster-pixel", "海报文字", (1330, 60, 340, 280), "text"),
    Roi("metal-pixel", "金属扫描线", (1320, 580, 340, 340), "texture"),
    Roi("hud-pixel", "HUD 字体", (1595, 10, 720, 205), "ui"),
)

ALL_ROIS = ROIS + PIXEL_ROIS

GROUPS = {
    "native-input": (
        "原生输入档",
        ("01-native-scale1", "02-native-scale2", "03-native-scale3"),
    ),
    "twox-pipeline": (
        "相同 2× 输入",
        (
            "02-native-scale2",
            "04-sync-fsr-scale2",
            "06-async-fsr-scale2",
            "08-async-direct-scale2",
        ),
    ),
    "async-input": (
        "异步 FSR 输入档",
        ("05-async-fsr-scale1", "06-async-fsr-scale2", "07-async-fsr-scale3"),
    ),
    "direct-comparison": (
        "异步 vs 直接输出",
        (
            "06-async-fsr-scale2",
            "08-async-direct-scale2",
            "07-async-fsr-scale3",
            "09-async-direct-scale3",
        ),
    ),
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--batch-dir", required=True, type=Path)
    parser.add_argument("--time", type=float, default=10.0)
    parser.add_argument(
        "--quality-times",
        default="5,8,10,12,15",
        help="Comma-separated timestamps for multi-frame sharpness sampling",
    )
    parser.add_argument("--ffmpeg", default="ffmpeg")
    parser.add_argument("--force", action="store_true")
    return parser.parse_args()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_profiles(batch_dir: Path) -> list[Profile]:
    profiles: list[Profile] = []
    with (batch_dir / "performance-table.csv").open(newline="", encoding="utf-8-sig") as handle:
        for row in csv.DictReader(handle):
            key = row["profile"]
            if key not in PROFILE_META:
                continue
            label, short = PROFILE_META[key]
            profiles.append(
                Profile(
                    key=key,
                    label=label,
                    short=short,
                    input_size=row["input"],
                    surface=row["surface"],
                    mode=row["mode"],
                    scale=row["scale"],
                    fps=row["video_avg_fps"],
                    one_percent_low=row["one_percent_low"],
                    minimum_fps=row["min_fps"],
                    frame_ms=row["frame_ms_median"],
                    pss_gb=row["pss_gb"],
                    video=batch_dir / "videos" / f"{key}.mp4",
                )
            )
    if len(profiles) != len(PROFILE_META):
        missing = sorted(set(PROFILE_META) - {profile.key for profile in profiles})
        raise RuntimeError(f"performance table is missing profiles: {missing}")
    return profiles


def extract_frame(ffmpeg: str, video: Path, timestamp: float, output: Path, force: bool) -> None:
    if output.exists() and not force:
        return
    output.parent.mkdir(parents=True, exist_ok=True)
    command = [
        ffmpeg,
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-i",
        str(video),
        "-ss",
        f"{timestamp:.3f}",
        "-frames:v",
        "1",
        "-vsync",
        "0",
        str(output),
    ]
    subprocess.run(command, check=True)


def image_to_rgb(path: Path) -> np.ndarray:
    image = cv2.imread(str(path), cv2.IMREAD_COLOR)
    if image is None:
        raise RuntimeError(f"could not read {path}")
    return cv2.cvtColor(image, cv2.COLOR_BGR2RGB)


def crop_rgb(image: np.ndarray, box: tuple[int, int, int, int]) -> np.ndarray:
    x, y, width, height = box
    height_px, width_px = image.shape[:2]
    if x < 0 or y < 0 or x + width > width_px or y + height > height_px:
        raise RuntimeError(f"ROI {box} is outside {width_px}x{height_px}")
    return image[y : y + height, x : x + width]


def normalized_gray(image: np.ndarray) -> np.ndarray:
    gray = cv2.cvtColor(image, cv2.COLOR_RGB2GRAY).astype(np.float32)
    gray -= gray.mean()
    gray /= max(float(gray.std()), 1e-6)
    return gray


def estimate_shift(reference: np.ndarray, target: np.ndarray) -> tuple[float, float, float]:
    # Use a central, static-heavy region and avoid both performance overlays.
    x, y, width, height = 430, 150, 1480, 860
    reference_gray = normalized_gray(crop_rgb(reference, (x, y, width, height)))
    target_gray = normalized_gray(crop_rgb(target, (x, y, width, height)))
    window = cv2.createHanningWindow((width, height), cv2.CV_32F)
    (dx, dy), response = cv2.phaseCorrelate(reference_gray, target_gray, window)
    return float(dx), float(dy), float(response)


def quality_metrics(image: np.ndarray, reference: np.ndarray) -> dict[str, float | str]:
    gray = cv2.cvtColor(image, cv2.COLOR_RGB2GRAY).astype(np.float32)
    ref_gray = cv2.cvtColor(reference, cv2.COLOR_RGB2GRAY).astype(np.float32)
    lap = cv2.Laplacian(gray, cv2.CV_32F, ksize=3)
    sobel_x = cv2.Sobel(gray, cv2.CV_32F, 1, 0, ksize=3)
    sobel_y = cv2.Sobel(gray, cv2.CV_32F, 0, 1, ksize=3)
    sobel = cv2.magnitude(sobel_x, sobel_y)
    low_variance = max(float(gray.var()), 1e-6)
    lab = rgb2lab(image.astype(np.float32) / 255.0)
    ref_lab = rgb2lab(reference.astype(np.float32) / 255.0)
    color_delta_e = np.linalg.norm(lab - ref_lab, axis=2)
    mse = float(np.mean((gray - ref_gray) ** 2))
    psnr = "" if mse == 0 else float(peak_signal_noise_ratio(ref_gray, gray, data_range=255.0))
    ssim = float(structural_similarity(ref_gray, gray, data_range=255.0))
    return {
        "mean_luma": float(gray.mean()),
        "std_luma": float(gray.std()),
        "laplacian_variance": float(lap.var()),
        "laplacian_variance_ratio": float(lap.var() / low_variance),
        "sobel_mean": float(sobel.mean()),
        "psnr_vs_native3": "" if psnr == "" else round(psnr, 3),
        "ssim_vs_native3": round(ssim, 5),
        "mean_delta_e_vs_native3": round(float(color_delta_e.mean()), 3),
    }


def load_font(size: int) -> ImageFont.FreeTypeFont | ImageFont.ImageFont:
    candidates = (
        Path("C:/Windows/Fonts/msyh.ttc"),
        Path("C:/Windows/Fonts/msyhbd.ttc"),
        Path("C:/Windows/Fonts/segoeui.ttf"),
    )
    for candidate in candidates:
        if candidate.exists():
            return ImageFont.truetype(str(candidate), size)
    return ImageFont.load_default()


def contain(image: Image.Image, size: tuple[int, int]) -> Image.Image:
    copy = image.copy()
    copy.thumbnail(size, Image.Resampling.LANCZOS)
    return copy


def make_contact_sheet(
    roi: Roi,
    profiles: list[Profile],
    crops: dict[tuple[str, str], Image.Image],
    output: Path,
    cell_size: tuple[int, int] = (360, 300),
) -> None:
    columns = 3
    rows = 3
    margin = 18
    label_height = 42
    title_height = 54
    cell_width, cell_height = cell_size
    canvas = Image.new(
        "RGB",
        (
            margin + columns * (cell_width + margin),
            title_height + margin + rows * (cell_height + label_height + margin),
        ),
        (20, 22, 25),
    )
    draw = ImageDraw.Draw(canvas)
    title_font = load_font(25)
    label_font = load_font(18)
    detail_font = load_font(15)
    draw.text(
        (margin, 14),
        f"第 8 章 · {roi.label} · 固定 10.000 秒",
        font=title_font,
        fill=(238, 240, 242),
    )
    for index, profile in enumerate(profiles):
        row, column = divmod(index, columns)
        x = margin + column * (cell_width + margin)
        y = title_height + margin + row * (cell_height + label_height + margin)
        source = crops[(profile.key, roi.key)]
        fitted = contain(source, (cell_width, cell_height))
        px = x + (cell_width - fitted.width) // 2
        py = y + (cell_height - fitted.height) // 2
        canvas.paste(fitted, (px, py))
        draw.text((x, y + cell_height + 2), profile.label, font=label_font, fill=(238, 240, 242))
        draw.text(
            (x, y + cell_height + 23),
            f"{profile.input_size}  |  {float(profile.fps):.1f} FPS  |  {float(profile.one_percent_low):.1f}% low",
            font=detail_font,
            fill=(170, 176, 184),
        )
    canvas.save(output, quality=88, subsampling=0)


def make_group_sheet(
    group_label: str,
    roi: Roi,
    selected_profiles: list[Profile],
    crops: dict[tuple[str, str], Image.Image],
    output: Path,
) -> None:
    column_width = 440
    image_height = 400
    label_height = 50
    margin = 16
    title_height = 50
    width = margin + len(selected_profiles) * (column_width + margin)
    height = title_height + image_height + label_height + 2 * margin
    canvas = Image.new("RGB", (width, height), (20, 22, 25))
    draw = ImageDraw.Draw(canvas)
    draw.text((margin, 10), f"{group_label} · {roi.label}", font=load_font(24), fill=(240, 242, 244))
    for index, profile in enumerate(selected_profiles):
        x = margin + index * (column_width + margin)
        source = crops[(profile.key, roi.key)]
        fitted = contain(source, (column_width, image_height))
        canvas.paste(fitted, (x + (column_width - fitted.width) // 2, title_height))
        draw.text((x, title_height + image_height + 3), profile.label, font=load_font(20), fill=(240, 242, 244))
        draw.text(
            (x, title_height + image_height + 27),
            f"输入 {profile.input_size}  输出 {profile.surface}",
            font=load_font(15),
            fill=(174, 180, 187),
        )
    canvas.save(output, quality=91, subsampling=0)


def make_full_frame_sheet(profiles: list[Profile], frames: dict[str, np.ndarray], output: Path) -> None:
    columns = 3
    cell_width = 520
    cell_height = 260
    label_height = 38
    margin = 14
    width = margin + columns * (cell_width + margin)
    height = margin + 3 * (cell_height + label_height + margin)
    canvas = Image.new("RGB", (width, height), (20, 22, 25))
    draw = ImageDraw.Draw(canvas)
    for index, profile in enumerate(profiles):
        row, column = divmod(index, columns)
        x = margin + column * (cell_width + margin)
        y = margin + row * (cell_height + label_height + margin)
        image = Image.fromarray(crop_rgb(frames[profile.key], (378, 0, 1620, 1080)))
        fitted = contain(image, (cell_width, cell_height))
        canvas.paste(fitted, (x + (cell_width - fitted.width) // 2, y))
        draw.text((x, y + cell_height + 4), profile.label, font=load_font(18), fill=(238, 240, 242))
        draw.text(
            (x + 160, y + cell_height + 6),
            f"{profile.input_size} → {profile.surface}",
            font=load_font(14),
            fill=(170, 176, 184),
        )
    canvas.save(output, quality=89, subsampling=0)


def write_csv(path: Path, rows: list[dict[str, object]], fields: list[str]) -> None:
    with path.open("w", newline="", encoding="utf-8-sig") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        writer.writerows(rows)


def main() -> None:
    args = parse_args()
    batch_dir = args.batch_dir.resolve()
    output_dir = batch_dir / "visual-comparison"
    frame_dir = output_dir / "frames"
    roi_dir = output_dir / "rois"
    comparison_dir = output_dir / "comparisons"
    quality_frame_dir = output_dir / "quality-frames"
    for directory in (frame_dir, roi_dir, comparison_dir, quality_frame_dir):
        directory.mkdir(parents=True, exist_ok=True)

    profiles = load_profiles(batch_dir)
    frames: dict[str, np.ndarray] = {}
    frame_hashes: dict[str, str] = {}
    for profile in profiles:
        frame_path = frame_dir / f"{profile.key}.png"
        extract_frame(args.ffmpeg, profile.video, args.time, frame_path, args.force)
        frames[profile.key] = image_to_rgb(frame_path)
        frame_hashes[profile.key] = sha256(frame_path)

    crops_rgb: dict[tuple[str, str], np.ndarray] = {}
    crops_image: dict[tuple[str, str], Image.Image] = {}
    for roi in ALL_ROIS:
        roi_dir.joinpath(roi.key).mkdir(parents=True, exist_ok=True)
        for profile in profiles:
            crop = crop_rgb(frames[profile.key], roi.box)
            crops_rgb[(profile.key, roi.key)] = crop
            image = Image.fromarray(crop)
            crops_image[(profile.key, roi.key)] = image
            image.save(roi_dir / roi.key / f"{profile.key}.png")

    alignment_rows: list[dict[str, object]] = []
    reference_frame = frames["03-native-scale3"]
    for profile in profiles:
        dx, dy, response = estimate_shift(reference_frame, frames[profile.key])
        alignment_rows.append(
            {
                "profile": profile.key,
                "label": profile.label,
                "shift_x_px": round(dx, 4),
                "shift_y_px": round(dy, 4),
                "phase_correlation_response": round(response, 6),
            }
        )
    write_csv(
        output_dir / "alignment-to-native3.csv",
        alignment_rows,
        ["profile", "label", "shift_x_px", "shift_y_px", "phase_correlation_response"],
    )

    reference_crops: dict[str, np.ndarray] = {
        roi.key: crops_rgb[("03-native-scale3", roi.key)] for roi in ALL_ROIS
    }
    metric_rows: list[dict[str, object]] = []
    for roi in ALL_ROIS:
        for profile in profiles:
            crop = crops_rgb[(profile.key, roi.key)]
            metrics = quality_metrics(crop, reference_crops[roi.key])
            metric_rows.append(
                {
                    "profile": profile.key,
                    "label": profile.label,
                    "mode": profile.mode,
                    "scale": profile.scale,
                    "input": profile.input_size,
                    "surface": profile.surface,
                    "roi": roi.key,
                    "roi_label": roi.label,
                    "roi_kind": roi.kind,
                    "roi_x": roi.box[0],
                    "roi_y": roi.box[1],
                    "roi_width": roi.box[2],
                    "roi_height": roi.box[3],
                    **metrics,
                    "source_frame_sha256": frame_hashes[profile.key],
                }
            )
    metric_fields = [
        "profile",
        "label",
        "mode",
        "scale",
        "input",
        "surface",
        "roi",
        "roi_label",
        "roi_kind",
        "roi_x",
        "roi_y",
        "roi_width",
        "roi_height",
        "mean_luma",
        "std_luma",
        "laplacian_variance",
        "laplacian_variance_ratio",
        "sobel_mean",
        "psnr_vs_native3",
        "ssim_vs_native3",
        "mean_delta_e_vs_native3",
        "source_frame_sha256",
    ]
    write_csv(output_dir / "detail-metrics.csv", metric_rows, metric_fields)

    quality_times = sorted({float(item) for item in args.quality_times.split(",") if item.strip()})
    quality_rois = tuple(roi for roi in ALL_ROIS if roi.kind != "ui")
    multitime_rows: list[dict[str, object]] = []
    for timestamp in quality_times:
        timestamp_frames: dict[str, np.ndarray] = {}
        for profile in profiles:
            if abs(timestamp - args.time) < 1e-6:
                timestamp_frames[profile.key] = frames[profile.key]
                continue
            frame_path = quality_frame_dir / f"t{timestamp:06.3f}" / f"{profile.key}.png"
            extract_frame(args.ffmpeg, profile.video, timestamp, frame_path, args.force)
            timestamp_frames[profile.key] = image_to_rgb(frame_path)
        timestamp_reference = timestamp_frames["03-native-scale3"]
        for roi in quality_rois:
            reference_crop = crop_rgb(timestamp_reference, roi.box)
            for profile in profiles:
                crop = crop_rgb(timestamp_frames[profile.key], roi.box)
                metrics = quality_metrics(crop, reference_crop)
                multitime_rows.append(
                    {
                        "time_s": f"{timestamp:.3f}",
                        "profile": profile.key,
                        "label": profile.label,
                        "mode": profile.mode,
                        "scale": profile.scale,
                        "input": profile.input_size,
                        "roi": roi.key,
                        "roi_label": roi.label,
                        "roi_kind": roi.kind,
                        **metrics,
                    }
                )
    write_csv(
        output_dir / "detail-metrics-timeseries.csv",
        multitime_rows,
        [
            "time_s",
            "profile",
            "label",
            "mode",
            "scale",
            "input",
            "roi",
            "roi_label",
            "roi_kind",
            "mean_luma",
            "std_luma",
            "laplacian_variance",
            "laplacian_variance_ratio",
            "sobel_mean",
            "psnr_vs_native3",
            "ssim_vs_native3",
            "mean_delta_e_vs_native3",
        ],
    )
    aggregate_rows: list[dict[str, object]] = []
    metric_names = (
        "mean_luma",
        "std_luma",
        "laplacian_variance",
        "laplacian_variance_ratio",
        "sobel_mean",
        "ssim_vs_native3",
        "mean_delta_e_vs_native3",
    )
    for profile in profiles:
        for roi in quality_rois:
            samples = [
                row
                for row in multitime_rows
                if row["profile"] == profile.key and row["roi"] == roi.key
            ]
            row: dict[str, object] = {
                "profile": profile.key,
                "label": profile.label,
                "mode": profile.mode,
                "scale": profile.scale,
                "input": profile.input_size,
                "roi": roi.key,
                "roi_label": roi.label,
                "roi_kind": roi.kind,
                "sample_count": len(samples),
                "sample_times_s": ";".join(str(sample["time_s"]) for sample in samples),
            }
            for name in metric_names:
                row[f"{name}_median"] = round(
                    statistics.median(float(sample[name]) for sample in samples),
                    5,
                )
            aggregate_rows.append(row)
    write_csv(
        output_dir / "detail-metrics-multitime.csv",
        aggregate_rows,
        [
            "profile",
            "label",
            "mode",
            "scale",
            "input",
            "roi",
            "roi_label",
            "roi_kind",
            "sample_count",
            "sample_times_s",
            *[f"{name}_median" for name in metric_names],
        ],
    )

    for roi in ROIS:
        make_contact_sheet(
            roi,
            profiles,
            crops_image,
            comparison_dir / f"all-modes__{roi.key}.jpg",
        )
        for group_key, (group_label, profile_keys) in GROUPS.items():
            selected = [profile for profile in profiles if profile.key in profile_keys]
            make_group_sheet(
                group_label,
                roi,
                selected,
                crops_image,
                comparison_dir / f"{group_key}__{roi.key}.jpg",
            )

    for roi in PIXEL_ROIS:
        pixel_crops = {
            (profile.key, roi.key): Image.fromarray(crop_rgb(frames[profile.key], roi.box))
            for profile in profiles
        }
        make_contact_sheet(
            roi,
            profiles,
            pixel_crops,
            comparison_dir / f"pixel-1to1__{roi.key}.jpg",
            cell_size=(720, 205) if roi.key == "hud-pixel" else (410, 360),
        )

    make_full_frame_sheet(profiles, frames, comparison_dir / "full-frame-3x3.jpg")

    manifest = {
        "batch_dir": str(batch_dir),
        "sample_time_seconds": args.time,
        "frame_size": [frames[profiles[0].key].shape[1], frames[profiles[0].key].shape[0]],
        "game_viewport": {"x": 378, "y": 0, "width": 1620, "height": 1080},
        "metrics_reference": "03-native-scale3",
        "quality_sample_times_seconds": quality_times,
        "metrics_notes": [
            "All inputs are H.264 display captures at about 20 Mbit/s, not raw framebuffer images.",
            "Laplacian variance and Sobel mean are no-reference sharpness proxies; higher is not always better.",
            "PSNR/SSIM/delta-E are measured on the raw, unaligned crop at 10.000 s and can include idle-scene motion.",
            "The HUD ROI contains different FPS text values across captures and must not be treated as a ground-truth SSIM target.",
            "This comparison does not measure input-to-photon latency, frame generation quality, or VRR behavior.",
        ],
        "profiles": [
            {
                "key": profile.key,
                "label": profile.label,
                "mode": profile.mode,
                "input": profile.input_size,
                "surface": profile.surface,
                "fps": profile.fps,
                "one_percent_low": profile.one_percent_low,
                "minimum_fps": profile.minimum_fps,
                "frame_ms": profile.frame_ms,
                "pss_gb": profile.pss_gb,
                "video_sha256": sha256(profile.video),
                "frame_sha256": frame_hashes[profile.key],
            }
            for profile in profiles
        ],
        "rois": [
            {
                "key": roi.key,
                "label": roi.label,
                "kind": roi.kind,
                "x": roi.box[0],
                "y": roi.box[1],
                "width": roi.box[2],
                "height": roi.box[3],
            }
            for roi in ROIS
        ],
        "pixel_rois": [
            {
                "key": roi.key,
                "label": roi.label,
                "kind": roi.kind,
                "x": roi.box[0],
                "y": roi.box[1],
                "width": roi.box[2],
                "height": roi.box[3],
                "rendering": "1:1 source pixels in contact sheet",
            }
            for roi in PIXEL_ROIS
        ],
    }
    (output_dir / "comparison-manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    print(output_dir)


if __name__ == "__main__":
    main()
