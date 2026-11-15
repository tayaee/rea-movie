#!/usr/bin/env python3
"""KBO 야구모자 투명 PNG 에셋 생성기.

`assets/props/kbo_cap.png`를 생성한다. 이 PNG는 `rea-movie`의 앵커 프레임 편집
(`customization/characters.toml`의 `cap_ref`)에서 인물 머리 위에 합성된다.

- 640x320 투명 배경, 실제 모자 그림은 가로로 긴 형태(넓은 챙 + 낮은 크라운)
- 네이비 크라운 + 레드 앞판 + 흰색 'KBO' 자수
- 머리 bbox에 맞춰 스케일되어 사용되므로 정중앙 정렬 기준점

합성 규칙(anchors): 캡 폭 = 머리 폭 * cap_scale, 캡 높이는 PNG 종횡비 유지.
캡은 머리 상단 위에 얹히므로 종횡비가 낮을수록 자연스럽다.

사용법:
    python3 scripts/make-kbo-cap.py [출력경로]
"""
from __future__ import annotations

import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

W, H = 640, 320
NAVY = (24, 32, 84, 255)
NAVY_DARK = (14, 19, 52, 255)
RED = (200, 16, 46, 255)
WHITE = (255, 255, 255, 255)

FONT_CANDIDATES = [
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf",
    "/usr/share/fonts/truetype/freefont/FreeSansBold.ttf",
]


def load_font(size: int) -> ImageFont.ImageFont:
    for path in FONT_CANDIDATES:
        if Path(path).is_file():
            return ImageFont.truetype(path, size)
    return ImageFont.load_default()


def build_cap() -> Image.Image:
    img = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)

    # 챙(brim): 가로로 길게 뻗은 타원 (앞쪽으로 내밀린 챙)
    draw.ellipse((40, 176, 600, 262), fill=NAVY_DARK)
    draw.ellipse((64, 190, 576, 250), fill=NAVY_DARK)
    draw.ellipse((40, 176, 600, 262), outline=RED, width=7)

    # 크라운: 낮은 반원 돔
    draw.pieslice((60, 40, 580, 300), 180, 360, fill=NAVY)
    draw.ellipse((60, 170, 580, 268), fill=NAVY)

    # 레드 앞판 (정면 패널)
    draw.pieslice((238, 84, 402, 262), 180, 360, fill=RED)
    draw.rectangle((238, 170, 402, 262), fill=RED)

    # 모자 앞 상단 버튼
    draw.ellipse((310, 30, 330, 50), fill=RED)

    # 'KBO' 자수
    font = load_font(58)
    text = "KBO"
    bbox = draw.textbbox((0, 0), text, font=font)
    tw = bbox[2] - bbox[0]
    th = bbox[3] - bbox[1]
    draw.text((W // 2 - tw // 2 - bbox[0], 168 - th // 2 - bbox[1]), text, font=font, fill=WHITE)

    return img


def main() -> int:
    out = (
        Path(sys.argv[1])
        if len(sys.argv) > 1
        else Path(__file__).resolve().parents[1] / "assets" / "props" / "kbo_cap.png"
    )
    out.parent.mkdir(parents=True, exist_ok=True)
    build_cap().save(out)
    print(f"wrote {out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
