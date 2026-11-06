#!/usr/bin/env python3
"""Build web-sized ROI images and an inline FSR detail comparison fragment."""

from __future__ import annotations

import argparse
import base64
import csv
import html
import io
import json
from pathlib import Path

from PIL import Image


ROI_LABELS = {
    "armor-pixel": "装甲发光细节",
    "weapon-pixel": "武器硬边 / 地面",
    "poster-pixel": "海报文字",
    "metal-pixel": "金属扫描线",
    "hud-pixel": "HUD 字体",
}

MODE_PARAMETERS = {
    "native": ("GLES 原生", "无", "否"),
    "upscale-depth-gpu": ("GLES→AHB→Vulkan FSR3.1", "当前帧同步等待", "否"),
    "upscale-depth-gpu-async": ("GLES→AHB→Vulkan FSR3.1", "显示上一帧", "否"),
    "upscale-depth-gpu-async-direct": ("GLES→AHB→Vulkan FSR3.1", "显示上一帧", "是"),
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--batch-dir", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--webp-quality", type=int, default=72)
    return parser.parse_args()


def write_webp(source: Image.Image, destination: Path, quality: int) -> tuple[str, int, int]:
    destination.parent.mkdir(parents=True, exist_ok=True)
    output = io.BytesIO()
    source.save(output, format="WEBP", quality=quality, method=6)
    payload = output.getvalue()
    destination.write_bytes(payload)
    encoded = base64.b64encode(payload).decode("ascii")
    return f"data:image/webp;base64,{encoded}", source.width, source.height


def table_cell(value: object) -> str:
    if value is None or value == "":
        return '<td class="text-end">—</td>'
    return f'<td class="text-end tabular-nums">{html.escape(str(value))}</td>'


def render_quality_rows(
    profiles: list[dict[str, object]],
    metrics: dict[tuple[str, str], dict[str, str]],
    roi: str,
) -> str:
    rows: list[str] = []
    for profile in profiles:
        key = str(profile["key"])
        values = metrics.get((key, roi))
        rows.append(
            "<tr>"
            f'<td>{html.escape(str(profile["label"]))}</td>'
            + table_cell(None if values is None else f'{float(values["laplacian_variance_median"]):.0f}')
            + table_cell(None if values is None else f'{float(values["sobel_mean_median"]):.1f}')
            + table_cell(None if values is None else f'{float(values["ssim_vs_native3_median"]):.3f}')
            + table_cell(
                None
                if values is None
                else f'{float(values["mean_delta_e_vs_native3_median"]):.1f}'
            )
            + "</tr>"
        )
    return "\n".join(rows)


def render_profile_rows(profiles: list[dict[str, object]], content_size: str) -> str:
    rows: list[str] = []
    for profile in profiles:
        mode = str(profile["mode"])
        renderer, delay, direct = MODE_PARAMETERS[mode]
        output = (
            str(profile["surface"])
            if mode == "native"
            else f"{content_size} 内容 / {profile['surface']} Surface"
        )
        rows.append(
            "<tr>"
            f'<td>{html.escape(str(profile["label"]))}</td>'
            f'<td>{html.escape(str(profile["input"]))} → {html.escape(output)}</td>'
            f'<td>{html.escape(renderer)}</td>'
            f'<td>{html.escape(delay)}</td>'
            f'<td class="text-center">{direct}</td>'
            f'<td class="text-end tabular-nums">{float(profile["fps"]):.1f}</td>'
            f'<td class="text-end tabular-nums">{float(profile["one_percent_low"]):.1f}</td>'
            f'<td class="text-end tabular-nums">{float(profile["frame_ms"]):.2f}</td>'
            f'<td class="text-end tabular-nums">{float(profile["pss_gb"]):.2f}</td>'
            "</tr>"
        )
    return "\n".join(rows)


def main() -> None:
    args = parse_args()
    batch_dir = args.batch_dir.resolve()
    comparison_dir = batch_dir / "visual-comparison"
    web_dir = comparison_dir / "web"
    manifest = json.loads(
        (comparison_dir / "comparison-manifest.json").read_text(encoding="utf-8")
    )
    profiles = manifest["profiles"]
    pixel_rois = [roi for roi in manifest["pixel_rois"] if roi["key"] in ROI_LABELS]
    roi_keys = [roi["key"] for roi in pixel_rois]

    metrics: dict[tuple[str, str], dict[str, str]] = {}
    with (comparison_dir / "detail-metrics-multitime.csv").open(
        newline="", encoding="utf-8-sig"
    ) as handle:
        for row in csv.DictReader(handle):
            metrics[(row["profile"], row["roi"])] = row

    image_data: dict[str, dict[str, dict[str, object]]] = {}
    for roi in pixel_rois:
        roi_key = str(roi["key"])
        image_data[roi_key] = {}
        x = int(roi["x"])
        y = int(roi["y"])
        width = int(roi["width"])
        height = int(roi["height"])
        for profile in profiles:
            profile_key = str(profile["key"])
            source = Image.open(
                comparison_dir / "rois" / roi_key / f"{profile_key}.png"
            ).convert("RGB")
            crop = source.crop((0, 0, width, height))
            data_uri, image_width, image_height = write_webp(
                crop,
                web_dir / "rois" / roi_key / f"{profile_key}.webp",
                args.webp_quality,
            )
            image_data[roi_key][profile_key] = {
                "src": data_uri,
                "width": image_width,
                "height": image_height,
            }

    default_roi = "armor-pixel"
    figures: list[str] = []
    for profile in profiles:
        profile_key = str(profile["key"])
        image = image_data[default_roi][profile_key]
        figures.append(
            "<figure class=\"compare-frame\">"
            f'<img id="compare-image-{html.escape(profile_key)}" '
            f'src="{image["src"]}" width="{image["width"]}" height="{image["height"]}" '
            f'alt="{html.escape(str(profile["label"]))}，{ROI_LABELS[default_roi]}" '
            'loading="eager">'
            "<figcaption>"
            f'<strong>{html.escape(str(profile["label"]))}</strong>'
            f'<span class="text-small">{html.escape(str(profile["input"]))} · {float(profile["fps"]):.1f} FPS · '
            f'{float(profile["one_percent_low"]):.1f}% low</span>'
            "</figcaption>"
            "</figure>"
        )

    roi_buttons: list[str] = []
    for roi_key in roi_keys:
        pressed = "true" if roi_key == default_roi else "false"
        roi_buttons.append(
            f'<button type="button" class="btn" data-roi="{html.escape(roi_key)}" '
            f'aria-pressed="{pressed}">{html.escape(ROI_LABELS[roi_key])}</button>'
        )

    quality_rows = render_quality_rows(profiles, metrics, default_roi)
    content_size = (
        f'{manifest["game_viewport"]["width"]}x{manifest["game_viewport"]["height"]}'
    )
    profile_rows = render_profile_rows(profiles, content_size)
    samples = manifest["quality_sample_times_seconds"]
    sample_text = " / ".join(f"{float(value):g}s" for value in samples)

    script_data = {
        "defaultRoi": default_roi,
        "profiles": [
            {
                "key": profile["key"],
                "label": profile["label"],
            }
            for profile in profiles
        ],
        "rois": {
            roi_key: {
                "label": ROI_LABELS[roi_key],
                "images": image_data[roi_key],
                "quality": {
                    str(profile["key"]): (
                        None
                        if metrics.get((str(profile["key"]), roi_key)) is None
                        else {
                            "lap": round(
                                float(
                                    metrics[(str(profile["key"]), roi_key)][
                                        "laplacian_variance_median"
                                    ]
                                )
                            ),
                            "sobel": round(
                                float(
                                    metrics[(str(profile["key"]), roi_key)][
                                        "sobel_mean_median"
                                    ]
                                ),
                                1,
                            ),
                            "ssim": round(
                                float(
                                    metrics[(str(profile["key"]), roi_key)][
                                        "ssim_vs_native3_median"
                                    ]
                                ),
                                3,
                            ),
                            "deltaE": round(
                                float(
                                    metrics[(str(profile["key"]), roi_key)][
                                        "mean_delta_e_vs_native3_median"
                                    ]
                                ),
                                1,
                            ),
                        }
                    )
                    for profile in profiles
                },
            }
            for roi_key in roi_keys
        },
    }

    fragment = f"""
<div id="fsr3-detail-compare">
  <style>
    #fsr3-detail-compare {{ color: var(--foreground); }}
    #fsr3-detail-compare .compare-heading {{ margin-bottom: 0.25rem; }}
    #fsr3-detail-compare .compare-note {{ margin: 0 0 1rem; color: var(--muted-foreground); }}
    #fsr3-detail-compare .roi-controls {{ margin-bottom: 1rem; }}
    #fsr3-detail-compare .compare-grid {{
      display: grid;
      grid-template-columns: repeat(3, minmax(0, 1fr));
      gap: 0.9rem;
      margin: 0.8rem 0 1.3rem;
    }}
    #fsr3-detail-compare .compare-frame {{ margin: 0; min-width: 0; }}
    #fsr3-detail-compare .compare-frame img {{
      display: block;
      width: 100%;
      height: auto;
    }}
    #fsr3-detail-compare .compare-frame figcaption {{
      display: grid;
      gap: 0.1rem;
      padding-top: 0.35rem;
    }}
    #fsr3-detail-compare .compare-frame figcaption span {{
      color: var(--muted-foreground);
    }}
    #fsr3-detail-compare .parameter-strip {{
      display: flex;
      flex-wrap: wrap;
      gap: 0.45rem 1rem;
      margin: 0.3rem 0 1rem;
      color: var(--muted-foreground);
    }}
    #fsr3-detail-compare .parameter-strip strong {{ color: var(--foreground); font-weight: 500; }}
    #fsr3-detail-compare .table-block + .table-block {{ margin-top: 1.35rem; }}
    #fsr3-detail-compare .metric-note {{ color: var(--muted-foreground); }}
    @media (max-width: 720px) {{
      #fsr3-detail-compare .compare-grid {{ grid-template-columns: repeat(2, minmax(0, 1fr)); }}
    }}
    @media (max-width: 460px) {{
      #fsr3-detail-compare .compare-grid {{ grid-template-columns: 1fr; }}
    }}
  </style>

  <h1 class="compare-heading">第 8 章 · 画质细节与运行参数</h1>
  <p class="compare-note">固定第 10.000 秒 · 1:1 像素裁图 · 指标取 {sample_text} 五个时间点的中位数</p>

  <div class="viz-row roi-controls" role="group" aria-label="选择对比区域">
    {''.join(roi_buttons)}
  </div>

  <div class="compare-grid" aria-live="polite">
    {''.join(figures)}
  </div>

  <div class="table-block">
    <h2>固定输入与算法状态</h2>
    <div class="parameter-strip">
      <span><strong>颜色</strong> R8G8B8A8</span>
      <span><strong>深度</strong> D16</span>
      <span><strong>运动向量</strong> 零纹理</span>
      <span><strong>相机抖动</strong> 0, 0</span>
      <span><strong>历史</strong> 每帧 reset</span>
      <span><strong>FSR 锐化</strong> 关闭 / 0.0</span>
      <span><strong>帧生成</strong> 关闭</span>
    </div>
  </div>

  <div class="table-block">
    <h2>运行参数</h2>
    <div class="table-responsive">
      <table class="table table-sm">
        <thead>
          <tr>
            <th>档位</th>
            <th>输入 → FSR 输出 / Surface</th>
            <th>呈现路径</th>
            <th>显示延迟</th>
            <th class="text-center">直写</th>
            <th class="text-end">FPS</th>
            <th class="text-end">1% low</th>
            <th class="text-end">帧时 ms</th>
            <th class="text-end">PSS GB</th>
          </tr>
        </thead>
        <tbody>
          {profile_rows}
        </tbody>
      </table>
    </div>
  </div>

  <div class="table-block">
    <h2>当前区域细节指标</h2>
    <div class="table-responsive">
      <table class="table table-sm">
        <thead>
          <tr>
            <th>档位</th>
            <th class="text-end">Laplacian</th>
            <th class="text-end">Sobel</th>
            <th class="text-end">SSIM vs 原生 3×</th>
            <th class="text-end">ΔE vs 原生 3×</th>
          </tr>
        </thead>
        <tbody id="quality-rows">
          {quality_rows}
        </tbody>
      </table>
    </div>
    <p class="metric-note text-small">Laplacian / Sobel 是无参考锐度代理，只用于同场景相对比较；HUD 文本数值随 FPS 变化，因此不计算这项指标。</p>
  </div>

  <script>
    (() => {{
      const data = {json.dumps(script_data, ensure_ascii=False, separators=(",", ":"))};
      const buttons = document.querySelectorAll('#fsr3-detail-compare [data-roi]');
      const qualityRows = document.getElementById('quality-rows');

      function dash(value) {{
        return value === null || value === undefined || Number.isNaN(value) ? '—' : value;
      }}

      function render(roiKey) {{
        const roi = data.rois[roiKey];
        buttons.forEach((button) => {{
          button.setAttribute('aria-pressed', String(button.dataset.roi === roiKey));
        }});
        data.profiles.forEach((profile) => {{
          const image = document.getElementById(`compare-image-${{profile.key}}`);
          const source = roi.images[profile.key];
          image.src = source.src;
          image.width = source.width;
          image.height = source.height;
          image.alt = `${{profile.label}}，${{roi.label}}`;
        }});
        qualityRows.replaceChildren(...data.profiles.map((profile) => {{
          const values = roi.quality[profile.key];
          const row = document.createElement('tr');
          const cells = [
            profile.label,
            values ? values.lap.toLocaleString('zh-CN') : '—',
            values ? values.sobel.toFixed(1) : '—',
            values ? values.ssim.toFixed(3) : '—',
            values ? values.deltaE.toFixed(1) : '—',
          ];
          cells.forEach((value, index) => {{
            const cell = document.createElement('td');
            cell.textContent = value;
            if (index > 0) {{
              cell.className = 'text-end tabular-nums';
            }}
            row.append(cell);
          }});
          return row;
        }}));
      }}

      buttons.forEach((button) => {{
        button.addEventListener('click', () => render(button.dataset.roi));
      }});
    }})();
  </script>
</div>
""".strip()

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(fragment + "\n", encoding="utf-8")
    print(args.output)
    print(f"fragment_bytes={args.output.stat().st_size}")


if __name__ == "__main__":
    main()
