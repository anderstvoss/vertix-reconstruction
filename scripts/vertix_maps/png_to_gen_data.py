#!/usr/bin/env python3
"""Convert a tile PNG to the nested genData shape used by Vertix 2016 setupMap.

This produces an experimental geometry fixture, NOT a historically verified
server gameSetup message. Only 8-bit RGB/RGBA, noninterlaced PNGs are supported.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import struct
import zlib

SIG = b"\x89PNG\r\n\x1a\n"


def decode_png(path: Path) -> tuple[int, int, list[int]]:
    data = path.read_bytes()
    if not data.startswith(SIG):
        raise ValueError("Invalid PNG signature")
    offset = 8
    size = None
    compressed = bytearray()
    saw_end = False
    while offset < len(data):
        if offset + 12 > len(data):
            raise ValueError("Truncated chunk")
        length = struct.unpack_from(">I", data, offset)[0]
        end = offset + 12 + length
        if end > len(data):
            raise ValueError("Truncated chunk body")
        tag = data[offset + 4:offset + 8]
        body = data[offset + 8:offset + 8 + length]
        expected_crc = struct.unpack_from(">I", data, offset + 8 + length)[0]
        if zlib.crc32(tag + body) & 0xffffffff != expected_crc:
            raise ValueError(f"PNG CRC mismatch: {tag!r}")
        if tag == b"IHDR":
            if size is not None or len(body) != 13:
                raise ValueError("Invalid or duplicate IHDR")
            width, height, depth, color, compression, filtering, interlace = struct.unpack(">IIBBBBB", body)
            if depth != 8 or color not in (2, 6) or compression or filtering or interlace:
                raise ValueError("Only 8-bit noninterlaced RGB or RGBA PNG supported")
            if width < 5 or height < 5 or width > 1024 or height > 1024:
                raise ValueError("Unreasonable map dimensions or missing 2-cell border")
            size = (width, height, 3 if color == 2 else 4)
        elif tag == b"IDAT":
            compressed.extend(body)
        elif tag == b"IEND":
            saw_end = True
            if offset + 12 != len(data):
                raise ValueError("Trailing bytes after IEND")
            break
        offset = end
    if not saw_end or size is None or not compressed:
        raise ValueError("Missing PNG IHDR, IDAT or IEND")
    width, height, channels = size
    raw = zlib.decompress(compressed)
    stride = width * channels
    if len(raw) != height * (stride + 1):
        raise ValueError("Unexpected decompressed pixel bytes")
    previous = bytearray(stride)
    rgba: list[int] = []
    for row in range(height):
        start = row * (stride + 1)
        filter_type = raw[start]
        if filter_type not in range(5):
            raise ValueError(f"Unsupported PNG filter {filter_type}")
        encoded = raw[start + 1:start + 1 + stride]
        output = bytearray(stride)
        for i, val in enumerate(encoded):
            left = output[i - channels] if i >= channels else 0
            up = previous[i]
            upper_left = previous[i - channels] if i >= channels else 0
            prediction = left + up - upper_left
            distances = [abs(prediction - c) for c in (left, up, upper_left)]
            predictor = (0, left, up, (left + up) // 2,
                         (left, up, upper_left)[distances.index(min(distances))])[filter_type]
            output[i] = (val + predictor) & 255
        for i in range(0, stride, channels):
            rgba.extend(output[i:i + 3])
            rgba.append(output[i + 3] if channels == 4 else 255)
        previous = output
    assert len(rgba) == 4 * width * height
    return width, height, rgba


def original_gen_data(path: Path) -> dict:
    width, height, rgba = decode_png(path)
    return {"width": width, "height": height, "data": {"data": rgba}}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("png", type=Path)
    parser.add_argument("--map-data", action="store_true",
                        help="emit mapData fixture with explicit game mode name and score")
    parser.add_argument("--mode-name", help="game mode name (caller-provided)")
    parser.add_argument("--score-to-win", type=int, help="caller-provided mode score")
    parser.add_argument("--tile-scale", type=int, help="caller-provided world scale")
    parser.add_argument("--pretty", action="store_true")
    args = parser.parse_args()
    gen_data = original_gen_data(args.png)
    if args.map_data:
        if not args.mode_name or args.score_to_win is None or args.tile_scale is None:
            parser.error("--map-data requires --mode-name, --score-to-win, --tile-scale")
        if args.score_to_win <= 0 or args.tile_scale <= 0:
            parser.error("score and tile-scale must be positive")
        result = {"genData": gen_data,
                  "width": (gen_data["width"] - 4) * args.tile_scale,
                  "height": (gen_data["height"] - 4) * args.tile_scale,
                  "gameMode": {"name": args.mode_name, "score": args.score_to_win},
                  "tiles": [], "clutter": [], "pickups": []}
    else:
        result = {"genData": gen_data}
    print(json.dumps(result, indent=2 if args.pretty else None,
                     separators=None if args.pretty else (",", ":")))


if __name__ == "__main__":
    main()
